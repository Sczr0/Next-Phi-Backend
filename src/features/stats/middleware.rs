use std::time::Instant;

use axum::{
    extract::{MatchedPath, Request, State},
    middleware::Next,
    response::Response,
};

use super::{StatsHandle, models::EventInsert};
use crate::config::AppConfig;
use hmac::{Hmac, Mac};
use sha2::Sha256;

/// 采集中间件：记录每次 HTTP 请求的基本信息
pub async fn stats_middleware(
    State(state): State<StateWithStats>,
    req: Request,
    next: Next,
) -> Response {
    let started = Instant::now();
    let method = req.method().to_string();
    let route = req
        .extensions()
        .get::<MatchedPath>()
        .map(|m| m.as_str().to_string());

    // 去敏 IP 哈希（优先 X-Forwarded-For / X-Real-IP）
    let ip_raw = client_ip_from_headers(req.headers());
    let client_ip_hash = ip_raw.and_then(|ip| {
        AppConfig::global()
            .stats
            .user_hash_salt
            .as_deref()
            .map(|salt| hmac_hex16(salt, &ip))
    });
    // 透传
    let res = next.run(req).await;
    let status = res.status().as_u16();
    let dur = started.elapsed();

    let evt = EventInsert {
        ts_utc: chrono::Utc::now(),
        route,
        feature: None,
        action: None,
        method: Some(method),
        status: Some(status),
        duration_ms: Some(dur.as_millis() as i64),
        user_hash: None,
        client_ip_hash,
        instance: Some(crate::features::stats::hostname()),
        extra_json: None,
    };
    // 异步上报（不阻塞）
    state.stats.track(evt).await;

    res
}

#[derive(Clone)]
pub struct StateWithStats {
    pub stats: StatsHandle,
}

fn hmac_hex16(salt: &str, value: &str) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(salt.as_bytes()).expect("HMAC key");
    mac.update(value.as_bytes());
    let bytes = mac.finalize().into_bytes();
    hex::encode(&bytes[..16])
}

fn client_ip_from_headers(headers: &axum::http::HeaderMap) -> Option<String> {
    if let Some(ip) = headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(',').next().map(|s| s.trim().to_string()))
        && !ip.is_empty()
    {
        return Some(ip);
    }
    if let Some(v) = headers.get("x-real-ip").and_then(|v| v.to_str().ok()) {
        let s = v.trim();
        if !s.is_empty() {
            return Some(s.to_string());
        }
    }
    None
}
