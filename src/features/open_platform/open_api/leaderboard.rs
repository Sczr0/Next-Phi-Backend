use crate::extract::ValidatedQuery;
use axum::{
    Json,
    extract::State,
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
    params(
        ("limit" = Option<i64>, Query, minimum = 1, maximum = 1000, description = "Items per page, default 50; max 200 normally, max 1000 with lite=true."),
        ("offset" = Option<i64>, Query, description = "Offset."),
        ("cursor" = Option<String>, Query, description = "Encrypted cursor; takes precedence over offset and after_*."),
        ("after_score" = Option<f64>, Query, description = "Legacy cursor: last item score (used with after_updated/after_user)."),
        ("after_updated" = Option<String>, Query, description = "Legacy cursor: last item updatedAt (RFC3339)."),
        ("after_user" = Option<String>, Query, description = "Legacy cursor: last item masked user (hash prefix)."),
        ("lite" = Option<bool>, Query, description = "Lite mode: omit bestTop3/apTop3 (default false).")
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
        ),
        (
            status = 422,
            description = "Invalid cursor or after_* parameters.",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        ),
        (
            status = 500,
            description = "Stats storage not initialized / query failed.",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        )
    ),
    tag = "OpenPlatformOpenApi"
)]
pub async fn open_get_leaderboard_top(
    State(state): State<AppState>,
    ValidatedQuery(query): ValidatedQuery<crate::leaderboard_api::TopQuery>,
) -> Result<Json<crate::leaderboard_api::LeaderboardTopResponse>, AppError> {
    crate::leaderboard_api::get_top(State(state), ValidatedQuery(query)).await
}

#[utoipa::path(
    get,
    path = "/open/leaderboard/rks/by-rank",
    summary = "Open API: Leaderboard Range",
    description = "Open platform endpoint for public RKS rank range query. Requires X-OpenApi-Token and scope public.read.",
    security(
        ("OpenApiToken" = [])
    ),
    params(
        ("rank" = Option<i64>, Query, description = "Single rank (1-based)."),
        ("start" = Option<i64>, Query, description = "Start rank (1-based)."),
        ("end" = Option<i64>, Query, description = "End rank (inclusive)."),
        ("count" = Option<i64>, Query, description = "Item count (combined with start, max 200)."),
        ("lite" = Option<bool>, Query, description = "Lite mode: omit bestTop3/apTop3 (default false).")
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
        ),
        (
            status = 422,
            description = "Validation failed (missing rank/start, etc.).",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        ),
        (
            status = 500,
            description = "Stats storage not initialized / query failed.",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        )
    ),
    tag = "OpenPlatformOpenApi"
)]
pub async fn open_get_leaderboard_by_rank(
    State(state): State<AppState>,
    ValidatedQuery(query): ValidatedQuery<crate::leaderboard_api::RankQuery>,
) -> Result<Json<crate::leaderboard_api::LeaderboardTopResponse>, AppError> {
    crate::leaderboard_api::get_by_rank(State(state), ValidatedQuery(query)).await
}
