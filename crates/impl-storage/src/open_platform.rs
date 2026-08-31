use std::sync::Arc;

use once_cell::sync::OnceCell;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::error::AppError;

mod api_keys;
mod connection;
mod developers;
mod events;
mod rows;
#[cfg(test)]
mod tests;

pub const API_KEY_STATUS_ACTIVE: &str = "active";
pub const API_KEY_STATUS_REVOKED: &str = "revoked";
pub const API_KEY_STATUS_EXPIRED: &str = "expired";
pub const API_KEY_STATUS_DELETED: &str = "deleted";

pub const API_KEY_EVENT_ISSUED: &str = "issued";
pub const API_KEY_EVENT_ROTATED: &str = "rotated";
pub const API_KEY_EVENT_REVOKED: &str = "revoked";
pub const API_KEY_EVENT_AUTH_FAILED: &str = "auth_failed";
pub const API_KEY_EVENT_DELETED: &str = "deleted";

static OPEN_PLATFORM_STORAGE: OnceCell<Arc<OpenPlatformStorage>> = OnceCell::new();

pub(super) const SELECT_DEVELOPER_BY_GITHUB_USER_ID: &str = "SELECT id, github_user_id, github_login, email, role, status, created_at, updated_at FROM developers WHERE github_user_id = ? LIMIT 1";
pub(super) const SELECT_DEVELOPER_BY_ID: &str = "SELECT id, github_user_id, github_login, email, role, status, created_at, updated_at FROM developers WHERE id = ? LIMIT 1";
pub(super) const SELECT_API_KEY_BY_ID: &str = "SELECT id, developer_id, name, key_prefix, key_last4, key_hash, scopes, status, created_at, expires_at, revoked_at, replaced_by_key_id, last_used_at, last_used_ip, usage_count FROM api_keys WHERE id = ? LIMIT 1";
pub(super) const SELECT_API_KEY_BY_HASH: &str = "SELECT id, developer_id, name, key_prefix, key_last4, key_hash, scopes, status, created_at, expires_at, revoked_at, replaced_by_key_id, last_used_at, last_used_ip, usage_count FROM api_keys WHERE key_hash = ? LIMIT 1";
pub(super) const SELECT_API_KEYS_BY_DEVELOPER: &str = "SELECT id, developer_id, name, key_prefix, key_last4, key_hash, scopes, status, created_at, expires_at, revoked_at, replaced_by_key_id, last_used_at, last_used_ip, usage_count FROM api_keys WHERE developer_id = ? ORDER BY created_at DESC";
pub(super) const SELECT_ACTIVE_API_KEYS_BY_DEVELOPER: &str = "SELECT id, developer_id, name, key_prefix, key_last4, key_hash, scopes, status, created_at, expires_at, revoked_at, replaced_by_key_id, last_used_at, last_used_ip, usage_count FROM api_keys WHERE developer_id = ? AND status = ? ORDER BY created_at DESC";
pub(super) const SELECT_API_KEY_EVENTS_BY_KEY: &str = "SELECT id, key_id, developer_id, event_type, event_reason, operator_id, request_id, created_at, metadata FROM api_key_events WHERE key_id = ? ORDER BY created_at DESC LIMIT ?";
pub(super) const CLEANUP_EXPIRED_ACTIVE_API_KEYS_SQL: &str = "UPDATE api_keys
             SET status = ?, revoked_at = COALESCE(revoked_at, expires_at)
             WHERE status = ? AND expires_at IS NOT NULL AND expires_at > 0 AND expires_at <= ?";

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
