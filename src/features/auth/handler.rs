use axum::{
    Router,
    extract::{Path, Query, State},
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Json, Response},
    routing::{get, post},
};
use base64::Engine;
use qrcode::{QrCode, render::svg};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::AppError;
use crate::state::AppState;

use super::qrcode_service::QrCodeStatus;

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UserIdResponse {
    /// 去敏后的稳定用户 ID（32 位 hex，等价于 stats/leaderboard 使用的 user_hash）
    #[schema(example = "ab12cd34ef56ab12cd34ef56ab12cd34")]
    pub user_id: String,
    /// 用于推导 user_id 的凭证类型（用于排查“为什么和以前不一致”）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_kind: Option<String>,
}

#[utoipa::path(
    post,
    path = "/auth/user-id",
    summary = "根据凭证生成去敏用户ID",
    description = "使用服务端配置的 stats.user_hash_salt 对凭证做 HMAC-SHA256 去敏（取前 16 字节，32 位 hex），用于同一用户的稳定标识。注意：salt 变更会导致 user_id 整体变化。",
    request_body = crate::features::save::models::UnifiedSaveRequest,
    responses(
        (status = 200, description = "生成成功", body = UserIdResponse),
        (
            status = 422,
            description = "凭证缺失/无效，或无法识别用户",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        ),
        (
            status = 500,
            description = "服务端未配置 user_hash_salt",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        )
    ),
    tag = "Auth"
)]
pub async fn post_user_id(
    Json(auth): Json<crate::features::save::models::UnifiedSaveRequest>,
) -> Result<(StatusCode, Json<UserIdResponse>), AppError> {
    // 与 /save 的凭证互斥语义保持一致，避免“同一个请求不同接口得到不同身份”的困惑。
    if auth.session_token.is_some() && auth.external_credentials.is_some() {
        return Err(AppError::Validation(
            "不能同时提供 sessionToken 和 externalCredentials，请只选择其中一种认证方式".into(),
        ));
    }

    let stable_ok = if let Some(tok) = auth.session_token.as_deref() {
        !tok.is_empty()
    } else if let Some(ext) = auth.external_credentials.as_ref() {
        let has_api_user_id = ext.api_user_id.as_deref().is_some_and(|v| !v.is_empty());
        let has_sessiontoken = ext.sessiontoken.as_deref().is_some_and(|v| !v.is_empty());
        let has_platform_pair = match (&ext.platform, &ext.platform_id) {
            (Some(p), Some(pid)) => !p.is_empty() && !pid.is_empty(),
            _ => false,
        };
        has_api_user_id || has_sessiontoken || has_platform_pair
    } else {
        false
    };
    if !stable_ok {
        return Err(AppError::Validation(
            "无法识别用户：请提供 sessionToken，或 externalCredentials 中的 platform+platformId / sessiontoken / apiUserId（且不能为空）".into(),
        ));
    }

    let salt = crate::config::AppConfig::global()
        .stats
        .user_hash_salt
        .as_deref()
        .ok_or_else(|| {
            AppError::Internal(
                "stats.user_hash_salt 未配置，无法生成稳定 user_id（可通过 APP_STATS_USER_HASH_SALT 设置）"
                    .into(),
            )
        })?;

    let (user_id_opt, user_kind) =
        crate::features::stats::derive_user_identity_from_auth(Some(salt), &auth);
    let user_id = user_id_opt.ok_or_else(|| AppError::Internal("生成 user_id 失败".into()))?;
    Ok((StatusCode::OK, Json(UserIdResponse { user_id, user_kind })))
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct QrCodeCreateResponse {
    /// 二维码标识，用于轮询状态
    #[schema(example = "8b8f2f8a-1a2b-4c3d-9e0f-112233445566")]
    pub qr_id: String,
    /// 用户在浏览器中访问以确认授权的 URL
    #[schema(example = "https://www.taptap.com/account/device?code=abcd-efgh")]
    pub verification_url: String,
    /// SVG 二维码的 data URL（base64 编码）
    #[schema(example = "data:image/svg+xml;base64,PHN2ZyB4bWxucz0uLi4=")]
    pub qrcode_base64: String,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct QrCodeStatusResponse {
    /// 当前状态：Pending/Scanned/Confirmed/Error/Expired
    #[schema(example = "Pending")]
    pub status: QrCodeStatusValue,
    /// 若 Confirmed，返回 LeanCloud Session Token
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_token: Option<String>,
    /// 可选：机器可读的错误码（仅在 status=Error 时出现）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
    /// 可选的人类可读提示消息
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// 若需延后轮询，返回建议的等待秒数
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retry_after: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "PascalCase")]
pub enum QrCodeStatusValue {
    Pending,
    Scanned,
    Confirmed,
    Error,
    Expired,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct QrCodeQuery {
    /// TapTap 版本：cn（大陆版）或 global（国际版）
    #[serde(default)]
    taptap_version: Option<String>,
}

fn normalize_taptap_version(v: Option<&str>) -> Result<Option<&'static str>, AppError> {
    let Some(v) = v else {
        return Ok(None);
    };
    let v = v.trim();
    if v.is_empty() {
        return Ok(None);
    }
    if v.eq_ignore_ascii_case("cn") {
        return Ok(Some("cn"));
    }
    if v.eq_ignore_ascii_case("global") {
        return Ok(Some("global"));
    }
    Err(AppError::Validation(
        "taptapVersion 必须为 cn 或 global".to_string(),
    ))
}

fn json_no_store<T: Serialize>(status: StatusCode, body: T) -> Response {
    let mut res = (status, Json(body)).into_response();
    res.headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    res
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SessionExchangeRequest {
    #[serde(flatten)]
    pub auth: crate::features::save::models::UnifiedSaveRequest,
}
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SessionExchangeResponse {
    pub access_token: String,
    pub expires_in: u64,
    pub token_type: &'static str,
}
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SessionLogoutRequest {
    pub scope: SessionLogoutScope,
}
#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SessionLogoutScope {
    Current,
    All,
}
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SessionLogoutResponse {
    pub scope: SessionLogoutScope,
    pub revoked_jti: String,
    pub logout_before: Option<String>,
}
#[derive(Debug, Serialize, Deserialize, Clone)]
struct SessionClaims {
    sub: String,
    jti: String,
    iss: String,
    aud: String,
    iat: i64,
    exp: i64,
}
#[derive(Debug)]
struct AuthzToken {
    claims: SessionClaims,
}
fn ensure_session_config() -> Result<&'static crate::config::SessionConfig, AppError> {
    let cfg = &crate::config::AppConfig::global().session;
    if !cfg.enabled {
        return Err(AppError::Auth("会话接口未启用".into()));
    }
    Ok(cfg)
}

