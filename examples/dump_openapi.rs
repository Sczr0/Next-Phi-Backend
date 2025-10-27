// 生成 OpenAPI JSON（无需启动服务），便于 SDK 代码生成
// 用法：cargo run --example dump_openapi > sdk/openapi.json

use utoipa::OpenApi;
use utoipa::{Modify};
use utoipa::openapi::security::{SecurityScheme, ApiKey, ApiKeyValue};

// 与 src/main.rs 内一致的安全设置
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

#[derive(OpenApi)]
#[openapi(
    paths(
        phi_backend::features::save::handler::get_save_data,
        phi_backend::features::auth::handler::get_qrcode,
        phi_backend::features::auth::handler::get_qrcode_status,
        phi_backend::features::song::handler::search_songs,
        phi_backend::features::image::handler::render_bn,
        phi_backend::features::image::handler::render_song,
        phi_backend::features::image::handler::render_bn_user,
        phi_backend::features::stats::handler::get_daily_stats,
        phi_backend::features::stats::handler::get_stats_summary,
        phi_backend::features::leaderboard::handler::get_top,
        phi_backend::features::leaderboard::handler::get_by_rank,
        phi_backend::features::leaderboard::handler::post_me,
        phi_backend::features::leaderboard::handler::put_alias,
        phi_backend::features::leaderboard::handler::put_profile,
        phi_backend::features::leaderboard::handler::get_public_profile,
        
    ),
    components(
        schemas(
            phi_backend::AppError,
            phi_backend::error::SaveProviderError,
            phi_backend::features::save::UnifiedSaveRequest,
            phi_backend::features::save::SaveResponse,
            phi_backend::features::save::models::ParsedSaveDoc,
            phi_backend::features::save::models::SaveResponseDoc,
            phi_backend::features::save::models::SaveAndRksResponseDoc,
            phi_backend::features::save::ExternalApiCredentials,
            phi_backend::features::save::handler::SaveAndRksResponse,
            phi_backend::features::rks::engine::PlayerRksResult,
            phi_backend::features::rks::engine::ChartRankingScore,
            phi_backend::features::save::models::Difficulty,
            phi_backend::features::auth::models::SessionData,
            phi_backend::features::auth::handler::QrCodeCreateResponse,
            phi_backend::features::auth::handler::QrCodeStatusResponse,
            phi_backend::features::song::models::SongInfo,
            phi_backend::features::song::handler::SongSearchResult,
            phi_backend::features::stats::models::DailyAggRow,
            phi_backend::features::stats::handler::FeatureUsageSummary,
            phi_backend::features::stats::handler::UniqueUsersSummary,
            phi_backend::features::stats::handler::StatsSummaryResponse,
            phi_backend::features::leaderboard::models::ChartTextItem,
            phi_backend::features::leaderboard::models::RksCompositionText,
            phi_backend::features::leaderboard::models::LeaderboardTopItem,
            phi_backend::features::leaderboard::models::LeaderboardTopResponse,
            phi_backend::features::leaderboard::models::MeResponse,
            phi_backend::features::leaderboard::models::AliasRequest,
            phi_backend::features::leaderboard::models::ProfileUpdateRequest,
            phi_backend::features::leaderboard::models::PublicProfileResponse,
        )
    ),
    modifiers(&AdminTokenSecurity),
    tags(
        (name = "Save", description = "Save APIs"),
        (name = "Auth", description = "Auth APIs"),
        (name = "Song", description = "Song APIs"),
        (name = "Image", description = "Image APIs"),
        (name = "Stats", description = "Stats APIs"),
        (name = "Leaderboard", description = "Leaderboard APIs"),
        (name = "Health", description = "Health APIs"),
    ),
    info(
        title = "Phi Backend API",
        version = "0.1.0",
        description = "Phigros backend service (Axum)"
    )
)]
struct ApiDocExample;

fn main() {
    let openapi = ApiDocExample::openapi();
    let json = serde_json::to_string_pretty(&openapi).expect("serialize openapi json");
    // 直接写入 UTF-8 文件，避免 PowerShell 重定向编码问题
    let _ = std::fs::create_dir_all("sdk");
    std::fs::write("sdk/openapi.json", json).expect("write openapi.json");
    println!("wrote sdk/openapi.json");
}
