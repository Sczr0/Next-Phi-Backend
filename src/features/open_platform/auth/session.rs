use axum::http::{HeaderMap, header};
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Validation};
use uuid::Uuid;

use crate::{
    config::{AppConfig, OpenPlatformConfig},
    error::AppError,
    features::open_platform::storage,
};

use super::models::DeveloperSessionClaims;

pub(super) fn ensure_open_platform_enabled() -> Result<&'static OpenPlatformConfig, AppError> {
    let cfg = &AppConfig::global().open_platform;
    if !cfg.enabled {
        return Err(AppError::Validation("开放平台未启用".into()));
    }
    Ok(cfg)
}

fn resolve_session_secret(cfg: &OpenPlatformConfig) -> Result<String, AppError> {
    if !cfg.session.jwt_secret.trim().is_empty() {
        return Ok(cfg.session.jwt_secret.clone());
    }
    let from_env = std::env::var("APP_OPEN_PLATFORM_SESSION_JWT_SECRET").unwrap_or_default();
    if !from_env.trim().is_empty() {
        return Ok(from_env);
    }
    Err(AppError::Internal(
        "open_platform.session.jwt_secret 未配置（可通过 APP_OPEN_PLATFORM_SESSION_JWT_SECRET 设置）"
            .into(),
    ))
}

fn saturating_u64_to_i64(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

pub(super) fn issue_developer_session_token(
    cfg: &OpenPlatformConfig,
    developer: &storage::DeveloperRecord,
) -> Result<String, AppError> {
    let now = chrono::Utc::now().timestamp();
    let claims = DeveloperSessionClaims {
        sub: developer.id.clone(),
        jti: Uuid::new_v4().to_string(),
        github_user_id: developer.github_user_id.clone(),
        github_login: developer.github_login.clone(),
        iss: cfg.session.jwt_issuer.clone(),
        aud: cfg.session.jwt_audience.clone(),
        iat: now,
        exp: now + saturating_u64_to_i64(cfg.session.ttl_secs.max(300)),
    };
    let secret = resolve_session_secret(cfg)?;
    jsonwebtoken::encode(
        &jsonwebtoken::Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .map_err(|e| AppError::Internal(format!("签发开发者会话令牌失败: {e}")))
}

fn decode_developer_session_token(
    cfg: &OpenPlatformConfig,
    token: &str,
) -> Result<DeveloperSessionClaims, AppError> {
    let secret = resolve_session_secret(cfg)?;
    let mut validation = Validation::new(Algorithm::HS256);
    validation.set_issuer(&[cfg.session.jwt_issuer.as_str()]);
    validation.set_audience(&[cfg.session.jwt_audience.as_str()]);
    let data = jsonwebtoken::decode::<DeveloperSessionClaims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &validation,
    )
    .map_err(|_| AppError::Auth("开发者会话无效或已过期".into()))?;
    Ok(data.claims)
}

pub(super) fn read_cookie_value(headers: &HeaderMap, key: &str) -> Option<String> {
    let cookies = headers.get(header::COOKIE)?.to_str().ok()?;
    cookies.split(';').find_map(|part| {
        let mut iter = part.trim().splitn(2, '=');
        let name = iter.next()?.trim();
        let value = iter.next()?.trim();
        if name == key && !value.is_empty() {
            Some(value.to_string())
        } else {
            None
        }
    })
}

pub(super) fn build_set_cookie_value(cfg: &OpenPlatformConfig, token: &str) -> String {
    let mut v = format!(
        "{}={}; Max-Age={}; Path=/; HttpOnly; SameSite=Lax",
        cfg.session.cookie_name, token, cfg.session.ttl_secs
    );
    if cfg.session.cookie_secure {
        v.push_str("; Secure");
    }
    v
}

pub(super) fn build_clear_cookie_value(cfg: &OpenPlatformConfig) -> String {
    let mut v = format!(
        "{}=; Max-Age=0; Path=/; HttpOnly; SameSite=Lax",
        cfg.session.cookie_name
    );
    if cfg.session.cookie_secure {
        v.push_str("; Secure");
    }
    v
}

fn extract_developer_claims(
    headers: &HeaderMap,
    cfg: &OpenPlatformConfig,
) -> Result<DeveloperSessionClaims, AppError> {
    let token = read_cookie_value(headers, &cfg.session.cookie_name)
        .ok_or_else(|| AppError::Auth("缺少开发者会话".into()))?;
    decode_developer_session_token(cfg, &token)
}

/// 从开发者会话中解析当前开发者，并校验开发者仍存在于本地存储。
pub async fn require_developer(headers: &HeaderMap) -> Result<storage::DeveloperRecord, AppError> {
    let cfg = ensure_open_platform_enabled()?;
    let claims = extract_developer_claims(headers, cfg)?;
    let storage = storage::global()?;
    storage
        .get_developer_by_id(&claims.sub)
        .await?
        .ok_or_else(|| AppError::Auth("开发者会话已失效".into()))
}
