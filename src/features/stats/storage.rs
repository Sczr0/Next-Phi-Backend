use std::{
    path::Path,
    sync::atomic::{AtomicI64, Ordering},
};

use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, Utc};
use sqlx::{
    ConnectOptions, QueryBuilder, Row, Sqlite, SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteRow},
};

use crate::error::AppError;

use super::models::{DailyAggRow, EventInsert};

const SESSION_CLEANUP_INTERVAL_SECS: i64 = 300;
static LAST_SESSION_CLEANUP_TS: AtomicI64 = AtomicI64::new(0);

/// 保存提交入库参数，减少函数参数数量
pub struct SubmissionRecord<'a> {
    pub user_hash: &'a str,
    pub total_rks: f64,
    pub rks_jump: f64,
    pub route: &'a str,
    pub client_ip_hash: Option<&'a str>,
    pub details_json: Option<&'a str>,
    pub suspicion_score: f64,
    pub now_rfc3339: &'a str,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ArchiveEventRow {
    pub ts_utc: String,
    pub route: Option<String>,
    pub feature: Option<String>,
    pub action: Option<String>,
    pub method: Option<String>,
    pub status: Option<i64>,
    pub duration_ms: Option<i64>,
    pub user_hash: Option<String>,
    pub client_ip_hash: Option<String>,
    pub instance: Option<String>,
    pub extra_json: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DailyAggSliceRow {
    pub feature: Option<String>,
    pub route: Option<String>,
    pub method: Option<String>,
    pub count: i64,
    pub err_count: i64,
}

#[derive(Debug, Clone)]
pub struct DailyFeatureUsageDateRow {
    pub date: String,
    pub feature: String,
    pub count: i64,
    pub unique_users: i64,
}

#[derive(Debug, Clone)]
pub struct DailyFeatureUsageSliceRow {
    pub feature: String,
    pub count: i64,
    pub unique_users: i64,
}

#[derive(Debug, Clone)]
pub struct DailyDauDateRow {
    pub date: String,
    pub active_users: i64,
    pub active_ips: i64,
}

