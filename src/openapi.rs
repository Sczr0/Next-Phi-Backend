#![allow(clippy::needless_for_each)]

use utoipa::openapi::security::{ApiKey, ApiKeyValue, SecurityScheme};
use utoipa::openapi::server::{ServerBuilder, ServerVariableBuilder};
use utoipa::{Modify, OpenApi};

struct AdminTokenSecurity;

impl Modify for AdminTokenSecurity {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        let components = openapi.components.get_or_insert_with(Default::default);
        components.add_security_scheme(
            "AdminToken",
            SecurityScheme::ApiKey(ApiKey::Header(ApiKeyValue::new("X-Admin-Token"))),
        );
        components.add_security_scheme(
            "OpenApiToken",
            SecurityScheme::ApiKey(ApiKey::Header(ApiKeyValue::new("X-OpenApi-Token"))),
        );
    }
}

struct ApiServers;

impl Modify for ApiServers {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        let api = ServerBuilder::new()
            .url("{api_prefix}")
            .description(Some("Business API endpoints"))
            .parameter(
                "api_prefix",
                ServerVariableBuilder::new()
                    .default_value("/api/v2")
                    .description(Some("API prefix (maps to config.api.prefix)")),
            )
            .build();

        let root = ServerBuilder::new()
            .url("/")
            .description(Some("Root endpoints such as /health"))
            .build();

        openapi.servers = Some(vec![api, root]);
    }
}

#[allow(clippy::needless_for_each)]
#[derive(OpenApi)]
#[openapi(
    paths(
        crate::features::health::handler::health_check,
        crate::features::save::handler::get_save_data,
        crate::features::auth::handler::qrcode::post_qrcode,
        crate::features::auth::handler::qrcode::get_qrcode_status,
        crate::features::auth::handler::user_id::post_user_id,
        crate::features::auth::handler::session::post_session_exchange,
        crate::features::auth::handler::session::post_session_refresh,
        crate::features::auth::handler::session::post_session_logout,
        crate::features::open_platform::auth::handlers::get_github_login,
        crate::features::open_platform::auth::handlers::get_github_callback,
        crate::features::open_platform::auth::handlers::get_me,
        crate::features::open_platform::auth::handlers::post_logout,
        crate::features::open_platform::keys::handlers::post_create_api_key,
        crate::features::open_platform::keys::handlers::get_api_keys,
        crate::features::open_platform::keys::handlers::post_rotate_api_key,
        crate::features::open_platform::keys::handlers::post_revoke_api_key,
        crate::features::open_platform::keys::handlers::post_delete_api_key,
        crate::features::open_platform::keys::handlers::get_api_key_events,
        crate::features::open_platform::keys::handlers::get_api_key_rate_limit,
        crate::features::open_platform::open_api::auth::open_auth_qrcode,
        crate::features::open_platform::open_api::auth::open_auth_qrcode_status,
        crate::features::open_platform::open_api::save::open_save_data,
        crate::features::open_platform::open_api::image::open_image_bn,
        crate::features::open_platform::open_api::image::open_image_song,
        crate::features::open_platform::open_api::search::open_search_songs,
        crate::features::open_platform::open_api::leaderboard::open_get_leaderboard_top,
        crate::features::open_platform::open_api::leaderboard::open_get_leaderboard_by_rank,
        crate::features::open_platform::open_api::rks::open_post_rks_history,
        crate::features::song::handler::search_songs,
        crate::features::image::handler::bn::render_bn,
        crate::features::image::handler::song::render_song,
        crate::features::image::handler::user_bn::render_bn_user,
        crate::features::stats::handler::get_daily_stats,
        crate::features::stats::handler::get_daily_features,
        crate::features::stats::handler::get_daily_dau,
        crate::features::stats::handler::daily_http::get_daily_http,
        crate::features::stats::handler::latency::get_latency_agg,
        crate::features::stats::handler::summary::get_stats_summary,
        crate::features::stats::handler::archive_now::trigger_archive_now,
        crate::features::leaderboard::handler::ranking::get_top,
        crate::features::leaderboard::handler::ranking::get_by_rank,
        crate::features::leaderboard::handler::ranking::post_me,
        crate::features::leaderboard::handler::profile::put_alias,
        crate::features::leaderboard::handler::profile::put_profile,
        crate::features::leaderboard::handler::profile::get_public_profile,
        crate::features::leaderboard::handler::admin::get_suspicious,
        crate::features::leaderboard::handler::admin::get_admin_leaderboard_users,
        crate::features::leaderboard::handler::admin::post_resolve,
        crate::features::leaderboard::handler::admin::get_admin_user_status,
        crate::features::leaderboard::handler::admin::post_admin_user_status,
        crate::features::leaderboard::handler::admin::post_alias_force,
        crate::features::rks::handler::post_rks_history,
    ),
    modifiers(&AdminTokenSecurity, &ApiServers),
    tags(
        (name = "Save", description = "Save parsing APIs"),
        (name = "Auth", description = "TapTap authentication APIs"),
        (
            name = "OpenPlatformAuth",
            description = "Open platform developer OAuth and session APIs"
        ),
        (
            name = "OpenPlatformKeys",
            description = "Open platform API key lifecycle management"
        ),
        (
            name = "OpenPlatformOpenApi",
            description = "Open platform business APIs under /open/*, secured by X-OpenApi-Token"
        ),
        (name = "Song", description = "Song search APIs"),
        (name = "Image", description = "Image rendering APIs"),
        (name = "Stats", description = "Service statistics APIs"),
        (name = "Leaderboard", description = "Leaderboard APIs"),
        (name = "RKS", description = "RKS history APIs"),
        (name = "Health", description = "Health check APIs"),
    ),
    info(
        title = "Phi Backend API",
        version = env!("CARGO_PKG_VERSION"),
        description = "Backend service API (Axum + utoipa). Business APIs are mounted under config.api.prefix (default /api/v2). Open platform APIs are exposed as /open/* and require X-OpenApi-Token."
    )
)]
pub struct ApiDoc;
