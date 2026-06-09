use base64::Engine;
use rand::RngCore;

use crate::{config::AppConfig, error::AppError, features::open_platform::storage};

use super::models::ApiKeyListItem;

pub(super) fn ensure_open_platform_enabled()
-> Result<&'static crate::config::OpenPlatformConfig, AppError> {
    let cfg = &AppConfig::global().open_platform;
    if !cfg.enabled {
        return Err(AppError::Validation("开放平台未启用".into()));
    }
    Ok(cfg)
}

pub(super) fn normalize_environment(raw: Option<&str>) -> Result<&'static str, AppError> {
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

pub(super) fn sanitize_name(name: &str) -> Result<String, AppError> {
    let n = name.trim();
    if n.is_empty() {
        return Err(AppError::Validation("name 不能为空".into()));
    }
    if n.chars().count() > 64 {
        return Err(AppError::Validation("name 过长（最大 64 字符）".into()));
    }
    Ok(n.to_string())
}

pub(super) fn normalize_scopes(
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

pub(super) fn resolve_prefix(cfg: &crate::config::OpenPlatformConfig, env: &str) -> String {
    if env == "test" {
        cfg.api_key.test_prefix.clone()
    } else {
        cfg.api_key.live_prefix.clone()
    }
}

pub(super) fn resolve_key_hash_secret(
    cfg: &crate::config::OpenPlatformConfig,
) -> Result<String, AppError> {
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

pub(super) fn generate_api_key(prefix: &str, random_bytes: usize) -> String {
    let byte_len = random_bytes.clamp(16, 64);
    let mut bytes = vec![0u8; byte_len];
    rand::thread_rng().fill_bytes(&mut bytes);
    let suffix = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);
    format!("{prefix}{suffix}")
}

pub(super) fn derive_key_last4(token: &str) -> String {
    let mut chars: Vec<char> = token.chars().rev().take(4).collect();
    chars.reverse();
    chars.into_iter().collect()
}

pub(super) fn hash_api_key(secret: &str, token: &str) -> String {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;

    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("hmac key");
    mac.update(token.as_bytes());
    let out = mac.finalize().into_bytes();
    hex::encode(out)
}

pub(super) fn mask_key(prefix: &str, last4: &str) -> String {
    format!("{prefix}****{last4}")
}

pub(super) fn saturating_u64_to_i64(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

pub(super) fn map_key_list_item(item: storage::ApiKeyRecord) -> ApiKeyListItem {
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

pub(super) async fn ensure_key_owned_by_developer(
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
