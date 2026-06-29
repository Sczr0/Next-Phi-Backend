pub mod archive;
pub mod handler;
pub mod middleware;
pub mod models;
pub mod storage;

use std::{path::PathBuf, sync::Arc, time::Duration};

use tokio::sync::{mpsc, watch};

use crate::{config::AppConfig, error::AppError};
use models::EventInsert;
use once_cell::sync::OnceCell;
use storage::StatsStorage;

/// 统计服务句柄：对外只暴露异步上报通道与优雅关闭
#[derive(Clone)]
pub struct StatsHandle {
    pub tx: mpsc::Sender<EventInsert>,
    shutdown_tx: watch::Sender<bool>,
    done_rx: watch::Receiver<bool>,
}

impl StatsHandle {
    pub fn track(&self, evt: EventInsert) {
        // 若队列已满则丢弃，不阻塞主流程
        let _ = self.tx.try_send(evt);
    }

    /// 优雅关闭统计服务，等待所有事件处理完成
    ///
    /// # Arguments
    /// * `timeout` - 等待超时时间
    ///
    /// # Returns
    ///
    /// Ok(()) 表示关闭成功，Err表示超时或失败
    pub async fn graceful_shutdown(&self, timeout: Duration) -> Result<(), AppError> {
        tracing::info!("开始关闭统计服务，超时时间: {:?}", timeout);
        // 通知写入任务立即进入收尾并退出
        let _ = self.shutdown_tx.send(true);

        // 等待写入任务完成信号
        let mut rx = self.done_rx.clone();
        let wait = async {
            loop {
                if *rx.borrow() {
                    break;
                }
                if rx.changed().await.is_err() {
                    break;
                }
            }
        };

        match tokio::time::timeout(timeout, wait).await {
            Ok(()) => {
                tracing::info!("统计服务已关闭");
                Ok(())
            }
            Err(_) => Err(AppError::Internal("统计服务关闭超时".into())),
        }
    }

    /// 检查统计服务是否仍在运行
    #[must_use]
    pub fn is_closed(&self) -> bool {
        self.tx.is_closed()
    }

    /// 业务级打点：功能/动作
    pub fn track_feature(
        &self,
        feature: &str,
        action: &str,
        user_hash: Option<String>,
        extra_json: Option<serde_json::Value>,
    ) {
        let now = chrono::Utc::now();
        let evt = EventInsert {
            ts_utc: now,
            route: None,
            feature: Some(feature.to_string()),
            action: Some(action.to_string()),
            method: None,
            status: None,
            duration_ms: None,
            user_hash,
            client_ip_hash: None,
            instance: Some(hostname().into()),
            extra_json,
        };
        let _ = self.tx.try_send(evt);
    }
}

fn hostname() -> &'static str {
    // hostname 在进程生命周期内稳定，缓存以减少重复系统调用与分配。
    static HOSTNAME: OnceCell<String> = OnceCell::new();
    HOSTNAME
        .get_or_init(|| gethostname::gethostname().to_string_lossy().to_string())
        .as_str()
}

