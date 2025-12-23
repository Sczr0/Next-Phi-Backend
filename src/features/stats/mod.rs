pub mod archive;
pub mod handler;
pub mod middleware;
pub mod models;
pub mod storage;

use std::{path::PathBuf, sync::Arc, time::Duration};

use tokio::sync::{mpsc, watch};

use crate::{config::AppConfig, error::AppError};
use hmac::{Hmac, Mac};
use models::EventInsert;
use once_cell::sync::OnceCell;
use sha2::Sha256;
use storage::StatsStorage;

/// 统计服务句柄：对外只暴露异步上报通道与优雅关闭
#[derive(Clone)]
pub struct StatsHandle {
    pub tx: mpsc::Sender<EventInsert>,
    shutdown_tx: watch::Sender<bool>,
    done_rx: watch::Receiver<bool>,
}

impl StatsHandle {
    pub async fn track(&self, evt: EventInsert) {
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
            Ok(_) => {
                tracing::info!("统计服务已关闭");
                Ok(())
            }
            Err(_) => Err(AppError::Internal("统计服务关闭超时".into())),
        }
    }

    /// 检查统计服务是否仍在运行
    pub fn is_closed(&self) -> bool {
        self.tx.is_closed()
    }

    /// 业务级打点：功能/动作
    pub async fn track_feature(
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
            instance: Some(hostname()),
            extra_json,
        };
        let _ = self.tx.try_send(evt);
    }
}

fn hostname() -> String {
    // hostname 在进程生命周期内稳定，缓存以减少重复系统调用与分配。
    static HOSTNAME: OnceCell<String> = OnceCell::new();
    HOSTNAME
        .get_or_init(|| gethostname::gethostname().to_string_lossy().to_string())
        .clone()
}

/// 初始化统计���务：创建 SQLite、spawn 批量写入与每日归档任务
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
                    match result {
                        Some(evt) => {
                            buf.push(evt);
                            if buf.len() >= batch_size {
                                if let Err(e) = writer_storage.insert_events(&buf).await {
                                    tracing::warn!("stats insert batch failed: {}", e);
                                }
                                buf.clear();
                                last = Instant::now();
                            }
                        }
                        None => {
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
                }
                // 定时刷新
                _ = sleep(timeout) => {
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

    // 每日聚合与归档任务
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
pub fn hmac_hex16(salt: &str, value: &str) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(salt.as_bytes()).expect("HMAC key");
    mac.update(value.as_bytes());
    let bytes = mac.finalize().into_bytes();
    hex::encode(&bytes[..16])
}

/// 依据 `UnifiedSaveRequest` 推导用户哈希（优先稳定标识）
pub fn derive_user_identity_from_auth(
    salt_opt: Option<&str>,
    auth: &crate::features::save::models::UnifiedSaveRequest,
) -> (Option<String>, Option<String>) {
    let Some(salt) = salt_opt else {
        return (None, None);
    };
    if let Some(tok) = &auth.session_token
        && !tok.is_empty()
    {
        return (
            Some(hmac_hex16(salt, tok)),
            Some("session_token".to_string()),
        );
    }
    if let Some(ext) = &auth.external_credentials {
        if let Some(id) = &ext.api_user_id
            && !id.is_empty()
        {
            return (
                Some(hmac_hex16(salt, id)),
                Some("external_api_user_id".to_string()),
            );
        }
        if let Some(st) = &ext.sessiontoken
            && !st.is_empty()
        {
            return (
                Some(hmac_hex16(salt, st)),
                Some("external_sessiontoken".to_string()),
            );
        }
        if let (Some(p), Some(pid)) = (&ext.platform, &ext.platform_id)
            && !p.is_empty()
            && !pid.is_empty()
        {
            let k = format!("{p}:{pid}");
            return (
                Some(hmac_hex16(salt, &k)),
                Some("platform_pair".to_string()),
            );
        }
    }
    (None, None)
}

#[cfg(test)]
mod tests {
    use super::{derive_user_identity_from_auth, hmac_hex16};
    use crate::features::save::{ExternalApiCredentials, UnifiedSaveRequest};

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