fn resolve_jwt_secret(cfg: &crate::config::SessionConfig) -> Result<String, AppError> {
    if !cfg.jwt_secret.trim().is_empty() {
        return Ok(cfg.jwt_secret.clone());
    }
    let from_env = std::env::var("APP_SESSION_JWT_SECRET").unwrap_or_default();
    if !from_env.trim().is_empty() {
        return Ok(from_env);
    }
    Err(AppError::Internal(
        "session.jwt_secret 未配置（可通过 APP_SESSION_JWT_SECRET 设置）".into(),
    ))
}

fn resolve_expected_exchange_secret(
    cfg: &crate::config::SessionConfig,
) -> Result<String, AppError> {
    if !cfg.exchange_shared_secret.trim().is_empty() {
        return Ok(cfg.exchange_shared_secret.clone());
    }
    let from_env = std::env::var("APP_SESSION_EXCHANGE_SHARED_SECRET").unwrap_or_default();
    if !from_env.trim().is_empty() {
        return Ok(from_env);
    }
    Err(AppError::Internal(
        "session.exchange_shared_secret 未配置（可通过 APP_SESSION_EXCHANGE_SHARED_SECRET 设置）"
            .into(),
    ))
}
fn resolve_exchange_secret(headers: &axum::http::HeaderMap) -> Option<String> {
    headers
        .get("x-exchange-secret")
        .or_else(|| headers.get("x-session-exchange-secret"))
        .and_then(|v| v.to_str().ok())
        .map(|v| v.trim().to_string())
}
fn extract_bearer_token(headers: &axum::http::HeaderMap) -> Result<String, AppError> {
    let raw = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| AppError::Auth("缺少 Authorization 头".into()))?;
    let raw = raw.trim();
    if !raw.starts_with("Bearer ") {
        return Err(AppError::Auth("Authorization 必须使用 Bearer 方案".into()));
    }
    let token = raw.trim_start_matches("Bearer ").trim();
    if token.is_empty() {
        return Err(AppError::Auth("Bearer token 不能为空".into()));
    }
    Ok(token.to_string())
}
fn decode_access_token(
    token: &str,
    cfg: &crate::config::SessionConfig,
) -> Result<SessionClaims, AppError> {
    let jwt_secret = resolve_jwt_secret(cfg)?;
    let mut validation = jsonwebtoken::Validation::new(jsonwebtoken::Algorithm::HS256);
    validation.validate_exp = true;
    validation.set_issuer(&[cfg.jwt_issuer.as_str()]);
    validation.set_audience(&[cfg.jwt_audience.as_str()]);
    let data = jsonwebtoken::decode::<SessionClaims>(
        token,
        &jsonwebtoken::DecodingKey::from_secret(jwt_secret.as_bytes()),
        &validation,
    )
    .map_err(|_| AppError::Auth("会话令牌无效或已过期".into()))?;
    Ok(data.claims)
}
async fn parse_authorization_token(
    headers: &axum::http::HeaderMap,
    cfg: &crate::config::SessionConfig,
) -> Result<AuthzToken, AppError> {
    let token = extract_bearer_token(headers)?;
    let claims = decode_access_token(&token, cfg)?;
    Ok(AuthzToken { claims })
}

