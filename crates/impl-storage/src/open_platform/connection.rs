use std::path::Path;
use std::time::Duration;

use sqlx::{
    ConnectOptions,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
};

use crate::error::AppError;

use super::OpenPlatformStorage;

impl OpenPlatformStorage {
    pub async fn connect_sqlite(path: &str, wal: bool) -> Result<Self, AppError> {
        // 与统计库一致：把 journal_mode/synchronous/foreign_keys/busy_timeout 写进
        // `SqliteConnectOptions`，确保**池中每条连接**都生效——此前的 per-connection
        // `PRAGMA ...`（execute(&pool)）在池轮转下可能落到任意/其它连接，行为不确定。
        let mut opt = SqliteConnectOptions::new()
            .filename(Path::new(path))
            .create_if_missing(true)
            .busy_timeout(Duration::from_secs(5))
            .foreign_keys(true)
            .synchronous(sqlx::sqlite::SqliteSynchronous::Normal)
            .log_statements(tracing::log::LevelFilter::Off);
        if wal {
            opt = opt.journal_mode(sqlx::sqlite::SqliteJournalMode::Wal);
        }
        let pool = SqlitePoolOptions::new()
            .max_connections(4)
            .acquire_timeout(Duration::from_secs(10))
            .connect_with(opt)
            .await
            .map_err(|e| AppError::Internal(format!("open platform sqlite connect: {e}")))?;
        Ok(Self { pool })
    }

    pub async fn init_schema(&self) -> Result<(), AppError> {
        let ddl = r"
        CREATE TABLE IF NOT EXISTS developers (
          id TEXT PRIMARY KEY,
          github_user_id TEXT NOT NULL UNIQUE,
          github_login TEXT NOT NULL,
          email TEXT,
          role TEXT NOT NULL DEFAULT 'developer',
          status TEXT NOT NULL DEFAULT 'active',
          created_at INTEGER NOT NULL,
          updated_at INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS api_keys (
          id TEXT PRIMARY KEY,
          developer_id TEXT NOT NULL,
          name TEXT NOT NULL,
          key_prefix TEXT NOT NULL,
          key_last4 TEXT NOT NULL,
          key_hash TEXT NOT NULL UNIQUE,
          scopes TEXT NOT NULL,
          status TEXT NOT NULL,
          created_at INTEGER NOT NULL,
          expires_at INTEGER,
          revoked_at INTEGER,
          replaced_by_key_id TEXT,
          last_used_at INTEGER,
          last_used_ip TEXT,
          usage_count INTEGER NOT NULL DEFAULT 0,
          FOREIGN KEY (developer_id) REFERENCES developers(id)
        );

        CREATE INDEX IF NOT EXISTS idx_api_keys_developer_id ON api_keys(developer_id);
        CREATE INDEX IF NOT EXISTS idx_api_keys_status ON api_keys(status);
        CREATE INDEX IF NOT EXISTS idx_api_keys_status_expires_at
          ON api_keys(status, expires_at)
          WHERE expires_at IS NOT NULL AND expires_at > 0;
        CREATE INDEX IF NOT EXISTS idx_api_keys_last_used_at ON api_keys(last_used_at);
        CREATE INDEX IF NOT EXISTS idx_api_keys_developer_created_at ON api_keys(developer_id, created_at DESC);
        CREATE INDEX IF NOT EXISTS idx_api_keys_developer_status_created_at ON api_keys(developer_id, status, created_at DESC);

        CREATE TABLE IF NOT EXISTS api_key_events (
          id TEXT PRIMARY KEY,
          key_id TEXT NOT NULL,
          developer_id TEXT NOT NULL,
          event_type TEXT NOT NULL,
          event_reason TEXT,
          operator_id TEXT,
          request_id TEXT,
          created_at INTEGER NOT NULL,
          metadata TEXT,
          FOREIGN KEY (key_id) REFERENCES api_keys(id),
          FOREIGN KEY (developer_id) REFERENCES developers(id)
        );

        CREATE INDEX IF NOT EXISTS idx_api_key_events_key_id ON api_key_events(key_id);
        CREATE INDEX IF NOT EXISTS idx_api_key_events_created_at ON api_key_events(created_at);
        CREATE INDEX IF NOT EXISTS idx_api_key_events_developer_id ON api_key_events(developer_id);
        CREATE INDEX IF NOT EXISTS idx_api_key_events_key_created_at ON api_key_events(key_id, created_at DESC);
        ";

        sqlx::query(ddl)
            .execute(&self.pool)
            .await
            .map_err(|e| AppError::Internal(format!("open platform init schema: {e}")))?;
        Ok(())
    }
}
