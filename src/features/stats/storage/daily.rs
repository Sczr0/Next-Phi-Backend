#![allow(clippy::items_after_test_module)]

use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, Utc};
use sqlx::{QueryBuilder, Row, Sqlite};

use crate::error::AppError;

use super::super::models::DailyAggRow;
use super::{
    DailyAggSliceRow, DailyDauDateRow, DailyFeatureUsageDateRow, DailyFeatureUsageSliceRow,
    StatsStorage,
};

fn push_daily_agg_filters(
    qb: &mut QueryBuilder<'_, Sqlite>,
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

fn push_daily_feature_filter(qb: &mut QueryBuilder<'_, Sqlite>, feature: Option<&str>) {
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
    ) -> crate::features::stats::models::EventInsert {
        use std::borrow::Cow;
        crate::features::stats::models::EventInsert {
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
            storage.aggregate_day(&day).await.unwrap();
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

    pub async fn query_daily(
        &self,
        start: NaiveDate,
        end: NaiveDate,
        feature: Option<String>,
        route: Option<String>,
        method: Option<String>,
    ) -> Result<Vec<DailyAggRow>, AppError> {
        // 若 daily_agg 尚未生成，临时从 events 动态聚合
        let start_dt = NaiveDateTime::new(start, NaiveTime::from_hms_opt(0, 0, 0).unwrap());
        let end_dt = NaiveDateTime::new(end, NaiveTime::from_hms_opt(23, 59, 59).unwrap());
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

    /// 将指定日期（UTC）的 events 聚合写入 daily_agg / daily_dau / daily_latency，
    /// 并同步预聚 summary 快速路径所需的三新增表（daily_status / daily_instance /
    /// daily_action / daily_user / daily_ip）。全部放入单一事务内完成，使 summary
    /// 在判断“daily_agg 已覆盖某日”后，可信赖地认为该日所有预聚合表一致可见。
    /// 幂等：可重复执行，不会重复计数。
    pub async fn aggregate_day(&self, day: &str) -> Result<(), AppError> {
        let start = format!("{day}T00:00:00Z");
        let end = format!("{day}T23:59:59Z");
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

    /// 补齐缺失于 `daily_agg` 且热窗口内尚未预聚的 UTC 日（逐天调用 `aggregate_day`）。
    /// 返回本次补齐的日期列表。
    pub async fn backfill_missing_daily_aggregate_days(
        &self,
        retention_hot_days: u32,
    ) -> Result<Vec<NaiveDate>, AppError> {
        let today = Utc::now().date_naive();
        let lower = today - chrono::Duration::days(i64::from(retention_hot_days.saturating_sub(1)));
        let upper = today - chrono::Duration::days(1);

        let event_days = self.list_event_day_counts().await?;
        let agg_day_rows = sqlx::query(
            "SELECT DISTINCT date FROM daily_agg WHERE date BETWEEN ? AND ? ORDER BY date ASC",
        )
        .bind(lower.format("%Y-%m-%d").to_string())
        .bind(upper.format("%Y-%m-%d").to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Internal(format!("backfill list daily_agg days: {e}")))?;
        let agg_days: BTreeSet<String> = agg_day_rows
            .iter()
            .filter_map(|r| r.try_get::<String, _>("date").ok())
            .collect();

        let mut missing: Vec<NaiveDate> = Vec::new();
        for (day_s, count) in event_days {
            if count > 0
                && !agg_days.contains(&day_s)
                && let Ok(d) = NaiveDate::parse_from_str(&day_s, "%Y-%m-%d")
                && (d >= lower)
                && (d <= upper)
            {
                missing.push(d);
            }
        }
        missing.sort_unstable();

        let mut done = Vec::new();
        for day in &missing {
            if let Err(e) = self
                .aggregate_day(&day.format("%Y-%m-%d").to_string())
                .await
            {
                tracing::warn!("summary 背景补预聚失败 ({day}): {e}");
                // 避免单个失败终止后续
                continue;
            }
            done.push(*day);
            // 限速，避免 IO 峰值干扰热路径。
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        Ok(done)
    }

    /// 一次性修复 `daily_agg` / `daily_latency` 中因历史 `REPLACE INTO` + NULL 主键
    /// 不强制唯一而累积的重复行（业务打点事件 route/method 为 NULL，主键含 NULL 时
    /// SQLite 不去重，每次重新聚合都追加一行，导致 summary 快路径计数成倍膨胀）。
    ///
    /// 策略：
    /// 1. 仍在 `events` 表内的天（热窗口内）：直接重跑 `aggregate_day`（现已改为
    ///    DELETE + INSERT），从 events 重建为正确计数。
    /// 2. 已不在 `events` 表内的天（已归档清理）：events 已不可用，但这些天在归档后
    ///    events 不再变化，重复行是精确副本，故按 (date, feature, route, method) 去重
    ///    保留一行即可恢复正确计数（GROUP BY 将 NULL 视为相等，可正确归并）。
    ///
    /// 由 `stats_meta` 键 `daily_agg_dup_repaired` 守护，仅执行一次。
    pub async fn repair_daily_agg_duplicates_once(&self) -> Result<bool, AppError> {
        const META_KEY: &str = "daily_agg_dup_repaired";
        if self.get_stats_meta(META_KEY).await? == Some("true".to_string()) {
            return Ok(false);
        }

        // (1) 去重：保留每组 (date, feature, route, method) 的最早 rowid 一行。
        //     GROUP BY 将 NULL 视为相等，能正确归并含 NULL 的业务打点分组。
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

        // (2) 仍在 events 内的天重跑 aggregate_day，用最新 events 重建精确计数。
        let event_days = self.list_event_day_counts().await?;
        let today = Utc::now().date_naive();
        let mut reaggregated = 0usize;
        for (day_s, _count) in &event_days {
            let Ok(d) = NaiveDate::parse_from_str(day_s, "%Y-%m-%d") else {
                continue;
            };
            // 不重聚今日（仍在写入）
            if d >= today {
                continue;
            }
            if let Err(e) = self.aggregate_day(day_s).await {
                tracing::warn!("repair 重聚失败 ({day_s}): {e}");
            } else {
                reaggregated += 1;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }

        tracing::info!(
            "daily_agg 重复修复完成: 去重 daily_agg={}行 daily_latency={}行，从 events 重聚 {} 天",
            da_deleted.rows_affected(),
            dl_deleted.rows_affected(),
            reaggregated
        );
        self.set_stats_meta(META_KEY, "true").await?;
        Ok(true)
    }

    /// 用于 summary 快速路径检测：给定 UTC 起止日（含）区间内是否已有 daily_agg 行。
    pub async fn daily_agg_has_rows_in_range(
        &self,
        start_date: &str,
        end_date: &str,
    ) -> Result<bool, AppError> {
        let r = sqlx::query(
            "SELECT EXISTS(SELECT 1 FROM daily_agg WHERE date BETWEEN ? AND ?) AS exists_flag",
        )
        .bind(start_date)
        .bind(end_date)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| AppError::Internal(format!("daily_agg has rows check: {e}")))?;
        Ok(r.try_get::<i64, _>("exists_flag").unwrap_or(0) != 0)
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