async fn try_cleanup_expired_session_records(state: &AppState) {
    let Some(storage) = state.stats_storage.as_ref() else {
        return;
    };
    if let Err(e) = storage
        .maybe_cleanup_expired_session_records(chrono::Utc::now())
        .await
    {
        tracing::warn!(err = %e, "session cleanup failed");
    }
}
#[utoipa::path(
    post,
    path = "/auth/session/exchange",
    summary = "签发后端会话令牌",
    description = "Next.js 服务端调用该接口换取后端短期 access token。",
    request_body = SessionExchangeRequest,
    params(("X-Exchange-Secret" = String, Header, description = "Next.js 与后端共享密钥")),
    responses(
        (status = 200, description = "签发成功", body = SessionExchangeResponse),
        (
            status = 401,
            description = "共享密钥无效",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        ),
        (
            status = 422,
            description = "凭证无效",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        ),
        (
            status = 500,
            description = "服务端配置错误",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        )
    ),
    tag = "Auth"
)]
pub async fn post_session_exchange(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(req): Json<SessionExchangeRequest>,
) -> Result<(StatusCode, Json<SessionExchangeResponse>), AppError> {
    try_cleanup_expired_session_records(&state).await;
    let cfg = ensure_session_config()?;
    let expected_exchange_secret = resolve_expected_exchange_secret(cfg)?;
    let jwt_secret = resolve_jwt_secret(cfg)?;
    let provided = resolve_exchange_secret(&headers).unwrap_or_default();
    if provided.is_empty() || provided != expected_exchange_secret {
        return Err(AppError::Auth("交换密钥无效".into()));
    }
    let auth = req.auth;
    if auth.session_token.is_some() && auth.external_credentials.is_some() {
        return Err(AppError::Validation(
            "不能同时提供 sessionToken 和 externalCredentials，请只选择其中一种认证方式".into(),
        ));
    }
    if let Some(tok) = auth.session_token.as_deref()
        && tok.trim().is_empty()
    {
        return Err(AppError::Validation("sessionToken 不能为空".into()));
    }
    if let Some(ext) = auth.external_credentials.as_ref()
        && !ext.is_valid()
    {
        return Err(AppError::Validation(
            "外部凭证无效：必须提供以下凭证之一：platform + platformId / sessiontoken / apiUserId"
                .into(),
        ));
    }
    if auth.session_token.is_none() && auth.external_credentials.is_none() {
        return Err(AppError::Validation(
            "必须提供 sessionToken 或 externalCredentials 中的一项".into(),
        ));
    }
    let salt_value = crate::config::AppConfig::global()
        .stats
        .user_hash_salt
        .clone()
        .or_else(|| std::env::var("APP_STATS_USER_HASH_SALT").ok())
        .filter(|v| !v.trim().is_empty())
        .ok_or_else(|| {
            AppError::Internal(
                "stats.user_hash_salt 未配置，无法签发稳定会话令牌（可通过 APP_STATS_USER_HASH_SALT 设置）"
                    .into(),
            )
        })?;
    let (user_hash_opt, _) =
        crate::features::stats::derive_user_identity_from_auth(Some(salt_value.as_str()), &auth);
    let user_hash =
        user_hash_opt.ok_or_else(|| AppError::Auth("无法识别用户（缺少可用凭证）".into()))?;
    let now = chrono::Utc::now();
    let iat = now.timestamp();
    let exp = (now + chrono::Duration::seconds(cfg.access_ttl_secs as i64)).timestamp();
    let claims = SessionClaims {
        sub: user_hash,
        jti: Uuid::new_v4().to_string(),
        iss: cfg.jwt_issuer.clone(),
        aud: cfg.jwt_audience.clone(),
        iat,
        exp,
    };
    let token = jsonwebtoken::encode(
        &jsonwebtoken::Header::new(jsonwebtoken::Algorithm::HS256),
        &claims,
        &jsonwebtoken::EncodingKey::from_secret(jwt_secret.as_bytes()),
    )
    .map_err(|e| AppError::Internal(format!("签发会话令牌失败: {e}")))?;
    Ok((
        StatusCode::OK,
        Json(SessionExchangeResponse {
            access_token: token,
            expires_in: cfg.access_ttl_secs,
            token_type: "Bearer",
        }),
    ))
}
#[utoipa::path(
    post,
    path = "/auth/session/logout",
    summary = "注销会话令牌",
    description = "scope=current 仅注销当前令牌，scope=all 注销该用户所有历史令牌。",
    request_body = SessionLogoutRequest,
    params(("Authorization" = String, Header, description = "Bearer access token")),
    responses(
        (status = 200, description = "注销成功", body = SessionLogoutResponse),
        (
            status = 401,
            description = "令牌无效",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        ),
        (
            status = 500,
            description = "存储不可用或配置错误",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        )
    ),
    tag = "Auth"
)]
pub async fn post_session_logout(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(req): Json<SessionLogoutRequest>,
) -> Result<(StatusCode, Json<SessionLogoutResponse>), AppError> {
    try_cleanup_expired_session_records(&state).await;
    let cfg = ensure_session_config()?;
    let jwt_secret = resolve_jwt_secret(cfg)?;
    let authz = parse_authorization_token(&headers, cfg).await?;
    let storage = state
        .stats_storage
        .as_ref()
        .ok_or_else(|| AppError::Internal("统计存储未初始化，无法执行会话撤销".into()))?;
    let now = chrono::Utc::now();
    let now_rfc3339 = now.to_rfc3339();
    if storage
        .is_token_blacklisted(&authz.claims.jti, &now_rfc3339)
        .await?
    {
        return Err(AppError::Auth("会话令牌已失效".into()));
    }
    if let Some(gate) = storage
        .get_logout_gate(&authz.claims.sub, &now_rfc3339)
        .await?
    {
        let gate_ts = chrono::DateTime::parse_from_rfc3339(&gate)
            .map_err(|e| AppError::Internal(format!("解析时间门失败: {e}")))?
            .timestamp();
        if authz.claims.iat < gate_ts {
            return Err(AppError::Auth("会话令牌已被用户作废".into()));
        }
    }
    let mut logout_before = None;
    if req.scope == SessionLogoutScope::All {
        let gate = now + chrono::Duration::seconds(cfg.revoke_all_grace_secs as i64);
        let gate_rfc3339 = gate.to_rfc3339();
        let gate_expire =
            (gate + chrono::Duration::seconds(cfg.revoke_ttl_secs as i64)).to_rfc3339();
        storage
            .upsert_logout_gate(&authz.claims.sub, &gate_rfc3339, &gate_expire, &now_rfc3339)
            .await?;
        logout_before = Some(gate_rfc3339);
    }
    let exp_ts = authz.claims.exp;
    let expires_at = chrono::DateTime::<chrono::Utc>::from_timestamp(exp_ts, 0)
        .unwrap_or(now + chrono::Duration::seconds(cfg.access_ttl_secs as i64))
        .to_rfc3339();

    let mut logout_validation = jsonwebtoken::Validation::new(jsonwebtoken::Algorithm::HS256);
    logout_validation.validate_exp = false;
    logout_validation.set_issuer(&[cfg.jwt_issuer.as_str()]);
    logout_validation.set_audience(&[cfg.jwt_audience.as_str()]);
    let _ = jsonwebtoken::decode::<SessionClaims>(
        &extract_bearer_token(&headers)?,
        &jsonwebtoken::DecodingKey::from_secret(jwt_secret.as_bytes()),
        &logout_validation,
    )
    .map_err(|_| AppError::Auth("会话令牌无效".into()))?;

    storage
        .add_token_blacklist(&authz.claims.jti, &expires_at, &now_rfc3339)
        .await?;
    Ok((
        StatusCode::OK,
        Json(SessionLogoutResponse {
            scope: req.scope,
            revoked_jti: authz.claims.jti,
            logout_before,
        }),
    ))
}

