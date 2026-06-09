use axum::{
    Json,
    extract::{Request, State},
};

use crate::{error::AppError, state::AppState};

#[utoipa::path(
    post,
    path = "/open/rks/history",
    summary = "Open API: RKS History",
    description = "Open platform endpoint for user RKS history. Requires X-OpenApi-Token and scope profile.read.",
    security(
        ("OpenApiToken" = [])
    ),
    request_body = crate::rks_api::RksHistoryRequest,
    responses(
        (status = 200, description = "Request succeeded.", body = crate::rks_api::RksHistoryResponse),
        (
            status = 401,
            description = "Token is missing, invalid, revoked or expired.",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        ),
        (
            status = 403,
            description = "Scope is insufficient or request is rate limited.",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        )
    ),
    tag = "OpenPlatformOpenApi"
)]
pub async fn open_post_rks_history(
    State(state): State<AppState>,
    req: Request,
) -> Result<Json<crate::rks_api::RksHistoryResponse>, AppError> {
    crate::rks_api::post_rks_history(State(state), req).await
}
