use axum::{
    http::{StatusCode, header},
    response::IntoResponse,
};

/// v2 契约关键点：全局错误必须为 RFC7807 ProblemDetails（application/problem+json）。
#[tokio::test]
async fn app_error_into_response_is_problem_details() {
    let resp = phi_backend::AppError::Json("缺少参数 q".to_string()).into_response();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    let content_type = resp
        .headers()
        .get(header::CONTENT_TYPE)
        .expect("missing Content-Type")
        .to_str()
        .expect("invalid Content-Type");
    assert_eq!(content_type, "application/problem+json");

    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("read body");
    let v: serde_json::Value = serde_json::from_slice(&bytes).expect("parse json");

    // 核心字段（强一致契约）
    assert_eq!(v["status"], 400);
    assert_eq!(v["code"], "BAD_REQUEST");
    assert!(v.get("type").is_some());
    assert!(v.get("title").is_some());
    assert!(v.get("detail").is_some());
}

/// v2 契约关键点：对外 JSON 字段命名统一 camelCase。
#[test]
fn render_bn_request_serializes_as_camel_case() {
    use phi_backend::features::image::{RenderBnRequest, Theme};
    use phi_backend::features::save::models::UnifiedSaveRequest;

    let req = RenderBnRequest {
        auth: UnifiedSaveRequest {
            session_token: Some("r:token".to_string()),
            external_credentials: None,
            taptap_version: None,
        },
        n: 30,
        theme: Theme::Black,
        embed_images: true,
        nickname: None,
    };

    let v = serde_json::to_value(req).expect("serialize json");

    // snake_case 字段应被重命名为 camelCase
    assert!(v.get("embedImages").is_some());
    assert!(v.get("embed_images").is_none());

    // flatten 的认证字段也应遵循 camelCase
    assert!(v.get("sessionToken").is_some());
    assert!(v.get("session_token").is_none());
}
