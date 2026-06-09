use axum::{
    Router,
    routing::{get, post},
};

use crate::state::AppState;

pub(crate) mod auth;
pub(crate) mod image;
pub(crate) mod leaderboard;
pub(crate) mod rks;
pub(crate) mod save;
pub(crate) mod search;

use super::token_auth::{OpenApiRoutePolicy, open_api_token_middleware};

pub(crate) use self::auth::{open_auth_qrcode, open_auth_qrcode_status};
pub use self::image::{open_image_bn, open_image_song};
pub use self::leaderboard::{open_get_leaderboard_by_rank, open_get_leaderboard_top};
pub use self::rks::open_post_rks_history;
pub use self::save::open_save_data;
pub use self::search::open_search_songs;

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
            "/open/image/bn",
            post(open_image_bn).route_layer(axum::middleware::from_fn_with_state(
                profile_read_policy.clone(),
                open_api_token_middleware,
            )),
        )
        .route(
            "/open/image/song",
            post(open_image_song).route_layer(axum::middleware::from_fn_with_state(
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
