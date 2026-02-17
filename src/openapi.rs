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

#[derive(OpenApi)]
#[openapi(
    paths(
        crate::features::health::handler::health_check,
        crate::features::save::handler::get_save_data,
        crate::features::auth::handler::post_qrcode,
        crate::features::auth::handler::get_qrcode_status,
        crate::features::auth::handler::post_user_id,
        crate::features::auth::handler::post_session_exchange,
        crate::features::auth::handler::post_session_refresh,
        crate::features::auth::handler::post_session_logout,
        crate::features::open_platform::auth::get_github_login,
        crate::features::open_platform::auth::get_github_callback,
        crate::features::open_platform::auth::get_me,
        crate::features::open_platform::auth::post_logout,
        crate::features::open_platform::keys::post_create_api_key,
        crate::features::open_platform::keys::get_api_keys,
        crate::features::open_platform::keys::post_rotate_api_key,
        crate::features::open_platform::keys::post_revoke_api_key,
        crate::features::open_platform::keys::post_delete_api_key,
        crate::features::open_platform::keys::get_api_key_events,
        crate::features::open_platform::open_api::open_auth_qrcode,
        crate::features::open_platform::open_api::open_auth_qrcode_status,
        crate::features::open_platform::open_api::open_save_data,
        crate::features::open_platform::open_api::open_search_songs,
        crate::features::open_platform::open_api::open_get_leaderboard_top,
        crate::features::open_platform::open_api::open_get_leaderboard_by_rank,
        crate::features::open_platform::open_api::open_post_rks_history,
        crate::features::song::handler::search_songs,
        crate::features::image::handler::render_bn,
        crate::features::image::handler::render_song,
        crate::features::image::handler::render_bn_user,
        crate::features::stats::handler::get_daily_stats,
        crate::features::stats::handler::get_daily_features,
        crate::features::stats::handler::get_daily_dau,
        crate::features::stats::handler::get_daily_http,
        crate::features::stats::handler::get_latency_agg,
        crate::features::stats::handler::get_stats_summary,
        crate::features::stats::handler::trigger_archive_now,
        crate::features::leaderboard::handler::get_top,
        crate::features::leaderboard::handler::get_by_rank,
        crate::features::leaderboard::handler::post_me,
        crate::features::leaderboard::handler::put_alias,
        crate::features::leaderboard::handler::put_profile,
        crate::features::leaderboard::handler::get_public_profile,
        crate::features::leaderboard::handler::get_suspicious,
        crate::features::leaderboard::handler::post_resolve,
        crate::features::leaderboard::handler::post_alias_force,
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
