use crate::extract::{ValidatedJson, ValidatedPath, ValidatedQuery};
use axum::{
    Json,
    http::{HeaderMap, StatusCode},
};

use crate::{
    error::AppError,
    features::open_platform::{auth, storage, token_auth},
};

use super::{
    helpers::{
        derive_key_last4, ensure_key_owned_by_developer, ensure_open_platform_enabled,
        generate_api_key, hash_api_key, map_key_list_item, mask_key, normalize_environment,
        normalize_scopes, resolve_key_hash_secret, resolve_prefix, sanitize_name,
        saturating_u64_to_i64,
    },
    models::{
        ApiKeyEventItem, ApiKeyEventsResponse, ApiKeyIssueResponse, ApiKeyListQuery,
        ApiKeyListResponse, ApiKeyRateLimitBucketItem, ApiKeyRateLimitQuery,
        ApiKeyRateLimitResponse, CreateApiKeyRequest, DeleteApiKeyRequest, EventsQuery, OkResponse,
        RevokeApiKeyRequest, RotateApiKeyRequest,
    },
};

#[utoipa::path(
    post,
    path = "/developer/api-keys",
    summary = "创建 API Key（明文仅返回一次）",
    request_body = CreateApiKeyRequest,
    security(("DeveloperCookie" = [])),
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
        ),
        (
            status = 500,
            description = "服务端内部错误（如 hash 密钥未配置）",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        )
    ),
    tag = "OpenPlatformKeys"
)]
pub async fn post_create_api_key(
    headers: HeaderMap,
    ValidatedJson(req): ValidatedJson<CreateApiKeyRequest>,
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
    security(("DeveloperCookie" = [])),
    responses(
        (status = 200, description = "查询成功", body = ApiKeyListResponse),
        (
            status = 401,
            description = "开发者会话无效",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        ),
        (
            status = 422,
            description = "查询参数无效（如 includeInactive 非布尔值）",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        ),
        (
            status = 500,
            description = "服务端内部错误（存储未初始化）",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        )
    ),
    tag = "OpenPlatformKeys"
)]
pub async fn get_api_keys(
    headers: HeaderMap,
    ValidatedQuery(query): ValidatedQuery<ApiKeyListQuery>,
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
    security(("DeveloperCookie" = [])),
    responses(
        (status = 201, description = "轮换成功", body = ApiKeyIssueResponse),
        (
            status = 401,
            description = "开发者会话无效或无权操作该 Key",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        ),
        (
            status = 404,
            description = "API Key 不存在",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        ),
        (
            status = 422,
            description = "参数校验失败",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        ),
        (
            status = 500,
            description = "服务端内部错误（如 hash 密钥未配置）",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        )
    ),
    tag = "OpenPlatformKeys"
)]
pub async fn post_rotate_api_key(
    headers: HeaderMap,
    ValidatedPath(key_id): ValidatedPath<String>,
    ValidatedJson(req): ValidatedJson<RotateApiKeyRequest>,
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
        Some(now + saturating_u64_to_i64(grace_secs.min(7 * 24 * 3600)))
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
    security(("DeveloperCookie" = [])),
    responses(
        (status = 200, description = "撤销成功", body = OkResponse),
        (
            status = 401,
            description = "开发者会话无效或无权操作该 Key",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        ),
        (
            status = 404,
            description = "API Key 不存在",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        ),
        (
            status = 422,
            description = "请求体 JSON 无效",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        ),
        (
            status = 500,
            description = "服务端内部错误（存储未初始化）",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        )
    ),
    tag = "OpenPlatformKeys"
)]
pub async fn post_revoke_api_key(
    headers: HeaderMap,
    ValidatedPath(key_id): ValidatedPath<String>,
    ValidatedJson(req): ValidatedJson<RevokeApiKeyRequest>,
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
    security(("DeveloperCookie" = [])),
    responses(
        (status = 200, description = "删除成功", body = OkResponse),
        (
            status = 401,
            description = "开发者会话无效或无权操作该 Key",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        ),
        (
            status = 404,
            description = "API Key 不存在",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        ),
        (
            status = 422,
            description = "请求体 JSON 无效",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        ),
        (
            status = 500,
            description = "服务端内部错误（存储未初始化）",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        )
    ),
    tag = "OpenPlatformKeys"
)]
pub async fn post_delete_api_key(
    headers: HeaderMap,
    ValidatedPath(key_id): ValidatedPath<String>,
    ValidatedJson(req): ValidatedJson<DeleteApiKeyRequest>,
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
        ("limit" = Option<i64>, Query, minimum = 1, maximum = 500, description = "返回条数，默认 100，最大 500")
    ),
    security(("DeveloperCookie" = [])),
    responses(
        (status = 200, description = "查询成功", body = ApiKeyEventsResponse),
        (
            status = 401,
            description = "开发者会话无效或无权操作该 Key",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        ),
        (
            status = 404,
            description = "API Key 不存在",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        ),
        (
            status = 422,
            description = "查询参数无效（如 limit 非整数）",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        ),
        (
            status = 500,
            description = "服务端内部错误（存储未初始化）",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        )
    ),
    tag = "OpenPlatformKeys"
)]
pub async fn get_api_key_events(
    headers: HeaderMap,
    ValidatedPath(key_id): ValidatedPath<String>,
    ValidatedQuery(query): ValidatedQuery<EventsQuery>,
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

#[utoipa::path(
    get,
    path = "/developer/api-keys/{key_id}/rate-limit",
    summary = "查询 API Key 限流窗口信息",
    params(
        ("key_id" = String, Path, description = "key_id"),
        ("includeClientIp" = Option<bool>, Query, description = "是否按 client_ip 展开，默认 false"),
        ("limit" = Option<usize>, Query, minimum = 1, maximum = 500, description = "最多返回桶数量，默认 100，最大 500")
    ),
    security(("DeveloperCookie" = [])),
    responses(
        (status = 200, description = "查询成功", body = ApiKeyRateLimitResponse),
        (
            status = 401,
            description = "开发者会话无效或无权操作该 Key",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        ),
        (
            status = 404,
            description = "API Key 不存在",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        ),
        (
            status = 422,
            description = "查询参数无效（如 includeClientIp 非布尔或 limit 非整数）",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        ),
        (
            status = 500,
            description = "服务端内部错误（存储未初始化）",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        )
    ),
    tag = "OpenPlatformKeys"
)]
pub async fn get_api_key_rate_limit(
    headers: HeaderMap,
    ValidatedPath(key_id): ValidatedPath<String>,
    ValidatedQuery(query): ValidatedQuery<ApiKeyRateLimitQuery>,
) -> Result<(StatusCode, Json<ApiKeyRateLimitResponse>), AppError> {
    let cfg = ensure_open_platform_enabled()?;
    let developer = auth::require_developer(&headers).await?;
    let _old_key = ensure_key_owned_by_developer(&key_id, &developer.id).await?;

    let now_ts = chrono::Utc::now().timestamp();
    let limit = query.limit.unwrap_or(100).clamp(1, 500);
    let per_minute_limit = cfg.api_key.rate_limit_per_minute.max(1);
    let snapshot =
        token_auth::snapshot_rate_limit_by_key(&key_id, query.include_client_ip, limit, now_ts)
            .await;
    let buckets = snapshot
        .buckets
        .into_iter()
        .map(|bucket| ApiKeyRateLimitBucketItem {
            route: bucket.route,
            client_ip: bucket.client_ip,
            request_count: bucket.request_count,
            remaining: per_minute_limit.saturating_sub(bucket.request_count),
        })
        .collect();

    Ok((
        StatusCode::OK,
        Json(ApiKeyRateLimitResponse {
            key_id,
            strategy: "apiKey+route+clientIp".to_string(),
            per_minute_limit,
            minute_slot: snapshot.minute_slot,
            window_start_ts: snapshot.minute_slot.saturating_mul(60),
            window_end_ts: snapshot.minute_slot.saturating_mul(60).saturating_add(59),
            total_request_count: snapshot.total_request_count,
            bucket_count: snapshot.bucket_count,
            buckets,
        }),
    ))
}