#[derive(Debug, Clone)]
pub struct LatencyAggBucketRow {
    pub bucket: String,
    pub feature: Option<String>,
    pub route: Option<String>,
    pub method: Option<String>,
    pub count: i64,
    pub min_ms: Option<i64>,
    pub avg_ms: Option<f64>,
    pub max_ms: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct LatencyAggSliceRow {
    pub feature: Option<String>,
    pub route: Option<String>,
    pub method: Option<String>,
    pub count: i64,
    pub min_ms: Option<i64>,
    pub avg_ms: Option<f64>,
    pub max_ms: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct DailyHttpRouteMetricRow {
    pub date: String,
    pub route: String,
    pub method: String,
    pub total: i64,
    pub errors: i64,
    pub client_errors: i64,
    pub server_errors: i64,
}

#[derive(Debug, Clone)]
pub struct DailyHttpRouteMetricSliceRow {
    pub route: String,
    pub method: String,
    pub total: i64,
    pub errors: i64,
    pub client_errors: i64,
    pub server_errors: i64,
}

#[derive(Debug, Clone)]
pub struct DailyHttpTotalMetricRow {
    pub date: String,
    pub total: i64,
    pub errors: i64,
    pub client_errors: i64,
    pub server_errors: i64,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SummaryIncludeFlags {
    pub routes: bool,
    pub methods: bool,
    pub status_codes: bool,
    pub instances: bool,
    pub actions: bool,
    pub latency: bool,
    pub unique_ips: bool,
    pub user_kinds: bool,
}

impl SummaryIncludeFlags {
    pub fn any(self) -> bool {
        self.routes
            || self.methods
            || self.status_codes
            || self.instances
            || self.actions
            || self.latency
            || self.unique_ips
            || self.user_kinds
    }

    pub fn any_http(self) -> bool {
        self.routes || self.methods || self.status_codes || self.latency || self.unique_ips
    }
}

#[derive(Debug, Clone)]
pub struct SummaryFeatureRow {
    pub feature: String,
    pub count: i64,
    pub last_ts: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SummaryRouteRow {
    pub route: String,
    pub count: i64,
    pub err_count: i64,
    pub last_ts: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SummaryMethodRow {
    pub method: String,
    pub count: i64,
}

#[derive(Debug, Clone)]
pub struct SummaryStatusCodeRow {
    pub status: i64,
    pub count: i64,
}

#[derive(Debug, Clone)]
pub struct SummaryInstanceRow {
    pub instance: String,
    pub count: i64,
    pub last_ts: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SummaryActionRow {
    pub feature: String,
    pub action: String,
    pub count: i64,
    pub last_ts: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SummaryLatencyData {
    pub sample_count: i64,
    pub avg_ms: Option<f64>,
    pub p50_ms: Option<i64>,
    pub p95_ms: Option<i64>,
    pub max_ms: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct StatsSummaryData {
    pub first_event_ts: Option<String>,
    pub last_event_ts: Option<String>,
    pub features: Vec<SummaryFeatureRow>,
    pub unique_users_total: i64,
    pub by_kind: Vec<(String, i64)>,
    pub events_total: Option<i64>,
    pub http_total: Option<i64>,
    pub http_errors: Option<i64>,
    pub routes: Option<Vec<SummaryRouteRow>>,
    pub methods: Option<Vec<SummaryMethodRow>>,
    pub status_codes: Option<Vec<SummaryStatusCodeRow>>,
    pub instances: Option<Vec<SummaryInstanceRow>>,
    pub actions: Option<Vec<SummaryActionRow>>,
    pub latency: Option<SummaryLatencyData>,
    pub unique_ips: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct RksHistoryEntry {
    pub rks: f64,
    pub rks_jump: f64,
    pub created_at: String,
}

#[derive(Clone)]
pub struct StatsStorage {
    pub pool: SqlitePool,
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
    pub async fn connect_sqlite(path: &str, wal: bool) -> Result<Self, AppError> {
        let opt = SqliteConnectOptions::new()
            .filename(Path::new(path))
            .create_if_missing(true)
            .log_statements(tracing::log::LevelFilter::Off);
        let pool = SqlitePool::connect_with(opt)
            .await
            .map_err(|e| AppError::Internal(format!("sqlite connect: {e}")))?;
        if wal {
            sqlx::query("PRAGMA journal_mode=WAL;")
                .execute(&pool)
                .await
                .ok();
        }
        sqlx::query("PRAGMA synchronous=NORMAL;")
            .execute(&pool)
            .await
            .ok();
        sqlx::query("PRAGMA foreign_keys=ON;")
            .execute(&pool)
            .await
            .ok();
        Ok(Self { pool })
    }

    pub async fn init_schema(&self) -> Result<(), AppError> {
        let ddl = r#"
        CREATE TABLE IF NOT EXISTS events (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            ts_utc TEXT NOT NULL,
            route TEXT,
            feature TEXT,
            action TEXT,
            method TEXT,
            status INTEGER,
            duration_ms INTEGER,
            user_hash TEXT,
            client_ip_hash TEXT,
            instance TEXT,
            extra_json TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_events_ts ON events(ts_utc);
        CREATE INDEX IF NOT EXISTS idx_events_feature_ts ON events(feature, ts_utc);
        CREATE INDEX IF NOT EXISTS idx_events_route_ts ON events(route, ts_utc);
        CREATE INDEX IF NOT EXISTS idx_events_ts_user_hash ON events(ts_utc, user_hash);
        CREATE INDEX IF NOT EXISTS idx_events_ts_client_ip_hash ON events(ts_utc, client_ip_hash);
        CREATE INDEX IF NOT EXISTS idx_events_http_agg ON events(ts_utc, route, method, status);
        CREATE INDEX IF NOT EXISTS idx_events_feature_action_ts ON events(feature, action, ts_utc);
        CREATE INDEX IF NOT EXISTS idx_events_instance_ts ON events(instance, ts_utc);
        CREATE INDEX IF NOT EXISTS idx_events_latency_route_duration_ts ON events(route, duration_ms, ts_utc)
            WHERE route IS NOT NULL AND duration_ms IS NOT NULL;

        CREATE TABLE IF NOT EXISTS daily_agg (
            date TEXT NOT NULL,
            feature TEXT,
            route TEXT,
            method TEXT,
            count INTEGER NOT NULL,
            err_count INTEGER NOT NULL,
            PRIMARY KEY(date, feature, route, method)
        );

        -- Leaderboard tables (no images, textual details only)
        CREATE TABLE IF NOT EXISTS leaderboard_rks (
            user_hash TEXT PRIMARY KEY,
            total_rks REAL NOT NULL,
            user_kind TEXT,
            suspicion_score REAL NOT NULL DEFAULT 0.0,
            is_hidden INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_lb_rks_order ON leaderboard_rks(total_rks DESC, updated_at ASC, user_hash ASC);

        CREATE TABLE IF NOT EXISTS user_profile (
            user_hash TEXT PRIMARY KEY,
            alias TEXT UNIQUE COLLATE NOCASE,
            is_public INTEGER NOT NULL DEFAULT 0,
            show_rks_composition INTEGER NOT NULL DEFAULT 1,
            show_best_top3 INTEGER NOT NULL DEFAULT 1,
            show_ap_top3 INTEGER NOT NULL DEFAULT 1,
            user_kind TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_profile_public ON user_profile(is_public);

        CREATE TABLE IF NOT EXISTS save_submissions (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            user_hash TEXT NOT NULL,
            total_rks REAL NOT NULL,
            acc_stats TEXT,
            rks_jump REAL,
            route TEXT,
            client_ip_hash TEXT,
            details_json TEXT,
            suspicion_score REAL NOT NULL DEFAULT 0.0,
            created_at TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_submissions_user ON save_submissions(user_hash, created_at DESC);

        CREATE TABLE IF NOT EXISTS leaderboard_details (
            user_hash TEXT PRIMARY KEY,
            rks_composition_json TEXT,
            best_top3_json TEXT,
            ap_top3_json TEXT,
            updated_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS session_token_blacklist (
            jti TEXT PRIMARY KEY,
            expires_at TEXT NOT NULL,
            created_at TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_session_blacklist_expires_at ON session_token_blacklist(expires_at);

        CREATE TABLE IF NOT EXISTS session_logout_gate (
            user_hash TEXT PRIMARY KEY,
            logout_before TEXT NOT NULL,
            expires_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_session_logout_gate_expires_at ON session_logout_gate(expires_at);

        CREATE TABLE IF NOT EXISTS user_moderation_state (
            user_hash TEXT PRIMARY KEY,
            status TEXT NOT NULL DEFAULT 'active',
            reason TEXT,
            updated_by TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            expires_at TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_user_moderation_status ON user_moderation_state(status, updated_at DESC);

        CREATE TABLE IF NOT EXISTS moderation_flags (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            user_hash TEXT NOT NULL,
            status TEXT NOT NULL,
            reason TEXT,
            severity INTEGER NOT NULL DEFAULT 0,
            created_by TEXT NOT NULL,
            created_at TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_moderation_flags_user_created ON moderation_flags(user_hash, created_at DESC);
        "#;
        sqlx::query(ddl)
            .execute(&self.pool)
            .await
            .map_err(|e| AppError::Internal(format!("init schema: {e}")))?;
        Ok(())
    }

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
                    .push_bind(e.status.map(|v| v as i64))
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
        Ok(res.rows_affected() as i64)
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
        sqlx::query_as::<_, ArchiveEventRow>(
            r#"SELECT ts_utc, route, feature, action, method, status, duration_ms, user_hash, client_ip_hash, instance, extra_json FROM events WHERE ts_utc BETWEEN ? AND ? ORDER BY ts_utc ASC"#,
        )
        .bind(start_rfc3339)
        .bind(end_rfc3339)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Internal(format!("archive query: {e}")))
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
            r#"
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
        "#,
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
            r#"
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
        "#,
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
            r#"
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
        "#,
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
            r#"
            SELECT feature,
                   COUNT(1) as count,
                   COUNT(DISTINCT user_hash) as unique_users
            FROM events
            WHERE feature IS NOT NULL
              AND ts_utc BETWEEN ? AND ?
              AND (? IS NULL OR feature = ?)
            GROUP BY feature
        "#,
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
            r#"
            SELECT date(ts_utc, ?) as date,
                   COUNT(DISTINCT user_hash) as active_users,
                   COUNT(DISTINCT client_ip_hash) as active_ips
            FROM events
            WHERE ts_utc BETWEEN ? AND ?
            GROUP BY date
            ORDER BY date ASC
        "#,
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
            r#"
            SELECT COUNT(DISTINCT user_hash) as active_users,
                   COUNT(DISTINCT client_ip_hash) as active_ips
            FROM events
            WHERE ts_utc BETWEEN ? AND ?
        "#,
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
            r#"
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
        "#,
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
            r#"
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
        "#,
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
                r#"
                SELECT date, route, method, total, errors, client_errors, server_errors
                FROM (
                    SELECT date(ts_utc, 
                "#,
            );
            qb.push_bind(modifier)
                .push(
                    r#") as date,
                           route,
                           method,
                           COUNT(1) as total,
                           COALESCE(SUM(CASE WHEN status >= 400 THEN 1 ELSE 0 END), 0) as errors,
                           COALESCE(SUM(CASE WHEN status BETWEEN 400 AND 499 THEN 1 ELSE 0 END), 0) as client_errors,
                           COALESCE(SUM(CASE WHEN status >= 500 THEN 1 ELSE 0 END), 0) as server_errors,
                           ROW_NUMBER() OVER (
                               PARTITION BY date(ts_utc, "#,
                )
                .push_bind(modifier)
                .push(
                    r#")
                               ORDER BY
                                   COALESCE(SUM(CASE WHEN status >= 400 THEN 1 ELSE 0 END), 0) DESC,
                                   COUNT(1) DESC,
                                   route ASC,
                                   method ASC
                           ) as rn
                    FROM events
                    WHERE route IS NOT NULL
                      AND status IS NOT NULL
                      AND ts_utc BETWEEN "#,
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
                r#"
                    GROUP BY date(ts_utc, "#,
            )
            .push_bind(modifier)
            .push(
                r#"), route, method
                ) ranked
                WHERE rn <= "#,
            )
            .push_bind(top_per_day)
            .push(" ORDER BY date ASC, errors DESC, total DESC, route ASC, method ASC");
            qb.build()
                .fetch_all(&self.pool)
                .await
                .map_err(|e| AppError::Internal(format!("daily http routes with offset: {e}")))?
        } else {
            let mut qb = QueryBuilder::<Sqlite>::new(
                r#"
                SELECT date(ts_utc, "#,
            );
            qb.push_bind(modifier)
                .push(
                    r#") as date,
                       route,
                       method,
                       COUNT(1) as total,
                       COALESCE(SUM(CASE WHEN status >= 400 THEN 1 ELSE 0 END), 0) as errors,
                       COALESCE(SUM(CASE WHEN status BETWEEN 400 AND 499 THEN 1 ELSE 0 END), 0) as client_errors,
                       COALESCE(SUM(CASE WHEN status >= 500 THEN 1 ELSE 0 END), 0) as server_errors
                FROM events
                WHERE route IS NOT NULL
                  AND status IS NOT NULL
                  AND ts_utc BETWEEN "#,
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
                r#"
                GROUP BY date(ts_utc, "#,
            )
            .push_bind(modifier)
            .push(
                r#"), route, method
                ORDER BY date ASC
            "#,
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
            r#"
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
        "#,
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
        let mut overall_qb = QueryBuilder::<Sqlite>::new(
            "SELECT MIN(ts_utc) as min_ts, MAX(ts_utc) as max_ts FROM events WHERE 1=1",
        );
        push_stats_ts_range_filters(&mut overall_qb, start_utc, end_utc);
        let row = overall_qb
            .build()
            .fetch_one(&self.pool)
            .await
            .map_err(|e| AppError::Internal(format!("summary overall: {e}")))?;
        let first_event_ts = row.try_get::<String, _>("min_ts").ok();
        let last_event_ts = row.try_get::<String, _>("max_ts").ok();

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
            let f: String = r.try_get("feature").unwrap_or_else(|_| "".into());
            let c: i64 = r.try_get("cnt").unwrap_or(0);
            let last_ts: Option<String> = r.try_get("last_ts").ok();
            features.push(SummaryFeatureRow {
                feature: f,
                count: c,
                last_ts,
            });
        }

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
        let unique_users_total: i64 = row.try_get("total").unwrap_or(0);

        let by_kind = if include.user_kinds {
            use std::collections::{HashMap, HashSet};
            const BATCH: i64 = 5000;
            let mut last_id: i64 = 0;
            let mut uniq: HashSet<(String, String)> = HashSet::new();
            loop {
                let mut by_kind_qb = QueryBuilder::<Sqlite>::new(
                    "SELECT id, user_hash, extra_json FROM events WHERE user_hash IS NOT NULL AND extra_json IS NOT NULL",
                );
                by_kind_qb.push(" AND id > ").push_bind(last_id);
                push_stats_ts_range_filters(&mut by_kind_qb, start_utc, end_utc);
                push_stats_feature_filter(&mut by_kind_qb, feature);
                by_kind_qb.push(" ORDER BY id ASC LIMIT ").push_bind(BATCH);
                let rows = by_kind_qb
                    .build()
                    .fetch_all(&self.pool)
                    .await
                    .map_err(|e| AppError::Internal(format!("summary by_kind: {e}")))?;
                if rows.is_empty() {
                    break;
                }
                let row_len = rows.len() as i64;
                for r in rows {
                    let id: i64 = match r.try_get("id") {
                        Ok(v) => v,
                        Err(_) => continue,
                    };
                    last_id = id.max(last_id);
                    let uh: String = match r.try_get("user_hash") {
                        Ok(v) => v,
                        Err(_) => continue,
                    };
                    let ej: String = match r.try_get("extra_json") {
                        Ok(v) => v,
                        Err(_) => continue,
                    };
                    if let Ok(val) = serde_json::from_str::<serde_json::Value>(&ej)
                        && let Some(kind) = val.get("user_kind").and_then(|v| v.as_str())
                    {
                        uniq.insert((uh, kind.to_string()));
                    }
                }
                if row_len < BATCH {
                    break;
                }
            }

            let mut by_kind_map: HashMap<String, i64> = HashMap::new();
            for (_, k) in uniq {
                *by_kind_map.entry(k).or_insert(0) += 1;
            }
            let mut by_kind: Vec<(String, i64)> = by_kind_map.into_iter().collect();
            by_kind.sort_by(|a, b| b.1.cmp(&a.1));
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
                    route: r.try_get("route").unwrap_or_else(|_| "".into()),
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
                    method: r.try_get("method").unwrap_or_else(|_| "".into()),
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
                    instance: r.try_get("instance").unwrap_or_else(|_| "".into()),
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
                    feature: r.try_get("feature").unwrap_or_else(|_| "".into()),
                    action: r.try_get("action").unwrap_or_else(|_| "".into()),
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
                let p50_idx = (((n - 1) as f64) * 0.50).round() as i64;
                let p95_idx = (((n - 1) as f64) * 0.95).round() as i64;

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

        let sql = r#"
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
        "#;
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

impl StatsStorage {
    pub async fn get_prev_rks(&self, user_hash: &str) -> Result<Option<(f64, String)>, AppError> {
        let row =
            sqlx::query("SELECT total_rks, updated_at FROM leaderboard_rks WHERE user_hash = ?")
                .bind(user_hash)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| AppError::Internal(format!("get prev rks: {e}")))?;
        if let Some(r) = row {
            Ok(Some((
                r.get::<f64, _>("total_rks"),
                r.get::<String, _>("updated_at"),
            )))
        } else {
            Ok(None)
        }
    }

    pub async fn count_public_leaderboard_total(&self) -> Result<i64, AppError> {
        let row = sqlx::query("SELECT COUNT(1) AS c FROM leaderboard_rks lr LEFT JOIN user_profile up ON up.user_hash=lr.user_hash WHERE COALESCE(up.is_public,0)=1 AND lr.is_hidden=0")
            .fetch_one(&self.pool)
            .await
            .map_err(|e| AppError::Internal(format!("count public leaderboard total: {e}")))?;
        Ok(row.try_get("c").unwrap_or(0))
    }

    pub async fn query_leaderboard_top_seek(
        &self,
        after_score: f64,
        after_updated: &str,
        after_user: &str,
        limit: i64,
    ) -> Result<Vec<SqliteRow>, AppError> {
        sqlx::query(
            "SELECT lr.user_hash, lr.total_rks, lr.updated_at, up.alias, COALESCE(up.show_best_top3,0) AS sbt, COALESCE(up.show_ap_top3,0) AS sat
             FROM leaderboard_rks lr LEFT JOIN user_profile up ON up.user_hash=lr.user_hash
             WHERE COALESCE(up.is_public,0)=1 AND lr.is_hidden=0 AND (
               lr.total_rks < ? OR (lr.total_rks = ? AND (lr.updated_at > ? OR (lr.updated_at = ? AND lr.user_hash > ?)))
             )
             ORDER BY lr.total_rks DESC, lr.updated_at ASC, lr.user_hash ASC
             LIMIT ?",
        )
        .bind(after_score)
        .bind(after_score)
        .bind(after_updated)
        .bind(after_updated)
        .bind(after_user)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Internal(format!("query top seek: {e}")))
    }

    pub async fn query_leaderboard_top_offset(
        &self,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<SqliteRow>, AppError> {
        sqlx::query(
            "SELECT lr.user_hash, lr.total_rks, lr.updated_at, up.alias, COALESCE(up.show_best_top3,0) AS sbt, COALESCE(up.show_ap_top3,0) AS sat
             FROM leaderboard_rks lr LEFT JOIN user_profile up ON up.user_hash=lr.user_hash
             WHERE COALESCE(up.is_public,0)=1 AND lr.is_hidden=0
             ORDER BY lr.total_rks DESC, lr.updated_at ASC, lr.user_hash ASC
             LIMIT ? OFFSET ?",
        )
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Internal(format!("query top offset: {e}")))
    }

    pub async fn query_leaderboard_by_rank(
        &self,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<SqliteRow>, AppError> {
        self.query_leaderboard_top_offset(limit, offset).await
    }

    pub async fn fetch_top3_details_for_users(
        &self,
        user_hashes: &[String],
    ) -> Result<std::collections::HashMap<String, (Option<String>, Option<String>)>, AppError> {
        if user_hashes.is_empty() {
            return Ok(std::collections::HashMap::new());
        }
        let mut qb = QueryBuilder::<Sqlite>::new(
            "SELECT user_hash, best_top3_json, ap_top3_json FROM leaderboard_details WHERE user_hash IN (",
        );
        let mut separated = qb.separated(", ");
        for uh in user_hashes {
            separated.push_bind(uh);
        }
        qb.push(")");
        let rows = qb
            .build()
            .fetch_all(&self.pool)
            .await
            .map_err(|e| AppError::Internal(format!("fetch top3 details: {e}")))?;
        let mut map = std::collections::HashMap::with_capacity(rows.len());
        for r in rows {
            let user_hash = r.try_get::<String, _>("user_hash").unwrap_or_default();
            let best_json = r.try_get::<String, _>("best_top3_json").ok();
            let ap_json = r.try_get::<String, _>("ap_top3_json").ok();
            map.insert(user_hash, (best_json, ap_json));
        }
        Ok(map)
    }

    pub async fn count_public_leaderboard_higher(
        &self,
        score: f64,
        updated_at: &str,
        user_hash: &str,
    ) -> Result<i64, AppError> {
        let row = sqlx::query(
            "SELECT COUNT(1) as higher FROM leaderboard_rks lr LEFT JOIN user_profile up ON up.user_hash=lr.user_hash
             WHERE COALESCE(up.is_public,0)=1 AND lr.is_hidden=0 AND (
               lr.total_rks > ? OR (lr.total_rks = ? AND (lr.updated_at < ? OR (lr.updated_at = ? AND lr.user_hash < ?)))
             )",
        )
        .bind(score)
        .bind(score)
        .bind(updated_at)
        .bind(updated_at)
        .bind(user_hash)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| AppError::Internal(format!("count public leaderboard higher: {e}")))?;
        Ok(row.try_get("higher").unwrap_or(0))
    }

    pub async fn update_user_profile_visibility(
        &self,
        user_hash: &str,
        now_rfc3339: &str,
        is_public: Option<i64>,
        show_rks_composition: Option<i64>,
        show_best_top3: Option<i64>,
        show_ap_top3: Option<i64>,
    ) -> Result<(), AppError> {
        let mut sets: Vec<&str> = Vec::new();
        if is_public.is_some() {
            sets.push("is_public=?");
        }
        if show_rks_composition.is_some() {
            sets.push("show_rks_composition=?");
        }
        if show_best_top3.is_some() {
            sets.push("show_best_top3=?");
        }
        if show_ap_top3.is_some() {
            sets.push("show_ap_top3=?");
        }
        sets.push("updated_at=?");
        let sql = format!(
            "UPDATE user_profile SET {} WHERE user_hash=?",
            sets.join(",")
        );
        let mut q = sqlx::query(&sql);
        if let Some(v) = is_public {
            q = q.bind(v);
        }
        if let Some(v) = show_rks_composition {
            q = q.bind(v);
        }
        if let Some(v) = show_best_top3 {
            q = q.bind(v);
        }
        if let Some(v) = show_ap_top3 {
            q = q.bind(v);
        }
        q = q.bind(now_rfc3339).bind(user_hash);
        q.execute(&self.pool)
            .await
            .map_err(|e| AppError::Internal(format!("update profile visibility: {e}")))?;
        Ok(())
    }

    pub async fn query_public_profile_by_alias(
        &self,
        alias: &str,
    ) -> Result<Option<SqliteRow>, AppError> {
        sqlx::query(
            "SELECT up.user_hash, up.is_public, up.show_rks_composition, up.show_best_top3, up.show_ap_top3, lr.total_rks, lr.updated_at
             FROM user_profile up LEFT JOIN leaderboard_rks lr ON lr.user_hash=up.user_hash WHERE up.alias = ?",
        )
        .bind(alias)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AppError::Internal(format!("query public profile by alias: {e}")))
    }

    pub async fn query_leaderboard_details_row(
        &self,
        user_hash: &str,
    ) -> Result<Option<SqliteRow>, AppError> {
        sqlx::query(
            "SELECT rks_composition_json, best_top3_json, ap_top3_json FROM leaderboard_details WHERE user_hash = ?",
        )
        .bind(user_hash)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AppError::Internal(format!("query leaderboard details row: {e}")))
    }

    pub async fn query_suspicious_rows(
        &self,
        min_score: f64,
        limit: i64,
    ) -> Result<Vec<SqliteRow>, AppError> {
        sqlx::query(
            "SELECT lr.user_hash, lr.total_rks, lr.suspicion_score, lr.updated_at, up.alias
             FROM leaderboard_rks lr LEFT JOIN user_profile up ON up.user_hash=lr.user_hash
             WHERE lr.suspicion_score >= ?
             ORDER BY lr.suspicion_score DESC, lr.total_rks DESC
             LIMIT ?",
        )
        .bind(min_score)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Internal(format!("query suspicious rows: {e}")))
    }

    pub async fn query_admin_leaderboard_users_count(
        &self,
        status_filter: Option<&str>,
        alias_like: Option<&str>,
    ) -> Result<i64, AppError> {
        let mut count_qb = QueryBuilder::<Sqlite>::new(
            "SELECT COUNT(1) AS c
             FROM leaderboard_rks lr
             LEFT JOIN user_profile up ON up.user_hash=lr.user_hash
             LEFT JOIN user_moderation_state ums ON ums.user_hash=lr.user_hash
             WHERE 1=1",
        );
        if let Some(status) = status_filter {
            count_qb
                .push(" AND LOWER(COALESCE(ums.status,'active')) = ")
                .push_bind(status.to_string());
        }
        if let Some(alias) = alias_like {
            count_qb
                .push(" AND up.alias LIKE ")
                .push_bind(alias.to_string());
        }
        let row = count_qb
            .build()
            .fetch_one(&self.pool)
            .await
            .map_err(|e| AppError::Internal(format!("admin users count: {e}")))?;
        Ok(row.try_get("c").unwrap_or(0))
    }

    pub async fn query_admin_leaderboard_users_rows(
        &self,
        status_filter: Option<&str>,
        alias_like: Option<&str>,
        page_size: i64,
        offset: i64,
    ) -> Result<Vec<SqliteRow>, AppError> {
        let mut qb = QueryBuilder::<Sqlite>::new(
            "SELECT
                lr.user_hash,
                up.alias,
                lr.total_rks,
                lr.suspicion_score,
                lr.is_hidden,
                lr.updated_at,
                COALESCE(ums.status, 'active') AS status
             FROM leaderboard_rks lr
             LEFT JOIN user_profile up ON up.user_hash=lr.user_hash
             LEFT JOIN user_moderation_state ums ON ums.user_hash=lr.user_hash
             WHERE 1=1",
        );
        if let Some(status) = status_filter {
            qb.push(" AND LOWER(COALESCE(ums.status,'active')) = ")
                .push_bind(status.to_string());
        }
        if let Some(alias) = alias_like {
            qb.push(" AND up.alias LIKE ").push_bind(alias.to_string());
        }
        qb.push(" ORDER BY lr.total_rks DESC, lr.updated_at ASC, lr.user_hash ASC");
        qb.push(" LIMIT ").push_bind(page_size);
        qb.push(" OFFSET ").push_bind(offset);
        qb.build()
            .fetch_all(&self.pool)
            .await
            .map_err(|e| AppError::Internal(format!("admin users list: {e}")))
    }

    pub async fn query_user_moderation_state_full_row(
        &self,
        user_hash: &str,
    ) -> Result<Option<SqliteRow>, AppError> {
        sqlx::query(
            "SELECT status, reason, updated_by, updated_at
             FROM user_moderation_state
             WHERE user_hash = ?
             LIMIT 1",
        )
        .bind(user_hash)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AppError::Internal(format!("query user moderation full row: {e}")))
    }

    pub async fn insert_submission(&self, record: SubmissionRecord<'_>) -> Result<(), AppError> {
        let SubmissionRecord {
            user_hash,
            total_rks,
            rks_jump,
            route,
            client_ip_hash,
            details_json,
            suspicion_score,
            now_rfc3339,
        } = record;
        sqlx::query("INSERT INTO save_submissions(user_hash,total_rks,acc_stats,rks_jump,route,client_ip_hash,details_json,suspicion_score,created_at) VALUES(?,?,?,?,?,?,?,?,?)")
            .bind(user_hash)
            .bind(total_rks)
            .bind(Option::<String>::None)
            .bind(rks_jump)
            .bind(route)
            .bind(client_ip_hash)
            .bind(details_json)
            .bind(suspicion_score)
            .bind(now_rfc3339)
            .execute(&self.pool)
            .await
            .map_err(|e| AppError::Internal(format!("insert submission: {e}")))?;
        Ok(())
    }

    pub async fn upsert_leaderboard_rks(
        &self,
        user_hash: &str,
        total_rks: f64,
        user_kind: Option<&str>,
        suspicion_score: f64,
        hide: bool,
        now_rfc3339: &str,
    ) -> Result<(), AppError> {
        let is_hidden_i = if hide { 1_i64 } else { 0_i64 };
        sqlx::query(
            "INSERT INTO leaderboard_rks(user_hash,total_rks,user_kind,suspicion_score,is_hidden,created_at,updated_at) VALUES(?,?,?,?,?,?,?)
             ON CONFLICT(user_hash) DO UPDATE SET
               total_rks = CASE WHEN excluded.total_rks > leaderboard_rks.total_rks THEN excluded.total_rks ELSE leaderboard_rks.total_rks END,
               updated_at = CASE WHEN excluded.total_rks > leaderboard_rks.total_rks THEN excluded.updated_at ELSE leaderboard_rks.updated_at END,
               user_kind = COALESCE(excluded.user_kind, leaderboard_rks.user_kind),
               suspicion_score = excluded.suspicion_score,
               is_hidden = CASE WHEN leaderboard_rks.is_hidden=1 OR excluded.is_hidden=1 THEN 1 ELSE 0 END"
        )
        .bind(user_hash)
        .bind(total_rks)
        .bind(user_kind)
        .bind(suspicion_score)
        .bind(is_hidden_i)
        .bind(now_rfc3339)
        .bind(now_rfc3339)
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Internal(format!("upsert leaderboard: {e}")))?;
        Ok(())
    }

    pub async fn set_leaderboard_hidden(
        &self,
        user_hash: &str,
        hide: bool,
    ) -> Result<(), AppError> {
        let is_hidden_i = if hide { 1_i64 } else { 0_i64 };
        sqlx::query("UPDATE leaderboard_rks SET is_hidden=? WHERE user_hash=?")
            .bind(is_hidden_i)
            .bind(user_hash)
            .execute(&self.pool)
            .await
            .map_err(|e| AppError::Internal(format!("update leaderboard hidden: {e}")))?;
        Ok(())
    }

    pub async fn upsert_details(
        &self,
        user_hash: &str,
        rks_comp_json: Option<&str>,
        best3_json: Option<&str>,
        ap3_json: Option<&str>,
        now_rfc3339: &str,
    ) -> Result<(), AppError> {
        sqlx::query(
            "INSERT INTO leaderboard_details(user_hash,rks_composition_json,best_top3_json,ap_top3_json,updated_at) VALUES(?,?,?,?,?)
             ON CONFLICT(user_hash) DO UPDATE SET
               rks_composition_json = COALESCE(excluded.rks_composition_json, leaderboard_details.rks_composition_json),
               best_top3_json = COALESCE(excluded.best_top3_json, leaderboard_details.best_top3_json),
               ap_top3_json = COALESCE(excluded.ap_top3_json, leaderboard_details.ap_top3_json),
               updated_at = excluded.updated_at"
        )
        .bind(user_hash)
        .bind(rks_comp_json)
        .bind(best3_json)
        .bind(ap3_json)
        .bind(now_rfc3339)
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Internal(format!("upsert details: {e}")))?;
        Ok(())
    }

    pub async fn ensure_default_public_profile(
        &self,
        user_hash: &str,
        user_kind: Option<&str>,
        show_rks_composition: bool,
        show_best_top3: bool,
        show_ap_top3: bool,
        now_rfc3339: &str,
    ) -> Result<(), AppError> {
        let show_rks_comp_i = if show_rks_composition { 1_i64 } else { 0_i64 };
        let show_best_top3_i = if show_best_top3 { 1_i64 } else { 0_i64 };
        let show_ap_top3_i = if show_ap_top3 { 1_i64 } else { 0_i64 };
        sqlx::query(
            "INSERT INTO user_profile(user_hash,is_public,show_rks_composition,show_best_top3,show_ap_top3,user_kind,created_at,updated_at) VALUES(?,?,?,?,?,?,?,?)
             ON CONFLICT(user_hash) DO NOTHING",
        )
        .bind(user_hash)
        .bind(1_i64)
        .bind(show_rks_comp_i)
        .bind(show_best_top3_i)
        .bind(show_ap_top3_i)
        .bind(user_kind)
        .bind(now_rfc3339)
        .bind(now_rfc3339)
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Internal(format!("ensure default public profile: {e}")))?;
        Ok(())
    }

    pub async fn ensure_user_profile_exists(
        &self,
        user_hash: &str,
        now_rfc3339: &str,
    ) -> Result<(), AppError> {
        sqlx::query(
            "INSERT INTO user_profile(user_hash,created_at,updated_at) VALUES(?,?,?) ON CONFLICT(user_hash) DO NOTHING",
        )
        .bind(user_hash)
        .bind(now_rfc3339)
        .bind(now_rfc3339)
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Internal(format!("ensure user profile exists: {e}")))?;
        Ok(())
    }

    pub async fn upsert_user_alias_with_defaults(
        &self,
        user_hash: &str,
        alias: &str,
        is_public: bool,
        show_rks_composition: bool,
        show_best_top3: bool,
        show_ap_top3: bool,
        now_rfc3339: &str,
    ) -> Result<(), AppError> {
        let is_public_i = if is_public { 1_i64 } else { 0_i64 };
        let show_rks_comp_i = if show_rks_composition { 1_i64 } else { 0_i64 };
        let show_best_top3_i = if show_best_top3 { 1_i64 } else { 0_i64 };
        let show_ap_top3_i = if show_ap_top3 { 1_i64 } else { 0_i64 };
        let res = sqlx::query(
            "INSERT INTO user_profile(user_hash,alias,is_public,show_rks_composition,show_best_top3,show_ap_top3,user_kind,created_at,updated_at) VALUES(?,?,?,?,?,?,?,?,?)
             ON CONFLICT(user_hash) DO UPDATE SET alias=excluded.alias, updated_at=excluded.updated_at",
        )
        .bind(user_hash)
        .bind(alias)
        .bind(is_public_i)
        .bind(show_rks_comp_i)
        .bind(show_best_top3_i)
        .bind(show_ap_top3_i)
        .bind(Option::<String>::None)
        .bind(now_rfc3339)
        .bind(now_rfc3339)
        .execute(&self.pool)
        .await;
        match res {
            Ok(_) => Ok(()),
            Err(e) => {
                if e.to_string().to_lowercase().contains("unique") {
                    return Err(AppError::Conflict("别名已被占用".into()));
                }
                Err(AppError::Internal(format!("set alias failed: {e}")))
            }
        }
    }

    pub async fn force_set_user_alias(
        &self,
        user_hash: &str,
        alias: &str,
        now_rfc3339: &str,
    ) -> Result<(), AppError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| AppError::Internal(format!("tx begin: {e}")))?;
        sqlx::query("UPDATE user_profile SET alias=NULL, updated_at=? WHERE alias=?")
            .bind(now_rfc3339)
            .bind(alias)
            .execute(&mut *tx)
            .await
            .map_err(|e| AppError::Internal(format!("clear alias: {e}")))?;
        sqlx::query(
            "INSERT INTO user_profile(user_hash,created_at,updated_at) VALUES(?,?,?) ON CONFLICT(user_hash) DO NOTHING",
        )
        .bind(user_hash)
        .bind(now_rfc3339)
        .bind(now_rfc3339)
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::Internal(format!("ensure profile: {e}")))?;
        sqlx::query("UPDATE user_profile SET alias=?, updated_at=? WHERE user_hash=?")
            .bind(alias)
            .bind(now_rfc3339)
            .bind(user_hash)
            .execute(&mut *tx)
            .await
            .map_err(|e| AppError::Internal(format!("set alias: {e}")))?;
        tx.commit()
            .await
            .map_err(|e| AppError::Internal(format!("tx commit: {e}")))?;
        Ok(())
    }

    fn build_banned_detail(reason: Option<&str>) -> String {
        if let Some(r) = reason.map(str::trim).filter(|v| !v.is_empty()) {
            return format!("用户已被全局封禁，原因：{r}");
        }
        "用户已被全局封禁".to_string()
    }

    pub async fn get_user_moderation_state(
        &self,
        user_hash: &str,
    ) -> Result<Option<(String, Option<String>)>, AppError> {
        let row = sqlx::query(
            "SELECT status, reason FROM user_moderation_state WHERE user_hash = ? LIMIT 1",
        )
        .bind(user_hash)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AppError::Internal(format!("query moderation state: {e}")))?;

        let Some(r) = row else {
            return Ok(None);
        };
        let status = r
            .try_get::<String, _>("status")
            .unwrap_or_else(|_| "active".to_string());
        let reason = r
            .try_get::<Option<String>, _>("reason")
            .unwrap_or(None)
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty());
        Ok(Some((status, reason)))
    }

    pub async fn get_user_moderation_status(
        &self,
        user_hash: &str,
    ) -> Result<Option<String>, AppError> {
        Ok(self
            .get_user_moderation_state(user_hash)
            .await?
            .map(|(status, _)| status))
    }

    pub async fn ensure_user_not_banned(&self, user_hash: &str) -> Result<(), AppError> {
        if let Some((status, reason)) = self.get_user_moderation_state(user_hash).await?
            && status.eq_ignore_ascii_case("banned")
        {
            return Err(AppError::Forbidden(Self::build_banned_detail(
                reason.as_deref(),
            )));
        }
        Ok(())
    }

    pub async fn set_user_moderation_status(
        &self,
        user_hash: &str,
        status: &str,
        reason: Option<&str>,
        updated_by: &str,
        updated_at: &str,
    ) -> Result<(), AppError> {
        let reason_clean = reason.map(str::trim).filter(|v| !v.is_empty());
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| AppError::Internal(format!("moderation tx begin: {e}")))?;
        sqlx::query(
            "INSERT INTO user_moderation_state(user_hash,status,reason,updated_by,updated_at,expires_at)
             VALUES(?,?,?,?,?,NULL)
             ON CONFLICT(user_hash) DO UPDATE SET
               status = excluded.status,
               reason = excluded.reason,
               updated_by = excluded.updated_by,
               updated_at = excluded.updated_at,
               expires_at = NULL",
        )
        .bind(user_hash)
        .bind(status)
        .bind(reason_clean)
        .bind(updated_by)
        .bind(updated_at)
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::Internal(format!("upsert moderation status: {e}")))?;

        sqlx::query(
            "INSERT INTO moderation_flags(user_hash,status,reason,severity,created_by,created_at)
             VALUES(?,?,?,?,?,?)",
        )
        .bind(user_hash)
        .bind(status)
        .bind(reason_clean.unwrap_or(""))
        .bind(0_i64)
        .bind(updated_by)
        .bind(updated_at)
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::Internal(format!("insert moderation flag: {e}")))?;

        tx.commit()
            .await
            .map_err(|e| AppError::Internal(format!("moderation tx commit: {e}")))?;
        Ok(())
    }

    /// 查询用户 RKS 历史记录
    ///
    /// 返回 (历史记录列表, 总记录数)
    pub async fn query_rks_history(
        &self,
        user_hash: &str,
        limit: i64,
        offset: i64,
    ) -> Result<(Vec<RksHistoryEntry>, i64), AppError> {
        // 归一化浮点噪声：避免把 1e-15 量级差值当成“RKS 变化”暴露给客户端。
        const RKS_JUMP_EPS: f64 = 1e-9;

        // 查询总数
        let count_row =
            sqlx::query("SELECT COUNT(1) as c FROM save_submissions WHERE user_hash = ?")
                .bind(user_hash)
                .fetch_one(&self.pool)
                .await
                .map_err(|e| AppError::Internal(format!("count rks history: {e}")))?;
        let total: i64 = count_row.try_get("c").unwrap_or(0);

        // 查询历史记录（按时间倒序）
        let rows = sqlx::query(
            "SELECT total_rks, rks_jump, created_at FROM save_submissions WHERE user_hash = ? ORDER BY created_at DESC LIMIT ? OFFSET ?"
        )
            .bind(user_hash)
            .bind(limit)
            .bind(offset)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| AppError::Internal(format!("query rks history: {e}")))?;

        let items: Vec<RksHistoryEntry> = rows
            .into_iter()
            .map(|r| {
                let rks = r.try_get::<f64, _>("total_rks").unwrap_or(0.0);
                let rks_jump = r.try_get::<f64, _>("rks_jump").unwrap_or(0.0);
                let rks_jump = if rks_jump.abs() < RKS_JUMP_EPS {
                    0.0
                } else {
                    rks_jump
                };
                RksHistoryEntry {
                    rks,
                    rks_jump,
                    created_at: r.try_get::<String, _>("created_at").unwrap_or_default(),
                }
            })
            .collect();

        Ok((items, total))
    }

    /// 获取用户历史最高 RKS
    pub async fn get_peak_rks(&self, user_hash: &str) -> Result<f64, AppError> {
        let row =
            sqlx::query("SELECT MAX(total_rks) as peak FROM save_submissions WHERE user_hash = ?")
                .bind(user_hash)
                .fetch_one(&self.pool)
                .await
                .map_err(|e| AppError::Internal(format!("get peak rks: {e}")))?;

        Ok(row.try_get::<f64, _>("peak").unwrap_or(0.0))
    }

    pub async fn add_token_blacklist(
        &self,
        jti: &str,
        expires_at: &str,
        created_at: &str,
    ) -> Result<(), AppError> {
        sqlx::query(
            "INSERT INTO session_token_blacklist(jti,expires_at,created_at) VALUES(?,?,?)
             ON CONFLICT(jti) DO UPDATE SET expires_at=excluded.expires_at, created_at=excluded.created_at",
        )
        .bind(jti)
        .bind(expires_at)
        .bind(created_at)
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Internal(format!("insert session blacklist: {e}")))?;
        Ok(())
    }

    pub async fn is_token_blacklisted(
        &self,
        jti: &str,
        now_rfc3339: &str,
    ) -> Result<bool, AppError> {
        let row = sqlx::query(
            "SELECT 1 FROM session_token_blacklist WHERE jti = ? AND expires_at > ? LIMIT 1",
        )
        .bind(jti)
        .bind(now_rfc3339)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AppError::Internal(format!("query session blacklist: {e}")))?;
        Ok(row.is_some())
    }

    pub async fn upsert_logout_gate(
        &self,
        user_hash: &str,
        logout_before: &str,
        expires_at: &str,
        updated_at: &str,
    ) -> Result<(), AppError> {
        sqlx::query(
            "INSERT INTO session_logout_gate(user_hash,logout_before,expires_at,updated_at) VALUES(?,?,?,?)
             ON CONFLICT(user_hash) DO UPDATE SET
               logout_before = excluded.logout_before,
               expires_at = excluded.expires_at,
               updated_at = excluded.updated_at",
        )
        .bind(user_hash)
        .bind(logout_before)
        .bind(expires_at)
        .bind(updated_at)
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Internal(format!("upsert session logout gate: {e}")))?;
        Ok(())
    }

    pub async fn get_logout_gate(
        &self,
        user_hash: &str,
        now_rfc3339: &str,
    ) -> Result<Option<String>, AppError> {
        let row = sqlx::query(
            "SELECT logout_before FROM session_logout_gate WHERE user_hash = ? AND expires_at > ? LIMIT 1",
        )
        .bind(user_hash)
        .bind(now_rfc3339)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AppError::Internal(format!("query session logout gate: {e}")))?;
        Ok(row.and_then(|r| r.try_get::<String, _>("logout_before").ok()))
    }

