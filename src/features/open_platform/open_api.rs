use std::collections::HashMap;

use axum::{
    Router,
    extract::{Path, Query, Request, State},
    response::{Json, Response},
    routing::{get, post},
};

use crate::{error::AppError, state::AppState};

use super::token_auth::{OpenApiRoutePolicy, open_api_token_middleware};

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
        (status = 200, description = "生成二维码成功", body = crate::features::auth::handler::QrCodeCreateResponse),
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
    Query(params): Query<crate::features::auth::handler::QrCodeQuery>,
) -> Result<Response, AppError> {
    crate::features::auth::handler::post_qrcode(State(state), Query(params)).await
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
        (status = 200, description = "状态返回", body = crate::features::auth::handler::QrCodeStatusResponse),
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
    crate::features::auth::handler::get_qrcode_status(State(state), Path(qr_id)).await
}

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
    request_body = crate::features::save::models::UnifiedSaveRequest,
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
pub async fn open_save_data(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
    req: Request,
) -> Result<Response, AppError> {
    crate::features::save::handler::get_save_data(State(state), Query(params), req).await
}

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
    Query(query): Query<crate::features::song::handler::SongSearchQuery>,
) -> Result<Response, AppError> {
    crate::features::song::handler::search_songs(State(state), Query(query)).await
}

#[utoipa::path(
    get,
    path = "/open/leaderboard/rks/top",
    summary = "Open API: Leaderboard Top",
    description = "Open platform endpoint for public RKS top list. Requires X-OpenApi-Token and scope public.read.",
    security(
        ("OpenApiToken" = [])
    ),
    responses(
        (status = 200, description = "Request succeeded.", body = crate::features::leaderboard::models::LeaderboardTopResponse),
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
    Query(query): Query<crate::features::leaderboard::handler::TopQuery>,
) -> Result<Json<crate::features::leaderboard::models::LeaderboardTopResponse>, AppError> {
    crate::features::leaderboard::handler::get_top(State(state), Query(query)).await
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
        (status = 200, description = "Request succeeded.", body = crate::features::leaderboard::models::LeaderboardTopResponse),
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
    Query(query): Query<crate::features::leaderboard::handler::RankQuery>,
) -> Result<Json<crate::features::leaderboard::models::LeaderboardTopResponse>, AppError> {
    crate::features::leaderboard::handler::get_by_rank(State(state), Query(query)).await
}

#[utoipa::path(
    post,
    path = "/open/rks/history",
    summary = "Open API: RKS History",
    description = "Open platform endpoint for user RKS history. Requires X-OpenApi-Token and scope profile.read.",
    security(
        ("OpenApiToken" = [])
    ),
    request_body = crate::features::rks::handler::RksHistoryRequest,
    responses(
        (status = 200, description = "Request succeeded.", body = crate::features::rks::handler::RksHistoryResponse),
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
) -> Result<Json<crate::features::rks::handler::RksHistoryResponse>, AppError> {
    crate::features::rks::handler::post_rks_history(State(state), req).await
}

pub fn create_open_platform_open_api_router() -> Router<AppState> {
    let public_read_policy = OpenApiRoutePolicy::new(&["public.read"]);
    let profile_read_policy = OpenApiRoutePolicy::new(&["profile.read"]);

    Router::<AppState>::new()
        .route(
            "/open/auth/qrcode",
            post(open_auth_qrcode).route_layer(axum::middleware::from_fn_with_state(
                profile_read_policy.clone(),
                open_api_token_middleware,
            )),
        )
        .route(
            "/open/auth/qrcode/:qr_id/status",
            get(open_auth_qrcode_status).route_layer(axum::middleware::from_fn_with_state(
                profile_read_policy.clone(),
                open_api_token_middleware,
            )),
        )
        .route(
            "/open/save",
            post(open_save_data).route_layer(axum::middleware::from_fn_with_state(
                profile_read_policy.clone(),
                open_api_token_middleware,
            )),
        )
        .route(
            "/open/songs/search",
            get(open_search_songs).route_layer(axum::middleware::from_fn_with_state(
                public_read_policy.clone(),
                open_api_token_middleware,
            )),
        )
        .route(
            "/open/leaderboard/rks/top",
            get(open_get_leaderboard_top).route_layer(axum::middleware::from_fn_with_state(
                public_read_policy.clone(),
                open_api_token_middleware,
            )),
        )
        .route(
            "/open/leaderboard/rks/by-rank",
            get(open_get_leaderboard_by_rank).route_layer(axum::middleware::from_fn_with_state(
                public_read_policy,
                open_api_token_middleware,
            )),
        )
        .route(
            "/open/rks/history",
            post(open_post_rks_history).route_layer(axum::middleware::from_fn_with_state(
                profile_read_policy,
                open_api_token_middleware,
            )),
        )
}
