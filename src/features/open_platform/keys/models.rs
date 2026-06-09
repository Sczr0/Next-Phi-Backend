use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateApiKeyRequest {
    /// Key 名称（控制台展示）
    pub name: String,
    /// scope 列表（为空则使用默认）
    #[serde(default)]
    pub scopes: Option<Vec<String>>,
    /// 环境：live 或 test（默认 live）
    #[serde(default)]
    pub environment: Option<String>,
    /// 过期时间戳（秒，可选）
    #[serde(default)]
    pub expires_at: Option<i64>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RotateApiKeyRequest {
    /// 可选：新 key 名称
    #[serde(default)]
    pub name: Option<String>,
    /// 可选：新 scopes
    #[serde(default)]
    pub scopes: Option<Vec<String>>,
    /// 可选：环境（live/test）
    #[serde(default)]
    pub environment: Option<String>,
    /// 可选：旧 key 过渡窗口（秒）
    #[serde(default)]
    pub grace_period_secs: Option<u64>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RevokeApiKeyRequest {
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeleteApiKeyRequest {
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventsQuery {
    #[serde(default)]
    pub limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiKeyRateLimitQuery {
    #[serde(default)]
    pub include_client_ip: bool,
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiKeyListQuery {
    #[serde(default)]
    pub include_inactive: bool,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ApiKeyIssueResponse {
    pub id: String,
    pub name: String,
    pub token: String,
    pub key_prefix: String,
    pub key_last4: String,
    pub key_masked: String,
    pub scopes: Vec<String>,
    pub status: String,
    pub created_at: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<i64>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ApiKeyListItem {
    pub id: String,
    pub name: String,
    pub key_prefix: String,
    pub key_last4: String,
    pub key_masked: String,
    pub scopes: Vec<String>,
    pub status: String,
    pub created_at: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revoked_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub replaced_by_key_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_used_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_used_ip: Option<String>,
    pub usage_count: i64,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ApiKeyListResponse {
    pub items: Vec<ApiKeyListItem>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OkResponse {
    pub ok: bool,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ApiKeyEventItem {
    pub id: String,
    pub key_id: String,
    pub event_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operator_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    pub created_at: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ApiKeyEventsResponse {
    pub items: Vec<ApiKeyEventItem>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ApiKeyRateLimitBucketItem {
    pub route: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_ip: Option<String>,
    pub request_count: u32,
    pub remaining: u32,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ApiKeyRateLimitResponse {
    pub key_id: String,
    pub strategy: String,
    pub per_minute_limit: u32,
    pub minute_slot: i64,
    pub window_start_ts: i64,
    pub window_end_ts: i64,
    pub total_request_count: u64,
    pub bucket_count: usize,
    pub buckets: Vec<ApiKeyRateLimitBucketItem>,
}