/// 初始化统计服务：创建 SQLite、spawn 批量写入与每日归档任务
pub async fn init_stats(config: &AppConfig) -> Result<(StatsHandle, Arc<StatsStorage>), AppError> {
    if !config.stats.enabled {
        tracing::warn!("统计功能已禁用（config.stats.enabled=false）");
    }

    // 用户哈希盐配置提示
    if config.stats.user_hash_salt.is_none() {
        tracing::info!("user_hash_salt 未配置，user_hash/client_ip_hash 将不会被记录");
    } else {
        tracing::info!("user_hash_salt 已配置，将记录用户和IP的去敏哈希");
    }

    // 确保目录存在
    let db_path = PathBuf::from(&config.stats.sqlite_path);
    if let Some(dir) = db_path.parent() {
        tokio::fs::create_dir_all(dir).await.ok();
    }

    let storage = Arc::new(
        StatsStorage::connect_sqlite(&config.stats.sqlite_path, config.stats.sqlite_wal).await?,
    );
    storage.init_schema().await?;

    let (tx, mut rx) = mpsc::channel::<EventInsert>(config.stats.batch_size * 10);
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let (done_tx, done_rx) = watch::channel(false);
    let writer_storage = storage.clone();
    let batch_size = config.stats.batch_size;
    let flush_interval = Duration::from_millis(config.stats.flush_interval_ms);
    tokio::spawn(async move {
        use tokio::time::{Instant, sleep};
        let mut buf: Vec<EventInsert> = Vec::with_capacity(batch_size);
        let mut last = Instant::now();
        let mut shutdown_rx = shutdown_rx; // mutable local receiver

        loop {
            let timeout = flush_interval;
            tokio::select! {
                // 收到关闭信号：尽快刷新并退出
                _ = shutdown_rx.changed() => {
                    if *shutdown_rx.borrow() {
                        // 尝试处理通道中剩余事件
                        while let Ok(evt) = rx.try_recv() {
                            buf.push(evt);
                            if buf.len() >= batch_size {
                                if let Err(e) = writer_storage.insert_events(&buf).await {
                                    tracing::warn!("stats final batch insert failed: {}", e);
                                }
                                buf.clear();
                            }
                        }
                        if !buf.is_empty() {
                            if let Err(e) = writer_storage.insert_events(&buf).await {
                                tracing::warn!("stats final flush failed: {}", e);
                            }
                            buf.clear();
                        }
                        tracing::info!("统计事件处理完成");
                        let _ = done_tx.send(true);
                        break;
                    }
                }
                // 接收到新事件或通道关闭
                result = rx.recv() => {
                    if let Some(evt) = result {
                        buf.push(evt);
                        if buf.len() >= batch_size {
                            if let Err(e) = writer_storage.insert_events(&buf).await {
                                tracing::warn!("stats insert batch failed: {}", e);
                            }
                            buf.clear();
                            last = Instant::now();
                        }
                    } else {
                        tracing::info!("统计事件通道关闭，处理剩余事件...");
                        // 处理剩余的所有事件
                        while let Ok(evt) = rx.try_recv() {
                            buf.push(evt);
                            if buf.len() >= batch_size {
                                if let Err(e) = writer_storage.insert_events(&buf).await {
                                    tracing::warn!("stats final batch insert failed: {}", e);
                                }
                                buf.clear();
                            }
                        }
                        // 处理最后一批事件
                        if !buf.is_empty() {
                            if let Err(e) = writer_storage.insert_events(&buf).await {
                                tracing::warn!("stats final flush failed: {}", e);
                            }
                            buf.clear();
                        }
                        tracing::info!("统计事件处理完成");
                        let _ = done_tx.send(true);
                        break;
                    }
                }
                // 定时刷新
                () = sleep(timeout) => {
                    if !buf.is_empty() && last.elapsed() >= flush_interval {
                        if let Err(e) = writer_storage.insert_events(&buf).await {
                            tracing::warn!("stats periodic flush failed: {}", e);
                        }
                        buf.clear();
                        last = Instant::now();
                    }
                }
            }
        }
    });

    // ── 每日预聚合任务（凌晨写入 daily_agg / daily_dau / daily_latency）──
    let agg_storage = storage.clone();
    let agg_cfg = config.stats.clone();
    tokio::spawn(async move {
        use chrono::{Duration, Timelike, Utc};
        use chrono_tz::Tz;
        loop {
            // 计算下次执行时间
            let tz: Tz = agg_cfg
                .timezone
                .parse()
                .unwrap_or(chrono_tz::Asia::Shanghai);
            let now_local = Utc::now().with_timezone(&tz);
            let (h, m) = parse_hour_min(&agg_cfg.daily_aggregate_time);
            let next_run_local =
                if now_local.hour() < h || (now_local.hour() == h && now_local.minute() < m) {
                    // 今天的聚合时间还没到
                    now_local
                        .with_hour(h)
                        .unwrap_or(now_local)
                        .with_minute(m)
                        .unwrap_or(now_local)
                        .with_second(0)
                        .unwrap_or(now_local)
                } else {
                    // 明天的
                    (now_local + Duration::days(1))
                        .with_hour(h)
                        .unwrap_or(now_local)
                        .with_minute(m)
                        .unwrap_or(now_local)
                        .with_second(0)
                        .unwrap_or(now_local)
                };
            let next_run_utc = next_run_local.with_timezone(&Utc);
            let delay = (next_run_utc - Utc::now())
                .to_std()
                .unwrap_or(std::time::Duration::from_mins(1));

            tokio::time::sleep(delay).await;

            // 聚合昨天的数据
            let yesterday = (Utc::now() - Duration::days(1))
                .format("%Y-%m-%d")
                .to_string();

            tracing::info!("开始每日预聚合: {yesterday}");
            match agg_storage.aggregate_day(&yesterday).await {
                Ok(()) => tracing::info!("每日预聚合完成: {yesterday}"),
                Err(e) => tracing::warn!("每日预聚合失败 ({yesterday}): {e}"),
            }
            // 预聚合新写入后清理 summary 缓存，避免返回看不到上一日新增数据的旧结果。
            crate::features::stats::handler::invalidate_all_stats_summary_cache();
        }
    });

    // ── 启动时后台补齐预聚合（覆盖热窗口内所有缺失日，保证 summary 快路径准确可用）──
    {
        let catchup_storage = storage.clone();
        let catchup_retention = config.stats.retention_hot_days;
        tokio::spawn(async move {
            // 先一次性修复历史遗留的 daily_agg / daily_latency 重复行（NULL 主键不强制唯一
            // 导致的重复累加），必须在启用快路径哨兵之前完成，否则 summary 仍会读到膨胀数据。
            if let Err(e) = catchup_storage.repair_daily_agg_duplicates_once().await {
                tracing::warn!("summary 预聚合重复修复失败: {e}");
            }
            tracing::info!(
                "summary 预补齐启动：检查热窗口 {} 天内缺失 daily_agg",
                catchup_retention
            );
            let done = match catchup_storage
                .backfill_missing_daily_aggregate_days(catchup_retention)
                .await
            {
                Ok(done) => done,
                Err(e) => {
                    tracing::warn!("summary 预补齐失败: {e}");
                    return;
                }
            };
            // 补齐完毕后写入哨兵，summary 才会启用快速路径。
            if let Err(e) = catchup_storage
                .set_stats_meta("backfill_complete", "true")
                .await
            {
                tracing::warn!("backfill_complete 标记写入失败: {e}");
                return;
            }
            tracing::info!("summary 预补齐完成 (补齐 {} 天)", done.len());
            crate::features::stats::handler::invalidate_all_stats_summary_cache();
        });
    }

    // 每日归档任务
    if config.stats.archive.parquet {
        let archiver_storage = storage.clone();
        let cfg = config.stats.clone();
        tokio::spawn(async move {
            crate::features::stats::archive::run_daily_archiver(archiver_storage, cfg).await;
        });
    }

    Ok((
        StatsHandle {
            tx,
            shutdown_tx,
            done_rx,
        },
        storage,
    ))
}