    pub async fn get_session_revoke_state(
        &self,
        jti: &str,
        user_hash: &str,
        now_rfc3339: &str,
    ) -> Result<(bool, Option<String>), AppError> {
        let row = sqlx::query(
            "SELECT
               EXISTS(SELECT 1 FROM session_token_blacklist WHERE jti = ? AND expires_at > ?) AS blacklisted,
               (SELECT logout_before FROM session_logout_gate WHERE user_hash = ? AND expires_at > ? LIMIT 1) AS logout_before",
        )
        .bind(jti)
        .bind(now_rfc3339)
        .bind(user_hash)
        .bind(now_rfc3339)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| AppError::Internal(format!("query session revoke state: {e}")))?;
        let blacklisted_num: i64 = row.try_get("blacklisted").unwrap_or(0);
        let logout_before: Option<String> = row.try_get("logout_before").unwrap_or(None);
        Ok((blacklisted_num != 0, logout_before))
    }

    pub async fn cleanup_expired_session_records(
        &self,
        now_rfc3339: &str,
    ) -> Result<(u64, u64), AppError> {
        let blacklist_deleted =
            sqlx::query("DELETE FROM session_token_blacklist WHERE expires_at <= ?")
                .bind(now_rfc3339)
                .execute(&self.pool)
                .await
                .map_err(|e| AppError::Internal(format!("cleanup session blacklist: {e}")))?
                .rows_affected();

        let gate_deleted = sqlx::query("DELETE FROM session_logout_gate WHERE expires_at <= ?")
            .bind(now_rfc3339)
            .execute(&self.pool)
            .await
            .map_err(|e| AppError::Internal(format!("cleanup session logout gate: {e}")))?
            .rows_affected();

        Ok((blacklist_deleted, gate_deleted))
    }

    pub async fn maybe_cleanup_expired_session_records(
        &self,
        now_utc: chrono::DateTime<chrono::Utc>,
    ) -> Result<bool, AppError> {
        let now_ts = now_utc.timestamp();
        let last_ts = LAST_SESSION_CLEANUP_TS.load(Ordering::Relaxed);
        if now_ts - last_ts < SESSION_CLEANUP_INTERVAL_SECS {
            return Ok(false);
        }

        if LAST_SESSION_CLEANUP_TS
            .compare_exchange(last_ts, now_ts, Ordering::SeqCst, Ordering::Relaxed)
            .is_err()
        {
            return Ok(false);
        }

        if let Err(e) = self
            .cleanup_expired_session_records(&now_utc.to_rfc3339())
            .await
        {
            LAST_SESSION_CLEANUP_TS.store(last_ts, Ordering::Relaxed);
            return Err(e);
        }

        Ok(true)
    }
}
