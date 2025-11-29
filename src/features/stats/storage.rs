use std::path::Path;

use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, Utc};
use sqlx::{ConnectOptions, Row, SqlitePool, sqlite::SqliteConnectOptions};

use crate::error::AppError;

use super::models::{DailyAggRow, EventInsert};

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
        "#;
        sqlx::query(ddl)
            .execute(&self.pool)
            .await
            .map_err(|e| AppError::Internal(format!("init schema: {e}")))?;
        Ok(())
    }

    pub async fn insert_events(&self, events: &[EventInsert]) -> Result<(), AppError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| AppError::Internal(format!("begin tx: {e}")))?;
        for e in events {
            let extra = e
                .extra_json
                .as_ref()
                .map(|v| serde_json::to_string(v).unwrap_or_default());
            sqlx::query("INSERT INTO events(ts_utc, route, feature, action, method, status, duration_ms, user_hash, client_ip_hash, instance, extra_json) VALUES(?,?,?,?,?,?,?,?,?,?,?)")
                .bind(e.ts_utc.to_rfc3339())
                .bind(&e.route)
                .bind(&e.feature)
                .bind(&e.action)
                .bind(&e.method)
                .bind(e.status.map(|v| v as i64))
                .bind(e.duration_ms)
                .bind(&e.user_hash)
                .bind(&e.client_ip_hash)
                .bind(&e.instance)
                .bind(extra)
                .execute(&mut *tx).await.map_err(|er| AppError::Internal(format!("insert event: {er}")))?;
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
            GROUP BY date, feature, route, method
            ORDER BY date ASC
        "#;
        let rows = sqlx::query(sql)
            .bind(&start_s)
            .bind(&end_s)
            .bind(feature.as_ref())
            .bind(feature.as_ref())
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
            .map(|r| crate::features::rks::handler::RksHistoryItem {
                rks: r.try_get::<f64, _>("total_rks").unwrap_or(0.0),
                rks_jump: r.try_get::<f64, _>("rks_jump").unwrap_or(0.0),
                created_at: r.try_get::<String, _>("created_at").unwrap_or_default(),
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
}
