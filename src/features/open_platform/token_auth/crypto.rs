use axum::http::HeaderMap;

use crate::{config::OpenPlatformConfig, error::AppError};

use super::OPEN_API_TOKEN_HEADER;

pub(super) fn resolve_key_hash_secret(cfg: &OpenPlatformConfig) -> Result<String, AppError> {
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
        "未配置 API Key hash 密钥（open_platform.api_key.hash_secret 或 APP_OPEN_PLATFORM_API_KEY_HASH_SECRET）"
            .into(),
    ))
}

pub(super) fn hash_api_key(secret: &str, token: &str) -> String {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;

    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("hmac key");
    mac.update(token.as_bytes());
    let out = mac.finalize().into_bytes();
    hex::encode(out)
}

pub(super) fn extract_open_api_token(headers: &HeaderMap) -> Result<String, AppError> {
    let raw = headers
        .get(OPEN_API_TOKEN_HEADER)
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| AppError::Auth("缺少 X-OpenApi-Token 请求头".into()))?;
    let token = raw.trim();
    if token.is_empty() {
        return Err(AppError::Auth("X-OpenApi-Token 不能为空".into()));
    }
    Ok(token.to_string())
}

pub(super) fn client_ip_from_headers(headers: &HeaderMap) -> Option<String> {
    if let Some(v) = headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.split(',').next())
    {
        let ip = v.trim();
        if !ip.is_empty() {
            return Some(ip.to_string());
        }
    }
    if let Some(v) = headers.get("x-real-ip").and_then(|v| v.to_str().ok()) {
        let ip = v.trim();
        if !ip.is_empty() {
            return Some(ip.to_string());
        }
    }
    None
}
