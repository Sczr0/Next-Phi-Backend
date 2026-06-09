use axum::{
    extract::{Query, Request, State},
    response::{IntoResponse, Response},
};

use crate::{error::AppError, state::AppState};

#[utoipa::path(
    post,
    path = "/open/image/bn",
    summary = "Open API: Render BestN Image (SVG Only)",
    description = "Open platform endpoint for BestN image rendering. Requires X-OpenApi-Token and scope profile.read. Only format=svg is allowed.",
    security(
        ("OpenApiToken" = [])
    ),
    params(
        ("format" = Option<String>, Query, description = "Only supports svg. Omit or pass svg.")
    ),
    request_body = crate::image_api::RenderBnRequest,
    responses(
        (
            status = 200,
            description = "Request succeeded.",
            content((String = "image/svg+xml"))
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
            status = 422,
            description = "Validation failed (only format=svg is allowed).",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        )
    ),
    tag = "OpenPlatformOpenApi"
)]
pub async fn open_image_bn(
    State(state): State<AppState>,
    Query(query): Query<crate::image_api::ImageQueryOpts>,
    req: Request,
) -> Result<Response, AppError> {
    let svg_only_query = query.into_open_svg_only()?;
    let resp = crate::image_api::render_bn(State(state), Query(svg_only_query), req).await?;
    Ok(resp.into_response())
}

#[utoipa::path(
    post,
    path = "/open/image/song",
    summary = "Open API: Render Song Image (SVG Only)",
    description = "Open platform endpoint for song image rendering. Requires X-OpenApi-Token and scope profile.read. Only format=svg is allowed.",
    security(
        ("OpenApiToken" = [])
    ),
    params(
        ("format" = Option<String>, Query, description = "Only supports svg. Omit or pass svg.")
    ),
    request_body = crate::image_api::RenderSongRequest,
    responses(
        (
            status = 200,
            description = "Request succeeded.",
            content((String = "image/svg+xml"))
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
            status = 422,
            description = "Validation failed (only format=svg is allowed).",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        )
    ),
    tag = "OpenPlatformOpenApi"
)]
pub async fn open_image_song(
    State(state): State<AppState>,
    Query(query): Query<crate::image_api::ImageQueryOpts>,
    req: Request,
) -> Result<Response, AppError> {
    let svg_only_query = query.into_open_svg_only()?;
    let resp = crate::image_api::render_song(State(state), Query(svg_only_query), req).await?;
    Ok(resp.into_response())
}
