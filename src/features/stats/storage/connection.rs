use std::{path::Path, time::Duration};

use sqlx::{ConnectOptions, SqlitePool, sqlite::SqliteConnectOptions};

use crate::error::AppError;

use super::StatsStorage;

impl StatsStorage {
    pub async fn connect_sqlite(path: &str, wal: bool) -> Result<Self, AppError> {
        let opt = SqliteConnectOptions::new()
            .filename(Path::new(path))
            .create_if_missing(true)
            .busy_timeout(Duration::from_secs(5))
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
        let ddl = r"
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
        ";
        sqlx::query(ddl)
            .execute(&self.pool)
            .await
            .map_err(|e| AppError::Internal(format!("init schema: {e}")))?;
        Ok(())
    }
}
