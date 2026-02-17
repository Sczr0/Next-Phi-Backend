use axum::{
    Json, Router,
    extract::{Path, Query},
    http::{HeaderMap, StatusCode},
    routing::{get, post},
};
use base64::Engine;
use rand::RngCore;
use serde::{Deserialize, Serialize};

use crate::{
    config::AppConfig,
    error::AppError,
    features::open_platform::{auth, storage},
    state::AppState,
};

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

fn ensure_open_platform_enabled() -> Result<&'static crate::config::OpenPlatformConfig, AppError> {
    let cfg = &AppConfig::global().open_platform;
    if !cfg.enabled {
        return Err(AppError::Validation("开放平台未启用".into()));
    }
    Ok(cfg)
}

fn normalize_environment(raw: Option<&str>) -> Result<&'static str, AppError> {
    let env = raw.unwrap_or("live").trim();
    if env.eq_ignore_ascii_case("live") {
        Ok("live")
    } else if env.eq_ignore_ascii_case("test") {
        Ok("test")
    } else {
        Err(AppError::Validation(
            "environment 仅支持 live 或 test".into(),
        ))
    }
}

fn sanitize_name(name: &str) -> Result<String, AppError> {
    let n = name.trim();
    if n.is_empty() {
        return Err(AppError::Validation("name 不能为空".into()));
    }
    if n.chars().count() > 64 {
        return Err(AppError::Validation("name 过长（最大 64 字符）".into()));
    }
    Ok(n.to_string())
}

fn normalize_scopes(
    cfg: &crate::config::OpenPlatformConfig,
    scopes: Option<Vec<String>>,
) -> Result<Vec<String>, AppError> {
    let raw = scopes.unwrap_or_else(|| cfg.api_key.default_scopes.clone());
    let mut out = Vec::<String>::new();
    for scope in raw {
        let s = scope.trim();
        if s.is_empty() {
            continue;
        }
        if !out.iter().any(|x| x == s) {
            out.push(s.to_string());
        }
    }
    if out.is_empty() {
        return Err(AppError::Validation("scopes 不能为空".into()));
    }
    Ok(out)
}

fn resolve_prefix(cfg: &crate::config::OpenPlatformConfig, env: &str) -> String {
    if env == "test" {
        cfg.api_key.test_prefix.clone()
    } else {
        cfg.api_key.live_prefix.clone()
    }
}

fn resolve_key_hash_secret(cfg: &crate::config::OpenPlatformConfig) -> Result<String, AppError> {
    if !cfg.api_key.hash_secret.trim().is_empty() {
        return Ok(cfg.api_key.hash_secret.clone());
    }
    let from_env = std::env::var("APP_OPEN_PLATFORM_API_KEY_HASH_SECRET").unwrap_or_default();
    if !from_env.trim().is_empty() {
        return Ok(from_env);
    }
    if !cfg.session.jwt_secret.trim().is_empty() {
        return Ok(cfg.session.jwt_secret.clone());
    }
    let session_from_env =
        std::env::var("APP_OPEN_PLATFORM_SESSION_JWT_SECRET").unwrap_or_default();
    if !session_from_env.trim().is_empty() {
        return Ok(session_from_env);
    }
    Err(AppError::Internal(
        "未配置 API Key hash 密钥（open_platform.api_key.hash_secret 或 APP_OPEN_PLATFORM_API_KEY_HASH_SECRET）".into(),
    ))
}

fn generate_api_key(prefix: &str, random_bytes: usize) -> String {
    let byte_len = random_bytes.clamp(16, 64);
    let mut bytes = vec![0u8; byte_len];
    rand::thread_rng().fill_bytes(&mut bytes);
    let suffix = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);
    format!("{prefix}{suffix}")
}

fn derive_key_last4(token: &str) -> String {
    let mut chars: Vec<char> = token.chars().rev().take(4).collect();
    chars.reverse();
    chars.into_iter().collect()
}

fn hash_api_key(secret: &str, token: &str) -> String {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;

    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("hmac key");
    mac.update(token.as_bytes());
    let out = mac.finalize().into_bytes();
    hex::encode(out)
}

fn mask_key(prefix: &str, last4: &str) -> String {
    format!("{prefix}****{last4}")
}

