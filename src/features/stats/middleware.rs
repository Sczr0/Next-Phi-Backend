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
    // 静态资源请求（尤其 SVG 模式下的曲绘引用）不纳入打点：
    // - 避免为每张图片做一次 IP 哈希/HMAC 与 SQLite 写入
    // - 降低高并发静态资源场景下的尾延迟与资源占用
    if req.uri().path().starts_with("/_ill/") {
        return next.run(req).await;
    }

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
            .map(|salt| hmac_hex16(salt, ip))
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
        instance: Some(crate::features::stats::hostname().into()),
        extra_json: None,
    };
    // 异步上报（不阻塞）
    state.stats.track(evt);

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

fn client_ip_from_headers(headers: &axum::http::HeaderMap) -> Option<&str> {
    if let Some(ip) = headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(',').next().map(|s| s.trim()))
        && !ip.is_empty()
    {
        return Some(ip);
    }
    if let Some(v) = headers.get("x-real-ip").and_then(|v| v.to_str().ok()) {
        let s = v.trim();
        if !s.is_empty() {
            return Some(s);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::client_ip_from_headers;
    use axum::http::{HeaderMap, HeaderValue};

    #[test]
    fn client_ip_prefers_x_forwarded_for_first_item() {
        let mut h = HeaderMap::new();
        h.insert(
            "x-forwarded-for",
            HeaderValue::from_static(" 1.2.3.4 , 5.6.7.8 "),
        );
        h.insert("x-real-ip", HeaderValue::from_static("9.9.9.9"));
        assert_eq!(client_ip_from_headers(&h), Some("1.2.3.4"));
    }

    #[test]
    fn client_ip_falls_back_to_x_real_ip() {
        let mut h = HeaderMap::new();
        h.insert("x-real-ip", HeaderValue::from_static(" 9.9.9.9 "));
        assert_eq!(client_ip_from_headers(&h), Some("9.9.9.9"));
    }

    #[test]
    fn client_ip_returns_none_for_missing_or_empty() {
        let h = HeaderMap::new();
        assert_eq!(client_ip_from_headers(&h), None);

        let mut h = HeaderMap::new();
        h.insert("x-forwarded-for", HeaderValue::from_static("   "));
        assert_eq!(client_ip_from_headers(&h), None);
    }

    #[test]
    fn client_ip_returns_none_for_non_utf8_header() {
        let mut h = HeaderMap::new();
        let v = HeaderValue::from_bytes(&[0xff, 0xfe, 0xfd]).unwrap();
        h.insert("x-forwarded-for", v);
        assert_eq!(client_ip_from_headers(&h), None);
    }
}
