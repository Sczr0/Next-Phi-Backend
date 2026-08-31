use sqlx::Row;
use uuid::Uuid;

use crate::error::AppError;

use super::rows::row_to_api_key;
use super::{
    API_KEY_EVENT_DELETED, API_KEY_EVENT_ISSUED, API_KEY_EVENT_REVOKED, API_KEY_EVENT_ROTATED,
    API_KEY_STATUS_ACTIVE, API_KEY_STATUS_DELETED, API_KEY_STATUS_EXPIRED, API_KEY_STATUS_REVOKED,
    ApiKeyRecord, CLEANUP_EXPIRED_ACTIVE_API_KEYS_SQL, CreateApiKeyParams, OpenPlatformStorage,
    RotateApiKeyParams, SELECT_ACTIVE_API_KEYS_BY_DEVELOPER, SELECT_API_KEY_BY_HASH,
    SELECT_API_KEY_BY_ID, SELECT_API_KEYS_BY_DEVELOPER,
};

impl OpenPlatformStorage {
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

        let row = sqlx::query(SELECT_API_KEY_BY_ID)
            .bind(&key_id)
            .fetch_one(&self.pool)
            .await
            .map_err(|e| AppError::Internal(format!("query api key after create: {e}")))?;
        row_to_api_key(&row)
    }

    pub async fn get_api_key_by_id(&self, key_id: &str) -> Result<Option<ApiKeyRecord>, AppError> {
        let row = sqlx::query(SELECT_API_KEY_BY_ID)
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
        let row = sqlx::query(SELECT_API_KEY_BY_HASH)
            .bind(key_hash)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| AppError::Internal(format!("query api key by hash: {e}")))?;
        row.map(|r| row_to_api_key(&r)).transpose()
    }

    pub async fn list_api_keys_by_developer(
        &self,
        developer_id: &str,
        include_inactive: bool,
    ) -> Result<Vec<ApiKeyRecord>, AppError> {
        let rows = if include_inactive {
            sqlx::query(SELECT_API_KEYS_BY_DEVELOPER)
                .bind(developer_id)
                .fetch_all(&self.pool)
                .await
                .map_err(|e| AppError::Internal(format!("list api keys by developer: {e}")))?
        } else {
            sqlx::query(SELECT_ACTIVE_API_KEYS_BY_DEVELOPER)
                .bind(developer_id)
                .bind(API_KEY_STATUS_ACTIVE)
                .fetch_all(&self.pool)
                .await
                .map_err(|e| {
                    AppError::Internal(format!("list active api keys by developer: {e}"))
                })?
        };

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
                id, developer_id, name, key_prefix, key_last4, key_hash, scopes, status, created_at, expires_at
             ) VALUES(?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
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
        .bind(Option::<i64>::None)
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

        let row = sqlx::query(SELECT_API_KEY_BY_ID)
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

    pub async fn soft_delete_api_key(
        &self,
        key_id: &str,
        reason: Option<&str>,
        operator_id: Option<&str>,
        request_id: Option<&str>,
        now_ts: i64,
    ) -> Result<(), AppError> {
        let row = sqlx::query("SELECT developer_id, status FROM api_keys WHERE id = ? LIMIT 1")
            .bind(key_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| AppError::Internal(format!("query api key before soft delete: {e}")))?;
        let Some(row) = row else {
            return Err(AppError::Validation("待删除的 API Key 不存在".into()));
        };
        let developer_id: String = row.get("developer_id");
        let status: String = row.get("status");
        if status == API_KEY_STATUS_DELETED {
            return Ok(());
        }

        sqlx::query(
            "UPDATE api_keys
             SET status = ?, revoked_at = COALESCE(revoked_at, ?), expires_at = COALESCE(expires_at, ?), replaced_by_key_id = NULL
             WHERE id = ?",
        )
        .bind(API_KEY_STATUS_DELETED)
        .bind(now_ts)
        .bind(now_ts)
        .bind(key_id)
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Internal(format!("soft delete api key: {e}")))?;

        self.record_api_key_event(
            key_id,
            &developer_id,
            API_KEY_EVENT_DELETED,
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
        let ret = sqlx::query(CLEANUP_EXPIRED_ACTIVE_API_KEYS_SQL)
            .bind(API_KEY_STATUS_EXPIRED)
            .bind(API_KEY_STATUS_ACTIVE)
            .bind(now_ts)
            .execute(&self.pool)
            .await
            .map_err(|e| AppError::Internal(format!("cleanup expired active keys: {e}")))?;
        Ok(ret.rows_affected())
    }
}
