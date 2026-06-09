use sqlx::Row;

use crate::error::AppError;

use super::{ApiKeyEventRecord, ApiKeyRecord, DeveloperRecord};

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

pub(super) fn row_to_developer(row: &sqlx::sqlite::SqliteRow) -> DeveloperRecord {
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

pub(super) fn row_to_api_key(row: &sqlx::sqlite::SqliteRow) -> Result<ApiKeyRecord, AppError> {
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
        expires_at: row.try_get::<Option<i64>, _>("expires_at").ok().flatten(),
        revoked_at: row.try_get::<Option<i64>, _>("revoked_at").ok().flatten(),
        replaced_by_key_id: normalize_optional_text(row.try_get("replaced_by_key_id").ok()),
        last_used_at: row.try_get::<Option<i64>, _>("last_used_at").ok().flatten(),
        last_used_ip: normalize_optional_text(row.try_get("last_used_ip").ok()),
        usage_count: row.get("usage_count"),
    })
}

pub(super) fn row_to_api_key_event(
    row: &sqlx::sqlite::SqliteRow,
) -> Result<ApiKeyEventRecord, AppError> {
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
