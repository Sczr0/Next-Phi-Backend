use crate::extract::ValidatedQuery;
use axum::{
    extract::State,
    response::Response,
};

use crate::{error::AppError, state::AppState};

#[utoipa::path(
    get,
    path = "/open/songs/search",
    summary = "Open API: Search Songs",
    description = "Open platform endpoint for song search. Requires X-OpenApi-Token and scope public.read.",
    security(
        ("OpenApiToken" = [])
    ),
    params(
        ("q" = String, Query, description = "Search query string (required)."),
        ("unique" = Option<bool>, Query, description = "Whether to force a unique match (optional)."),
        ("mode" = Option<String>, Query, description = "Multi-keyword mode (optional: and/or)."),
        ("limit" = Option<u32>, Query, minimum = 1, maximum = 100, description = "Max items (optional, default 20, max 100, min 1)."),
        ("offset" = Option<u32>, Query, description = "Result offset (optional, default 0).")
    ),
    responses(
        (
            status = 200,
            description = "Request succeeded (single SongInfo when unique=true, otherwise a paged result).",
            body = crate::features::song::handler::SongSearchResult
        ),
        (
            status = 400,
            description = "Bad request (q is empty).",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        ),
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
            status = 404,
            description = "Not found (unique=true, no match).",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        ),
        (
            status = 409,
            description = "Not unique (unique=true, multiple matches).",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        ),
        (
            status = 422,
            description = "Validation failed (missing q / q too long / invalid limit / invalid mode).",
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
pub async fn open_search_songs(
    State(state): State<AppState>,
    ValidatedQuery(query): ValidatedQuery<crate::song_api::SongSearchQuery>,
) -> Result<Response, AppError> {
    crate::song_api::search_songs(State(state), ValidatedQuery(query)).await
}
