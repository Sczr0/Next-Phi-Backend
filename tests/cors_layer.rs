use axum::{
    Router,
    body::Body,
    http::{Request, header},
    routing::get,
};
use tower::ServiceExt;

use phi_backend::config::CorsConfig;
use phi_backend::cors::build_cors_layer;

#[tokio::test]
async fn cors_layer_adds_allow_origin_header() {
    let cors = CorsConfig {
        enabled: true,
        allowed_origins: vec!["https://example.com".to_string()],
        allowed_methods: vec!["GET".to_string()],
        allowed_headers: vec!["Content-Type".to_string()],
        ..CorsConfig::default()
    };

    let layer = build_cors_layer(&cors).expect("cors layer");
    let app = Router::new()
        .route("/", get(|| async { "ok" }))
        .layer(layer);

    let req = Request::builder()
        .method("GET")
        .uri("/")
        .header(header::ORIGIN, "https://example.com")
        .body(Body::empty())
        .expect("build request");
    let resp = app.oneshot(req).await.expect("call app");

    let allow_origin = resp
        .headers()
        .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
        .expect("missing allow origin")
        .to_str()
        .expect("invalid allow origin");
    assert_eq!(allow_origin, "https://example.com");
}

#[tokio::test]
async fn cors_preflight_includes_allow_methods() {
    let cors = CorsConfig {
        enabled: true,
        allowed_origins: vec!["https://example.com".to_string()],
        allowed_methods: vec!["GET".to_string(), "POST".to_string()],
        ..CorsConfig::default()
    };

    let layer = build_cors_layer(&cors).expect("cors layer");
    let app = Router::new()
        .route("/", get(|| async { "ok" }))
        .layer(layer);

    let req = Request::builder()
        .method("OPTIONS")
        .uri("/")
        .header(header::ORIGIN, "https://example.com")
        .header(header::ACCESS_CONTROL_REQUEST_METHOD, "POST")
        .body(Body::empty())
        .expect("build request");
    let resp = app.oneshot(req).await.expect("call app");

    let allow_methods = resp
        .headers()
        .get(header::ACCESS_CONTROL_ALLOW_METHODS)
        .expect("missing allow methods")
        .to_str()
        .expect("invalid allow methods");
    assert!(allow_methods.contains("POST"));
}