#[utoipa::path(
    post,
    path = "/auth/qrcode",
    summary = "生成登录二维码",
    description = "为设备申请 TapTap 设备码并返回可扫码的 SVG 二维码（base64）与校验 URL。客户端需保存返回的 qrId 以轮询授权状态。",
    params(
        ("taptapVersion" = Option<String>, Query, description = "TapTap 版本：cn（大陆版）或 global（国际版）")
    ),
    responses(
        (status = 200, description = "生成二维码成功", body = QrCodeCreateResponse),
        (
            status = 401,
            description = "认证失败（TapTap 返回认证错误）",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        ),
        (
            status = 422,
            description = "参数校验失败（taptapVersion 非法等）",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        ),
        (
            status = 502,
            description = "上游网络错误",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        ),
        (
            status = 500,
            description = "服务器内部错误",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        )
    ),
    tag = "Auth"
)]
pub(crate) async fn post_qrcode(
    State(state): State<AppState>,
    Query(params): Query<QrCodeQuery>,
) -> Result<Response, AppError> {
    // 生成 device_id 与 qr_id
    let device_id = Uuid::new_v4().to_string();
    let qr_id = Uuid::new_v4().to_string();

    // 获取版本参数
    let version = normalize_taptap_version(params.taptap_version.as_deref())?;

    // 请求 TapTap 设备码
    let device = state
        .taptap_client
        .request_device_code(&device_id, version)
        .await?;

    let device_code = device
        .device_code
        .ok_or_else(|| AppError::Internal("TapTap 未返回 device_code".to_string()))?;
    let verification_url = device
        .verification_url
        .ok_or_else(|| AppError::Internal("TapTap 未返回 verification_url".to_string()))?;

    // 组合用于扫码/跳转的最终链接：优先使用服务端提供的 qrcode_url；
    // 否则在 verification_url 基础上拼接 user_code 参数。
    let verification_url_for_scan = if let Some(qr) = device.qrcode_url.clone() {
        qr
    } else if let Some(code) = device.user_code.clone() {
        if verification_url.contains('?') {
            format!("{verification_url}&qrcode=1&user_code={code}")
        } else {
            format!("{verification_url}?qrcode=1&user_code={code}")
        }
    } else {
        verification_url.clone()
    };

    // 生成二维码（SVG）并 Base64 编码
    let code = QrCode::new(&verification_url_for_scan)
        .map_err(|e| AppError::Internal(format!("生成二维码失败: {e}")))?;
    let image = code
        .render()
        .min_dimensions(256, 256)
        .dark_color(svg::Color("#000"))
        .light_color(svg::Color("#fff"))
        .build();
    let qrcode_base64 = format!(
        "data:image/svg+xml;base64,{}",
        base64::prelude::BASE64_STANDARD.encode(image)
    );

    // 写入缓存为 Pending 状态
    let interval_secs = device.interval.unwrap_or(5);
    state
        .qrcode_service
        .set_pending(
            qr_id.clone(),
            device_code,
            device_id,
            interval_secs,
            device.expires_in,
            version.map(|v| v.to_string()),
        )
        .await;

    let resp = QrCodeCreateResponse {
        qr_id,
        verification_url: verification_url_for_scan,
        qrcode_base64,
    };
    Ok(json_no_store(StatusCode::OK, resp))
}