/// HMAC-SHA256(盐, 值) -> hex 前 32 位（16字节）
#[must_use]
pub fn hmac_hex16(salt: &str, value: &str) -> String {
    crate::identity_hash::hmac_hex16(salt, value)
}

/// 解析 "HH:MM" 格式的配置字符串为 (小时, 分钟)
fn parse_hour_min(s: &str) -> (u32, u32) {
    let parts: Vec<&str> = s.split(':').collect();
    let h = parts
        .first()
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(3);
    let m = parts
        .get(1)
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(0);
    (h.min(23), m.min(59))
}

/// 依据 `UnifiedSaveRequest` 推导用户哈希（优先稳定标识）
#[must_use]
pub fn derive_user_identity_from_auth(
    salt_opt: Option<&str>,
    auth: &crate::auth_contract::UnifiedSaveRequest,
) -> (Option<String>, Option<String>) {
    crate::identity_hash::derive_user_identity_from_auth(salt_opt, auth)
}

#[cfg(test)]
mod tests {
    use super::{derive_user_identity_from_auth, hmac_hex16};
    use crate::auth_contract::{ExternalApiCredentials, UnifiedSaveRequest};

    const SALT: &str = "test-salt";

    #[test]
    fn returns_none_when_salt_missing() {
        let req = UnifiedSaveRequest {
            session_token: Some("tok".into()),
            external_credentials: None,
            taptap_version: None,
        };
        let (id, kind) = derive_user_identity_from_auth(None, &req);
        assert!(id.is_none());
        assert!(kind.is_none());
    }

    #[test]
    fn prefers_session_token_over_external_credentials() {
        let req = UnifiedSaveRequest {
            session_token: Some("tok".into()),
            external_credentials: Some(ExternalApiCredentials {
                platform: Some("TapTap".into()),
                platform_id: Some("user_1".into()),
                sessiontoken: Some("ext-st".into()),
                api_user_id: Some("10086".into()),
                api_token: None,
            }),
            taptap_version: None,
        };
        let (id, kind) = derive_user_identity_from_auth(Some(SALT), &req);
        assert_eq!(kind.as_deref(), Some("session_token"));
        assert_eq!(id.as_deref(), Some(hmac_hex16(SALT, "tok").as_str()));
    }

    #[test]
    fn uses_external_api_user_id_when_present() {
        let req = UnifiedSaveRequest {
            session_token: None,
            external_credentials: Some(ExternalApiCredentials {
                platform: None,
                platform_id: None,
                sessiontoken: None,
                api_user_id: Some("10086".into()),
                api_token: None,
            }),
            taptap_version: None,
        };
        let (id, kind) = derive_user_identity_from_auth(Some(SALT), &req);
        assert_eq!(kind.as_deref(), Some("external_api_user_id"));
        assert_eq!(id.as_deref(), Some(hmac_hex16(SALT, "10086").as_str()));
    }

    #[test]
    fn uses_external_sessiontoken_when_present() {
        let req = UnifiedSaveRequest {
            session_token: None,
            external_credentials: Some(ExternalApiCredentials {
                platform: None,
                platform_id: None,
                sessiontoken: Some("ext-st".into()),
                api_user_id: None,
                api_token: None,
            }),
            taptap_version: None,
        };
        let (id, kind) = derive_user_identity_from_auth(Some(SALT), &req);
        assert_eq!(kind.as_deref(), Some("external_sessiontoken"));
        assert_eq!(id.as_deref(), Some(hmac_hex16(SALT, "ext-st").as_str()));
    }

    #[test]
    fn uses_platform_pair_when_present() {
        let req = UnifiedSaveRequest {
            session_token: None,
            external_credentials: Some(ExternalApiCredentials {
                platform: Some("TapTap".into()),
                platform_id: Some("user_1".into()),
                sessiontoken: None,
                api_user_id: None,
                api_token: None,
            }),
            taptap_version: None,
        };
        let (id, kind) = derive_user_identity_from_auth(Some(SALT), &req);
        assert_eq!(kind.as_deref(), Some("platform_pair"));
        assert_eq!(
            id.as_deref(),
            Some(hmac_hex16(SALT, "TapTap:user_1").as_str())
        );
    }
}
