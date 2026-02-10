use std::{
    path::Path,
    sync::atomic::{AtomicI64, Ordering},
};

use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, Utc};
use sqlx::{ConnectOptions, QueryBuilder, Row, SqlitePool, sqlite::SqliteConnectOptions};

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

#[derive(Clone)]
pub struct StatsStorage {
    pub pool: SqlitePool,
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

    /// 查询用户 RKS 历史记录
    ///
    /// 返回 (历史记录列表, 总记录数)
    pub async fn query_rks_history(
        &self,
        user_hash: &str,
        limit: i64,
        offset: i64,
    ) -> Result<(Vec<crate::features::rks::handler::RksHistoryItem>, i64), AppError> {
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

        let items: Vec<crate::features::rks::handler::RksHistoryItem> = rows
            .into_iter()
            .map(|r| {
                let rks = r.try_get::<f64, _>("total_rks").unwrap_or(0.0);
                let rks_jump = r.try_get::<f64, _>("rks_jump").unwrap_or(0.0);
                let rks_jump = if rks_jump.abs() < RKS_JUMP_EPS {
                    0.0
                } else {
                    rks_jump
                };
                crate::features::rks::handler::RksHistoryItem {
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
