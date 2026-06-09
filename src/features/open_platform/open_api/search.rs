use axum::{
    extract::{Query, State},
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
    responses(
        (status = 200, description = "Request succeeded."),
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
pub async fn open_search_songs(
    State(state): State<AppState>,
    Query(query): Query<crate::song_api::SongSearchQuery>,
) -> Result<Response, AppError> {
    crate::song_api::search_songs(State(state), Query(query)).await
}
