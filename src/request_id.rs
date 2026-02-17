use axum::{extract::Request, http::HeaderValue, middleware::Next, response::Response};
use uuid::Uuid;

/// 请求上下文中的 request_id。
#[derive(Debug, Clone)]
pub struct RequestId(pub String);

impl RequestId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

tokio::task_local! {
    /// 当前异步任务绑定的 request_id，用于错误响应透传。
    static TASK_REQUEST_ID: String;
}

/// 获取当前请求上下文中的 request_id。
pub fn current_request_id() -> Option<String> {
    TASK_REQUEST_ID.try_with(|v| v.clone()).ok()
}

fn is_valid_request_id(v: &str) -> bool {
    !v.is_empty()
        && v.len() <= 128
        && v.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.')
}

fn resolve_request_id(req: &Request) -> String {
    if let Some(raw) = req
        .headers()
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        && is_valid_request_id(raw)
    {
        return raw.to_string();
    }
    format!("req_{}", Uuid::new_v4().simple())
}

/// 全局 request_id 中间件：
/// - 优先透传客户端传入的 `X-Request-Id`
/// - 缺失或非法时服务端自动生成
/// - 回写到响应头，并注入请求上下文供错误响应使用
pub async fn request_id_middleware(mut req: Request, next: Next) -> Response {
    let request_id = resolve_request_id(&req);
    req.extensions_mut().insert(RequestId(request_id.clone()));

    let mut res = TASK_REQUEST_ID
        .scope(request_id.clone(), async move { next.run(req).await })
        .await;

    if let Ok(value) = HeaderValue::from_str(&request_id) {
        res.headers_mut().insert("x-request-id", value);
    }

    res
}

#[cfg(test)]
mod tests {
    use super::is_valid_request_id;

    #[test]
    fn request_id_validation_accepts_safe_chars() {
        assert!(is_valid_request_id("req-123_abc.def"));
    }

    #[test]
    fn request_id_validation_rejects_empty_and_unsafe_chars() {
        assert!(!is_valid_request_id(""));
        assert!(!is_valid_request_id("bad id"));
        assert!(!is_valid_request_id("bad/xx"));
    }
}
