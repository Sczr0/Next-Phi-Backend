//! 网关边缘限流中间件（Phase 1）。
//!
//! 采用 `governor` 的 GCRA 每 key 限流器（dashmap 状态存储，无全局锁，天然避免
//! 旧实现里 `Mutex<HashMap>` 串行化所有请求的问题）。
//!
//! 默认关闭（`limits.rate_limit_enabled = false`）；启用后超限返回 429
//! （`application/problem+json`）。此行为属对外契约 C1 的**受控变更**，见
//! `docs/adr/0006-edge-rate-limit-timeout.md`。

use std::net::SocketAddr;
use std::num::NonZeroU32;
use std::sync::Arc;

use axum::extract::{ConnectInfo, Request, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use governor::clock::DefaultClock;
use governor::state::keyed::DefaultKeyedStateStore;
use governor::{Quota, RateLimiter};

use crate::config::LimitsConfig;

/// governor 默认 keyed 限流器（key = 客户端 IP 字符串）。
type KeyedLimiter = RateLimiter<String, DefaultKeyedStateStore<String>, DefaultClock>;

/// 限流中间件共享状态（两层：全局宽松 + 昂贵端点更严）。
#[derive(Clone)]
pub struct RateLimitState {
    inner: Arc<RateLimitInner>,
}

struct RateLimitInner {
    global: KeyedLimiter,
    expensive: KeyedLimiter,
    trust_forwarded_for: bool,
}

impl RateLimitState {
    /// 依据配置构建限流器；`rate_limit_enabled = false` 时返回 `None`（不装配中间件）。
    #[must_use]
    pub fn from_config(cfg: &LimitsConfig) -> Option<Self> {
        if !cfg.rate_limit_enabled {
            return None;
        }
        Some(Self {
            inner: Arc::new(RateLimitInner {
                global: keyed(cfg.rate_limit_per_minute),
                expensive: keyed(cfg.expensive_rate_limit_per_minute),
                trust_forwarded_for: cfg.trust_forwarded_for,
            }),
        })
    }
}

fn keyed(per_minute: u32) -> KeyedLimiter {
    let n = NonZeroU32::new(per_minute).unwrap_or(NonZeroU32::MIN);
    RateLimiter::keyed(Quota::per_minute(n))
}

/// 全局限流：装配在整个应用外层；静态资源、健康检查、Swagger 文档不参与限流。
pub async fn global_rate_limit(
    State(state): State<RateLimitState>,
    req: Request,
    next: Next,
) -> Response {
    if should_skip(req.uri().path()) {
        return next.run(req).await;
    }
    let key = client_ip(&req, state.inner.trust_forwarded_for);
    if state.inner.global.check_key(&key).is_err() {
        return too_many_requests();
    }
    next.run(req).await
}

/// 昂贵端点限流：仅装配在图片渲染 / 存档 / 搜索子路由。
pub async fn expensive_rate_limit(
    State(state): State<RateLimitState>,
    req: Request,
    next: Next,
) -> Response {
    let key = client_ip(&req, state.inner.trust_forwarded_for);
    if state.inner.expensive.check_key(&key).is_err() {
        return too_many_requests();
    }
    next.run(req).await
}

/// 不参与全局限流的路径（静态/健康/文档）。
fn should_skip(path: &str) -> bool {
    path == "/health"
        || path.starts_with("/_ill/")
        || path.starts_with("/docs")
        || path == "/api-docs/openapi.json"
}

/// 解析客户端 IP：优先 `X-Forwarded-For`（取最左）→ `X-Real-IP` → 连接地址。
/// 全部缺失时兜底为 `"unknown"`，**绝不因取不到 IP 而报错/返回 5xx**。
fn client_ip(req: &Request, trust_forwarded_for: bool) -> String {
    if trust_forwarded_for {
        if let Some(ip) = header_first_ip(req.headers().get("x-forwarded-for")) {
            return ip;
        }
        if let Some(ip) = req
            .headers()
            .get("x-real-ip")
            .and_then(|v| v.to_str().ok())
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            return ip.to_string();
        }
    }
    if let Some(ci) = req.extensions().get::<ConnectInfo<SocketAddr>>() {
        return ci.0.ip().to_string();
    }
    "unknown".to_string()
}

fn header_first_ip(value: Option<&HeaderValue>) -> Option<String> {
    value
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.split(',').next())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn too_many_requests() -> Response {
    let problem = crate::error::ProblemDetails {
        type_url: "about:blank".to_string(),
        title: "Too Many Requests".to_string(),
        status: StatusCode::TOO_MANY_REQUESTS.as_u16(),
        detail: Some("请求过于频繁，请稍后重试".to_string()),
        code: "RATE_LIMITED".to_string(),
        request_id: crate::request_id::current_request_id(),
        errors: None,
        candidates: None,
        candidates_total: None,
    };
    let mut res = axum::Json(problem).into_response();
    *res.status_mut() = StatusCode::TOO_MANY_REQUESTS;
    res.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/problem+json"),
    );
    res
}

#[cfg(test)]
mod tests {
    use super::{header_first_ip, should_skip};
    use axum::http::HeaderValue;

    #[test]
    fn first_ip_from_forwarded_chain() {
        let v = HeaderValue::from_static("203.0.113.7, 10.0.0.1, 10.0.0.2");
        assert_eq!(header_first_ip(Some(&v)).as_deref(), Some("203.0.113.7"));
    }

    #[test]
    fn blank_or_absent_header_yields_none() {
        assert!(header_first_ip(None).is_none());
        let v = HeaderValue::from_static("  ");
        assert!(header_first_ip(Some(&v)).is_none());
    }

    #[test]
    fn skips_static_health_and_docs() {
        assert!(should_skip("/health"));
        assert!(should_skip("/_ill/ill/1.png"));
        assert!(should_skip("/docs"));
        assert!(!should_skip("/api/v2/save"));
    }
}
