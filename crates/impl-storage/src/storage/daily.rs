#![allow(clippy::items_after_test_module)]

use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc};
use chrono_tz::Tz;
use sqlx::{AssertSqlSafe, QueryBuilder, Row, Sqlite};

use crate::error::AppError;

use super::super::models::DailyAggRow;
use super::{
    DailyAggSliceRow, DailyDauDateRow, DailyFeatureUsageDateRow, DailyFeatureUsageSliceRow,
    StatsStorage,
};

fn push_daily_agg_filters(
    qb: &mut QueryBuilder<Sqlite>,
    feature: Option<&str>,
    route: Option<&str>,
    method: Option<&str>,
) {
    if let Some(feature) = feature {
        qb.push(" AND feature = ").push_bind(feature.to_string());
    }
    if let Some(route) = route {
        qb.push(" AND route = ").push_bind(route.to_string());
    }
    if let Some(method) = method {
        qb.push(" AND method = ").push_bind(method.to_string());
    }
}

fn push_daily_feature_filter(qb: &mut QueryBuilder<Sqlite>, feature: Option<&str>) {
    if let Some(feature) = feature {
        qb.push(" AND feature = ").push_bind(feature.to_string());
    }
}

const DAILY_DAU_USERS_WITH_OFFSET_SQL: &str = r"
    SELECT date(ts_utc, ?) as date,
           COUNT(DISTINCT user_hash) as active_users
    FROM events
    WHERE ts_utc BETWEEN ? AND ?
    GROUP BY date
    ORDER BY date ASC
";

const DAILY_DAU_IPS_WITH_OFFSET_SQL: &str = r"
    SELECT date(ts_utc, ?) as date,
           COUNT(DISTINCT client_ip_hash) as active_ips
    FROM events
    WHERE ts_utc BETWEEN ? AND ?
    GROUP BY date
    ORDER BY date ASC
";

const DAILY_DAU_USERS_SLICE_SQL: &str = r"
    SELECT COUNT(DISTINCT user_hash) as active_users
    FROM events
    WHERE ts_utc BETWEEN ? AND ?
";

const DAILY_DAU_IPS_SLICE_SQL: &str = r"
    SELECT COUNT(DISTINCT client_ip_hash) as active_ips
    FROM events
    WHERE ts_utc BETWEEN ? AND ?
";

fn merge_daily_dau_counts(
    user_rows: Vec<(String, i64)>,
    ip_rows: Vec<(String, i64)>,
) -> Vec<DailyDauDateRow> {
    let mut by_date: BTreeMap<String, (i64, i64)> = BTreeMap::new();
    for (date, active_users) in user_rows {
        by_date.entry(date).or_insert((0, 0)).0 = active_users;
    }
    for (date, active_ips) in ip_rows {
        by_date.entry(date).or_insert((0, 0)).1 = active_ips;
    }

    by_date
        .into_iter()
        .map(|(date, (active_users, active_ips))| DailyDauDateRow {
            date,
            active_users,
            active_ips,
        })
        .collect()
}