fn map_key_list_item(item: storage::ApiKeyRecord) -> ApiKeyListItem {
    ApiKeyListItem {
        id: item.id,
        name: item.name,
        key_prefix: item.key_prefix.clone(),
        key_last4: item.key_last4.clone(),
        key_masked: mask_key(&item.key_prefix, &item.key_last4),
        scopes: item.scopes,
        status: item.status,
        created_at: item.created_at,
        expires_at: item.expires_at,
        revoked_at: item.revoked_at,
        replaced_by_key_id: item.replaced_by_key_id,
        last_used_at: item.last_used_at,
        last_used_ip: item.last_used_ip,
        usage_count: item.usage_count,
    }
}

async fn ensure_key_owned_by_developer(
    key_id: &str,
    developer_id: &str,
) -> Result<storage::ApiKeyRecord, AppError> {
    let st = storage::global()?;
    let key = st
        .get_api_key_by_id(key_id)
        .await?
        .ok_or(AppError::Search(crate::error::SearchError::NotFound))?;
    if key.developer_id != developer_id {
        return Err(AppError::Auth("无权操作该 API Key".into()));
    }
    Ok(key)
}

#[utoipa::path(
    post,
    path = "/developer/api-keys",
    summary = "创建 API Key（明文仅返回一次）",
    request_body = CreateApiKeyRequest,
    responses(
        (status = 201, description = "创建成功", body = ApiKeyIssueResponse),
        (
            status = 401,
            description = "开发者会话无效",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        ),
        (
            status = 422,
            description = "参数校验失败",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        )
    ),
    tag = "OpenPlatformKeys"
)]
pub async fn post_create_api_key(
    headers: HeaderMap,
    Json(req): Json<CreateApiKeyRequest>,
) -> Result<(StatusCode, Json<ApiKeyIssueResponse>), AppError> {
    let cfg = ensure_open_platform_enabled()?;
    let developer = auth::require_developer(&headers).await?;
    let name = sanitize_name(&req.name)?;
    let scopes = normalize_scopes(cfg, req.scopes)?;
    let env = normalize_environment(req.environment.as_deref())?;
    let prefix = resolve_prefix(cfg, env);

    let now = chrono::Utc::now().timestamp();
    if let Some(exp) = req.expires_at
        && exp <= now
    {
        return Err(AppError::Validation("expiresAt 必须大于当前时间".into()));
    }

    let token = generate_api_key(&prefix, cfg.api_key.random_bytes);
    let key_last4 = derive_key_last4(&token);
    let hash_secret = resolve_key_hash_secret(cfg)?;
    let key_hash = hash_api_key(&hash_secret, &token);

    let st = storage::global()?;
    let created = st
        .create_api_key(storage::CreateApiKeyParams {
            developer_id: developer.id.clone(),
            name,
            key_prefix: prefix,
            key_last4,
            key_hash,
            scopes,
            expires_at: req.expires_at,
            now_ts: now,
        })
        .await?;
    let _ = st
        .record_api_key_event(
            &created.id,
            &developer.id,
            storage::API_KEY_EVENT_ISSUED,
            Some("key created"),
            Some(&developer.id),
            crate::request_id::current_request_id().as_deref(),
            None,
            now,
        )
        .await;

    Ok((
        StatusCode::CREATED,
        Json(ApiKeyIssueResponse {
            id: created.id,
            name: created.name,
            token,
            key_prefix: created.key_prefix.clone(),
            key_last4: created.key_last4.clone(),
            key_masked: mask_key(&created.key_prefix, &created.key_last4),
            scopes: created.scopes,
            status: created.status,
            created_at: created.created_at,
            expires_at: created.expires_at,
        }),
    ))
}

#[utoipa::path(
    get,
    path = "/developer/api-keys",
    summary = "列出当前开发者 API Keys（掩码）",
    params(
        ("includeInactive" = Option<bool>, Query, description = "是否包含非 active 的历史 Key（默认 false）")
    ),
    responses(
        (status = 200, description = "查询成功", body = ApiKeyListResponse),
        (
            status = 401,
            description = "开发者会话无效",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        )
    ),
    tag = "OpenPlatformKeys"
)]
pub async fn get_api_keys(
    headers: HeaderMap,
    Query(query): Query<ApiKeyListQuery>,
) -> Result<(StatusCode, Json<ApiKeyListResponse>), AppError> {
    let developer = auth::require_developer(&headers).await?;
    let st = storage::global()?;
    let items = st
        .list_api_keys_by_developer(&developer.id, query.include_inactive)
        .await?;
    Ok((
        StatusCode::OK,
        Json(ApiKeyListResponse {
            items: items.into_iter().map(map_key_list_item).collect(),
        }),
    ))
}

