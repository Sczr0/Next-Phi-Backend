use std::collections::HashMap;

use axum::{
    extract::{Request, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use once_cell::sync::Lazy;
use tokio::sync::Mutex;

use crate::{
    config::{AppConfig, OpenPlatformConfig},
    error::AppError,
    features::open_platform::storage,
};

pub const OPEN_API_TOKEN_HEADER: &str = "x-openapi-token";

#[derive(Debug, Clone)]
pub struct OpenApiAuthContext {
    pub developer_id: String,
    pub key_id: String,
    pub scopes: Vec<String>,
    pub client_ip: Option<String>,
}

#[derive(Debug, Clone)]
pub struct OpenApiRoutePolicy {
    pub required_scopes: &'static [&'static str],
}

impl OpenApiRoutePolicy {
    pub const fn new(required_scopes: &'static [&'static str]) -> Self {
        Self { required_scopes }
    }
}

#[derive(Debug, Clone, Copy)]
struct RateWindow {
    minute_slot: i64,
    count: u32,
}

static OPEN_API_RATE_LIMITER: Lazy<Mutex<HashMap<String, RateWindow>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

fn resolve_key_hash_secret(cfg: &OpenPlatformConfig) -> Result<String, AppError> {
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

fn hash_api_key(secret: &str, token: &str) -> String {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;

    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("hmac key");
    mac.update(token.as_bytes());
    let out = mac.finalize().into_bytes();
    hex::encode(out)
}

fn extract_open_api_token(headers: &HeaderMap) -> Result<String, AppError> {
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

fn client_ip_from_headers(headers: &HeaderMap) -> Option<String> {
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

async fn ensure_rate_limit(
    key_id: &str,
    client_ip: Option<&str>,
    per_minute_limit: u32,
    now_ts: i64,
) -> bool {
    let minute_slot = now_ts / 60;
    let bucket_key = format!("{key_id}:{}", client_ip.unwrap_or("-"));
    let mut guard = OPEN_API_RATE_LIMITER.lock().await;
    let entry = guard.entry(bucket_key).or_insert(RateWindow {
        minute_slot,
        count: 0,
    });
    if entry.minute_slot != minute_slot {
        entry.minute_slot = minute_slot;
        entry.count = 0;
    }

    let max = per_minute_limit.max(1);
    if entry.count >= max {
        return false;
    }
    entry.count += 1;

    if guard.len() > 50_000 {
        guard.retain(|_, v| v.minute_slot >= minute_slot - 1);
    }
    true
}

fn forbidden_response(detail: impl Into<String>) -> Response {
    let problem = crate::error::ProblemDetails {
        type_url: "about:blank".to_string(),
        title: "Forbidden".to_string(),
        status: StatusCode::FORBIDDEN.as_u16(),
        detail: Some(detail.into()),
        code: "FORBIDDEN".to_string(),
        request_id: crate::request_id::current_request_id(),
        errors: None,
        candidates: None,
        candidates_total: None,
    };
    let mut res = axum::Json(problem).into_response();
    *res.status_mut() = StatusCode::FORBIDDEN;
    res.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/problem+json"),
    );
    res
}

async fn record_auth_failed_event(
    key: &storage::ApiKeyRecord,
    reason: &str,
    request_id: Option<&str>,
    now_ts: i64,
    client_ip: Option<&str>,
) {
    let Ok(st) = storage::global() else {
        return;
    };

    let metadata = serde_json::json!({
        "status": key.status,
        "clientIp": client_ip,
        "expiresAt": key.expires_at,
    });
    let _ = st
        .record_api_key_event(
            &key.id,
            &key.developer_id,
            storage::API_KEY_EVENT_AUTH_FAILED,
            Some(reason),
            None,
            request_id,
            Some(&metadata),
            now_ts,
        )
        .await;
}

pub async fn open_api_token_middleware(
    State(policy): State<OpenApiRoutePolicy>,
    mut req: Request,
    next: Next,
) -> Response {
    let cfg = &AppConfig::global().open_platform;
    if !cfg.enabled {
        return AppError::Validation("开放平台未启用".into()).into_response();
    }

    let now_ts = chrono::Utc::now().timestamp();
    let request_id = crate::request_id::current_request_id();
    let client_ip = client_ip_from_headers(req.headers());

    let token = match extract_open_api_token(req.headers()) {
        Ok(token) => token,
        Err(e) => return e.into_response(),
    };
    let hash_secret = match resolve_key_hash_secret(cfg) {
        Ok(secret) => secret,
        Err(e) => return e.into_response(),
    };
    let token_hash = hash_api_key(&hash_secret, &token);

    let st = match storage::global() {
        Ok(s) => s,
        Err(e) => return e.into_response(),
    };

    let key = match st.get_api_key_by_hash(&token_hash).await {
        Ok(Some(k)) => k,
        Ok(None) => return AppError::Auth("无效的 Open API Token".into()).into_response(),
        Err(e) => return e.into_response(),
    };

    if key.status != storage::API_KEY_STATUS_ACTIVE {
        record_auth_failed_event(
            &key,
            "api_key_not_active",
            request_id.as_deref(),
            now_ts,
            client_ip.as_deref(),
        )
        .await;
        return AppError::Auth("API Key 已失效".into()).into_response();
    }

    if let Some(expires_at) = key.expires_at
        && expires_at > 0
        && expires_at <= now_ts
    {
        let _ = st.cleanup_expired_active_keys(now_ts).await;
        record_auth_failed_event(
            &key,
            "api_key_expired",
            request_id.as_deref(),
            now_ts,
            client_ip.as_deref(),
        )
        .await;
        return AppError::Auth("API Key 已过期".into()).into_response();
    }

    for required_scope in policy.required_scopes {
        if !key.scopes.iter().any(|owned| owned == required_scope) {
            let reason = format!("missing_scope:{required_scope}");
            record_auth_failed_event(
                &key,
                &reason,
                request_id.as_deref(),
                now_ts,
                client_ip.as_deref(),
            )
            .await;
            return forbidden_response(format!("缺少 scope: {required_scope}"));
        }
    }

    if !ensure_rate_limit(
        &key.id,
        client_ip.as_deref(),
        cfg.api_key.rate_limit_per_minute,
        now_ts,
    )
    .await
    {
        record_auth_failed_event(
            &key,
            "rate_limited",
            request_id.as_deref(),
            now_ts,
            client_ip.as_deref(),
        )
        .await;
        return forbidden_response("开放平台请求频率超限");
    }

    req.extensions_mut().insert(OpenApiAuthContext {
        developer_id: key.developer_id.clone(),
        key_id: key.id.clone(),
        scopes: key.scopes.clone(),
        client_ip: client_ip.clone(),
    });

    let res = next.run(req).await;

    if let Err(e) = st
        .touch_api_key_usage(&key.id, now_ts, client_ip.as_deref())
        .await
    {
        tracing::warn!(
            target: "phi_backend::open_platform",
            "touch api key usage failed: {}",
            e
        );
    }

    res
}

#[cfg(test)]
mod tests {
    use axum::http::{HeaderMap, HeaderValue};

    use super::{client_ip_from_headers, ensure_rate_limit};

    #[test]
    fn client_ip_prefers_x_forwarded_for() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-forwarded-for",
            HeaderValue::from_static("1.2.3.4, 9.9.9.9"),
        );
        headers.insert("x-real-ip", HeaderValue::from_static("8.8.8.8"));
        assert_eq!(
            client_ip_from_headers(&headers),
            Some("1.2.3.4".to_string())
        );
    }

    #[tokio::test]
    async fn rate_limit_blocks_when_exceeded() {
        let key = format!("key_test_{}", uuid::Uuid::new_v4().simple());
        let ip = Some("127.0.0.1");
        let now = 1_700_000_000_i64;

        assert!(ensure_rate_limit(&key, ip, 2, now).await);
        assert!(ensure_rate_limit(&key, ip, 2, now + 1).await);
        assert!(!ensure_rate_limit(&key, ip, 2, now + 2).await);
    }
}