/// 计算本地日（配置时区）的 UTC 边界 `[start, end)`：start = 当日 00:00 转 UTC，
/// end = 次日 00:00 转 UTC。半开区间避免依赖 `23:59:59Z` 这类与存储格式相关的
/// 字典序巧合（详见聚合口径修复：预聚合表统一按本地日存储）。
#[allow(clippy::expect_used)] // 日期数学不变量：00:00:00/23:59:59 恒合法
fn local_day_bounds_utc(tz: Tz, day: NaiveDate) -> (String, String) {
    let start_ndt = NaiveDateTime::new(
        day,
        NaiveTime::from_hms_opt(0, 0, 0).expect("00:00:00 恒合法"),
    );
    let end_ndt = NaiveDateTime::new(
        day + chrono::Duration::days(1),
        NaiveTime::from_hms_opt(0, 0, 0).expect("00:00:00 恒合法"),
    );
    let start_local = match tz.from_local_datetime(&start_ndt) {
        chrono::LocalResult::Single(v) => v,
        chrono::LocalResult::Ambiguous(a, _) => a,
        chrono::LocalResult::None => tz.from_utc_datetime(&start_ndt),
    };
    let end_local = match tz.from_local_datetime(&end_ndt) {
        chrono::LocalResult::Single(v) => v,
        chrono::LocalResult::Ambiguous(_, b) => b,
        chrono::LocalResult::None => tz.from_utc_datetime(&end_ndt),
    };
    (
        start_local.with_timezone(&Utc).to_rfc3339(),
        end_local.with_timezone(&Utc).to_rfc3339(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    async fn build_tmp_storage_daily(label: &str) -> StatsStorage {
        let path = std::env::temp_dir().join(format!(
            "phi_daily_agg_{label}_{}.db",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        if let Some(p) = path.parent() {
            std::fs::create_dir_all(p).ok();
        }
        let storage = StatsStorage::connect_sqlite(path.to_string_lossy().as_ref(), false)
            .await
            .expect("connect sqlite");
        storage.init_schema().await.expect("init schema");
        storage
    }

    fn evt_null_route(
        ts: chrono::DateTime<chrono::Utc>,
        feature: &str,
        action: &str,
        instance: &str,
    ) -> crate::models::EventInsert {
        use std::borrow::Cow;
        crate::models::EventInsert {
            ts_utc: ts,
            route: None,
            feature: Some(feature.to_string()),
            action: Some(action.to_string()),
            method: None,
            status: None,
            duration_ms: None,
            user_hash: Some("u_test".to_string()),
            client_ip_hash: None,
            instance: Some(Cow::Owned(instance.to_string())),
            extra_json: Some(serde_json::json!({"user_kind": "official"})),
        }
    }

    #[tokio::test]
    async fn aggregate_day_is_idempotent_for_null_route_method_primary_key() {
        // 回归：daily_agg 主键 (date, feature, route, method) 在 route/method 为 NULL 时
        // SQLite 不强制唯一，旧实现用 REPLACE INTO 会追加重复行、累加计数。
        let storage = build_tmp_storage_daily("idempotent").await;
        let day = (chrono::Utc::now().date_naive() - chrono::Duration::days(2))
            .format("%Y-%m-%d")
            .to_string();
        let ts = chrono::Utc
            .from_utc_datetime(
                &chrono::NaiveDate::parse_from_str(&day, "%Y-%m-%d")
                    .unwrap()
                    .and_hms_opt(1, 0, 0)
                    .unwrap(),
            )
            .naive_utc()
            .and_utc();
        storage
            .insert_events(&[
                evt_null_route(ts, "save", "submit", "inst-a"),
                evt_null_route(ts, "save", "submit", "inst-a"),
                evt_null_route(ts, "bestn", "render", "inst-a"),
            ])
            .await
            .unwrap();

        // 重复聚合多次，计数不应增长。
        for _ in 0..5 {
            storage.aggregate_day(&day, chrono_tz::UTC).await.unwrap();
        }
        let rows = sqlx::query(
            "SELECT feature, SUM(count) AS cnt FROM daily_agg WHERE date = ? GROUP BY feature",
        )
        .bind(&day)
        .fetch_all(&storage.pool)
        .await
        .unwrap();
        let mut counts: std::collections::HashMap<String, i64> = rows
            .iter()
            .map(|r| {
                (
                    r.try_get::<String, _>("feature").unwrap_or_default(),
                    r.try_get::<i64, _>("cnt").unwrap_or(0),
                )
            })
            .collect();
        assert_eq!(
            counts.remove("save"),
            Some(2),
            "save 应仅计 2，不应被重复累加"
        );
        assert_eq!(counts.remove("bestn"), Some(1));
        assert!(counts.is_empty(), "无多余 feature 行: {counts:?}");

        // daily_latency 同样不应有重复行（route IS NOT NULL 过滤后此例无行，但确保不报错）。
        let lat = sqlx::query("SELECT COUNT(1) AS c FROM daily_latency WHERE date = ?")
            .bind(&day)
            .fetch_one(&storage.pool)
            .await
            .unwrap();
        assert_eq!(lat.try_get::<i64, _>("c").unwrap_or(0), 0);
    }

    #[tokio::test]
    async fn repair_daily_agg_duplicates_collapses_inflated_rows() {
        // 模拟历史遗留：直接注入重复的 NULL-route 行，验证修复函数能归并为一行。
        let storage = build_tmp_storage_daily("repair").await;
        for _ in 0..10 {
            sqlx::query(
                "INSERT INTO daily_agg (date, feature, route, method, count, err_count, last_ts) \
                 VALUES ('2026-01-01', 'save', NULL, NULL, 7, 0, '2026-01-01T01:00:00Z')",
            )
            .execute(&storage.pool)
            .await
            .unwrap();
        }
        let before = sqlx::query("SELECT SUM(count) AS s FROM daily_agg WHERE date='2026-01-01'")
            .fetch_one(&storage.pool)
            .await
            .unwrap()
            .try_get::<i64, _>("s")
            .unwrap_or(0);
        assert_eq!(before, 70, "构造 10 行重复 ×7");

        let repaired = storage.repair_daily_agg_duplicates_once().await.unwrap();
        assert!(repaired, "首次应执行修复");
        let after = sqlx::query("SELECT SUM(count) AS s FROM daily_agg WHERE date='2026-01-01'")
            .fetch_one(&storage.pool)
            .await
            .unwrap()
            .try_get::<i64, _>("s")
            .unwrap_or(0);
        assert_eq!(after, 7, "去重后应仅保留一行 count=7");

        // 再次调用应跳过（哨兵已写）。
        let again = storage.repair_daily_agg_duplicates_once().await.unwrap();
        assert!(!again, "哨兵存在时不应重复执行");
    }

    #[test]
    fn daily_dau_queries_split_user_and_ip_distinct_counts() {
        assert!(DAILY_DAU_USERS_WITH_OFFSET_SQL.contains("COUNT(DISTINCT user_hash)"));
        assert!(!DAILY_DAU_USERS_WITH_OFFSET_SQL.contains("client_ip_hash"));
        assert!(DAILY_DAU_USERS_WITH_OFFSET_SQL.contains("WHERE ts_utc BETWEEN ? AND ?"));
        assert!(DAILY_DAU_USERS_WITH_OFFSET_SQL.contains("GROUP BY date"));

        assert!(DAILY_DAU_IPS_WITH_OFFSET_SQL.contains("COUNT(DISTINCT client_ip_hash)"));
        assert!(!DAILY_DAU_IPS_WITH_OFFSET_SQL.contains("user_hash"));
        assert!(DAILY_DAU_IPS_WITH_OFFSET_SQL.contains("WHERE ts_utc BETWEEN ? AND ?"));
        assert!(DAILY_DAU_IPS_WITH_OFFSET_SQL.contains("GROUP BY date"));

        assert!(DAILY_DAU_USERS_SLICE_SQL.contains("COUNT(DISTINCT user_hash)"));
        assert!(DAILY_DAU_IPS_SLICE_SQL.contains("COUNT(DISTINCT client_ip_hash)"));
    }

    #[test]
    fn merge_daily_dau_counts_preserves_sorted_zero_sided_dates() {
        let merged = merge_daily_dau_counts(
            vec![("2026-01-02".to_string(), 3), ("2026-01-01".to_string(), 0)],
            vec![("2026-01-03".to_string(), 4), ("2026-01-02".to_string(), 1)],
        );

        assert_eq!(merged.len(), 3);
        assert_eq!(merged[0].date, "2026-01-01");
        assert_eq!(merged[0].active_users, 0);
        assert_eq!(merged[0].active_ips, 0);
        assert_eq!(merged[1].date, "2026-01-02");
        assert_eq!(merged[1].active_users, 3);
        assert_eq!(merged[1].active_ips, 1);
        assert_eq!(merged[2].date, "2026-01-03");
        assert_eq!(merged[2].active_users, 0);
        assert_eq!(merged[2].active_ips, 4);
    }
    #[tokio::test]
    async fn ensure_hot_window_migrates_old_rows_and_self_heals() {
        // 回归：首次运行全量重聚（迁移旧口径行），后续只补缺失天（自愈）。
        let storage = build_tmp_storage_daily("ensure").await;
        let sh: chrono_tz::Tz = "Asia/Shanghai".parse().unwrap();
        let today = chrono::Utc::now().with_timezone(&sh).date_naive();
        let d1 = today - chrono::Duration::days(2);
        let d2 = today - chrono::Duration::days(1);

        let make_evt = |day: NaiveDate, user: &str| crate::models::EventInsert {
            ts_utc: sh
                .from_local_datetime(&NaiveDateTime::new(
                    day,
                    NaiveTime::from_hms_opt(12, 0, 0).unwrap(),
                ))
                .single()
                .unwrap()
                .with_timezone(&Utc),
            route: Some("/song/search".to_string()),
            feature: None,
            action: None,
            method: Some("GET".to_string()),
            status: Some(200),
            duration_ms: Some(10),
            user_hash: Some(user.to_string()),
            client_ip_hash: Some("ip1".to_string()),
            instance: Some("inst-a".into()),
            extra_json: None,
        };
        storage
            .insert_events(&[make_evt(d1, "u1"), make_evt(d2, "u2")])
            .await
            .unwrap();

        // 模拟旧口径残留：手动写入错误行（值 999）
        sqlx::query("INSERT INTO daily_dau (date, active_users, active_ips) VALUES (?, 999, 999)")
            .bind(d1.format("%Y-%m-%d").to_string())
            .execute(&storage.pool)
            .await
            .unwrap();

        // 首次 ensure → 全量重聚（旧行被清除，重聚 d1/d2）
        let done = storage.ensure_hot_window_aggregated(30, sh).await.unwrap();
        assert_eq!(done, 2, "首次迁移应重聚全部有事件的天");
        let r = sqlx::query("SELECT active_users FROM daily_dau WHERE date = ?")
            .bind(d1.format("%Y-%m-%d").to_string())
            .fetch_one(&storage.pool)
            .await
            .unwrap();
        assert_eq!(
            r.try_get::<i64, _>("active_users").unwrap(),
            1,
            "旧口径残留行应被迁移重聚覆盖"
        );

        // 第二次 ensure → 无缺失，done = 0
        let done2 = storage.ensure_hot_window_aggregated(30, sh).await.unwrap();
        assert_eq!(done2, 0, "无缺失时不应重聚");

        // 再插入 d0 事件（模拟凌晨聚合失败/停机漏掉的天）→ 第三次 ensure 自愈补齐
        let d0 = today - chrono::Duration::days(3);
        storage.insert_events(&[make_evt(d0, "u0")]).await.unwrap();
        let done3 = storage.ensure_hot_window_aggregated(30, sh).await.unwrap();
        assert_eq!(done3, 1, "缺失天应被自愈补齐");
    }
}

impl StatsStorage {
    pub async fn query_daily_agg_with_offset(
        &self,
        modifier: &str,
        start_utc: &str,
        end_utc: &str,
        feature: Option<&str>,
        route: Option<&str>,
        method: Option<&str>,
    ) -> Result<Vec<DailyAggRow>, AppError> {
        let mut qb = QueryBuilder::<Sqlite>::new(
            r"
            SELECT date(ts_utc, 
        ",
        );
        qb.push_bind(modifier.to_string())
            .push(
                r") as date,
                   feature,
                   route,
                   method,
                   COUNT(1) as count,
                   COALESCE(SUM(CASE WHEN status >= 400 THEN 1 ELSE 0 END), 0) as err_count
            FROM events
            WHERE ts_utc BETWEEN 
        ",
            )
            .push_bind(start_utc.to_string())
            .push(" AND ")
            .push_bind(end_utc.to_string());

        push_daily_agg_filters(&mut qb, feature, route, method);

        qb.push(
            r"
            GROUP BY date, feature, route, method
            ORDER BY date ASC
        ",
        );

        let rows = qb
            .build()
            .fetch_all(&self.pool)
            .await
            .map_err(|e| AppError::Internal(format!("query daily with offset: {e}")))?;

        let mut out = Vec::with_capacity(rows.len());
        for r in rows {
            out.push(DailyAggRow {
                date: r.get::<String, _>("date"),
                feature: r.try_get::<String, _>("feature").ok(),
                route: r.try_get::<String, _>("route").ok(),
                method: r.try_get::<String, _>("method").ok(),
                count: r.get::<i64, _>("count"),
                err_count: r.get::<i64, _>("err_count"),
            });
        }
        Ok(out)
    }

    pub async fn query_daily_agg_slice(
        &self,
        start_utc: &str,
        end_utc: &str,
        feature: Option<&str>,
        route: Option<&str>,
        method: Option<&str>,
    ) -> Result<Vec<DailyAggSliceRow>, AppError> {
        let mut qb = QueryBuilder::<Sqlite>::new(
            r"
            SELECT feature,
                   route,
                   method,
                   COUNT(1) as count,
                   COALESCE(SUM(CASE WHEN status >= 400 THEN 1 ELSE 0 END), 0) as err_count
            FROM events
            WHERE ts_utc BETWEEN 
        ",
        );
        qb.push_bind(start_utc.to_string())
            .push(" AND ")
            .push_bind(end_utc.to_string());

        push_daily_agg_filters(&mut qb, feature, route, method);

        qb.push(
            r"
            GROUP BY feature, route, method
        ",
        );

        let rows = qb
            .build()
            .fetch_all(&self.pool)
            .await
            .map_err(|e| AppError::Internal(format!("query daily slice: {e}")))?;

        let mut out = Vec::with_capacity(rows.len());
        for r in rows {
            out.push(DailyAggSliceRow {
                feature: r.try_get::<String, _>("feature").ok(),
                route: r.try_get::<String, _>("route").ok(),
                method: r.try_get::<String, _>("method").ok(),
                count: r.get::<i64, _>("count"),
                err_count: r.get::<i64, _>("err_count"),
            });
        }
        Ok(out)
    }

    pub async fn query_daily_feature_usage_with_offset(
        &self,
        modifier: &str,
        start_utc: &str,
        end_utc: &str,
        feature: Option<&str>,
    ) -> Result<Vec<DailyFeatureUsageDateRow>, AppError> {
        let mut qb = QueryBuilder::<Sqlite>::new(
            r"
            SELECT date(ts_utc, 
        ",
        );
        qb.push_bind(modifier.to_string())
            .push(
                r") as date,
                   feature,
                   COUNT(1) as count,
                   COUNT(DISTINCT user_hash) as unique_users
            FROM events
            WHERE feature IS NOT NULL
              AND ts_utc BETWEEN 
        ",
            )
            .push_bind(start_utc.to_string())
            .push(" AND ")
            .push_bind(end_utc.to_string());

        push_daily_feature_filter(&mut qb, feature);

        qb.push(
            r"
            GROUP BY date, feature
            ORDER BY date ASC
        ",
        );

        let rows = qb
            .build()
            .fetch_all(&self.pool)
            .await
            .map_err(|e| AppError::Internal(format!("daily features with offset: {e}")))?;

        let mut out = Vec::with_capacity(rows.len());
        for r in rows {
            out.push(DailyFeatureUsageDateRow {
                date: r.get::<String, _>("date"),
                feature: r.get::<String, _>("feature"),
                count: r.get::<i64, _>("count"),
                unique_users: r.get::<i64, _>("unique_users"),
            });
        }
        Ok(out)
    }

    pub async fn query_daily_feature_usage_slice(
        &self,
        start_utc: &str,
        end_utc: &str,
        feature: Option<&str>,
    ) -> Result<Vec<DailyFeatureUsageSliceRow>, AppError> {
        let mut qb = QueryBuilder::<Sqlite>::new(
            r"
            SELECT feature,
                   COUNT(1) as count,
                   COUNT(DISTINCT user_hash) as unique_users
            FROM events
            WHERE feature IS NOT NULL
              AND ts_utc BETWEEN 
        ",
        );
        qb.push_bind(start_utc.to_string())
            .push(" AND ")
            .push_bind(end_utc.to_string());

        push_daily_feature_filter(&mut qb, feature);

        qb.push(
            r"
            GROUP BY feature
        ",
        );

        let rows = qb
            .build()
            .fetch_all(&self.pool)
            .await
            .map_err(|e| AppError::Internal(format!("daily features slice: {e}")))?;

        let mut out = Vec::with_capacity(rows.len());
        for r in rows {
            out.push(DailyFeatureUsageSliceRow {
                feature: r.get::<String, _>("feature"),
                count: r.get::<i64, _>("count"),
                unique_users: r.get::<i64, _>("unique_users"),
            });
        }
        Ok(out)
    }

    pub async fn query_daily_dau_with_offset(
        &self,
        modifier: &str,
        start_utc: &str,
        end_utc: &str,
    ) -> Result<Vec<DailyDauDateRow>, AppError> {
        let users_fut = async {
            let rows = sqlx::query(DAILY_DAU_USERS_WITH_OFFSET_SQL)
                .bind(modifier)
                .bind(start_utc)
                .bind(end_utc)
                .fetch_all(&self.pool)
                .await
                .map_err(|e| AppError::Internal(format!("daily dau users with offset: {e}")))?;

            Ok::<Vec<(String, i64)>, AppError>(
                rows.into_iter()
                    .map(|r| (r.get::<String, _>("date"), r.get::<i64, _>("active_users")))
                    .collect(),
            )
        };
        let ips_fut = async {
            let rows = sqlx::query(DAILY_DAU_IPS_WITH_OFFSET_SQL)
                .bind(modifier)
                .bind(start_utc)
                .bind(end_utc)
                .fetch_all(&self.pool)
                .await
                .map_err(|e| AppError::Internal(format!("daily dau ips with offset: {e}")))?;

            Ok::<Vec<(String, i64)>, AppError>(
                rows.into_iter()
                    .map(|r| (r.get::<String, _>("date"), r.get::<i64, _>("active_ips")))
                    .collect(),
            )
        };

        let (user_rows, ip_rows) = tokio::try_join!(users_fut, ips_fut)?;
        Ok(merge_daily_dau_counts(user_rows, ip_rows))
    }

    pub async fn query_daily_dau_slice(
        &self,
        start_utc: &str,
        end_utc: &str,
    ) -> Result<(i64, i64), AppError> {
        let users_fut = async {
            let r = sqlx::query(DAILY_DAU_USERS_SLICE_SQL)
                .bind(start_utc)
                .bind(end_utc)
                .fetch_one(&self.pool)
                .await
                .map_err(|e| AppError::Internal(format!("daily dau users slice: {e}")))?;
            Ok::<i64, AppError>(r.get::<i64, _>("active_users"))
        };
        let ips_fut = async {
            let r = sqlx::query(DAILY_DAU_IPS_SLICE_SQL)
                .bind(start_utc)
                .bind(end_utc)
                .fetch_one(&self.pool)
                .await
                .map_err(|e| AppError::Internal(format!("daily dau ips slice: {e}")))?;
            Ok::<i64, AppError>(r.get::<i64, _>("active_ips"))
        };

        tokio::try_join!(users_fut, ips_fut)
    }

    #[allow(clippy::expect_used)] // 日期数学不变量：start/end 均为合法 NaiveDate
    pub async fn query_daily(
        &self,
        start: NaiveDate,
        end: NaiveDate,
        feature: Option<String>,
        route: Option<String>,
        method: Option<String>,
    ) -> Result<Vec<DailyAggRow>, AppError> {
        // 若 daily_agg 尚未生成，临时从 events 动态聚合
        let start_dt = NaiveDateTime::new(
            start,
            NaiveTime::from_hms_opt(0, 0, 0).expect("00:00:00 恒合法"),
        );
        let end_dt = NaiveDateTime::new(
            end,
            NaiveTime::from_hms_opt(23, 59, 59).expect("23:59:59 恒合法"),
        );
        let start_s = DateTime::<Utc>::from_naive_utc_and_offset(start_dt, Utc).to_rfc3339();
        let end_s = DateTime::<Utc>::from_naive_utc_and_offset(end_dt, Utc).to_rfc3339();

        let mut qb = QueryBuilder::<Sqlite>::new(
            r"
            SELECT substr(ts_utc, 1, 10) as date,
                   feature,
                   route,
                   method,
                   COUNT(1) as count,
                   SUM(CASE WHEN status >= 400 THEN 1 ELSE 0 END) as err_count
            FROM events
            WHERE ts_utc BETWEEN 
        ",
        );
        qb.push_bind(start_s).push(" AND ").push_bind(end_s);

        push_daily_agg_filters(
            &mut qb,
            feature.as_deref(),
            route.as_deref(),
            method.as_deref(),
        );

        qb.push(
            r"
            GROUP BY date, feature, route, method
            ORDER BY date ASC
        ",
        );

        let rows = qb
            .build()
            .fetch_all(&self.pool)
            .await
            .map_err(|e| AppError::Internal(format!("query daily: {e}")))?;
        let mut out = Vec::with_capacity(rows.len());
        for r in rows {
            out.push(DailyAggRow {
                date: r.get::<String, _>("date"),
                feature: r.try_get::<String, _>("feature").ok(),
                route: r.try_get::<String, _>("route").ok(),
                method: r.try_get::<String, _>("method").ok(),
                count: r.get::<i64, _>("count"),
                err_count: r.get::<i64, _>("err_count"),
            });
        }
        Ok(out)
    }

    // ── 每日预聚合 ──

    /// 将指定日期（按配置时区解释的本地日）的 events 聚合写入 daily_agg /
    /// daily_dau / daily_latency，并同步预聚 summary 快速路径所需的三新增表
    /// （daily_status / daily_instance / daily_action / daily_user / daily_ip）。
    /// 全部放入单一事务内完成，使 summary 在判断"daily_agg 已覆盖某日"后，
    /// 可信赖地认为该日所有预聚合表一致可见。
    /// 幂等：可重复执行，不会重复计数。
    ///
    /// 口径说明：`day` 是配置时区（tz）下的本地日，聚合窗口取
    /// [本地 00:00, 次日 00:00) 的 UTC 半开区间，与查询接口按 timezone 解释日期
    /// 的口径一致（此前按 UTC 日聚合会导致 Asia/Shanghai 等时区下数据错位 8 小时）。
    pub async fn aggregate_day(&self, day: &str, tz: Tz) -> Result<(), AppError> {
        let day_date = NaiveDate::parse_from_str(day, "%Y-%m-%d")
            .map_err(|e| AppError::Internal(format!("aggregate_day 无效日期 ({day}): {e}")))?;
        let (start, end) = local_day_bounds_utc(tz, day_date);
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| AppError::Internal(format!("aggregate begin tx ({day}): {e}")))?;

        // 1) daily_agg：按 feature/route/method 聚合计数与错误数，并保留 MAX(ts_utc) 以供 summary last_ts 输出
        // 关键：必须先 DELETE 再 INSERT，而不能用 REPLACE INTO。daily_agg 主键为
        // (date, feature, route, method)，业务打点事件的 route/method 为 NULL，而 SQLite
        // 主键列含 NULL 时不强制唯一性（NULL 在唯一索引中被视为互不相同），REPLACE INTO
        // 无法命中既有行，每次重新聚合都会追加重复行，导致计数被反复累加而膨胀。
        sqlx::query("DELETE FROM daily_agg WHERE date = ?")
            .bind(day)
            .execute(&mut *tx)
            .await
            .map_err(|e| AppError::Internal(format!("aggregate daily_agg delete ({day}): {e}")))?;
        sqlx::query(
            r"
            INSERT INTO daily_agg (date, feature, route, method, count, err_count, last_ts)
            SELECT
                ? AS date,
                feature,
                route,
                method,
                COUNT(1) AS count,
                COALESCE(SUM(CASE WHEN status >= 400 THEN 1 ELSE 0 END), 0) AS err_count,
                MAX(ts_utc) AS last_ts
            FROM events
            WHERE ts_utc >= ? AND ts_utc < ?
            GROUP BY feature, route, method
            ",
        )
        .bind(day)
        .bind(&start)
        .bind(&end)
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::Internal(format!("aggregate daily_agg ({day}): {e}")))?;

        // 2) daily_dau：每日去重用户/IP 计数。
        sqlx::query(
            r"
            REPLACE INTO daily_dau (date, active_users, active_ips)
            SELECT
                ? AS date,
                COUNT(DISTINCT user_hash) AS active_users,
                COUNT(DISTINCT client_ip_hash) AS active_ips
            FROM events
            WHERE ts_utc >= ? AND ts_utc < ?
            ",
        )
        .bind(day)
        .bind(&start)
        .bind(&end)
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::Internal(format!("aggregate daily_dau ({day}): {e}")))?;

        // 3) daily_latency：按 feature/route/method 预聚延迟统计。
        // 同 daily_agg：主键含 NULL 时不强制唯一，必须 DELETE + INSERT 而非 REPLACE INTO，
        // 否则 route/method 为 NULL 的分组会被重复聚合累加。
        sqlx::query("DELETE FROM daily_latency WHERE date = ?")
            .bind(day)
            .execute(&mut *tx)
            .await
            .map_err(|e| {
                AppError::Internal(format!("aggregate daily_latency delete ({day}): {e}"))
            })?;
        sqlx::query(
            r"
            INSERT INTO daily_latency (date, feature, route, method, sample_count, min_ms, avg_ms, max_ms)
            SELECT
                ? AS date,
                feature,
                route,
                method,
                COUNT(1) AS sample_count,
                MIN(duration_ms) AS min_ms,
                AVG(duration_ms) AS avg_ms,
                MAX(duration_ms) AS max_ms
            FROM events
            WHERE route IS NOT NULL
              AND duration_ms IS NOT NULL
              AND ts_utc >= ? AND ts_utc < ?
            GROUP BY feature, route, method
            ",
        )
        .bind(day)
        .bind(&start)
        .bind(&end)
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::Internal(format!("aggregate daily_latency ({day}): {e}")))?;

        Self::compound_aggregate_preaggregate_tables(&mut tx, day, &start, &end).await?;

        tx.commit()
            .await
            .map_err(|e| AppError::Internal(format!("aggregate commit ({day}): {e}")))?;
        tracing::info!("daily_agg 预聚合完成: {day}");
        Ok(())
    }

    /// 为 summary 快速路径同步预聚三新增表（status / instance / action / user / ip）。
    /// 复用于 summary 在检测到某日缺失预聚时的按需补齐。
    async fn compound_aggregate_preaggregate_tables(
        tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        day: &str,
        start: &str,
        end: &str,
    ) -> Result<(), AppError> {
        // daily_status：(date, status, count) 仅 route NOT NULL 的 http 事件，按状态码计数。
        sqlx::query("DELETE FROM daily_status WHERE date = ?")
            .bind(day)
            .execute(&mut **tx)
            .await
            .map_err(|e| {
                AppError::Internal(format!("aggregate daily_status delete ({day}): {e}"))
            })?;
        let rows = sqlx::query(
            r"
            SELECT ? AS date, status, COUNT(1) AS cnt
            FROM events
            WHERE route IS NOT NULL AND status IS NOT NULL AND ts_utc >= ? AND ts_utc < ?
            GROUP BY status
            ",
        )
        .bind(day)
        .bind(start)
        .bind(end)
        .fetch_all(&mut **tx)
        .await
        .map_err(|e| AppError::Internal(format!("aggregate daily_status read ({day}): {e}")))?;
        for r in rows {
            let date: String = r.try_get("date").unwrap_or_else(|_| day.to_string());
            let status: i64 = r.try_get("status").unwrap_or(0);
            let cnt: i64 = r.try_get("cnt").unwrap_or(0);
            sqlx::query("INSERT INTO daily_status (date, status, count) VALUES (?, ?, ?)")
                .bind(date)
                .bind(status)
                .bind(cnt)
                .execute(&mut **tx)
                .await
                .map_err(|e| {
                    AppError::Internal(format!("aggregate daily_status insert ({day}): {e}"))
                })?;
        }

        // daily_instance：按 instance 聚合（涵盖 http 与业务打点事件），保留 MAX(ts_utc)。
        sqlx::query("DELETE FROM daily_instance WHERE date = ?")
            .bind(day)
            .execute(&mut **tx)
            .await
            .map_err(|e| {
                AppError::Internal(format!("aggregate daily_instance delete ({day}): {e}"))
            })?;
        sqlx::query(
            r"
            INSERT INTO daily_instance (date, instance, count, last_ts)
            SELECT ? AS date, instance, COUNT(1) AS cnt, MAX(ts_utc) AS last_ts
            FROM events
            WHERE instance IS NOT NULL AND ts_utc >= ? AND ts_utc < ?
            GROUP BY instance
            ",
        )
        .bind(day)
        .bind(start)
        .bind(end)
        .execute(&mut **tx)
        .await
        .map_err(|e| AppError::Internal(format!("aggregate daily_instance ({day}): {e}")))?;

        // daily_action：按 feature+action 聚合（业务打点），保留 MAX(ts_utc)。
        sqlx::query("DELETE FROM daily_action WHERE date = ?")
            .bind(day)
            .execute(&mut **tx)
            .await
            .map_err(|e| {
                AppError::Internal(format!("aggregate daily_action delete ({day}): {e}"))
            })?;
        sqlx::query(
            r"        
            INSERT INTO daily_action (date, feature, action, count, last_ts)
            SELECT ? AS date, feature, action, COUNT(1) AS cnt, MAX(ts_utc) AS last_ts
            FROM events
            WHERE feature IS NOT NULL AND action IS NOT NULL AND ts_utc >= ? AND ts_utc < ?
            GROUP BY feature, action
            ",
        )
        .bind(day)
        .bind(start)
        .bind(end)
        .execute(&mut **tx)
        .await
        .map_err(|e| AppError::Internal(format!("aggregate daily_action ({day}): {e}")))?;

        // daily_user：按 (date, user_hash, kind) 每日去重，kind 从 extra_json 提取。
        sqlx::query("DELETE FROM daily_user WHERE date = ?")
            .bind(day)
            .execute(&mut **tx)
            .await
            .map_err(|e| AppError::Internal(format!("aggregate daily_user delete ({day}): {e}")))?;
        sqlx::query(
            r"
            INSERT INTO daily_user (date, user_hash, kind)
            SELECT DISTINCT
                ? AS date,
                user_hash,
                CASE
                    WHEN json_valid(extra_json)
                         AND json_type(extra_json, '$.user_kind') = 'text'
                    THEN json_extract(extra_json, '$.user_kind')
                    ELSE NULL
                END AS kind
            FROM events
            WHERE user_hash IS NOT NULL AND ts_utc >= ? AND ts_utc < ?
            ",
        )
        .bind(day)
        .bind(start)
        .bind(end)
        .execute(&mut **tx)
        .await
        .map_err(|e| AppError::Internal(format!("aggregate daily_user ({day}): {e}")))?;

        // daily_ip：按 (date, ip_hash) 每日去重，仅 route NOT NULL 的 http 行。
        sqlx::query("DELETE FROM daily_ip WHERE date = ?")
            .bind(day)
            .execute(&mut **tx)
            .await
            .map_err(|e| AppError::Internal(format!("aggregate daily_ip delete ({day}): {e}")))?;
        sqlx::query(
            r"
            INSERT INTO daily_ip (date, ip_hash)
            SELECT DISTINCT ? AS date, client_ip_hash
            FROM events
            WHERE route IS NOT NULL AND client_ip_hash IS NOT NULL AND ts_utc >= ? AND ts_utc < ?
            ",
        )
        .bind(day)
        .bind(start)
        .bind(end)
        .execute(&mut **tx)
        .await
        .map_err(|e| AppError::Internal(format!("aggregate daily_ip ({day}): {e}")))?;
        Ok(())
    }

    // ── 快速查询路径（读预聚合表） ──

    /// 从 daily_agg 表快速读取聚合数据（仅对已聚合日期可用）
    pub async fn query_daily_agg_fast(
        &self,
        start_date: &str,
        end_date: &str,
        feature: Option<&str>,
        route: Option<&str>,
        method: Option<&str>,
    ) -> Result<Vec<DailyAggRow>, AppError> {
        let mut qb = QueryBuilder::<Sqlite>::new(
            "SELECT date, feature, route, method, count, err_count FROM daily_agg WHERE date BETWEEN ",
        );
        qb.push_bind(start_date.to_string())
            .push(" AND ")
            .push_bind(end_date.to_string());

        push_daily_agg_filters(&mut qb, feature, route, method);
        qb.push(" ORDER BY date ASC, feature ASC, route ASC, method ASC");

        let rows = qb
            .build()
            .fetch_all(&self.pool)
            .await
            .map_err(|e| AppError::Internal(format!("query daily_agg fast: {e}")))?;

        let mut out = Vec::with_capacity(rows.len());
        for r in rows {
            out.push(DailyAggRow {
                date: r.get::<String, _>("date"),
                feature: r.try_get::<String, _>("feature").ok(),
                route: r.try_get::<String, _>("route").ok(),
                method: r.try_get::<String, _>("method").ok(),
                count: r.get::<i64, _>("count"),
                err_count: r.get::<i64, _>("err_count"),
            });
        }
        Ok(out)
    }

    /// 确保热窗口内所有本地日已预聚合（自愈 + 首次口径迁移）：
    ///
    /// - 首次运行（stats_meta `daily_agg_tz_v2` 未标记）：对热窗口内全部有事件的
    ///   本地日全量重聚——先清空窗口内所有 daily_* 行再重建，将历史上按 UTC 日
    ///   聚合的旧口径数据迁移为配置时区本地日口径（DELETE+INSERT 幂等，失败可重跑）。
    /// - 后续运行：只补齐"有事件但 daily_agg 无行"的缺失天（凌晨聚合任务停机/
    ///   失败后的自愈）。
    ///
    /// 返回本次实际聚合的天数。
    pub async fn ensure_hot_window_aggregated(
        &self,
        retention_hot_days: u32,
        tz: Tz,
    ) -> Result<usize, AppError> {
        let today = Utc::now().with_timezone(&tz).date_naive();
        let lower = today - chrono::Duration::days(i64::from(retention_hot_days.saturating_sub(1)));
        let upper = today - chrono::Duration::days(1);
        let lower_s = lower.format("%Y-%m-%d").to_string();
        let upper_s = upper.format("%Y-%m-%d").to_string();

        let event_days = self.local_event_day_counts(&lower_s, &upper_s, tz).await?;
        let agg_days = self.daily_agg_dates_in_range(&lower_s, &upper_s).await?;

        let migrated = self.get_stats_meta("daily_agg_tz_v2").await? == Some("true".to_string());
        let mut done = 0usize;

        if !migrated {
            // 首次迁移：清空热窗口内旧口径行，再按本地日逐日重建。
            for table in [
                "daily_agg",
                "daily_dau",
                "daily_latency",
                "daily_status",
                "daily_instance",
                "daily_action",
                "daily_user",
                "daily_ip",
            ] {
                sqlx::query(AssertSqlSafe(format!(
                    "DELETE FROM {table} WHERE date BETWEEN ? AND ?"
                )))
                .bind(&lower_s)
                .bind(&upper_s)
                .execute(&self.pool)
                .await
                .map_err(|e| AppError::Internal(format!("ensure 迁移清理 {table}: {e}")))?;
            }
            for (day_s, count) in &event_days {
                if *count == 0 {
                    continue;
                }
                self.aggregate_day(day_s, tz).await?;
                done += 1;
                // 限速，避免 IO 峰值干扰热路径。
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
            self.set_stats_meta("daily_agg_tz_v2", "true").await?;
            tracing::info!("daily_agg 本地日口径迁移完成: 重聚 {done} 天");
            return Ok(done);
        }

        // 常规自愈：补齐"有事件但未预聚合"的缺失天。
        for (day_s, count) in &event_days {
            if *count > 0 && !agg_days.contains(day_s) {
                if let Err(e) = self.aggregate_day(day_s, tz).await {
                    tracing::warn!("每日预聚合自愈失败 ({day_s}): {e}");
                    // 避免单个失败终止后续
                    continue;
                }
                done += 1;
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
        }
        if done > 0 {
            tracing::info!("每日预聚合自愈补齐 {done} 天");
        }
        Ok(done)
    }

    /// 按配置时区的本地日统计热窗口内每天的事件数（用于缺失检测）。
    /// 偏移取"当前时刻"的时区偏移；对 DST 时区的个别历史天可能误判，但自愈与
    /// 查询覆盖检查会兜底，不影响最终正确性。
    pub async fn local_event_day_counts(
        &self,
        start_date: &str,
        end_date: &str,
        tz: Tz,
    ) -> Result<Vec<(String, i64)>, AppError> {
        let now_utc = Utc::now();
        let off_min =
            (now_utc.with_timezone(&tz).naive_local() - now_utc.naive_utc()).num_minutes();
        let modifier = format!("{off_min:+} minutes");
        let rows = sqlx::query(
            "SELECT date(ts_utc, ?) as day, COUNT(1) as c
             FROM events
             WHERE ts_utc >= ? AND ts_utc < ?
             GROUP BY day
             ORDER BY day ASC",
        )
        .bind(&modifier)
        .bind(format!("{start_date}T00:00:00Z"))
        .bind(format!("{end_date}T23:59:59Z"))
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Internal(format!("local event day counts: {e}")))?;

        let mut out = Vec::with_capacity(rows.len());
        for r in rows {
            let day: String = r
                .try_get("day")
                .map_err(|e| AppError::Internal(format!("read day: {e}")))?;
            let count: i64 = r
                .try_get("c")
                .map_err(|e| AppError::Internal(format!("read day count: {e}")))?;
            out.push((day, count));
        }
        Ok(out)
    }

    /// 返回 `daily_agg` 中指定日期区间内已存在的日期集合（快速路径覆盖检查用）。
    pub async fn daily_agg_dates_in_range(
        &self,
        start_date: &str,
        end_date: &str,
    ) -> Result<BTreeSet<String>, AppError> {
        let rows = sqlx::query(
            "SELECT DISTINCT date FROM daily_agg WHERE date BETWEEN ? AND ? ORDER BY date ASC",
        )
        .bind(start_date.to_string())
        .bind(end_date.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Internal(format!("list daily_agg days: {e}")))?;
        Ok(rows
            .iter()
            .filter_map(|r| r.try_get::<String, _>("date").ok())
            .collect())
    }

    /// 返回 `daily_dau` 中指定日期区间内已存在的日期集合（快速路径覆盖检查用）。
    pub async fn daily_dau_dates_in_range(
        &self,
        start_date: &str,
        end_date: &str,
    ) -> Result<BTreeSet<String>, AppError> {
        let rows = sqlx::query(
            "SELECT DISTINCT date FROM daily_dau WHERE date BETWEEN ? AND ? ORDER BY date ASC",
        )
        .bind(start_date.to_string())
        .bind(end_date.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Internal(format!("list daily_dau days: {e}")))?;
        Ok(rows
            .iter()
            .filter_map(|r| r.try_get::<String, _>("date").ok())
            .collect())
    }

    /// 一次性修复 `daily_agg` / `daily_latency` 中因历史 `REPLACE INTO` + NULL 主键
    /// 不强制唯一而累积的重复行（业务打点事件 route/method 为 NULL，主键含 NULL 时
    /// SQLite 不去重，每次重新聚合都追加一行，导致 summary 快路径计数成倍膨胀）。
    ///
    /// 策略：仅做去重（保留每组 (date, feature, route, method) 的最早 rowid 一行，
    /// GROUP BY 将 NULL 视为相等，可正确归并含 NULL 的分组）。热窗口内天数的计数
    /// 重建由 `ensure_hot_window_aggregated` 的 DELETE+INSERT 全量重聚完成；窗口外
    /// 的天 events 已归档删除，去重是恢复正确计数的唯一手段。
    ///
    /// 由 `stats_meta` 键 `daily_agg_dup_repaired` 守护，仅执行一次。
    pub async fn repair_daily_agg_duplicates_once(&self) -> Result<bool, AppError> {
        const META_KEY: &str = "daily_agg_dup_repaired";
        if self.get_stats_meta(META_KEY).await? == Some("true".to_string()) {
            return Ok(false);
        }

        let da_deleted = sqlx::query(
            "DELETE FROM daily_agg
             WHERE rowid NOT IN (
                 SELECT MIN(rowid) FROM daily_agg
                 GROUP BY date, feature, route, method
             )",
        )
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Internal(format!("repair daily_agg dedup: {e}")))?;
        let dl_deleted = sqlx::query(
            "DELETE FROM daily_latency
             WHERE rowid NOT IN (
                 SELECT MIN(rowid) FROM daily_latency
                 GROUP BY date, feature, route, method
             )",
        )
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Internal(format!("repair daily_latency dedup: {e}")))?;

        tracing::info!(
            "daily_agg 重复修复完成: 去重 daily_agg={}行 daily_latency={}行（热窗口内重建由 ensure_hot_window_aggregated 负责）",
            da_deleted.rows_affected(),
            dl_deleted.rows_affected(),
        );
        self.set_stats_meta(META_KEY, "true").await?;
        Ok(true)
    }

    /// 从 daily_dau 表快速读取 DAU 数据
    pub async fn query_daily_dau_fast(
        &self,
        start_date: &str,
        end_date: &str,
    ) -> Result<Vec<super::DailyDauDateRow>, AppError> {
        let rows = sqlx::query(
            "SELECT date, active_users, active_ips FROM daily_dau WHERE date BETWEEN ? AND ? ORDER BY date ASC"
        )
        .bind(start_date.to_string())
        .bind(end_date.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Internal(format!("query daily_dau fast: {e}")))?;

        let mut out = Vec::with_capacity(rows.len());
        for r in rows {
            out.push(super::DailyDauDateRow {
                date: r.get::<String, _>("date"),
                active_users: r.get::<i64, _>("active_users"),
                active_ips: r.get::<i64, _>("active_ips"),
            });
        }
        Ok(out)
    }
}
