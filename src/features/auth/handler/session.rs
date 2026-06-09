use axum::{Json, extract::State, http::StatusCode};
use serde::{Deserialize, Serialize};
use std::time::Instant;
use uuid::Uuid;

use crate::error::AppError;
use crate::state::AppState;

use crate::features::auth::bearer::{
    SessionClaims, build_embedded_auth_claim, decode_access_token,
    decode_access_token_allow_expired, decode_embedded_auth_with_claims, ensure_session_config,
    extract_bearer_token, resolve_exchange_secret, resolve_expected_exchange_secret,
    resolve_jwt_secret, validate_bearer_not_revoked,
};

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

fn parse_authorization_token(
    headers: &axum::http::HeaderMap,
    cfg: &crate::config::SessionConfig,
) -> Result<AuthzToken, AppError> {
    let token = extract_bearer_token(headers)?;
    let claims = decode_access_token(&token, cfg, true)?;
    Ok(AuthzToken { token, claims })
}

fn parse_authorization_token_allow_expired(
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

fn saturating_u64_to_i64(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

fn issue_session_access_token(
    auth: &crate::auth_contract::UnifiedSaveRequest,
    sub: &str,
    cfg: &crate::config::SessionConfig,
    jwt_secret: &str,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<String, AppError> {
    let iat = now.timestamp();
    let exp =
        (now + chrono::Duration::seconds(saturating_u64_to_i64(cfg.access_ttl_secs))).timestamp();
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
    let authz = parse_authorization_token_allow_expired(&headers, cfg)?;

    let storage = state
        .stats_storage
        .as_ref()
        .ok_or_else(|| AppError::Internal("统计存储未初始化，无法执行会话刷新".into()))?;

    let now = chrono::Utc::now();
    validate_bearer_not_revoked(Some(storage), &authz.claims).await?;

    let refresh_window_secs = saturating_u64_to_i64(resolve_refresh_window_secs(cfg));
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
    let authz = parse_authorization_token(&headers, cfg)?;
    let storage = state
        .stats_storage
        .as_ref()
        .ok_or_else(|| AppError::Internal("统计存储未初始化，无法执行会话撤销".into()))?;
    let now = chrono::Utc::now();
    let now_rfc3339 = now.to_rfc3339();
    validate_bearer_not_revoked(Some(storage), &authz.claims).await?;
    let mut logout_before = None;
    if req.scope == SessionLogoutScope::All {
        let gate =
            now + chrono::Duration::seconds(saturating_u64_to_i64(cfg.revoke_all_grace_secs));
        let gate_rfc3339 = gate.to_rfc3339();
        let gate_expire = (gate
            + chrono::Duration::seconds(saturating_u64_to_i64(cfg.revoke_ttl_secs)))
        .to_rfc3339();
        storage
            .upsert_logout_gate(&authz.claims.sub, &gate_rfc3339, &gate_expire, &now_rfc3339)
            .await?;
        logout_before = Some(gate_rfc3339);
    }
    let exp_ts = authz.claims.exp;
    let expires_at = chrono::DateTime::<chrono::Utc>::from_timestamp(exp_ts, 0)
        .unwrap_or(now + chrono::Duration::seconds(saturating_u64_to_i64(cfg.access_ttl_secs)))
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
