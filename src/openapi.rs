use utoipa::openapi::security::{ApiKey, ApiKeyValue, SecurityScheme};
use utoipa::openapi::server::{ServerBuilder, ServerVariableBuilder};
use utoipa::{Modify, OpenApi};

/// 在 OpenAPI 中注入 `X-Admin-Token` 的安全定义，供管理端接口复用。
struct AdminTokenSecurity;

impl Modify for AdminTokenSecurity {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        let components = openapi.components.get_or_insert_with(Default::default);
        components.add_security_scheme(
            "AdminToken",
            SecurityScheme::ApiKey(ApiKey::Header(ApiKeyValue::new("X-Admin-Token"))),
        );
    }
}

/// 为 Swagger UI 提供正确的“业务接口前缀”Servers 配置。
///
/// - 业务接口默认前缀为 `/api/v2`（对应 `config.api.prefix` / `APP_API_PREFIX`）。
/// - `/health` 不带前缀，因此额外提供 `/` 作为备用 server 以便在 Swagger UI 中切换测试。
struct ApiServers;

impl Modify for ApiServers {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        let api = ServerBuilder::new()
            .url("{api_prefix}")
            .description(Some("业务接口（默认 /api/v2）"))
            .parameter(
                "api_prefix",
                ServerVariableBuilder::new()
                    .default_value("/api/v2")
                    .description(Some(
                        "业务接口前缀：对应 config.api.prefix（可通过 APP_API_PREFIX 覆盖）",
                    )),
            )
            .build();

        let root = ServerBuilder::new()
            .url("/")
            .description(Some("根路径（用于 /health 等不带前缀接口）"))
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
        crate::features::auth::handler::post_session_logout,
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
        (
            name = "Save",
            description = "存档解析：通过官方 sessionToken 或外部凭证获取并解析存档。"
        ),
        (
            name = "Auth",
            description = "鉴权相关：TapTap 扫码登录、以及从凭证派生稳定 user_id。"
        ),
        (name = "Song", description = "曲目查询：按关键词/别名/ID 搜索曲目。"),
        (
            name = "Image",
            description = "图片渲染：BestN/单曲成绩图等（支持 png/jpeg/webp/svg 输出）。"
        ),
        (name = "Stats", description = "统计：调用量聚合、归档触发等。"),
        (
            name = "Leaderboard",
            description = "排行榜：Top/区间查询、公开资料，以及管理端审核接口。"
        ),
        (name = "RKS", description = "RKS：历史记录查询。"),
        (name = "Health", description = "健康检查：服务探活。"),
    ),
    info(
        title = "Phi Backend API",
        version = env!("CARGO_PKG_VERSION"),
        description = "后端服务 API（Axum + utoipa）。注意：除 /health 外，其余业务接口实际挂载在 `config.api.prefix`（默认 /api/v2）下，OpenAPI 的 paths 不包含该前缀。"
    )
)]
pub struct ApiDoc;
