use std::sync::Arc;

use axum::{
    extract::{Request, State},
    middleware::Next,
    response::Response,
};
use base64::Engine;
use lru::LruCache;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::error::AppError;
use crate::features::save::models::UnifiedSaveRequest;
use crate::state::AppState;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionClaims {
    pub sub: String,
    pub jti: String,
    pub iss: String,
    pub aud: String,
    pub iat: i64,
    pub exp: i64,
}

#[derive(Debug, Clone)]
pub struct BearerAuthContext {
    pub token: String,
    pub claims: SessionClaims,
}

#[derive(Debug, Clone)]
pub enum BearerAuthState {
    Absent,
    Valid(BearerAuthContext),
    Invalid(String),
}

impl Default for BearerAuthState {
    fn default() -> Self {
        Self::Absent
    }
}

#[derive(Debug, Clone)]
struct AuthDecryptCacheItem {
    value: UnifiedSaveRequest,
    expires_at_unix: i64,
}

type AuthDecryptCache = LruCache<String, AuthDecryptCacheItem>;

fn auth_decrypt_cache_capacity() -> usize {
    std::env::var("APP_SESSION_AUTH_CACHE_CAPACITY")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(50_000)
}

static AUTH_DECRYPT_CACHE: Lazy<RwLock<AuthDecryptCache>> = Lazy::new(|| {
    let cap = auth_decrypt_cache_capacity();
    let non_zero = std::num::NonZeroUsize::new(cap).expect("cache capacity must be non-zero");
    RwLock::new(LruCache::new(non_zero))
});

async fn cache_get_session_auth(token: &str, now_unix: i64) -> Option<UnifiedSaveRequest> {
    let mut guard = AUTH_DECRYPT_CACHE.write().await;
    let entry = guard.get(token).cloned();
    match entry {
        Some(item) if item.expires_at_unix > now_unix => Some(item.value),
        Some(_) => {
            guard.pop(token);
            None
        }
        None => None,
    }
}

async fn cache_put_session_auth(token: String, value: UnifiedSaveRequest, expires_at_unix: i64) {
    let mut guard = AUTH_DECRYPT_CACHE.write().await;
    guard.put(
        token,
        AuthDecryptCacheItem {
            value,
            expires_at_unix,
        },
    );
}

pub fn has_auth_credentials(auth: &UnifiedSaveRequest) -> bool {
    auth.session_token.is_some() || auth.external_credentials.is_some()
}

pub fn ensure_session_config() -> Result<&'static crate::config::SessionConfig, AppError> {
    let cfg = &crate::config::AppConfig::global().session;
    if !cfg.enabled {
        return Err(AppError::Auth("会话接口未启用".into()));
    }
    Ok(cfg)
}

