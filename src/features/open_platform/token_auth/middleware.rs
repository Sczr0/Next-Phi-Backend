use axum::{
    extract::{Request, State},
    http::{HeaderValue, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};

use crate::{config::AppConfig, error::AppError, features::open_platform::storage};

use super::{
    crypto::{
        client_ip_from_headers, extract_open_api_token, hash_api_key, resolve_key_hash_secret,
    },
    models::{OpenApiAuthContext, OpenApiRoutePolicy},
    rate_limit::{ensure_rate_limit, resolve_route_bucket},
};

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
    let route_bucket = resolve_route_bucket(&req);

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
        &route_bucket,
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
