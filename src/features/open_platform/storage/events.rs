use uuid::Uuid;

use crate::error::AppError;

use super::rows::row_to_api_key_event;
use super::{ApiKeyEventRecord, OpenPlatformStorage, SELECT_API_KEY_EVENTS_BY_KEY};

impl OpenPlatformStorage {
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
        let rows = sqlx::query(SELECT_API_KEY_EVENTS_BY_KEY)
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
