use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
    routing::get,
};
use tower::ServiceExt;

async fn ok_handler() -> &'static str {
    "ok"
}

async fn fail_handler() -> Result<&'static str, phi_backend::AppError> {
    Err(phi_backend::AppError::Validation("bad request".into()))
}

fn build_app() -> Router {
    Router::new()
        .route("/ok", get(ok_handler))
        .route("/fail", get(fail_handler))
        .layer(axum::middleware::from_fn(
            phi_backend::request_id::request_id_middleware,
        ))
}

#[tokio::test]
async fn request_id_is_generated_when_missing() {
    let app = build_app();
    let resp = app
        .oneshot(Request::builder().uri("/ok").body(Body::empty()).unwrap())
        .await
        .expect("request /ok");

    assert_eq!(resp.status(), StatusCode::OK);
    let request_id = resp
        .headers()
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(!request_id.is_empty(), "x-request-id should be generated");
}

#[tokio::test]
async fn request_id_uses_client_value_when_valid() {
    let app = build_app();
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/ok")
                .header("x-request-id", "client.req-001")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("request /ok");

    assert_eq!(resp.status(), StatusCode::OK);
    let request_id = resp
        .headers()
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert_eq!(request_id, "client.req-001");
}

#[tokio::test]
async fn problem_details_contains_request_id() {
    let app = build_app();
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/fail")
                .header("x-request-id", "err.req-001")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("request /fail");

    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let request_id_header = resp
        .headers()
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    let body = to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("read body");
    let json: serde_json::Value = serde_json::from_slice(&body).expect("parse json");
    assert_eq!(json["requestId"].as_str(), Some(request_id_header.as_str()));
}
