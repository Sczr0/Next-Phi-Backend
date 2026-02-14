use axum::http::{HeaderValue, Method, header};
use std::time::Duration;
use tower_http::cors::{Any, CorsLayer};

use crate::config::CorsConfig;

/// 根据配置构建 CORS 中间件
pub fn build_cors_layer(cors: &CorsConfig) -> Option<CorsLayer> {
    if !cors.enabled {
        return None;
    }

    let (any_origin, origins) = parse_allowed_origins(&cors.allowed_origins);
    if !any_origin && origins.is_empty() {
        tracing::warn!("CORS 已启用但 allowed_origins 为空，已跳过启用");
        return None;
    }

    let (any_methods, methods) = parse_allowed_methods(&cors.allowed_methods);
    let (any_headers, headers) = parse_header_names("allowed_headers", &cors.allowed_headers);
    let (any_expose, expose_headers) = parse_header_names("expose_headers", &cors.expose_headers);

    if cors.allow_credentials && (any_origin || any_methods || any_headers || any_expose) {
        tracing::error!("CORS 配置无效：allow_credentials=true 不能与 \"*\" 同时使用，已跳过启用");
        return None;
    }

    let mut layer = CorsLayer::new();

    if any_origin {
        layer = layer.allow_origin(Any);
    } else if !origins.is_empty() {
        layer = layer.allow_origin(origins);
    }

    if any_methods {
        layer = layer.allow_methods(Any);
    } else if !methods.is_empty() {
        layer = layer.allow_methods(methods);
    }

    if any_headers {
        layer = layer.allow_headers(Any);
    } else if !headers.is_empty() {
        layer = layer.allow_headers(headers);
    }

    if any_expose {
        layer = layer.expose_headers(Any);
    } else if !expose_headers.is_empty() {
        layer = layer.expose_headers(expose_headers);
    }

    if cors.allow_credentials {
        layer = layer.allow_credentials(true);
    }

    if let Some(secs) = cors.max_age_secs
        && secs > 0
    {
        layer = layer.max_age(Duration::from_secs(secs));
    }

    Some(layer)
}

fn parse_allowed_origins(values: &[String]) -> (bool, Vec<HeaderValue>) {
    let mut any = false;
    let mut origins = Vec::new();
    for raw in values {
        let value = raw.trim();
        if value.is_empty() {
            continue;
        }
        if value == "*" {
            any = true;
            continue;
        }
        match HeaderValue::from_str(value) {
            Ok(v) => origins.push(v),
            Err(_) => tracing::warn!("CORS allowed_origins 含无效值: {}", value),
        }
    }
    (any, origins)
}

fn parse_allowed_methods(values: &[String]) -> (bool, Vec<Method>) {
    let mut any = false;
    let mut methods = Vec::new();
    for raw in values {
        let value = raw.trim();
        if value.is_empty() {
            continue;
        }
        if value == "*" {
            any = true;
            continue;
        }
        let normalized = value.to_ascii_uppercase();
        match Method::from_bytes(normalized.as_bytes()) {
            Ok(m) => methods.push(m),
            Err(_) => tracing::warn!("CORS allowed_methods 含无效值: {}", value),
        }
    }
    (any, methods)
}

fn parse_header_names(label: &str, values: &[String]) -> (bool, Vec<header::HeaderName>) {
    let mut any = false;
    let mut headers = Vec::new();
    for raw in values {
        let value = raw.trim();
        if value.is_empty() {
            continue;
        }
        if value == "*" {
            any = true;
            continue;
        }
        let normalized = value.to_ascii_lowercase();
        match header::HeaderName::from_bytes(normalized.as_bytes()) {
            Ok(h) => headers.push(h),
            Err(_) => tracing::warn!("CORS {} 含无效值: {}", label, value),
        }
    }
    (any, headers)
}

#[cfg(test)]
mod tests {
    use super::{build_cors_layer, parse_allowed_methods};
    use crate::config::CorsConfig;
    use axum::http::Method;

    #[test]
    fn build_cors_layer_skips_when_origins_empty() {
        let cors = CorsConfig {
            enabled: true,
            ..CorsConfig::default()
        };
        let layer = build_cors_layer(&cors);
        assert!(layer.is_none());
    }

    #[test]
    fn build_cors_layer_rejects_credentials_with_wildcard() {
        let cors = CorsConfig {
            enabled: true,
            allow_credentials: true,
            allowed_origins: vec!["*".to_string()],
            ..CorsConfig::default()
        };
        let layer = build_cors_layer(&cors);
        assert!(layer.is_none());
    }

    #[test]
    fn parse_allowed_methods_normalizes_case() {
        let input = vec!["get".to_string(), " POST ".to_string()];
        let (any, methods) = parse_allowed_methods(&input);
        assert!(!any);
        assert_eq!(methods, vec![Method::GET, Method::POST]);
    }
}