#[utoipa::path(
    post,
    path = "/developer/api-keys/{key_id}/rotate",
    summary = "轮换 API Key",
    request_body = RotateApiKeyRequest,
    params(("key_id" = String, Path, description = "待轮换的 key_id")),
    responses(
        (status = 201, description = "轮换成功", body = ApiKeyIssueResponse),
        (
            status = 401,
            description = "开发者会话无效",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        ),
        (
            status = 422,
            description = "参数校验失败",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        )
    ),
    tag = "OpenPlatformKeys"
)]
pub async fn post_rotate_api_key(
    headers: HeaderMap,
    Path(key_id): Path<String>,
    Json(req): Json<RotateApiKeyRequest>,
) -> Result<(StatusCode, Json<ApiKeyIssueResponse>), AppError> {
    let cfg = ensure_open_platform_enabled()?;
    let developer = auth::require_developer(&headers).await?;
    let old_key = ensure_key_owned_by_developer(&key_id, &developer.id).await?;

    let name = sanitize_name(req.name.as_deref().unwrap_or(&old_key.name))?;
    let scopes = normalize_scopes(cfg, req.scopes.or_else(|| Some(old_key.scopes.clone())))?;
    let env = normalize_environment(req.environment.as_deref())?;
    let prefix = resolve_prefix(cfg, env);
    let now = chrono::Utc::now().timestamp();

    let grace_secs = req
        .grace_period_secs
        .unwrap_or(cfg.api_key.rotate_grace_secs);
    let grace_expires_at = if grace_secs == 0 {
        None
    } else {
        Some(now + grace_secs.min(7 * 24 * 3600) as i64)
    };

    let token = generate_api_key(&prefix, cfg.api_key.random_bytes);
    let key_last4 = derive_key_last4(&token);
    let hash_secret = resolve_key_hash_secret(cfg)?;
    let key_hash = hash_api_key(&hash_secret, &token);

    let st = storage::global()?;
    let request_id = crate::request_id::current_request_id();
    let created = st
        .rotate_api_key(storage::RotateApiKeyParams {
            key_id,
            new_name: name,
            new_key_prefix: prefix,
            new_key_last4: key_last4,
            new_key_hash: key_hash,
            new_scopes: scopes,
            grace_expires_at,
            now_ts: now,
            operator_id: Some(developer.id.clone()),
            request_id,
        })
        .await?;

    Ok((
        StatusCode::CREATED,
        Json(ApiKeyIssueResponse {
            id: created.id,
            name: created.name,
            token,
            key_prefix: created.key_prefix.clone(),
            key_last4: created.key_last4.clone(),
            key_masked: mask_key(&created.key_prefix, &created.key_last4),
            scopes: created.scopes,
            status: created.status,
            created_at: created.created_at,
            expires_at: created.expires_at,
        }),
    ))
}

#[utoipa::path(
    post,
    path = "/developer/api-keys/{key_id}/revoke",
    summary = "撤销 API Key",
    request_body = RevokeApiKeyRequest,
    params(("key_id" = String, Path, description = "待撤销的 key_id")),
    responses(
        (status = 200, description = "撤销成功", body = OkResponse),
        (
            status = 401,
            description = "开发者会话无效",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        )
    ),
    tag = "OpenPlatformKeys"
)]
pub async fn post_revoke_api_key(
    headers: HeaderMap,
    Path(key_id): Path<String>,
    Json(req): Json<RevokeApiKeyRequest>,
) -> Result<(StatusCode, Json<OkResponse>), AppError> {
    let developer = auth::require_developer(&headers).await?;
    let _old_key = ensure_key_owned_by_developer(&key_id, &developer.id).await?;
    let st = storage::global()?;
    st.revoke_api_key(
        &key_id,
        req.reason.as_deref(),
        Some(&developer.id),
        crate::request_id::current_request_id().as_deref(),
        chrono::Utc::now().timestamp(),
    )
    .await?;
    Ok((StatusCode::OK, Json(OkResponse { ok: true })))
}

