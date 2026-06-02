use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, Utc};
use futures_util::TryStreamExt;
use sqlx::{QueryBuilder, Row, Sqlite};

use crate::error::AppError;

use super::super::models::{DailyAggRow, EventInsert};
use super::{
    ArchiveEventRow, DailyAggSliceRow, DailyDauDateRow, DailyFeatureUsageDateRow,
    DailyFeatureUsageSliceRow, DailyHttpRouteMetricRow, DailyHttpRouteMetricSliceRow,
    DailyHttpTotalMetricRow, LatencyAggBucketRow, LatencyAggSliceRow, StatsStorage,
    StatsSummaryData, SummaryActionRow, SummaryFeatureRow, SummaryIncludeFlags, SummaryInstanceRow,
    SummaryLatencyData, SummaryMethodRow, SummaryRouteRow, SummaryStatusCodeRow,
};

fn saturating_u64_to_i64(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

fn push_stats_ts_range_filters(
    qb: &mut QueryBuilder<'_, Sqlite>,
    start_utc: Option<&str>,
    end_utc: Option<&str>,
) {
    if let Some(start) = start_utc {
        qb.push(" AND ts_utc >= ").push_bind(start.to_string());
    }
    if let Some(end) = end_utc {
        qb.push(" AND ts_utc <= ").push_bind(end.to_string());
    }
}

fn push_stats_feature_filter(qb: &mut QueryBuilder<'_, Sqlite>, feature: Option<&str>) {
    if let Some(feature) = feature {
        qb.push(" AND feature = ").push_bind(feature.to_string());
    }
}

impl StatsStorage {
    pub async fn insert_events(&self, events: &[EventInsert]) -> Result<(), AppError> {
        if events.is_empty() {
            return Ok(());
        }
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| AppError::Internal(format!("begin tx: {e}")))?;

        // SQLite 默认 `SQLITE_MAX_VARIABLE_NUMBER=999`，这里每行 11 列绑定参数，保守按 90 行/批次拆分。
        // 性能优化：从“逐条 INSERT”改为“单语句多行 INSERT”，降低 SQL 解析与往返开销，不改变写入语义。
        const COLS: usize = 11;
        const SQLITE_MAX_VARS: usize = 999;
        const MAX_ROWS_PER_INSERT: usize = SQLITE_MAX_VARS / COLS; // 90

        for chunk in events.chunks(MAX_ROWS_PER_INSERT) {
            let mut qb = QueryBuilder::new(
                "INSERT INTO events(ts_utc, route, feature, action, method, status, duration_ms, user_hash, client_ip_hash, instance, extra_json) ",
            );
            qb.push_values(chunk, |mut b, e| {
                let extra = e
                    .extra_json
                    .as_ref()
                    .map(|v| serde_json::to_string(v).unwrap_or_default());
                b.push_bind(e.ts_utc.to_rfc3339())
                    .push_bind(&e.route)
                    .push_bind(&e.feature)
                    .push_bind(&e.action)
                    .push_bind(&e.method)
                    .push_bind(e.status.map(i64::from))
                    .push_bind(e.duration_ms)
                    .push_bind(&e.user_hash)
                    .push_bind(&e.client_ip_hash)
                    .push_bind(e.instance.as_deref())
                    .push_bind(extra);
            });
            qb.build()
                .execute(&mut *tx)
                .await
                .map_err(|e| AppError::Internal(format!("insert event: {e}")))?;
        }
        tx.commit()
            .await
            .map_err(|e| AppError::Internal(format!("commit: {e}")))?;
        Ok(())
    }

    pub async fn checkpoint_wal_truncate(&self) -> Result<(), AppError> {
        sqlx::query("PRAGMA wal_checkpoint(TRUNCATE);")
            .execute(&self.pool)
            .await
            .map_err(|e| AppError::Internal(format!("wal checkpoint: {e}")))?;
        Ok(())
    }

    pub async fn list_event_day_counts(&self) -> Result<Vec<(String, i64)>, AppError> {
        let rows = sqlx::query(
            "SELECT substr(ts_utc,1,10) as day, COUNT(1) as c \
             FROM events GROUP BY day ORDER BY day ASC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Internal(format!("load daily counts: {e}")))?;

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

    pub async fn delete_events_in_range_batch(
        &self,
        from_rfc3339: &str,
        to_rfc3339: &str,
        batch_size: i64,
    ) -> Result<i64, AppError> {
        let res = sqlx::query(
            "DELETE FROM events WHERE id IN (
               SELECT id FROM events
               WHERE ts_utc >= ? AND ts_utc < ?
               ORDER BY id ASC
               LIMIT ?
             )",
        )
        .bind(from_rfc3339)
        .bind(to_rfc3339)
        .bind(batch_size)
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Internal(format!("delete events range: {e}")))?;
        Ok(saturating_u64_to_i64(res.rows_affected()))
    }

    pub async fn count_events_in_range(
        &self,
        from_rfc3339: &str,
        to_rfc3339: &str,
    ) -> Result<i64, AppError> {
        let row = sqlx::query("SELECT COUNT(1) as c FROM events WHERE ts_utc >= ? AND ts_utc < ?")
            .bind(from_rfc3339)
            .bind(to_rfc3339)
            .fetch_one(&self.pool)
            .await
            .map_err(|e| AppError::Internal(format!("count events range: {e}")))?;
        row.try_get::<i64, _>("c")
            .map_err(|e| AppError::Internal(format!("read events range count: {e}")))
    }

    pub async fn query_archive_events_between(
        &self,
        start_rfc3339: &str,
        end_rfc3339: &str,
    ) -> Result<Vec<ArchiveEventRow>, AppError> {
        let rows = sqlx::query(
            r"SELECT ts_utc, route, feature, action, method, status, duration_ms, user_hash, client_ip_hash, instance, extra_json FROM events WHERE ts_utc BETWEEN ? AND ? ORDER BY ts_utc ASC",
        )
        .bind(start_rfc3339)
        .bind(end_rfc3339)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Internal(format!("archive query: {e}")))?;
        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            out.push(ArchiveEventRow {
                ts_utc: row.try_get::<String, _>("ts_utc").unwrap_or_default(),
                route: row.try_get::<String, _>("route").ok(),
                feature: row.try_get::<String, _>("feature").ok(),
                action: row.try_get::<String, _>("action").ok(),
                method: row.try_get::<String, _>("method").ok(),
                status: row.try_get::<i64, _>("status").ok(),
                duration_ms: row.try_get::<i64, _>("duration_ms").ok(),
                user_hash: row.try_get::<String, _>("user_hash").ok(),
                client_ip_hash: row.try_get::<String, _>("client_ip_hash").ok(),
                instance: row.try_get::<String, _>("instance").ok(),
                extra_json: row.try_get::<String, _>("extra_json").ok(),
            });
        }
        Ok(out)
    }

    pub async fn query_daily_agg_with_offset(
        &self,
        modifier: &str,
        start_utc: &str,
        end_utc: &str,
        feature: Option<&str>,
        route: Option<&str>,
        method: Option<&str>,
    ) -> Result<Vec<DailyAggRow>, AppError> {
        let rows = sqlx::query(
            r"
            SELECT date(ts_utc, ?) as date,
                   feature,
                   route,
                   method,
                   COUNT(1) as count,
                   COALESCE(SUM(CASE WHEN status >= 400 THEN 1 ELSE 0 END), 0) as err_count
            FROM events
            WHERE ts_utc BETWEEN ? AND ?
              AND (? IS NULL OR feature = ?)
              AND (? IS NULL OR route = ?)
              AND (? IS NULL OR method = ?)
            GROUP BY date, feature, route, method
            ORDER BY date ASC
        ",
        )
        .bind(modifier)
        .bind(start_utc)
        .bind(end_utc)
        .bind(feature)
        .bind(feature)
        .bind(route)
        .bind(route)
        .bind(method)
        .bind(method)
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
        let rows = sqlx::query(
            r"
            SELECT feature,
                   route,
                   method,
                   COUNT(1) as count,
                   COALESCE(SUM(CASE WHEN status >= 400 THEN 1 ELSE 0 END), 0) as err_count
            FROM events
            WHERE ts_utc BETWEEN ? AND ?
              AND (? IS NULL OR feature = ?)
              AND (? IS NULL OR route = ?)
              AND (? IS NULL OR method = ?)
            GROUP BY feature, route, method
        ",
        )
        .bind(start_utc)
        .bind(end_utc)
        .bind(feature)
        .bind(feature)
        .bind(route)
        .bind(route)
        .bind(method)
        .bind(method)
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
        let rows = sqlx::query(
            r"
            SELECT date(ts_utc, ?) as date,
                   feature,
                   COUNT(1) as count,
                   COUNT(DISTINCT user_hash) as unique_users
            FROM events
            WHERE feature IS NOT NULL
              AND ts_utc BETWEEN ? AND ?
              AND (? IS NULL OR feature = ?)
            GROUP BY date, feature
            ORDER BY date ASC
        ",
        )
        .bind(modifier)
        .bind(start_utc)
        .bind(end_utc)
        .bind(feature)
        .bind(feature)
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
        let rows = sqlx::query(
            r"
            SELECT feature,
                   COUNT(1) as count,
                   COUNT(DISTINCT user_hash) as unique_users
            FROM events
            WHERE feature IS NOT NULL
              AND ts_utc BETWEEN ? AND ?
              AND (? IS NULL OR feature = ?)
            GROUP BY feature
        ",
        )
        .bind(start_utc)
        .bind(end_utc)
        .bind(feature)
        .bind(feature)
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
        let rows = sqlx::query(
            r"
            SELECT date(ts_utc, ?) as date,
                   COUNT(DISTINCT user_hash) as active_users,
                   COUNT(DISTINCT client_ip_hash) as active_ips
            FROM events
            WHERE ts_utc BETWEEN ? AND ?
            GROUP BY date
            ORDER BY date ASC
        ",
        )
        .bind(modifier)
        .bind(start_utc)
        .bind(end_utc)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Internal(format!("daily dau with offset: {e}")))?;

        let mut out = Vec::with_capacity(rows.len());
        for r in rows {
            out.push(DailyDauDateRow {
                date: r.get::<String, _>("date"),
                active_users: r.get::<i64, _>("active_users"),
                active_ips: r.get::<i64, _>("active_ips"),
            });
        }
        Ok(out)
    }

    pub async fn query_daily_dau_slice(
        &self,
        start_utc: &str,
        end_utc: &str,
    ) -> Result<(i64, i64), AppError> {
        let r = sqlx::query(
            r"
            SELECT COUNT(DISTINCT user_hash) as active_users,
                   COUNT(DISTINCT client_ip_hash) as active_ips
            FROM events
            WHERE ts_utc BETWEEN ? AND ?
        ",
        )
        .bind(start_utc)
        .bind(end_utc)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| AppError::Internal(format!("daily dau slice: {e}")))?;
        Ok((
            r.get::<i64, _>("active_users"),
            r.get::<i64, _>("active_ips"),
        ))
    }

    pub async fn query_latency_agg_with_offset(
        &self,
        modifier: &str,
        start_utc: &str,
        end_utc: &str,
        feature: Option<&str>,
        route: Option<&str>,
        method: Option<&str>,
    ) -> Result<Vec<LatencyAggBucketRow>, AppError> {
        let rows = sqlx::query(
            r"
            SELECT date(ts_utc, ?) as bucket,
                   feature,
                   route,
                   method,
                   COUNT(1) as count,
                   MIN(duration_ms) as min_ms,
                   AVG(duration_ms) as avg_ms,
                   MAX(duration_ms) as max_ms
            FROM events
            WHERE route IS NOT NULL
              AND duration_ms IS NOT NULL
              AND ts_utc BETWEEN ? AND ?
              AND (? IS NULL OR feature = ?)
              AND (? IS NULL OR route = ?)
              AND (? IS NULL OR method = ?)
            GROUP BY bucket, feature, route, method
            ORDER BY bucket ASC, route ASC, method ASC
        ",
        )
        .bind(modifier)
        .bind(start_utc)
        .bind(end_utc)
        .bind(feature)
        .bind(feature)
        .bind(route)
        .bind(route)
        .bind(method)
        .bind(method)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Internal(format!("latency agg with offset: {e}")))?;

        let mut out = Vec::with_capacity(rows.len());
        for r in rows {
            out.push(LatencyAggBucketRow {
                bucket: r.get::<String, _>("bucket"),
                feature: r.try_get::<String, _>("feature").ok(),
                route: r.try_get::<String, _>("route").ok(),
                method: r.try_get::<String, _>("method").ok(),
                count: r.get::<i64, _>("count"),
                min_ms: r.try_get::<i64, _>("min_ms").ok(),
                avg_ms: r.try_get::<f64, _>("avg_ms").ok(),
                max_ms: r.try_get::<i64, _>("max_ms").ok(),
            });
        }
        Ok(out)
    }

    pub async fn query_latency_agg_slice(
        &self,
        start_utc: &str,
        end_utc: &str,
        feature: Option<&str>,
        route: Option<&str>,
        method: Option<&str>,
    ) -> Result<Vec<LatencyAggSliceRow>, AppError> {
        let rows = sqlx::query(
            r"
            SELECT feature,
                   route,
                   method,
                   COUNT(1) as count,
                   MIN(duration_ms) as min_ms,
                   AVG(duration_ms) as avg_ms,
                   MAX(duration_ms) as max_ms
            FROM events
            WHERE route IS NOT NULL
              AND duration_ms IS NOT NULL
              AND ts_utc BETWEEN ? AND ?
              AND (? IS NULL OR feature = ?)
              AND (? IS NULL OR route = ?)
              AND (? IS NULL OR method = ?)
            GROUP BY feature, route, method
            ORDER BY route ASC, method ASC
        ",
        )
        .bind(start_utc)
        .bind(end_utc)
        .bind(feature)
        .bind(feature)
        .bind(route)
        .bind(route)
        .bind(method)
        .bind(method)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Internal(format!("latency agg slice: {e}")))?;

        let mut out = Vec::with_capacity(rows.len());
        for r in rows {
            out.push(LatencyAggSliceRow {
                feature: r.try_get::<String, _>("feature").ok(),
                route: r.try_get::<String, _>("route").ok(),
                method: r.try_get::<String, _>("method").ok(),
                count: r.get::<i64, _>("count"),
                min_ms: r.try_get::<i64, _>("min_ms").ok(),
                avg_ms: r.try_get::<f64, _>("avg_ms").ok(),
                max_ms: r.try_get::<i64, _>("max_ms").ok(),
            });
        }
        Ok(out)
    }

    pub async fn query_daily_http_routes_with_offset(
        &self,
        modifier: &str,
        start_utc: &str,
        end_utc: &str,
        route: Option<&str>,
        method: Option<&str>,
        top_per_day: i64,
    ) -> Result<Vec<DailyHttpRouteMetricRow>, AppError> {
        let rows = if top_per_day > 0 {
            let mut qb = QueryBuilder::<Sqlite>::new(
                r"
                SELECT date, route, method, total, errors, client_errors, server_errors
                FROM (
                    SELECT date(ts_utc, 
                ",
            );
            qb.push_bind(modifier)
                .push(
                    r") as date,
                           route,
                           method,
                           COUNT(1) as total,
                           COALESCE(SUM(CASE WHEN status >= 400 THEN 1 ELSE 0 END), 0) as errors,
                           COALESCE(SUM(CASE WHEN status BETWEEN 400 AND 499 THEN 1 ELSE 0 END), 0) as client_errors,
                           COALESCE(SUM(CASE WHEN status >= 500 THEN 1 ELSE 0 END), 0) as server_errors,
                           ROW_NUMBER() OVER (
                               PARTITION BY date(ts_utc, ",
                )
                .push_bind(modifier)
                .push(
                    r")
                               ORDER BY
                                   COALESCE(SUM(CASE WHEN status >= 400 THEN 1 ELSE 0 END), 0) DESC,
                                   COUNT(1) DESC,
                                   route ASC,
                                   method ASC
                           ) as rn
                    FROM events
                    WHERE route IS NOT NULL
                      AND status IS NOT NULL
                      AND ts_utc BETWEEN ",
                )
                .push_bind(start_utc.to_string())
                .push(" AND ")
                .push_bind(end_utc.to_string());

            if let Some(route) = route {
                qb.push(" AND route = ").push_bind(route.to_string());
            }
            if let Some(method) = method {
                qb.push(" AND method = ").push_bind(method.to_string());
            }

            qb.push(
                r"
                    GROUP BY date(ts_utc, ",
            )
            .push_bind(modifier)
            .push(
                r"), route, method
                ) ranked
                WHERE rn <= ",
            )
            .push_bind(top_per_day)
            .push(" ORDER BY date ASC, errors DESC, total DESC, route ASC, method ASC");
            qb.build()
                .fetch_all(&self.pool)
                .await
                .map_err(|e| AppError::Internal(format!("daily http routes with offset: {e}")))?
        } else {
            let mut qb = QueryBuilder::<Sqlite>::new(
                r"
                SELECT date(ts_utc, ",
            );
            qb.push_bind(modifier)
                .push(
                    r") as date,
                       route,
                       method,
                       COUNT(1) as total,
                       COALESCE(SUM(CASE WHEN status >= 400 THEN 1 ELSE 0 END), 0) as errors,
                       COALESCE(SUM(CASE WHEN status BETWEEN 400 AND 499 THEN 1 ELSE 0 END), 0) as client_errors,
                       COALESCE(SUM(CASE WHEN status >= 500 THEN 1 ELSE 0 END), 0) as server_errors
                FROM events
                WHERE route IS NOT NULL
                  AND status IS NOT NULL
                  AND ts_utc BETWEEN ",
                )
                .push_bind(start_utc.to_string())
                .push(" AND ")
                .push_bind(end_utc.to_string());

            if let Some(route) = route {
                qb.push(" AND route = ").push_bind(route.to_string());
            }
            if let Some(method) = method {
                qb.push(" AND method = ").push_bind(method.to_string());
            }

            qb.push(
                r"
                GROUP BY date(ts_utc, ",
            )
            .push_bind(modifier)
            .push(
                r"), route, method
                ORDER BY date ASC
            ",
            );
            qb.build()
                .fetch_all(&self.pool)
                .await
                .map_err(|e| AppError::Internal(format!("daily http routes with offset: {e}")))?
        };

        let mut out = Vec::with_capacity(rows.len());
        for r in rows {
            out.push(DailyHttpRouteMetricRow {
                date: r.get::<String, _>("date"),
                route: r.get::<String, _>("route"),
                method: r.get::<String, _>("method"),
                total: r.get::<i64, _>("total"),
                errors: r.get::<i64, _>("errors"),
                client_errors: r.get::<i64, _>("client_errors"),
                server_errors: r.get::<i64, _>("server_errors"),
            });
        }
        Ok(out)
    }

    pub async fn query_daily_http_totals_with_offset(
        &self,
        modifier: &str,
        start_utc: &str,
        end_utc: &str,
        route: Option<&str>,
        method: Option<&str>,
    ) -> Result<Vec<DailyHttpTotalMetricRow>, AppError> {
        let mut qb = QueryBuilder::<Sqlite>::new("SELECT date(ts_utc, ");
        qb.push_bind(modifier).push(
            ") as date, COUNT(1) as total, COALESCE(SUM(CASE WHEN status >= 400 THEN 1 ELSE 0 END), 0) as errors, COALESCE(SUM(CASE WHEN status BETWEEN 400 AND 499 THEN 1 ELSE 0 END), 0) as client_errors, COALESCE(SUM(CASE WHEN status >= 500 THEN 1 ELSE 0 END), 0) as server_errors FROM events WHERE route IS NOT NULL AND status IS NOT NULL AND ts_utc BETWEEN ",
        )
        .push_bind(start_utc.to_string())
        .push(" AND ")
        .push_bind(end_utc.to_string());

        if let Some(route) = route {
            qb.push(" AND route = ").push_bind(route.to_string());
        }
        if let Some(method) = method {
            qb.push(" AND method = ").push_bind(method.to_string());
        }

        qb.push(" GROUP BY date(ts_utc, ")
            .push_bind(modifier)
            .push(") ORDER BY date ASC");

        let rows = qb
            .build()
            .fetch_all(&self.pool)
            .await
            .map_err(|e| AppError::Internal(format!("daily http totals with offset: {e}")))?;

        let mut out = Vec::with_capacity(rows.len());
        for r in rows {
            out.push(DailyHttpTotalMetricRow {
                date: r.get::<String, _>("date"),
                total: r.get::<i64, _>("total"),
                errors: r.get::<i64, _>("errors"),
                client_errors: r.get::<i64, _>("client_errors"),
                server_errors: r.get::<i64, _>("server_errors"),
            });
        }
        Ok(out)
    }

    pub async fn query_daily_http_route_slice(
        &self,
        start_utc: &str,
        end_utc: &str,
        route: Option<&str>,
        method: Option<&str>,
    ) -> Result<Vec<DailyHttpRouteMetricSliceRow>, AppError> {
        let rows = sqlx::query(
            r"
            SELECT route,
                   method,
                   COUNT(1) as total,
                   COALESCE(SUM(CASE WHEN status >= 400 THEN 1 ELSE 0 END), 0) as errors,
                   COALESCE(SUM(CASE WHEN status BETWEEN 400 AND 499 THEN 1 ELSE 0 END), 0) as client_errors,
                   COALESCE(SUM(CASE WHEN status >= 500 THEN 1 ELSE 0 END), 0) as server_errors
            FROM events
            WHERE route IS NOT NULL
              AND status IS NOT NULL
              AND ts_utc BETWEEN ? AND ?
              AND (? IS NULL OR route = ?)
              AND (? IS NULL OR method = ?)
            GROUP BY route, method
        ",
        )
        .bind(start_utc)
        .bind(end_utc)
        .bind(route)
        .bind(route)
        .bind(method)
        .bind(method)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Internal(format!("daily http route slice: {e}")))?;

        let mut out = Vec::with_capacity(rows.len());
        for r in rows {
            out.push(DailyHttpRouteMetricSliceRow {
                route: r.get::<String, _>("route"),
                method: r.get::<String, _>("method"),
                total: r.get::<i64, _>("total"),
                errors: r.get::<i64, _>("errors"),
                client_errors: r.get::<i64, _>("client_errors"),
                server_errors: r.get::<i64, _>("server_errors"),
            });
        }
        Ok(out)
    }

    pub async fn query_stats_summary_data(
        &self,
        start_utc: Option<&str>,
        end_utc: Option<&str>,
        feature: Option<&str>,
        include: SummaryIncludeFlags,
        top: i64,
        want_meta: bool,
    ) -> Result<StatsSummaryData, AppError> {
        let overall_fut = async {
            let mut overall_qb = QueryBuilder::<Sqlite>::new(
                "SELECT MIN(ts_utc) as min_ts, MAX(ts_utc) as max_ts FROM events WHERE 1=1",
            );
            push_stats_ts_range_filters(&mut overall_qb, start_utc, end_utc);
            let row = overall_qb
                .build()
                .fetch_one(&self.pool)
                .await
                .map_err(|e| AppError::Internal(format!("summary overall: {e}")))?;
            Ok::<(Option<String>, Option<String>), AppError>((
                row.try_get::<String, _>("min_ts").ok(),
                row.try_get::<String, _>("max_ts").ok(),
            ))
        };

        let features_fut = async {
            let mut features_qb = QueryBuilder::<Sqlite>::new(
                "SELECT feature, COUNT(1) as cnt, MAX(ts_utc) as last_ts FROM events WHERE feature IS NOT NULL",
            );
            push_stats_ts_range_filters(&mut features_qb, start_utc, end_utc);
            push_stats_feature_filter(&mut features_qb, feature);
            features_qb.push(" GROUP BY feature");
            let feat_rows = features_qb
                .build()
                .fetch_all(&self.pool)
                .await
                .map_err(|e| AppError::Internal(format!("summary features: {e}")))?;

            let mut features = Vec::with_capacity(feat_rows.len());
            for r in feat_rows {
                let f: String = r.try_get("feature").unwrap_or_else(|_| String::new());
                let c: i64 = r.try_get("cnt").unwrap_or(0);
                let last_ts: Option<String> = r.try_get("last_ts").ok();
                features.push(SummaryFeatureRow {
                    feature: f,
                    count: c,
                    last_ts,
                });
            }
            Ok::<Vec<SummaryFeatureRow>, AppError>(features)
        };

        let users_fut = async {
            let mut users_qb = QueryBuilder::<Sqlite>::new(
                "SELECT COUNT(DISTINCT user_hash) as total FROM events WHERE user_hash IS NOT NULL",
            );
            push_stats_ts_range_filters(&mut users_qb, start_utc, end_utc);
            push_stats_feature_filter(&mut users_qb, feature);
            let row = users_qb
                .build()
                .fetch_one(&self.pool)
                .await
                .map_err(|e| AppError::Internal(format!("summary users: {e}")))?;
            Ok::<i64, AppError>(row.try_get("total").unwrap_or(0))
        };

        let ((first_event_ts, last_event_ts), features, unique_users_total) =
            tokio::try_join!(overall_fut, features_fut, users_fut)?;

        let by_kind = if include.user_kinds {
            #[derive(serde::Deserialize)]
            struct UserKindFromExtra {
                user_kind: Option<String>,
            }

            use std::collections::{HashMap, HashSet};
            let mut by_kind_qb = QueryBuilder::<Sqlite>::new(
                "SELECT user_hash, extra_json FROM events WHERE user_hash IS NOT NULL AND extra_json IS NOT NULL",
            );
            push_stats_ts_range_filters(&mut by_kind_qb, start_utc, end_utc);
            push_stats_feature_filter(&mut by_kind_qb, feature);

            let mut stream = by_kind_qb.build().fetch(&self.pool);
            let mut uniq: HashSet<(String, String)> = HashSet::new();
            while let Some(r) = stream
                .try_next()
                .await
                .map_err(|e| AppError::Internal(format!("summary by_kind: {e}")))?
            {
                let uh: String = match r.try_get("user_hash") {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                let ej: String = match r.try_get("extra_json") {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                if !ej.contains("user_kind") {
                    continue;
                }
                if let Ok(extra) = serde_json::from_str::<UserKindFromExtra>(&ej)
                    && let Some(kind) = extra.user_kind
                    && !kind.is_empty()
                {
                    uniq.insert((uh, kind));
                }
            }

            let mut by_kind_map: HashMap<String, i64> = HashMap::new();
            for (_, k) in uniq {
                *by_kind_map.entry(k).or_insert(0) += 1;
            }
            let mut by_kind: Vec<(String, i64)> = by_kind_map.into_iter().collect();
            by_kind.sort_by_key(|b| std::cmp::Reverse(b.1));
            by_kind
        } else {
            Vec::new()
        };

        let events_total = if want_meta || include.any() {
            let mut events_total_qb =
                QueryBuilder::<Sqlite>::new("SELECT COUNT(1) as total FROM events WHERE 1=1");
            push_stats_ts_range_filters(&mut events_total_qb, start_utc, end_utc);
            let row = events_total_qb
                .build()
                .fetch_one(&self.pool)
                .await
                .map_err(|e| AppError::Internal(format!("summary events_total: {e}")))?;
            Some(row.try_get::<i64, _>("total").unwrap_or(0))
        } else {
            None
        };

        let (http_total, http_errors) = if include.any_http() {
            let mut http_total_qb = QueryBuilder::<Sqlite>::new(
                "SELECT COUNT(1) as total, COALESCE(SUM(CASE WHEN status >= 400 THEN 1 ELSE 0 END), 0) as err FROM events WHERE route IS NOT NULL",
            );
            push_stats_ts_range_filters(&mut http_total_qb, start_utc, end_utc);
            let row = http_total_qb
                .build()
                .fetch_one(&self.pool)
                .await
                .map_err(|e| AppError::Internal(format!("summary http_total: {e}")))?;
            (
                Some(row.try_get::<i64, _>("total").unwrap_or(0)),
                Some(row.try_get::<i64, _>("err").unwrap_or(0)),
            )
        } else {
            (None, None)
        };

        let routes = if include.routes {
            let mut routes_qb = QueryBuilder::<Sqlite>::new(
                "SELECT route, COUNT(1) as cnt, COALESCE(SUM(CASE WHEN status >= 400 THEN 1 ELSE 0 END), 0) as err_cnt, MAX(ts_utc) as last_ts FROM events WHERE route IS NOT NULL",
            );
            push_stats_ts_range_filters(&mut routes_qb, start_utc, end_utc);
            routes_qb
                .push(" GROUP BY route ORDER BY cnt DESC LIMIT ")
                .push_bind(top);
            let rows = routes_qb
                .build()
                .fetch_all(&self.pool)
                .await
                .map_err(|e| AppError::Internal(format!("summary routes: {e}")))?;

            let mut out = Vec::with_capacity(rows.len());
            for r in rows {
                out.push(SummaryRouteRow {
                    route: r.try_get("route").unwrap_or_else(|_| String::new()),
                    count: r.try_get("cnt").unwrap_or(0),
                    err_count: r.try_get("err_cnt").unwrap_or(0),
                    last_ts: r.try_get("last_ts").ok(),
                });
            }
            Some(out)
        } else {
            None
        };

        let methods = if include.methods {
            let mut methods_qb = QueryBuilder::<Sqlite>::new(
                "SELECT method, COUNT(1) as cnt FROM events WHERE route IS NOT NULL AND method IS NOT NULL",
            );
            push_stats_ts_range_filters(&mut methods_qb, start_utc, end_utc);
            methods_qb
                .push(" GROUP BY method ORDER BY cnt DESC LIMIT ")
                .push_bind(top);
            let rows = methods_qb
                .build()
                .fetch_all(&self.pool)
                .await
                .map_err(|e| AppError::Internal(format!("summary methods: {e}")))?;

            let mut out = Vec::with_capacity(rows.len());
            for r in rows {
                out.push(SummaryMethodRow {
                    method: r.try_get("method").unwrap_or_else(|_| String::new()),
                    count: r.try_get("cnt").unwrap_or(0),
                });
            }
            Some(out)
        } else {
            None
        };

        let status_codes = if include.status_codes {
            let mut status_codes_qb = QueryBuilder::<Sqlite>::new(
                "SELECT status, COUNT(1) as cnt FROM events WHERE route IS NOT NULL AND status IS NOT NULL",
            );
            push_stats_ts_range_filters(&mut status_codes_qb, start_utc, end_utc);
            status_codes_qb
                .push(" GROUP BY status ORDER BY cnt DESC LIMIT ")
                .push_bind(top);
            let rows = status_codes_qb
                .build()
                .fetch_all(&self.pool)
                .await
                .map_err(|e| AppError::Internal(format!("summary status: {e}")))?;

            let mut out = Vec::with_capacity(rows.len());
            for r in rows {
                out.push(SummaryStatusCodeRow {
                    status: r.try_get("status").unwrap_or(0),
                    count: r.try_get("cnt").unwrap_or(0),
                });
            }
            Some(out)
        } else {
            None
        };

        let instances = if include.instances {
            let mut instances_qb = QueryBuilder::<Sqlite>::new(
                "SELECT instance, COUNT(1) as cnt, MAX(ts_utc) as last_ts FROM events WHERE instance IS NOT NULL",
            );
            push_stats_ts_range_filters(&mut instances_qb, start_utc, end_utc);
            instances_qb
                .push(" GROUP BY instance ORDER BY cnt DESC LIMIT ")
                .push_bind(top);
            let rows = instances_qb
                .build()
                .fetch_all(&self.pool)
                .await
                .map_err(|e| AppError::Internal(format!("summary instances: {e}")))?;

            let mut out = Vec::with_capacity(rows.len());
            for r in rows {
                out.push(SummaryInstanceRow {
                    instance: r.try_get("instance").unwrap_or_else(|_| String::new()),
                    count: r.try_get("cnt").unwrap_or(0),
                    last_ts: r.try_get("last_ts").ok(),
                });
            }
            Some(out)
        } else {
            None
        };

        let actions = if include.actions {
            let mut actions_qb = QueryBuilder::<Sqlite>::new(
                "SELECT feature, action, COUNT(1) as cnt, MAX(ts_utc) as last_ts FROM events WHERE feature IS NOT NULL AND action IS NOT NULL",
            );
            push_stats_ts_range_filters(&mut actions_qb, start_utc, end_utc);
            push_stats_feature_filter(&mut actions_qb, feature);
            actions_qb
                .push(" GROUP BY feature, action ORDER BY cnt DESC LIMIT ")
                .push_bind(top);
            let rows = actions_qb
                .build()
                .fetch_all(&self.pool)
                .await
                .map_err(|e| AppError::Internal(format!("summary actions: {e}")))?;

            let mut out = Vec::with_capacity(rows.len());
            for r in rows {
                out.push(SummaryActionRow {
                    feature: r.try_get("feature").unwrap_or_else(|_| String::new()),
                    action: r.try_get("action").unwrap_or_else(|_| String::new()),
                    count: r.try_get("cnt").unwrap_or(0),
                    last_ts: r.try_get("last_ts").ok(),
                });
            }
            Some(out)
        } else {
            None
        };

        let latency = if include.latency {
            let mut latency_base_qb = QueryBuilder::<Sqlite>::new(
                "SELECT COUNT(duration_ms) as n, AVG(duration_ms) as avg, MAX(duration_ms) as max FROM events WHERE route IS NOT NULL AND duration_ms IS NOT NULL",
            );
            push_stats_ts_range_filters(&mut latency_base_qb, start_utc, end_utc);
            let row = latency_base_qb
                .build()
                .fetch_one(&self.pool)
                .await
                .map_err(|e| AppError::Internal(format!("summary latency base: {e}")))?;

            let n: i64 = row.try_get("n").unwrap_or(0);
            let avg_ms: Option<f64> = row.try_get("avg").ok();
            let max_ms: Option<i64> = row.try_get("max").ok();

            let (p50_ms, p95_ms) = if n > 0 {
                let span = n - 1;
                let p50_idx = (span.saturating_mul(50) + 50) / 100;
                let p95_idx = (span.saturating_mul(95) + 50) / 100;

                let mut p50_qb = QueryBuilder::<Sqlite>::new(
                    "SELECT duration_ms as v FROM events WHERE route IS NOT NULL AND duration_ms IS NOT NULL",
                );
                push_stats_ts_range_filters(&mut p50_qb, start_utc, end_utc);
                p50_qb
                    .push(" ORDER BY duration_ms ASC LIMIT 1 OFFSET ")
                    .push_bind(p50_idx);
                let p50 = p50_qb
                    .build()
                    .fetch_optional(&self.pool)
                    .await
                    .map_err(|e| AppError::Internal(format!("summary latency pick: {e}")))?
                    .and_then(|row| row.try_get::<i64, _>("v").ok());

                let mut p95_qb = QueryBuilder::<Sqlite>::new(
                    "SELECT duration_ms as v FROM events WHERE route IS NOT NULL AND duration_ms IS NOT NULL",
                );
                push_stats_ts_range_filters(&mut p95_qb, start_utc, end_utc);
                p95_qb
                    .push(" ORDER BY duration_ms ASC LIMIT 1 OFFSET ")
                    .push_bind(p95_idx);
                let p95 = p95_qb
                    .build()
                    .fetch_optional(&self.pool)
                    .await
                    .map_err(|e| AppError::Internal(format!("summary latency pick: {e}")))?
                    .and_then(|row| row.try_get::<i64, _>("v").ok());

                (p50, p95)
            } else {
                (None, None)
            };

            Some(SummaryLatencyData {
                sample_count: n,
                avg_ms,
                p50_ms,
                p95_ms,
                max_ms,
            })
        } else {
            None
        };

        let unique_ips = if include.unique_ips {
            let mut unique_ips_qb = QueryBuilder::<Sqlite>::new(
                "SELECT COUNT(DISTINCT client_ip_hash) as cnt FROM events WHERE route IS NOT NULL AND client_ip_hash IS NOT NULL",
            );
            push_stats_ts_range_filters(&mut unique_ips_qb, start_utc, end_utc);
            let row = unique_ips_qb
                .build()
                .fetch_one(&self.pool)
                .await
                .map_err(|e| AppError::Internal(format!("summary unique_ips: {e}")))?;
            Some(row.try_get::<i64, _>("cnt").unwrap_or(0))
        } else {
            None
        };

        Ok(StatsSummaryData {
            first_event_ts,
            last_event_ts,
            features,
            unique_users_total,
            by_kind,
            events_total,
            http_total,
            http_errors,
            routes,
            methods,
            status_codes,
            instances,
            actions,
            latency,
            unique_ips,
        })
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

        let sql = r"
            SELECT substr(ts_utc, 1, 10) as date,
                   feature,
                   route,
                   method,
                   COUNT(1) as count,
                   SUM(CASE WHEN status >= 400 THEN 1 ELSE 0 END) as err_count
            FROM events
            WHERE ts_utc BETWEEN ? AND ?
              AND (? IS NULL OR feature = ?)
              AND (? IS NULL OR route = ?)
              AND (? IS NULL OR method = ?)
            GROUP BY date, feature, route, method
            ORDER BY date ASC
        ";
        let rows = sqlx::query(sql)
            .bind(&start_s)
            .bind(&end_s)
            .bind(feature.as_ref())
            .bind(feature.as_ref())
            .bind(route.as_ref())
            .bind(route.as_ref())
            .bind(method.as_ref())
            .bind(method.as_ref())
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
}
