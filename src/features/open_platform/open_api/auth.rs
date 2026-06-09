use axum::{
    extract::{Path, Query, State},
    response::Response,
};

use crate::{error::AppError, state::AppState};

#[utoipa::path(
    post,
    path = "/open/auth/qrcode",
    summary = "Open API: 生成 TapTap 登录二维码",
    description = "开放平台二维码登录入口。需要 X-OpenApi-Token，且 API Key 包含 profile.read scope。",
    security(
        ("OpenApiToken" = [])
    ),
    params(
        ("taptapVersion" = Option<String>, Query, description = "TapTap 版本：cn（大陆版）或 global（国际版）")
    ),
    responses(
        (status = 200, description = "生成二维码成功", body = crate::auth_qrcode_api::QrCodeCreateResponse),
        (
            status = 401,
            description = "Token 缺失、无效、被吊销或已过期",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        ),
        (
            status = 403,
            description = "Scope 不足或请求触发限流",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        )
    ),
    tag = "OpenPlatformOpenApi"
)]
pub(crate) async fn open_auth_qrcode(
    State(state): State<AppState>,
    Query(params): Query<crate::auth_qrcode_api::QrCodeQuery>,
) -> Result<Response, AppError> {
    crate::auth_qrcode_api::post_qrcode(State(state), Query(params)).await
}

#[utoipa::path(
    get,
    path = "/open/auth/qrcode/{qr_id}/status",
    summary = "Open API: 轮询 TapTap 二维码登录状态",
    description = "开放平台二维码登录状态轮询入口。需要 X-OpenApi-Token，且 API Key 包含 profile.read scope。",
    security(
        ("OpenApiToken" = [])
    ),
    params(
        ("qr_id" = String, Path, description = "二维码 ID")
    ),
    responses(
        (status = 200, description = "状态返回", body = crate::auth_qrcode_api::QrCodeStatusResponse),
        (
            status = 401,
            description = "Token 缺失、无效、被吊销或已过期",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        ),
        (
            status = 403,
            description = "Scope 不足或请求触发限流",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        )
    ),
    tag = "OpenPlatformOpenApi"
)]
pub(crate) async fn open_auth_qrcode_status(
    State(state): State<AppState>,
    Path(qr_id): Path<String>,
) -> Result<Response, AppError> {
    crate::auth_qrcode_api::get_qrcode_status(State(state), Path(qr_id)).await
}
