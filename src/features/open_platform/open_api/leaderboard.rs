use axum::{
    Json,
    extract::{Query, State},
};

use crate::{error::AppError, state::AppState};

#[utoipa::path(
    get,
    path = "/open/leaderboard/rks/top",
    summary = "Open API: Leaderboard Top",
    description = "Open platform endpoint for public RKS top list. Requires X-OpenApi-Token and scope public.read.",
    security(
        ("OpenApiToken" = [])
    ),
    responses(
        (status = 200, description = "Request succeeded.", body = crate::leaderboard_api::LeaderboardTopResponse),
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
pub async fn open_get_leaderboard_top(
    State(state): State<AppState>,
    Query(query): Query<crate::leaderboard_api::TopQuery>,
) -> Result<Json<crate::leaderboard_api::LeaderboardTopResponse>, AppError> {
    crate::leaderboard_api::get_top(State(state), Query(query)).await
}

#[utoipa::path(
    get,
    path = "/open/leaderboard/rks/by-rank",
    summary = "Open API: Leaderboard Range",
    description = "Open platform endpoint for public RKS rank range query. Requires X-OpenApi-Token and scope public.read.",
    security(
        ("OpenApiToken" = [])
    ),
    responses(
        (status = 200, description = "Request succeeded.", body = crate::leaderboard_api::LeaderboardTopResponse),
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
pub async fn open_get_leaderboard_by_rank(
    State(state): State<AppState>,
    Query(query): Query<crate::leaderboard_api::RankQuery>,
) -> Result<Json<crate::leaderboard_api::LeaderboardTopResponse>, AppError> {
    crate::leaderboard_api::get_by_rank(State(state), Query(query)).await
}
