use axum::{http::StatusCode, response::Json};
use serde::Serialize;

/// 健康检查响应
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct HealthResponse {
    /// 服务状态
    #[schema(example = "healthy")]
    pub status: String,
    /// 服务名称
    #[schema(example = "phi-backend")]
    pub service: String,
    /// 当前版本（Cargo package version）
    #[schema(example = "0.1.0")]
    pub version: String,
}

#[utoipa::path(
    get,
    path = "/health",
    summary = "健康检查",
    description = "用于探活的健康检查端点，返回服务状态与版本信息。",
    responses((status = 200, description = "服务健康", body = HealthResponse)),
    tag = "Health"
)]
pub async fn health_check() -> (StatusCode, Json<HealthResponse>) {
    (
        StatusCode::OK,
        Json(HealthResponse {
            status: "healthy".to_string(),
            service: "phi-backend".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
        }),
    )
}
