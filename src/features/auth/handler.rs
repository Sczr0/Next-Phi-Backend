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
use std::time::Instant;
use uuid::Uuid;

use crate::error::AppError;
use crate::state::AppState;

use super::bearer::{
    SessionClaims, build_embedded_auth_claim, decode_access_token_allow_expired,
    decode_embedded_auth_with_claims, ensure_session_config, extract_bearer_token,
    resolve_exchange_secret, resolve_expected_exchange_secret, resolve_jwt_secret,
    validate_bearer_not_revoked,
};
use super::qrcode_service::QrCodeStatus;

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UserIdResponse {
    /// 鍘绘晱鍚庣殑绋冲畾鐢ㄦ埛 ID锛?2 浣?hex锛岀瓑浠蜂簬 stats/leaderboard 浣跨敤鐨?user_hash锛?    #[schema(example = "ab12cd34ef56ab12cd34ef56ab12cd34")]
    pub user_id: String,
    /// 鐢ㄤ簬鎺ㄥ user_id 鐨勫嚟璇佺被鍨嬶紙鐢ㄤ簬鎺掓煡鈥滀负浠€涔堝拰浠ュ墠涓嶄竴鑷粹€濓級
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_kind: Option<String>,
}

#[utoipa::path(
    post,
    path = "/auth/user-id",
    summary = "根据凭证生成去敏用户ID",
    description = "使用服务端配置的 stats.user_hash_salt 对凭证做 HMAC-SHA256 去敏，生成稳定用户标识。",
    request_body = crate::auth_contract::UnifiedSaveRequest,
    responses(
        (status = 200, description = "鐢熸垚鎴愬姛", body = UserIdResponse),
        (
            status = 422,
            description = "凭证缺失或无效",
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
    Json(auth): Json<crate::auth_contract::UnifiedSaveRequest>,
) -> Result<(StatusCode, Json<UserIdResponse>), AppError> {
    // 与 /save 的凭证互斥规则保持一致，避免同一请求在不同接口出现身份不一致。
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
            "无法识别用户：请提供 sessionToken，或 externalCredentials 中的 platform+platformId / sessiontoken / apiUserId（且不能为空）"
                .into(),
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

    let (user_id_opt, user_kind) = crate::identity_hash::derive_user_identity_from_auth(
        Some(salt),
        &auth,
    );
    let user_id = user_id_opt.ok_or_else(|| AppError::Internal("生成 user_id 失败".into()))?;
    Ok((StatusCode::OK, Json(UserIdResponse { user_id, user_kind })))
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct QrCodeCreateResponse {
    /// 浜岀淮鐮佹爣璇嗭紝鐢ㄤ簬杞鐘舵€?    #[schema(example = "8b8f2f8a-1a2b-4c3d-9e0f-112233445566")]
    pub qr_id: String,
    /// 鐢ㄦ埛鍦ㄦ祻瑙堝櫒涓闂互纭鎺堟潈鐨?URL
    #[schema(example = "https://www.taptap.com/account/device?code=abcd-efgh")]
    pub verification_url: String,
    /// SVG 浜岀淮鐮佺殑 data URL锛坆ase64 缂栫爜锛?    #[schema(example = "data:image/svg+xml;base64,PHN2ZyB4bWxucz0uLi4=")]
    pub qrcode_base64: String,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct QrCodeStatusResponse {
    /// 褰撳墠鐘舵€侊細Pending/Scanned/Confirmed/Error/Expired
    #[schema(example = "Pending")]
    pub status: QrCodeStatusValue,
    /// 鑻?Confirmed锛岃繑鍥?LeanCloud Session Token
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_token: Option<String>,
    /// 鍙€夛細鏈哄櫒鍙鐨勯敊璇爜锛堜粎鍦?status=Error 鏃跺嚭鐜帮級
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
    /// 鍙€夌殑浜虹被鍙鎻愮ず娑堟伅
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// 鑻ラ渶寤跺悗杞锛岃繑鍥炲缓璁殑绛夊緟绉掓暟
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
    /// TapTap 鐗堟湰锛歝n锛堝ぇ闄嗙増锛夋垨 global锛堝浗闄呯増锛?    #[serde(default)]
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
        "taptapVersion 蹇呴』涓?cn 鎴?global".to_string(),
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
    pub auth: crate::auth_contract::UnifiedSaveRequest,
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
#[derive(Debug)]
struct AuthzToken {
    token: String,
    claims: SessionClaims,
}

#[derive(Debug, Serialize)]
struct SessionTokenClaims<'a> {
    sub: &'a str,
    jti: &'a str,
    iss: &'a str,
    aud: &'a str,
    iat: i64,
    exp: i64,
    sae: String,
}

async fn parse_authorization_token(
    headers: &axum::http::HeaderMap,
    cfg: &crate::config::SessionConfig,
) -> Result<AuthzToken, AppError> {
    let token = extract_bearer_token(headers)?;
    let claims = super::bearer::decode_access_token(&token, cfg, true)?;
    Ok(AuthzToken { token, claims })
}

async fn parse_authorization_token_allow_expired(
    headers: &axum::http::HeaderMap,
    cfg: &crate::config::SessionConfig,
) -> Result<AuthzToken, AppError> {
    let token = extract_bearer_token(headers)?;
    let claims = decode_access_token_allow_expired(&token, cfg)?;
    Ok(AuthzToken { token, claims })
}

fn ensure_exchange_secret_valid(
    headers: &axum::http::HeaderMap,
    cfg: &crate::config::SessionConfig,
) -> Result<(), AppError> {
    let expected_exchange_secret = resolve_expected_exchange_secret(cfg)?;
    let provided = resolve_exchange_secret(headers).unwrap_or_default();
    if provided.is_empty() || provided != expected_exchange_secret {
        return Err(AppError::Auth("交换密钥无效".into()));
    }
    Ok(())
}

fn resolve_refresh_window_secs(cfg: &crate::config::SessionConfig) -> u64 {
    std::env::var("APP_SESSION_REFRESH_WINDOW_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(cfg.revoke_ttl_secs)
}

fn issue_session_access_token(
    auth: &crate::auth_contract::UnifiedSaveRequest,
    sub: &str,
    cfg: &crate::config::SessionConfig,
    jwt_secret: &str,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<String, AppError> {
    let iat = now.timestamp();
    let exp = (now + chrono::Duration::seconds(cfg.access_ttl_secs as i64)).timestamp();
    let claims = SessionClaims {
        sub: sub.to_string(),
        jti: Uuid::new_v4().to_string(),
        iss: cfg.jwt_issuer.clone(),
        aud: cfg.jwt_audience.clone(),
        iat,
        exp,
    };
    let embedded_auth = build_embedded_auth_claim(auth, &claims.jti, &claims.sub)?;
    let token_claims = SessionTokenClaims {
        sub: claims.sub.as_str(),
        jti: claims.jti.as_str(),
        iss: claims.iss.as_str(),
        aud: claims.aud.as_str(),
        iat: claims.iat,
        exp: claims.exp,
        sae: embedded_auth,
    };
    let token = jsonwebtoken::encode(
        &jsonwebtoken::Header::new(jsonwebtoken::Algorithm::HS256),
        &token_claims,
        &jsonwebtoken::EncodingKey::from_secret(jwt_secret.as_bytes()),
    )
    .map_err(|e| AppError::Internal(format!("绛惧彂浼氳瘽浠ょ墝澶辫触: {e}")))?;
    Ok(token)
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
    description = "使用登录凭证交换后端短期 access token。",
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
    let t_total = Instant::now();
    try_cleanup_expired_session_records(&state).await;
    let cfg = ensure_session_config()?;
    ensure_exchange_secret_valid(&headers, cfg)?;
    let jwt_secret = resolve_jwt_secret(cfg)?;
    let auth = req.auth;
    if auth.session_token.is_some() && auth.external_credentials.is_some() {
        return Err(AppError::Validation(
            "不能同时提供 sessionToken 和 externalCredentials，请只选择其中一种认证方式".into(),
        ));
    }
    if let Some(tok) = auth.session_token.as_deref()
        && tok.trim().is_empty()
    {
        return Err(AppError::Validation("sessionToken 涓嶈兘涓虹┖".into()));
    }
    if let Some(ext) = auth.external_credentials.as_ref()
        && !ext.is_valid()
    {
        return Err(AppError::Validation(
            "澶栭儴鍑瘉鏃犳晥锛氬繀椤绘彁渚涗互涓嬪嚟璇佷箣涓€锛歱latform + platformId / sessiontoken / apiUserId"
                .into(),
        ));
    }
    if auth.session_token.is_none() && auth.external_credentials.is_none() {
        return Err(AppError::Validation(
            "必须提供 sessionToken 或 externalCredentials 其中一项".into(),
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
        crate::identity_hash::derive_user_identity_from_auth(Some(salt_value.as_str()), &auth);
    let user_hash = user_hash_opt
        .ok_or_else(|| AppError::Auth("鏃犳硶璇嗗埆鐢ㄦ埛锛堢己灏戝彲鐢ㄥ嚟璇侊級".into()))?;
    if let Some(storage) = state.stats_storage.as_ref() {
        storage.ensure_user_not_banned(&user_hash).await?;
    }
    let token =
        issue_session_access_token(&auth, &user_hash, cfg, &jwt_secret, chrono::Utc::now())?;
    tracing::info!(
        target: "phi_backend::auth::performance",
        route = "/auth/session/exchange",
        phase = "total",
        status = "ok",
        dur_ms = t_total.elapsed().as_millis(),
        "auth performance"
    );
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
    path = "/auth/session/refresh",
    summary = "刷新会话令牌",
    description = "使用旧的 Bearer access token（允许过期）与 X-Exchange-Secret 换取新的短期 access token。",
    params(
        ("Authorization" = String, Header, description = "Bearer access token（可过期）"),
        ("X-Exchange-Secret" = String, Header, description = "Next.js 与后端共享密钥")
    ),
    responses(
        (status = 200, description = "刷新成功", body = SessionExchangeResponse),
        (
            status = 401,
            description = "共享密钥无效、令牌无效或已撤销",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        ),
        (
            status = 500,
            description = "存储不可用或服务端配置错误",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        )
    ),
    tag = "Auth"
)]
pub async fn post_session_refresh(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> Result<(StatusCode, Json<SessionExchangeResponse>), AppError> {
    let t_total = Instant::now();
    try_cleanup_expired_session_records(&state).await;
    let cfg = ensure_session_config()?;
    ensure_exchange_secret_valid(&headers, cfg)?;
    let jwt_secret = resolve_jwt_secret(cfg)?;
    let authz = parse_authorization_token_allow_expired(&headers, cfg).await?;

    let storage = state
        .stats_storage
        .as_ref()
        .ok_or_else(|| AppError::Internal("统计存储未初始化，无法执行会话刷新".into()))?;

    let now = chrono::Utc::now();
    validate_bearer_not_revoked(Some(storage), &authz.claims).await?;

    let refresh_window_secs = resolve_refresh_window_secs(cfg) as i64;
    let expired_for_secs = now.timestamp().saturating_sub(authz.claims.exp);
    if expired_for_secs > refresh_window_secs {
        return Err(AppError::Auth("会话令牌过期时间过长，无法刷新".into()));
    }

    let auth = decode_embedded_auth_with_claims(&authz.token, &authz.claims)?;
    let token = issue_session_access_token(&auth, &authz.claims.sub, cfg, &jwt_secret, now)?;
    tracing::info!(
        target: "phi_backend::auth::performance",
        route = "/auth/session/refresh",
        phase = "total",
        status = "ok",
        dur_ms = t_total.elapsed().as_millis(),
        "auth performance"
    );

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
    let t_total = Instant::now();
    try_cleanup_expired_session_records(&state).await;
    let cfg = ensure_session_config()?;
    let authz = parse_authorization_token(&headers, cfg).await?;
    let storage = state
        .stats_storage
        .as_ref()
        .ok_or_else(|| AppError::Internal("统计存储未初始化，无法执行会话撤销".into()))?;
    let now = chrono::Utc::now();
    let now_rfc3339 = now.to_rfc3339();
    validate_bearer_not_revoked(Some(storage), &authz.claims).await?;
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

    let _embedded_auth = decode_embedded_auth_with_claims(&authz.token, &authz.claims)?;

    storage
        .add_token_blacklist(&authz.claims.jti, &expires_at, &now_rfc3339)
        .await?;
    tracing::info!(
        target: "phi_backend::auth::performance",
        route = "/auth/session/logout",
        phase = "total",
        status = "ok",
        dur_ms = t_total.elapsed().as_millis(),
        "auth performance"
    );
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
    description = "为设备申请 TapTap 设备码并返回可扫码的 SVG 二维码（base64）与校验 URL。",
    params(
        ("taptapVersion" = Option<String>, Query, description = "TapTap 版本：cn 或 global")
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
    let t_total = Instant::now();

    let device_id = Uuid::new_v4().to_string();
    let qr_id = Uuid::new_v4().to_string();

    let t_version = Instant::now();
    let version = match normalize_taptap_version(params.taptap_version.as_deref()) {
        Ok(v) => {
            tracing::info!(
                target: "phi_backend::auth::performance",
                route = "/auth/qrcode",
                phase = "normalize_version",
                status = "ok",
                dur_ms = t_version.elapsed().as_millis(),
                "auth performance"
            );
            v
        }
        Err(e) => {
            tracing::info!(
                target: "phi_backend::auth::performance",
                route = "/auth/qrcode",
                phase = "normalize_version",
                status = "failed",
                dur_ms = t_version.elapsed().as_millis(),
                "auth performance"
            );
            return Err(e);
        }
    };

    let t_request = Instant::now();
    let device = match state
        .taptap_client
        .request_device_code(&device_id, version)
        .await
    {
        Ok(device) => {
            tracing::info!(
                target: "phi_backend::auth::performance",
                route = "/auth/qrcode",
                phase = "request_device_code",
                status = "ok",
                dur_ms = t_request.elapsed().as_millis(),
                "auth performance"
            );
            device
        }
        Err(e) => {
            tracing::info!(
                target: "phi_backend::auth::performance",
                route = "/auth/qrcode",
                phase = "request_device_code",
                status = "failed",
                dur_ms = t_request.elapsed().as_millis(),
                err = %e,
                "auth performance"
            );
            return Err(e);
        }
    };

    let device_code = device
        .device_code
        .ok_or_else(|| AppError::Internal("TapTap 未返回 device_code".to_string()))?;
    let verification_url = device
        .verification_url
        .ok_or_else(|| AppError::Internal("TapTap 未返回 verification_url".to_string()))?;

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

    let t_qrcode = Instant::now();
    let code = match QrCode::new(&verification_url_for_scan) {
        Ok(code) => code,
        Err(e) => {
            tracing::info!(
                target: "phi_backend::auth::performance",
                route = "/auth/qrcode",
                phase = "generate_qrcode_svg",
                status = "failed",
                dur_ms = t_qrcode.elapsed().as_millis(),
                "auth performance"
            );
            return Err(AppError::Internal(format!("生成二维码失败: {e}")));
        }
    };
    let image = code
        .render()
        .min_dimensions(256, 256)
        .dark_color(svg::Color("#000"))
        .light_color(svg::Color("#fff"))
        .build();
    tracing::info!(
        target: "phi_backend::auth::performance",
        route = "/auth/qrcode",
        phase = "generate_qrcode_svg",
        status = "ok",
        dur_ms = t_qrcode.elapsed().as_millis(),
        "auth performance"
    );
    let qrcode_base64 = format!(
        "data:image/svg+xml;base64,{}",
        base64::prelude::BASE64_STANDARD.encode(image)
    );

    let interval_secs = device.interval.unwrap_or(5);
    let t_cache_set = Instant::now();
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
    tracing::info!(
        target: "phi_backend::auth::performance",
        route = "/auth/qrcode",
        phase = "cache_set_pending",
        status = "ok",
        dur_ms = t_cache_set.elapsed().as_millis(),
        "auth performance"
    );

    let resp = QrCodeCreateResponse {
        qr_id,
        verification_url: verification_url_for_scan,
        qrcode_base64,
    };
    tracing::info!(
        target: "phi_backend::auth::performance",
        route = "/auth/qrcode",
        phase = "total",
        status = "ok",
        dur_ms = t_total.elapsed().as_millis(),
        "auth performance"
    );
    Ok(json_no_store(StatusCode::OK, resp))
}

#[utoipa::path(
    get,
    path = "/auth/qrcode/{qr_id}/status",
    summary = "轮询二维码授权状态",
    description = "根据 qr_id 查询当前授权进度。若返回 Pending 且包含 retry_after，客户端应按该秒数后再轮询。",
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
    let t_total = Instant::now();
    let log_total = |result_status: &'static str| {
        tracing::info!(
            target: "phi_backend::auth::performance",
            route = "/auth/qrcode/:qr_id/status",
            phase = "total",
            status = "ok",
            result_status,
            dur_ms = t_total.elapsed().as_millis(),
            "auth performance"
        );
    };

    let t_cache_get = Instant::now();
    let current = match state.qrcode_service.get(&qr_id).await {
        Some(c) => {
            tracing::info!(
                target: "phi_backend::auth::performance",
                route = "/auth/qrcode/:qr_id/status",
                phase = "cache_get",
                status = "hit",
                dur_ms = t_cache_get.elapsed().as_millis(),
                "auth performance"
            );
            c
        }
        None => {
            tracing::info!(
                target: "phi_backend::auth::performance",
                route = "/auth/qrcode/:qr_id/status",
                phase = "cache_get",
                status = "miss",
                dur_ms = t_cache_get.elapsed().as_millis(),
                "auth performance"
            );
            log_total("expired_not_found");
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
            let t_cache_remove = Instant::now();
            state.qrcode_service.remove(&qr_id).await;
            tracing::info!(
                target: "phi_backend::auth::performance",
                route = "/auth/qrcode/:qr_id/status",
                phase = "cache_remove",
                status = "ok",
                dur_ms = t_cache_remove.elapsed().as_millis(),
                "auth performance"
            );
            log_total("confirmed");
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
            let now = std::time::Instant::now();

            if now >= expires_at {
                let t_cache_remove = Instant::now();
                state.qrcode_service.remove(&qr_id).await;
                tracing::info!(
                    target: "phi_backend::auth::performance",
                    route = "/auth/qrcode/:qr_id/status",
                    phase = "cache_remove",
                    status = "expired",
                    dur_ms = t_cache_remove.elapsed().as_millis(),
                    "auth performance"
                );
                log_total("expired");
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
                tracing::info!(
                    target: "phi_backend::auth::performance",
                    route = "/auth/qrcode/:qr_id/status",
                    phase = "poll_gate",
                    status = "deferred",
                    retry_after = retry_secs,
                    dur_ms = 0_u64,
                    "auth performance"
                );
                log_total("pending_wait");
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

            let t_poll = Instant::now();
            match state
                .taptap_client
                .poll_for_token(&device_code, &device_id, version.as_deref())
                .await
            {
                Ok(session) => {
                    tracing::info!(
                        target: "phi_backend::auth::performance",
                        route = "/auth/qrcode/:qr_id/status",
                        phase = "poll_for_token",
                        status = "ok",
                        dur_ms = t_poll.elapsed().as_millis(),
                        "auth performance"
                    );
                    let t_cache_update = Instant::now();
                    state
                        .qrcode_service
                        .set_confirmed(&qr_id, session.clone())
                        .await;
                    state.qrcode_service.remove(&qr_id).await;
                    tracing::info!(
                        target: "phi_backend::auth::performance",
                        route = "/auth/qrcode/:qr_id/status",
                        phase = "cache_update",
                        status = "confirmed",
                        dur_ms = t_cache_update.elapsed().as_millis(),
                        "auth performance"
                    );
                    log_total("confirmed");
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
                    tracing::info!(
                        target: "phi_backend::auth::performance",
                        route = "/auth/qrcode/:qr_id/status",
                        phase = "poll_for_token",
                        status = "pending",
                        dur_ms = t_poll.elapsed().as_millis(),
                        "auth performance"
                    );
                    let t_cache_update = Instant::now();
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
                    tracing::info!(
                        target: "phi_backend::auth::performance",
                        route = "/auth/qrcode/:qr_id/status",
                        phase = "cache_update",
                        status = "pending",
                        dur_ms = t_cache_update.elapsed().as_millis(),
                        "auth performance"
                    );
                    log_total("pending");
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
                    tracing::info!(
                        target: "phi_backend::auth::performance",
                        route = "/auth/qrcode/:qr_id/status",
                        phase = "poll_for_token",
                        status = "failed",
                        dur_ms = t_poll.elapsed().as_millis(),
                        err = %e,
                        "auth performance"
                    );
                    let (error_code, message) = match &e {
                        AppError::Auth(_) => ("UNAUTHORIZED", "认证失败"),
                        AppError::Forbidden(_) => ("FORBIDDEN", "访问被禁止"),
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
                    log_total("error");
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
        QrCodeStatus::Scanned => {
            log_total("scanned");
            Ok(json_no_store(
                StatusCode::OK,
                QrCodeStatusResponse {
                    status: QrCodeStatusValue::Scanned,
                    session_token: None,
                    error_code: None,
                    message: None,
                    retry_after: None,
                },
            ))
        }
    }
}

pub fn create_auth_router() -> Router<AppState> {
    Router::<AppState>::new()
        .route("/qrcode", post(post_qrcode))
        .route("/qrcode/:qr_id/status", get(get_qrcode_status))
        .route("/user-id", post(post_user_id))
        .route("/session/exchange", post(post_session_exchange))
        .route("/session/refresh", post(post_session_refresh))
        .route("/session/logout", post(post_session_logout))
}