#[utoipa::path(
    post,
    path = "/developer/api-keys/{key_id}/delete",
    summary = "删除 API Key（软删除）",
    request_body = DeleteApiKeyRequest,
    params(("key_id" = String, Path, description = "待删除的 key_id")),
    responses(
        (status = 200, description = "删除成功", body = OkResponse),
        (
            status = 401,
            description = "开发者会话无效",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        )
    ),
    tag = "OpenPlatformKeys"
)]
pub async fn post_delete_api_key(
    headers: HeaderMap,
    Path(key_id): Path<String>,
    Json(req): Json<DeleteApiKeyRequest>,
) -> Result<(StatusCode, Json<OkResponse>), AppError> {
    let developer = auth::require_developer(&headers).await?;
    let _old_key = ensure_key_owned_by_developer(&key_id, &developer.id).await?;
    let st = storage::global()?;
    st.soft_delete_api_key(
        &key_id,
        req.reason.as_deref(),
        Some(&developer.id),
        crate::request_id::current_request_id().as_deref(),
        chrono::Utc::now().timestamp(),
    )
    .await?;
    Ok((StatusCode::OK, Json(OkResponse { ok: true })))
}

#[utoipa::path(
    get,
    path = "/developer/api-keys/{key_id}/events",
    summary = "查询 API Key 事件",
    params(
        ("key_id" = String, Path, description = "key_id"),
        ("limit" = Option<i64>, Query, description = "返回条数，默认 100")
    ),
    responses(
        (status = 200, description = "查询成功", body = ApiKeyEventsResponse),
        (
            status = 401,
            description = "开发者会话无效",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        )
    ),
    tag = "OpenPlatformKeys"
)]
pub async fn get_api_key_events(
    headers: HeaderMap,
    Path(key_id): Path<String>,
    Query(query): Query<EventsQuery>,
) -> Result<(StatusCode, Json<ApiKeyEventsResponse>), AppError> {
    let developer = auth::require_developer(&headers).await?;
    let _old_key = ensure_key_owned_by_developer(&key_id, &developer.id).await?;
    let st = storage::global()?;
    let events = st
        .list_api_key_events(&key_id, query.limit.unwrap_or(100))
        .await?;

    Ok((
        StatusCode::OK,
        Json(ApiKeyEventsResponse {
            items: events
                .into_iter()
                .map(|e| ApiKeyEventItem {
                    id: e.id,
                    key_id: e.key_id,
                    event_type: e.event_type,
                    event_reason: e.event_reason,
                    operator_id: e.operator_id,
                    request_id: e.request_id,
                    created_at: e.created_at,
                    metadata: e.metadata,
                })
                .collect(),
        }),
    ))
}

pub fn create_open_platform_keys_router() -> Router<AppState> {
    Router::<AppState>::new()
        .route("/developer/api-keys", post(post_create_api_key))
        .route("/developer/api-keys", get(get_api_keys))
        .route(
            "/developer/api-keys/:key_id/rotate",
            post(post_rotate_api_key),
        )
        .route(
            "/developer/api-keys/:key_id/revoke",
            post(post_revoke_api_key),
        )
        .route(
            "/developer/api-keys/:key_id/delete",
            post(post_delete_api_key),
        )
        .route(
            "/developer/api-keys/:key_id/events",
            get(get_api_key_events),
        )
}

#[cfg(test)]
mod tests {
    use super::{derive_key_last4, mask_key, sanitize_name};

    #[test]
    fn sanitize_name_rejects_empty() {
        assert!(sanitize_name("   ").is_err());
        assert!(sanitize_name("valid-name").is_ok());
    }

    #[test]
    fn key_masking_works() {
        let last4 = derive_key_last4("pgr_live_xxxxxxxxxxABCD");
        assert_eq!(last4, "ABCD");
        let masked = mask_key("pgr_live_", &last4);
        assert_eq!(masked, "pgr_live_****ABCD");
    }
}
