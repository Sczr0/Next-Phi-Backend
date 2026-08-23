use crate::extract::ValidatedQuery;
use axum::{
    extract::{Request, State},
    response::Response,
};

use crate::{error::AppError, state::AppState};

#[utoipa::path(
    post,
    path = "/open/save",
    summary = "Open API: Parse Save Data",
    description = "Open platform endpoint for save parsing. Requires X-OpenApi-Token and scope profile.read.",
    security(
        ("OpenApiToken" = [])
    ),
    params(
        ("calculate_rks" = Option<bool>, Query, description = "Set true to include RKS calculation result.")
    ),
    request_body = crate::auth_contract::UnifiedSaveRequest,
    responses(
        (
            status = 200,
            description = "Request succeeded.",
            body = crate::save_contract::SaveApiResponse
        ),
        (
            status = 400,
            description = "Bad request (missing/invalid credentials).",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        ),
        (
            status = 401,
            description = "Token is missing, invalid, revoked or expired, or upstream authentication failed.",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        ),
        (
            status = 403,
            description = "Scope is insufficient or request is rate limited.",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        ),
        (
            status = 422,
            description = "Validation failed / save data invalid.",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        ),
        (
            status = 502,
            description = "Upstream network error.",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        ),
        (
            status = 504,
            description = "Upstream timeout.",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        ),
        (
            status = 500,
            description = "Internal server error.",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        )
    ),
    tag = "OpenPlatformOpenApi"
)]
pub async fn open_save_data(
    State(state): State<AppState>,
    ValidatedQuery(params): ValidatedQuery<std::collections::BTreeMap<String, String>>,
    req: Request,
) -> Result<Response, AppError> {
    crate::save_api::get_save_data(State(state), ValidatedQuery(params), req).await
}
