use std::{path::Path, sync::Arc};

use once_cell::sync::OnceCell;
use serde::{Deserialize, Serialize};
use sqlx::{ConnectOptions, Row, SqlitePool, sqlite::SqliteConnectOptions};
use uuid::Uuid;

use crate::error::AppError;

pub const API_KEY_STATUS_ACTIVE: &str = "active";
pub const API_KEY_STATUS_REVOKED: &str = "revoked";
pub const API_KEY_STATUS_EXPIRED: &str = "expired";

pub const API_KEY_EVENT_ISSUED: &str = "issued";
pub const API_KEY_EVENT_ROTATED: &str = "rotated";
pub const API_KEY_EVENT_REVOKED: &str = "revoked";
pub const API_KEY_EVENT_AUTH_FAILED: &str = "auth_failed";

static OPEN_PLATFORM_STORAGE: OnceCell<Arc<OpenPlatformStorage>> = OnceCell::new();

pub fn init_global(storage: Arc<OpenPlatformStorage>) -> Result<(), AppError> {
    OPEN_PLATFORM_STORAGE
        .set(storage)
        .map_err(|_| AppError::Internal("开放平台存储已初始化".into()))
}

pub fn global() -> Result<&'static Arc<OpenPlatformStorage>, AppError> {
    OPEN_PLATFORM_STORAGE
        .get()
        .ok_or_else(|| AppError::Internal("开放平台存储未初始化".into()))
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DeveloperRecord {
    pub id: String,
    pub github_user_id: String,
    pub github_login: String,
    pub email: Option<String>,
    pub role: String,
    pub status: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ApiKeyRecord {
    pub id: String,
    pub developer_id: String,
    pub name: String,
    pub key_prefix: String,
    pub key_last4: String,
    pub key_hash: String,
    pub scopes: Vec<String>,
    pub status: String,
    pub created_at: i64,
    pub expires_at: Option<i64>,
    pub revoked_at: Option<i64>,
    pub replaced_by_key_id: Option<String>,
    pub last_used_at: Option<i64>,
    pub last_used_ip: Option<String>,
    pub usage_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ApiKeyEventRecord {
    pub id: String,
    pub key_id: String,
    pub developer_id: String,
    pub event_type: String,
    pub event_reason: Option<String>,
    pub operator_id: Option<String>,
    pub request_id: Option<String>,
    pub created_at: i64,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Clone)]
pub struct CreateApiKeyParams {
    pub developer_id: String,
    pub name: String,
    pub key_prefix: String,
    pub key_last4: String,
    pub key_hash: String,
    pub scopes: Vec<String>,
    pub expires_at: Option<i64>,
    pub now_ts: i64,
}

#[derive(Debug, Clone)]
pub struct RotateApiKeyParams {
    pub key_id: String,
    pub new_name: String,
    pub new_key_prefix: String,
    pub new_key_last4: String,
    pub new_key_hash: String,
    pub new_scopes: Vec<String>,
    pub grace_expires_at: Option<i64>,
    pub now_ts: i64,
    pub operator_id: Option<String>,
    pub request_id: Option<String>,
}

#[derive(Clone)]
pub struct OpenPlatformStorage {
    pub pool: SqlitePool,
}

fn parse_scopes_json(raw: &str) -> Result<Vec<String>, AppError> {
    serde_json::from_str::<Vec<String>>(raw)
        .map_err(|e| AppError::Internal(format!("解析 API Key scopes 失败: {e}")))
}

fn parse_metadata_json(raw: Option<String>) -> Result<Option<serde_json::Value>, AppError> {
    match raw {
        Some(s) if s.trim().is_empty() => Ok(None),
        Some(s) => serde_json::from_str::<serde_json::Value>(&s)
            .map(Some)
            .map_err(|e| AppError::Internal(format!("解析 API Key 事件 metadata 失败: {e}"))),
        None => Ok(None),
    }
}

fn normalize_optional_text(v: Option<String>) -> Option<String> {
    v.and_then(|s| {
        let trimmed = s.trim();
        if trimmed.is_empty() {
            None
        } else if trimmed.len() == s.len() {
            Some(s)
        } else {
            Some(trimmed.to_string())
        }
    })
}

fn row_to_developer(row: &sqlx::sqlite::SqliteRow) -> DeveloperRecord {
    DeveloperRecord {
        id: row.get("id"),
        github_user_id: row.get("github_user_id"),
        github_login: row.get("github_login"),
        email: normalize_optional_text(row.try_get("email").ok()),
        role: row.get("role"),
        status: row.get("status"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    }
}

fn row_to_api_key(row: &sqlx::sqlite::SqliteRow) -> Result<ApiKeyRecord, AppError> {
    let scopes_raw: String = row.get("scopes");
    Ok(ApiKeyRecord {
        id: row.get("id"),
        developer_id: row.get("developer_id"),
        name: row.get("name"),
        key_prefix: row.get("key_prefix"),
        key_last4: row.get("key_last4"),
        key_hash: row.get("key_hash"),
        scopes: parse_scopes_json(&scopes_raw)?,
        status: row.get("status"),
        created_at: row.get("created_at"),
        expires_at: row.try_get("expires_at").ok(),
        revoked_at: row.try_get("revoked_at").ok(),
        replaced_by_key_id: normalize_optional_text(row.try_get("replaced_by_key_id").ok()),
        last_used_at: row.try_get("last_used_at").ok(),
        last_used_ip: normalize_optional_text(row.try_get("last_used_ip").ok()),
        usage_count: row.get("usage_count"),
    })
}

fn row_to_api_key_event(row: &sqlx::sqlite::SqliteRow) -> Result<ApiKeyEventRecord, AppError> {
    let metadata_raw: Option<String> = row.try_get("metadata").ok();
    Ok(ApiKeyEventRecord {
        id: row.get("id"),
        key_id: row.get("key_id"),
        developer_id: row.get("developer_id"),
        event_type: row.get("event_type"),
        event_reason: normalize_optional_text(row.try_get("event_reason").ok()),
        operator_id: normalize_optional_text(row.try_get("operator_id").ok()),
        request_id: normalize_optional_text(row.try_get("request_id").ok()),
        created_at: row.get("created_at"),
        metadata: parse_metadata_json(metadata_raw)?,
    })
}

impl OpenPlatformStorage {
    pub async fn connect_sqlite(path: &str, wal: bool) -> Result<Self, AppError> {
        let opt = SqliteConnectOptions::new()
            .filename(Path::new(path))
            .create_if_missing(true)
            .log_statements(tracing::log::LevelFilter::Off);
        let pool = SqlitePool::connect_with(opt)
            .await
            .map_err(|e| AppError::Internal(format!("open platform sqlite connect: {e}")))?;
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
        CREATE INDEX IF NOT EXISTS idx_api_keys_last_used_at ON api_keys(last_used_at);

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
        "#;

        sqlx::query(ddl)
            .execute(&self.pool)
            .await
            .map_err(|e| AppError::Internal(format!("open platform init schema: {e}")))?;
        Ok(())
    }

    pub async fn upsert_developer_by_github(
        &self,
        github_user_id: &str,
        github_login: &str,
        email: Option<&str>,
        now_ts: i64,
    ) -> Result<DeveloperRecord, AppError> {
        let developer_id = format!("dev_{}", Uuid::new_v4().simple());
        sqlx::query(
            "INSERT INTO developers(id, github_user_id, github_login, email, role, status, created_at, updated_at)
             VALUES(?, ?, ?, ?, 'developer', 'active', ?, ?)
             ON CONFLICT(github_user_id) DO UPDATE SET
               github_login = excluded.github_login,
               email = excluded.email,
               updated_at = excluded.updated_at",
        )
        .bind(&developer_id)
        .bind(github_user_id)
        .bind(github_login)
        .bind(email)
        .bind(now_ts)
        .bind(now_ts)
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Internal(format!("upsert developer: {e}")))?;

        let row = sqlx::query("SELECT * FROM developers WHERE github_user_id = ? LIMIT 1")
            .bind(github_user_id)
            .fetch_one(&self.pool)
            .await
            .map_err(|e| AppError::Internal(format!("query developer after upsert: {e}")))?;
        Ok(row_to_developer(&row))
    }

    pub async fn get_developer_by_id(
        &self,
        developer_id: &str,
    ) -> Result<Option<DeveloperRecord>, AppError> {
        let row = sqlx::query("SELECT * FROM developers WHERE id = ? LIMIT 1")
            .bind(developer_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| AppError::Internal(format!("query developer by id: {e}")))?;
        Ok(row.as_ref().map(row_to_developer))
    }

    pub async fn get_developer_by_github_user_id(
        &self,
        github_user_id: &str,
    ) -> Result<Option<DeveloperRecord>, AppError> {
        let row = sqlx::query("SELECT * FROM developers WHERE github_user_id = ? LIMIT 1")
            .bind(github_user_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| AppError::Internal(format!("query developer by github user id: {e}")))?;
        Ok(row.as_ref().map(row_to_developer))
    }

    pub async fn create_api_key(
        &self,
        params: CreateApiKeyParams,
    ) -> Result<ApiKeyRecord, AppError> {
        let CreateApiKeyParams {
            developer_id,
            name,
            key_prefix,
            key_last4,
            key_hash,
            scopes,
            expires_at,
            now_ts,
        } = params;

        let key_id = format!("key_{}", Uuid::new_v4().simple());
        let scopes_json = serde_json::to_string(&scopes)
            .map_err(|e| AppError::Internal(format!("serialize api key scopes: {e}")))?;

        sqlx::query(
            "INSERT INTO api_keys(
                id, developer_id, name, key_prefix, key_last4, key_hash, scopes, status, created_at, expires_at
             ) VALUES(?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&key_id)
        .bind(&developer_id)
        .bind(&name)
        .bind(&key_prefix)
        .bind(&key_last4)
        .bind(&key_hash)
        .bind(scopes_json)
        .bind(API_KEY_STATUS_ACTIVE)
        .bind(now_ts)
        .bind(expires_at)
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Internal(format!("insert api key: {e}")))?;

        let row = sqlx::query("SELECT * FROM api_keys WHERE id = ? LIMIT 1")
            .bind(&key_id)
            .fetch_one(&self.pool)
            .await
            .map_err(|e| AppError::Internal(format!("query api key after create: {e}")))?;
        row_to_api_key(&row)
    }

    pub async fn get_api_key_by_id(&self, key_id: &str) -> Result<Option<ApiKeyRecord>, AppError> {
        let row = sqlx::query("SELECT * FROM api_keys WHERE id = ? LIMIT 1")
            .bind(key_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| AppError::Internal(format!("query api key by id: {e}")))?;
        row.map(|r| row_to_api_key(&r)).transpose()
    }

    pub async fn get_api_key_by_hash(
        &self,
        key_hash: &str,
    ) -> Result<Option<ApiKeyRecord>, AppError> {
        let row = sqlx::query("SELECT * FROM api_keys WHERE key_hash = ? LIMIT 1")
            .bind(key_hash)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| AppError::Internal(format!("query api key by hash: {e}")))?;
        row.map(|r| row_to_api_key(&r)).transpose()
    }

    pub async fn list_api_keys_by_developer(
        &self,
        developer_id: &str,
    ) -> Result<Vec<ApiKeyRecord>, AppError> {
        let rows =
            sqlx::query("SELECT * FROM api_keys WHERE developer_id = ? ORDER BY created_at DESC")
                .bind(developer_id)
                .fetch_all(&self.pool)
                .await
                .map_err(|e| AppError::Internal(format!("list api keys by developer: {e}")))?;

        rows.into_iter()
            .map(|r| row_to_api_key(&r))
            .collect::<Result<Vec<_>, _>>()
    }

    pub async fn rotate_api_key(
        &self,
        params: RotateApiKeyParams,
    ) -> Result<ApiKeyRecord, AppError> {
        let RotateApiKeyParams {
            key_id,
            new_name,
            new_key_prefix,
            new_key_last4,
            new_key_hash,
            new_scopes,
            grace_expires_at,
            now_ts,
            operator_id,
            request_id,
        } = params;

        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| AppError::Internal(format!("begin rotate api key tx: {e}")))?;

        let old_row = sqlx::query("SELECT developer_id, status FROM api_keys WHERE id = ? LIMIT 1")
            .bind(&key_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| AppError::Internal(format!("query old api key for rotate: {e}")))?;

        let Some(old_row) = old_row else {
            return Err(AppError::Validation("待轮换的 API Key 不存在".into()));
        };
        let developer_id: String = old_row.get("developer_id");
        let old_status: String = old_row.get("status");
        if old_status != API_KEY_STATUS_ACTIVE {
            return Err(AppError::Validation(
                "仅 active 状态的 API Key 可轮换".into(),
            ));
        }

        let new_key_id = format!("key_{}", Uuid::new_v4().simple());
        let scopes_json = serde_json::to_string(&new_scopes)
            .map_err(|e| AppError::Internal(format!("serialize rotate scopes: {e}")))?;

        sqlx::query(
            "INSERT INTO api_keys(
                id, developer_id, name, key_prefix, key_last4, key_hash, scopes, status, created_at
             ) VALUES(?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&new_key_id)
        .bind(&developer_id)
        .bind(&new_name)
        .bind(&new_key_prefix)
        .bind(&new_key_last4)
        .bind(&new_key_hash)
        .bind(scopes_json)
        .bind(API_KEY_STATUS_ACTIVE)
        .bind(now_ts)
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::Internal(format!("insert rotated api key: {e}")))?;

        let (old_next_status, revoked_at) = if grace_expires_at.is_some() {
            (API_KEY_STATUS_ACTIVE, Option::<i64>::None)
        } else {
            (API_KEY_STATUS_REVOKED, Some(now_ts))
        };
        let next_expires_at = grace_expires_at.or(Some(now_ts));

        sqlx::query(
            "UPDATE api_keys
             SET status = ?, revoked_at = ?, expires_at = ?, replaced_by_key_id = ?
             WHERE id = ?",
        )
        .bind(old_next_status)
        .bind(revoked_at)
        .bind(next_expires_at)
        .bind(&new_key_id)
        .bind(&key_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::Internal(format!("update old api key on rotate: {e}")))?;

        let rotate_event_id = format!("evt_{}", Uuid::new_v4().simple());
        sqlx::query(
            "INSERT INTO api_key_events(
                id, key_id, developer_id, event_type, event_reason, operator_id, request_id, created_at, metadata
             ) VALUES(?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(rotate_event_id)
        .bind(&key_id)
        .bind(&developer_id)
        .bind(API_KEY_EVENT_ROTATED)
        .bind("key rotated")
        .bind(operator_id.as_deref())
        .bind(request_id.as_deref())
        .bind(now_ts)
        .bind(
            serde_json::to_string(&serde_json::json!({
                "replacedByKeyId": new_key_id,
                "graceExpiresAt": grace_expires_at
            }))
            .map_err(|e| AppError::Internal(format!("serialize rotate metadata: {e}")))?,
        )
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::Internal(format!("insert rotate event: {e}")))?;

        let issue_event_id = format!("evt_{}", Uuid::new_v4().simple());
        sqlx::query(
            "INSERT INTO api_key_events(
                id, key_id, developer_id, event_type, event_reason, operator_id, request_id, created_at
             ) VALUES(?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(issue_event_id)
        .bind(&new_key_id)
        .bind(&developer_id)
        .bind(API_KEY_EVENT_ISSUED)
        .bind("rotated new key issued")
        .bind(operator_id.as_deref())
        .bind(request_id.as_deref())
        .bind(now_ts)
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::Internal(format!("insert issue event in rotate: {e}")))?;

        tx.commit()
            .await
            .map_err(|e| AppError::Internal(format!("commit rotate api key tx: {e}")))?;

        let row = sqlx::query("SELECT * FROM api_keys WHERE id = ? LIMIT 1")
            .bind(&new_key_id)
            .fetch_one(&self.pool)
            .await
            .map_err(|e| AppError::Internal(format!("query new api key after rotate: {e}")))?;
        row_to_api_key(&row)
    }

    pub async fn revoke_api_key(
        &self,
        key_id: &str,
        reason: Option<&str>,
        operator_id: Option<&str>,
        request_id: Option<&str>,
        now_ts: i64,
    ) -> Result<(), AppError> {
        let row = sqlx::query("SELECT developer_id FROM api_keys WHERE id = ? LIMIT 1")
            .bind(key_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| AppError::Internal(format!("query api key before revoke: {e}")))?;
        let Some(row) = row else {
            return Err(AppError::Validation("待撤销的 API Key 不存在".into()));
        };
        let developer_id: String = row.get("developer_id");

        sqlx::query(
            "UPDATE api_keys
             SET status = ?, revoked_at = ?, expires_at = ?, replaced_by_key_id = NULL
             WHERE id = ?",
        )
        .bind(API_KEY_STATUS_REVOKED)
        .bind(now_ts)
        .bind(now_ts)
        .bind(key_id)
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Internal(format!("revoke api key: {e}")))?;

        self.record_api_key_event(
            key_id,
            &developer_id,
            API_KEY_EVENT_REVOKED,
            reason,
            operator_id,
            request_id,
            None,
            now_ts,
        )
        .await?;

        Ok(())
    }

    pub async fn touch_api_key_usage(
        &self,
        key_id: &str,
        now_ts: i64,
        client_ip: Option<&str>,
    ) -> Result<(), AppError> {
        sqlx::query(
            "UPDATE api_keys
             SET last_used_at = ?, last_used_ip = ?, usage_count = usage_count + 1
             WHERE id = ?",
        )
        .bind(now_ts)
        .bind(client_ip)
        .bind(key_id)
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Internal(format!("touch api key usage: {e}")))?;
        Ok(())
    }

    pub async fn cleanup_expired_active_keys(&self, now_ts: i64) -> Result<u64, AppError> {
        let ret = sqlx::query(
            "UPDATE api_keys
             SET status = ?, revoked_at = COALESCE(revoked_at, expires_at)
             WHERE status = ? AND expires_at IS NOT NULL AND expires_at <= ?",
        )
        .bind(API_KEY_STATUS_EXPIRED)
        .bind(API_KEY_STATUS_ACTIVE)
        .bind(now_ts)
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Internal(format!("cleanup expired active keys: {e}")))?;
        Ok(ret.rows_affected())
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn record_api_key_event(
        &self,
        key_id: &str,
        developer_id: &str,
        event_type: &str,
        event_reason: Option<&str>,
        operator_id: Option<&str>,
        request_id: Option<&str>,
        metadata: Option<&serde_json::Value>,
        now_ts: i64,
    ) -> Result<String, AppError> {
        let event_id = format!("evt_{}", Uuid::new_v4().simple());
        let metadata_json = metadata
            .map(serde_json::to_string)
            .transpose()
            .map_err(|e| AppError::Internal(format!("serialize api key event metadata: {e}")))?;

        sqlx::query(
            "INSERT INTO api_key_events(
                id, key_id, developer_id, event_type, event_reason, operator_id, request_id, created_at, metadata
             ) VALUES(?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&event_id)
        .bind(key_id)
        .bind(developer_id)
        .bind(event_type)
        .bind(event_reason)
        .bind(operator_id)
        .bind(request_id)
        .bind(now_ts)
        .bind(metadata_json)
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Internal(format!("insert api key event: {e}")))?;

        Ok(event_id)
    }

    pub async fn list_api_key_events(
        &self,
        key_id: &str,
        limit: i64,
    ) -> Result<Vec<ApiKeyEventRecord>, AppError> {
        let limit = limit.clamp(1, 500);
        let rows = sqlx::query(
            "SELECT * FROM api_key_events WHERE key_id = ? ORDER BY created_at DESC LIMIT ?",
        )
        .bind(key_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Internal(format!("list api key events: {e}")))?;

        rows.into_iter()
            .map(|r| row_to_api_key_event(&r))
            .collect::<Result<Vec<_>, _>>()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn temp_db_path() -> PathBuf {
        std::env::temp_dir().join(format!("phi_open_platform_{}.db", Uuid::new_v4()))
    }

    async fn setup_storage() -> OpenPlatformStorage {
        let path = temp_db_path();
        let storage = OpenPlatformStorage::connect_sqlite(path.to_string_lossy().as_ref(), true)
            .await
            .expect("connect sqlite for open platform");
        storage.init_schema().await.expect("init schema");
        storage
    }

    #[tokio::test]
    async fn upsert_developer_by_github_is_idempotent() {
        let storage = setup_storage().await;
        let now1 = 1_700_000_000_i64;
        let now2 = now1 + 10;

        let dev1 = storage
            .upsert_developer_by_github("1001", "alice", Some("alice@x.test"), now1)
            .await
            .expect("first upsert developer");
        let dev2 = storage
            .upsert_developer_by_github("1001", "alice-renamed", None, now2)
            .await
            .expect("second upsert developer");

        assert_eq!(dev1.id, dev2.id);
        assert_eq!(dev2.github_login, "alice-renamed");
        assert_eq!(dev2.email, None);
        assert_eq!(dev2.updated_at, now2);
    }

    #[tokio::test]
    async fn api_key_lifecycle_issue_rotate_revoke_and_events() {
        let storage = setup_storage().await;
        let now = 1_700_000_100_i64;

        let developer = storage
            .upsert_developer_by_github("2002", "bob", Some("bob@x.test"), now)
            .await
            .expect("upsert developer");

        let key1 = storage
            .create_api_key(CreateApiKeyParams {
                developer_id: developer.id.clone(),
                name: "prod-key".to_string(),
                key_prefix: "pgr_live_".to_string(),
                key_last4: "a1b2".to_string(),
                key_hash: "hash_key_1".to_string(),
                scopes: vec![String::from("public.read"), String::from("profile.read")],
                expires_at: None,
                now_ts: now,
            })
            .await
            .expect("create key1");
        assert_eq!(key1.status, API_KEY_STATUS_ACTIVE);
        assert_eq!(key1.scopes.len(), 2);

        let listed = storage
            .list_api_keys_by_developer(&developer.id)
            .await
            .expect("list keys");
        assert_eq!(listed.len(), 1);

        let rotate_grace = now + 3600;
        let key2 = storage
            .rotate_api_key(RotateApiKeyParams {
                key_id: key1.id.clone(),
                new_name: "prod-key-v2".to_string(),
                new_key_prefix: "pgr_live_".to_string(),
                new_key_last4: "c3d4".to_string(),
                new_key_hash: "hash_key_2".to_string(),
                new_scopes: vec![String::from("public.read")],
                grace_expires_at: Some(rotate_grace),
                now_ts: now + 5,
                operator_id: Some(developer.id.clone()),
                request_id: Some("req_rotate_001".to_string()),
            })
            .await
            .expect("rotate key");
        assert_eq!(key2.status, API_KEY_STATUS_ACTIVE);
        assert_eq!(key2.name, "prod-key-v2");

        let old_after_rotate = storage
            .get_api_key_by_id(&key1.id)
            .await
            .expect("query old key")
            .expect("old key should exist");
        assert_eq!(old_after_rotate.status, API_KEY_STATUS_ACTIVE);
        assert_eq!(
            old_after_rotate.replaced_by_key_id.as_deref(),
            Some(key2.id.as_str())
        );
        assert_eq!(old_after_rotate.expires_at, Some(rotate_grace));

        storage
            .revoke_api_key(
                &key2.id,
                Some("manual revoke"),
                Some(&developer.id),
                Some("req_revoke_001"),
                now + 20,
            )
            .await
            .expect("revoke key2");

        let key2_after_revoke = storage
            .get_api_key_by_id(&key2.id)
            .await
            .expect("query key2")
            .expect("key2 exists");
        assert_eq!(key2_after_revoke.status, API_KEY_STATUS_REVOKED);
        assert_eq!(key2_after_revoke.revoked_at, Some(now + 20));

        let events_key1 = storage
            .list_api_key_events(&key1.id, 20)
            .await
            .expect("list events key1");
        assert!(
            events_key1
                .iter()
                .any(|e| e.event_type == API_KEY_EVENT_ROTATED),
            "old key should have rotated event"
        );

        let events_key2 = storage
            .list_api_key_events(&key2.id, 20)
            .await
            .expect("list events key2");
        assert!(
            events_key2
                .iter()
                .any(|e| e.event_type == API_KEY_EVENT_ISSUED),
            "new key should have issued event"
        );
        assert!(
            events_key2
                .iter()
                .any(|e| e.event_type == API_KEY_EVENT_REVOKED),
            "new key should have revoked event"
        );

        let expired_rows = storage
            .cleanup_expired_active_keys(rotate_grace + 1)
            .await
            .expect("cleanup expired keys");
        assert!(expired_rows >= 1);

        let old_after_cleanup = storage
            .get_api_key_by_id(&key1.id)
            .await
            .expect("query old key after cleanup")
            .expect("old key exists");
        assert_eq!(old_after_cleanup.status, API_KEY_STATUS_EXPIRED);
    }
}