pub fn resolve_jwt_secret(cfg: &crate::config::SessionConfig) -> Result<String, AppError> {
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

pub fn resolve_expected_exchange_secret(
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

pub fn resolve_exchange_secret(headers: &axum::http::HeaderMap) -> Option<String> {
    headers
        .get("x-exchange-secret")
        .or_else(|| headers.get("x-session-exchange-secret"))
        .and_then(|v| v.to_str().ok())
        .map(|v| v.trim().to_string())
}

pub fn extract_bearer_token(headers: &axum::http::HeaderMap) -> Result<String, AppError> {
    let raw = headers
        .get(axum::http::header::AUTHORIZATION)
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

pub fn decode_access_token(
    token: &str,
    cfg: &crate::config::SessionConfig,
    validate_exp: bool,
) -> Result<SessionClaims, AppError> {
    let jwt_secret = resolve_jwt_secret(cfg)?;
    let mut validation = jsonwebtoken::Validation::new(jsonwebtoken::Algorithm::HS256);
    validation.validate_exp = validate_exp;
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

pub fn decode_access_token_allow_expired(
    token: &str,
    cfg: &crate::config::SessionConfig,
) -> Result<SessionClaims, AppError> {
    decode_access_token(token, cfg, false)
}

async fn validate_bearer_not_revoked(
    storage: Option<&Arc<crate::features::stats::storage::StatsStorage>>,
    claims: &SessionClaims,
) -> Result<(), AppError> {
    let Some(storage) = storage else {
        return Ok(());
    };
    let now_rfc3339 = chrono::Utc::now().to_rfc3339();
    let (blacklisted, logout_before) = storage
        .get_session_revoke_state(&claims.jti, &claims.sub, &now_rfc3339)
        .await?;
    if blacklisted {
        return Err(AppError::Auth("会话令牌已失效".into()));
    }
    if let Some(gate) = logout_before {
        let gate_ts = chrono::DateTime::parse_from_rfc3339(&gate)
            .map_err(|e| AppError::Internal(format!("解析会话撤销时间失败: {e}")))?
            .timestamp();
        if claims.iat < gate_ts {
            return Err(AppError::Auth("会话令牌已被用户作废".into()));
        }
    }
    Ok(())
}

pub async fn bearer_auth_middleware(
    State(state): State<AppState>,
    mut req: Request,
    next: Next,
) -> Response {
    let bearer_state = if req
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .is_none()
    {
        BearerAuthState::Absent
    } else {
        match ensure_session_config()
            .and_then(|cfg| extract_bearer_token(req.headers()).map(|token| (cfg, token)))
        {
            Ok((cfg, token)) => match decode_access_token(&token, cfg, true) {
                Ok(claims) => {
                    match validate_bearer_not_revoked(state.stats_storage.as_ref(), &claims).await {
                        Ok(()) => BearerAuthState::Valid(BearerAuthContext { token, claims }),
                        Err(e) => BearerAuthState::Invalid(e.to_string()),
                    }
                }
                Err(e) => BearerAuthState::Invalid(e.to_string()),
            },
            Err(e) => BearerAuthState::Invalid(e.to_string()),
        }
    };

    req.extensions_mut().insert(bearer_state);
    next.run(req).await
}

pub async fn merge_auth_from_bearer_if_missing(
    _stats_storage: Option<&Arc<crate::features::stats::storage::StatsStorage>>,
    bearer: &BearerAuthState,
    auth: &mut UnifiedSaveRequest,
) -> Result<(), AppError> {
    if has_auth_credentials(auth) {
        return Ok(());
    }

    match bearer {
        BearerAuthState::Absent => Ok(()),
        BearerAuthState::Invalid(msg) => Err(AppError::Auth(msg.clone())),
        BearerAuthState::Valid(ctx) => {
            tracing::debug!(target: "phi_backend::auth::bearer", "merge auth from bearer: token-present=true");
            let now_unix = chrono::Utc::now().timestamp();
            if let Some(bound) = cache_get_session_auth(&ctx.token, now_unix).await {
                tracing::debug!(target: "phi_backend::auth::bearer", "merge auth from bearer: cache hit");
                let request_taptap_version = auth.taptap_version.clone();
                *auth = bound;
                if request_taptap_version.is_some() {
                    auth.taptap_version = request_taptap_version;
                }
                return Ok(());
            }

            let parsed = decode_embedded_auth(&ctx.token)?;
            tracing::debug!(target: "phi_backend::auth::bearer", "merge auth from bearer: decoded embedded auth");
            cache_put_session_auth(ctx.token.clone(), parsed.clone(), ctx.claims.exp).await;

            let request_taptap_version = auth.taptap_version.clone();
            *auth = parsed;
            if request_taptap_version.is_some() {
                auth.taptap_version = request_taptap_version;
            }
            Ok(())
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EmbeddedAuthEnvelope {
    auth_b64: String,
}

fn session_auth_crypto_secret() -> Result<String, AppError> {
    std::env::var("APP_SESSION_AUTH_EMBED_SECRET")
        .ok()
        .or_else(|| std::env::var("APP_SESSION_JWT_SECRET").ok())
        .or_else(|| {
            let cfg_secret = crate::config::AppConfig::global()
                .session
                .jwt_secret
                .clone();
            if cfg_secret.trim().is_empty() {
                None
            } else {
                Some(cfg_secret)
            }
        })
        .filter(|v| !v.trim().is_empty())
        .ok_or_else(|| {
            AppError::Internal(
                "未配置会话凭证加密密钥（APP_SESSION_AUTH_EMBED_SECRET / APP_SESSION_JWT_SECRET）"
                    .into(),
            )
        })
}

fn derive_embed_key(secret: &str) -> [u8; 32] {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;

    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("hmac key");
    mac.update(b"phi-backend/session-auth/embed-v1");
    let bytes = mac.finalize().into_bytes();
    let mut key = [0u8; 32];
    key.copy_from_slice(&bytes[..32]);
    key
}

fn seal_auth_payload_json(raw_auth_json: &str, jti: &str, sub: &str) -> Result<String, AppError> {
    use aes_gcm::aead::{Aead, KeyInit};

    let secret = session_auth_crypto_secret()?;
    let key = derive_embed_key(&secret);
    let cipher = aes_gcm::Aes256Gcm::new(aes_gcm::Key::<aes_gcm::Aes256Gcm>::from_slice(&key));

    let nonce_bytes = uuid::Uuid::new_v4().as_bytes().to_owned();
    let nonce = aes_gcm::Nonce::from_slice(&nonce_bytes[..12]);
    let aad = format!("{jti}:{sub}");
    let encrypted = cipher
        .encrypt(
            nonce,
            aes_gcm::aead::Payload {
                msg: raw_auth_json.as_bytes(),
                aad: aad.as_bytes(),
            },
        )
        .map_err(|e| AppError::Internal(format!("加密会话凭证失败: {e}")))?;

    let mut payload = Vec::with_capacity(12 + encrypted.len());
    payload.extend_from_slice(&nonce_bytes[..12]);
    payload.extend_from_slice(&encrypted);
    Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(payload))
}

fn open_auth_payload_json(token: &str, jti: &str, sub: &str) -> Result<String, AppError> {
    use aes_gcm::aead::{Aead, KeyInit};

    let secret = session_auth_crypto_secret()?;
    let key = derive_embed_key(&secret);
    let cipher = aes_gcm::Aes256Gcm::new(aes_gcm::Key::<aes_gcm::Aes256Gcm>::from_slice(&key));

    let raw = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(token)
        .map_err(|_| AppError::Auth("Bearer 内嵌凭证格式无效".into()))?;
    if raw.len() < 13 {
        return Err(AppError::Auth("Bearer 内嵌凭证长度无效".into()));
    }
    let nonce = aes_gcm::Nonce::from_slice(&raw[..12]);
    let aad = format!("{jti}:{sub}");
    let plain = cipher
        .decrypt(
            nonce,
            aes_gcm::aead::Payload {
                msg: &raw[12..],
                aad: aad.as_bytes(),
            },
        )
        .map_err(|_| AppError::Auth("Bearer 内嵌凭证解密失败".into()))?;
    let plain_text = String::from_utf8(plain)
        .map_err(|e| AppError::Internal(format!("内嵌凭证不是有效 UTF-8: {e}")))?;
    Ok(plain_text)
}

pub fn build_embedded_auth_claim(
    auth: &UnifiedSaveRequest,
    jti: &str,
    sub: &str,
) -> Result<String, AppError> {
    let auth_json = serde_json::to_string(auth)
        .map_err(|e| AppError::Internal(format!("序列化登录凭证失败: {e}")))?;
    let auth_b64 = seal_auth_payload_json(&auth_json, jti, sub)?;
    let envelope = EmbeddedAuthEnvelope { auth_b64 };
    let env_json = serde_json::to_string(&envelope)
        .map_err(|e| AppError::Internal(format!("序列化凭证信封失败: {e}")))?;
    Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(env_json))
}

pub fn decode_embedded_auth(token: &str) -> Result<UnifiedSaveRequest, AppError> {
    let cfg = ensure_session_config()?;
    let claims = decode_access_token(token, cfg, true)?;
    decode_embedded_auth_with_claims(token, &claims)
}

pub fn decode_embedded_auth_allow_expired(token: &str) -> Result<UnifiedSaveRequest, AppError> {
    let cfg = ensure_session_config()?;
    let claims = decode_access_token_allow_expired(token, cfg)?;
    decode_embedded_auth_with_claims(token, &claims)
}

fn decode_embedded_auth_with_claims(
    token: &str,
    claims: &SessionClaims,
) -> Result<UnifiedSaveRequest, AppError> {
    let mut parts = token.split('.');
    let _header = parts
        .next()
        .ok_or_else(|| AppError::Auth("Bearer 格式无效".into()))?;
    let payload_b64 = parts
        .next()
        .ok_or_else(|| AppError::Auth("Bearer 格式无效".into()))?;

    let payload_json = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload_b64)
        .map_err(|_| AppError::Auth("Bearer payload 解码失败".into()))?;
    let payload_val: serde_json::Value = serde_json::from_slice(&payload_json)
        .map_err(|_| AppError::Auth("Bearer payload 解析失败".into()))?;
    let embed_raw = payload_val
        .get("sae")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AppError::Auth("Bearer 未携带内嵌凭证".into()))?;

    let env_json = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(embed_raw)
        .map_err(|_| AppError::Auth("Bearer 内嵌凭证信封解码失败".into()))?;
    let env: EmbeddedAuthEnvelope = serde_json::from_slice(&env_json)
        .map_err(|_| AppError::Auth("Bearer 内嵌凭证信封解析失败".into()))?;
    let plain = open_auth_payload_json(&env.auth_b64, &claims.jti, &claims.sub)?;
    let parsed: UnifiedSaveRequest = serde_json::from_str(&plain)
        .map_err(|_| AppError::Auth("Bearer 内嵌凭证内容无效".into()))?;
    Ok(parsed)
}

pub fn derive_user_identity_with_bearer(
    salt_opt: Option<&str>,
    auth: &UnifiedSaveRequest,
    bearer: &BearerAuthState,
) -> Result<(Option<String>, Option<String>), AppError> {
    if has_auth_credentials(auth) {
        let derived = crate::features::stats::derive_user_identity_from_auth(salt_opt, auth);
        if derived.0.is_some() {
            return Ok(derived);
        }

        return match bearer {
            BearerAuthState::Valid(ctx) => {
                Ok((Some(ctx.claims.sub.clone()), Some("session_bearer".into())))
            }
            BearerAuthState::Invalid(msg) => Err(AppError::Auth(msg.clone())),
            BearerAuthState::Absent => Ok(derived),
        };
    }

    match bearer {
        BearerAuthState::Valid(ctx) => {
            Ok((Some(ctx.claims.sub.clone()), Some("session_bearer".into())))
        }
        BearerAuthState::Invalid(msg) => Err(AppError::Auth(msg.clone())),
        BearerAuthState::Absent => Ok(crate::features::stats::derive_user_identity_from_auth(
            salt_opt, auth,
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        BearerAuthContext, BearerAuthState, SessionClaims, derive_user_identity_with_bearer,
        has_auth_credentials,
    };
    use crate::features::save::{ExternalApiCredentials, UnifiedSaveRequest};

    #[test]
    fn has_auth_credentials_checks_both_paths() {
        let req = UnifiedSaveRequest {
            session_token: Some("r:abc".into()),
            external_credentials: None,
            taptap_version: None,
        };
        assert!(has_auth_credentials(&req));

        let req = UnifiedSaveRequest {
            session_token: None,
            external_credentials: Some(ExternalApiCredentials {
                platform: Some("TapTap".into()),
                platform_id: Some("u1".into()),
                sessiontoken: None,
                api_user_id: None,
                api_token: None,
            }),
            taptap_version: None,
        };
        assert!(has_auth_credentials(&req));

        let req = UnifiedSaveRequest {
            session_token: None,
            external_credentials: None,
            taptap_version: None,
        };
        assert!(!has_auth_credentials(&req));
    }

    #[test]
    fn derive_user_identity_with_bearer_prefers_legacy_body_auth() {
        let req = UnifiedSaveRequest {
            session_token: Some("legacy-token".into()),
            external_credentials: None,
            taptap_version: None,
        };
        let bearer = BearerAuthState::Valid(BearerAuthContext {
            token: "jwt".into(),
            claims: SessionClaims {
                sub: "bearer-user-hash".into(),
                jti: "jti-1".into(),
                iss: "phi-backend".into(),
                aud: "phi-clients".into(),
                iat: 1,
                exp: i64::MAX,
            },
        });

        let (id, kind) = derive_user_identity_with_bearer(Some("salt"), &req, &bearer)
            .expect("derive identity should succeed");
        assert!(id.is_some());
        assert_eq!(kind.as_deref(), Some("session_token"));
    }

    #[test]
    fn derive_user_identity_with_bearer_uses_bearer_when_body_empty() {
        let req = UnifiedSaveRequest {
            session_token: None,
            external_credentials: None,
            taptap_version: None,
        };
        let bearer = BearerAuthState::Valid(BearerAuthContext {
            token: "jwt".into(),
            claims: SessionClaims {
                sub: "bearer-user-hash".into(),
                jti: "jti-1".into(),
                iss: "phi-backend".into(),
                aud: "phi-clients".into(),
                iat: 1,
                exp: i64::MAX,
            },
        });

        let (id, kind) = derive_user_identity_with_bearer(None, &req, &bearer)
            .expect("derive identity should succeed");
        assert_eq!(id.as_deref(), Some("bearer-user-hash"));
        assert_eq!(kind.as_deref(), Some("session_bearer"));
    }

    #[test]
    fn derive_user_identity_with_bearer_falls_back_when_legacy_auth_not_derivable() {
        let req = UnifiedSaveRequest {
            session_token: Some("legacy-token".into()),
            external_credentials: None,
            taptap_version: None,
        };
        let bearer = BearerAuthState::Valid(BearerAuthContext {
            token: "jwt".into(),
            claims: SessionClaims {
                sub: "bearer-user-hash".into(),
                jti: "jti-1".into(),
                iss: "phi-backend".into(),
                aud: "phi-clients".into(),
                iat: 1,
                exp: i64::MAX,
            },
        });

        let (id, kind) = derive_user_identity_with_bearer(None, &req, &bearer)
            .expect("derive identity should succeed");
        assert_eq!(id.as_deref(), Some("bearer-user-hash"));
        assert_eq!(kind.as_deref(), Some("session_bearer"));
    }
}