#[utoipa::path(
    get,
    path = "/auth/qrcode/{qr_id}/status",
    summary = "轮询二维码授权状态",
    description = "根据 qr_id 查询当前授权进度。若返回 Pending 且包含 retry_after，客户端应按该秒数后再发起轮询。",
    params(("qr_id" = String, Path, description = "二维码ID")),
    responses(
        (status = 200, description = "状态返回", body = QrCodeStatusResponse)
    ),
    tag = "Auth"
)]
pub async fn get_qrcode_status(
    State(state): State<AppState>,
    Path(qr_id): Path<String>,
) -> Result<Response, AppError> {
    let current = match state.qrcode_service.get(&qr_id).await {
        Some(c) => c,
        None => {
            // v2 契约：二维码状态接口始终返回 200 + 状态对象，避免出现“404 但仍返回 JSON body”的特例。
            return Ok(json_no_store(
                StatusCode::OK,
                QrCodeStatusResponse {
                    status: QrCodeStatusValue::Expired,
                    session_token: None,
                    error_code: None,
                    message: Some("二维码不存在或已过期".to_string()),
                    retry_after: None,
                },
            ));
        }
    };

    match current {
        QrCodeStatus::Confirmed { session_data } => {
            // 命中确认，删除缓存并返回 token
            state.qrcode_service.remove(&qr_id).await;
            Ok(json_no_store(
                StatusCode::OK,
                QrCodeStatusResponse {
                    status: QrCodeStatusValue::Confirmed,
                    session_token: Some(session_data.session_token),
                    error_code: None,
                    message: None,
                    retry_after: None,
                },
            ))
        }
        QrCodeStatus::Pending {
            device_code,
            device_id,
            interval_secs,
            next_poll_at,
            expires_at,
            version,
        } => {
            // 频率限制：遵循 TapTap 建议的 interval
            let now = std::time::Instant::now();
            // 先判断是否已过期（避免无意义轮询）
            if now >= expires_at {
                state.qrcode_service.remove(&qr_id).await;
                return Ok(json_no_store(
                    StatusCode::OK,
                    QrCodeStatusResponse {
                        status: QrCodeStatusValue::Expired,
                        session_token: None,
                        error_code: None,
                        message: Some("二维码已过期".to_string()),
                        retry_after: None,
                    },
                ));
            }
            if now < next_poll_at {
                let retry_secs = (next_poll_at - now).as_secs();
                return Ok(json_no_store(
                    StatusCode::OK,
                    QrCodeStatusResponse {
                        status: QrCodeStatusValue::Pending,
                        session_token: None,
                        error_code: None,
                        message: None,
                        retry_after: Some(retry_secs),
                    },
                ));
            }
            // 轮询 TapTap，判断授权状态
            // 使用生成二维码时保存的版本信息
            match state
                .taptap_client
                .poll_for_token(&device_code, &device_id, version.as_deref())
                .await
            {
                Ok(session) => {
                    // 更新为 Confirmed 并返回
                    state
                        .qrcode_service
                        .set_confirmed(&qr_id, session.clone())
                        .await;
                    state.qrcode_service.remove(&qr_id).await;
                    Ok(json_no_store(
                        StatusCode::OK,
                        QrCodeStatusResponse {
                            status: QrCodeStatusValue::Confirmed,
                            session_token: Some(session.session_token),
                            error_code: None,
                            message: None,
                            retry_after: None,
                        },
                    ))
                }
                Err(AppError::AuthPending(_)) => {
                    // 按 interval 延后下一次轮询
                    state
                        .qrcode_service
                        .set_pending_next_poll(
                            &qr_id,
                            device_code,
                            device_id,
                            interval_secs,
                            expires_at,
                            version,
                        )
                        .await;
                    Ok(json_no_store(
                        StatusCode::OK,
                        QrCodeStatusResponse {
                            status: QrCodeStatusValue::Pending,
                            session_token: None,
                            error_code: None,
                            message: None,
                            retry_after: Some(interval_secs),
                        },
                    ))
                }
                Err(e) => {
                    tracing::warn!(err = %e, "qrcode poll failed");
                    let (error_code, message) = match &e {
                        AppError::Auth(_) => ("UNAUTHORIZED", "认证失败"),
                        AppError::Network(_) => ("UPSTREAM_ERROR", "上游网络错误"),
                        AppError::Timeout(_) => ("UPSTREAM_TIMEOUT", "上游超时"),
                        AppError::Json(_) => ("UPSTREAM_ERROR", "上游响应解析失败"),
                        AppError::Validation(_) => ("VALIDATION_FAILED", "请求参数错误"),
                        AppError::Conflict(_) => ("CONFLICT", "资源冲突"),
                        AppError::Internal(_) => ("INTERNAL_ERROR", "服务器内部错误"),
                        AppError::SaveProvider(_)
                        | AppError::Search(_)
                        | AppError::SaveHandlerError(_)
                        | AppError::ImageRendererError(_)
                        | AppError::AuthPending(_) => ("INTERNAL_ERROR", "服务器内部错误"),
                    };
                    Ok(json_no_store(
                        StatusCode::OK,
                        QrCodeStatusResponse {
                            status: QrCodeStatusValue::Error,
                            session_token: None,
                            error_code: Some(error_code.to_string()),
                            message: Some(message.to_string()),
                            retry_after: None,
                        },
                    ))
                }
            }
        }
        QrCodeStatus::Scanned => Ok(json_no_store(
            StatusCode::OK,
            QrCodeStatusResponse {
                status: QrCodeStatusValue::Scanned,
                session_token: None,
                error_code: None,
                message: None,
                retry_after: None,
            },
        )),
    }
}

pub fn create_auth_router() -> Router<AppState> {
    Router::<AppState>::new()
        .route("/qrcode", post(post_qrcode))
        .route("/qrcode/:qr_id/status", get(get_qrcode_status))
        .route("/user-id", post(post_user_id))
        .route("/session/exchange", post(post_session_exchange))
        .route("/session/logout", post(post_session_logout))
}
