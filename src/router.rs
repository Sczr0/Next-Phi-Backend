//! 路由装配与中间件构建
//!
//! 将 route 注册、middleware 层叠、压缩策略等横切关注点从 main.rs 中提取，
//! 保持 main.rs 专注于进程初始化与生命周期管理。

use axum::http::{HeaderValue, header};
use axum::middleware::Next;
use axum::response::Response;
use axum::{Router, extract::Request, routing::get};
use tower_http::compression::CompressionLayer;
use tower_http::services::ServeDir;
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

use crate::config::AppConfig;
use crate::cors::build_cors_layer;
use crate::features::health::handler::health_check;
use crate::features::leaderboard::handler::create_leaderboard_router;
use crate::features::open_platform;
use crate::features::stats::{
    StatsHandle,
    middleware::{StateWithStats, stats_middleware},
};
use crate::features::{auth, save, song};
use crate::openapi::ApiDoc;
use crate::state::AppState;

fn compression_predicate() -> impl tower_http::compression::predicate::Predicate {
    use tower_http::compression::predicate::{NotForContentType, Predicate, SizeAbove};

    SizeAbove::default()
        .and(NotForContentType::GRPC)
        .and(NotForContentType::IMAGES)
        .and(NotForContentType::SSE)
        .and(NotForContentType::const_new("application/octet-stream"))
        .and(NotForContentType::const_new("application/zip"))
        .and(NotForContentType::const_new("application/gzip"))
        .and(NotForContentType::const_new("application/x-gzip"))
        .and(NotForContentType::const_new("application/x-7z-compressed"))
        .and(NotForContentType::const_new("application/vnd.rar"))
        .and(NotForContentType::const_new("video/"))
        .and(NotForContentType::const_new("audio/"))
}

/// 为曲绘静态资源（`/_ill/*`）添加缓存头。
async fn ill_cache_control_middleware(req: Request, next: Next) -> Response {
    let is_ill = req.uri().path().starts_with("/_ill/");
    let mut res = next.run(req).await;
    if is_ill && res.headers().get(header::CACHE_CONTROL).is_none() {
        res.headers_mut().insert(
            header::CACHE_CONTROL,
            HeaderValue::from_static("public, max-age=604800, immutable"),
        );
    }
    res
}

/// 构建 API 子路由（`/api/v2/*` 下所有功能端点 + bearer 鉴权中间件）。
fn build_api_router(state: &AppState, config: &AppConfig) -> Router<AppState> {
    let mut api_router = Router::<AppState>::new()
        .nest("/auth", auth::create_auth_router())
        .merge(save::create_save_router())
        .merge(song::create_song_router())
        .merge(crate::features::image::create_image_router())
        .merge(create_leaderboard_router())
        .merge(crate::features::rks::handler::create_rks_router())
        .merge(crate::features::stats::handler::create_stats_router());

    if config.open_platform.enabled {
        api_router = api_router
            .merge(open_platform::auth::create_open_platform_auth_router())
            .merge(open_platform::keys::create_open_platform_keys_router())
            .merge(open_platform::open_api::create_open_platform_open_api_router());
    }

    api_router.layer(axum::middleware::from_fn_with_state(
        state.clone(),
        crate::features::auth::bearer::bearer_auth_middleware,
    ))
}

/// 构建完整的应用路由树，包含静态文件服务、Swagger UI 和所有中间件。
pub fn build_app(
    state: AppState,
    config: &AppConfig,
    stats_handle: Option<&StatsHandle>,
) -> Router {
    let ill_root = config.illustration_path();
    let api_router = build_api_router(&state, config);

    let mut app = Router::<AppState>::new()
        .route("/health", get(health_check))
        .nest_service("/_ill/ill", ServeDir::new(ill_root.join("ill")))
        .nest_service("/_ill/illLow", ServeDir::new(ill_root.join("illLow")))
        .nest_service("/_ill/illBlur", ServeDir::new(ill_root.join("illBlur")))
        .nest(&config.api.prefix, api_router)
        .merge(SwaggerUi::new("/docs").url("/api-docs/openapi.json", ApiDoc::openapi()))
        .with_state(state);

    // /_ill 缓存头
    app = app.layer(axum::middleware::from_fn(ill_cache_control_middleware));

    // 统计采集中间件
    if let Some(stats_handle) = stats_handle {
        let s = StateWithStats {
            stats: stats_handle.clone(),
        };
        app = app.layer(axum::middleware::from_fn_with_state(s, stats_middleware));
    }

    // 响应压缩
    app = app.layer(CompressionLayer::new().compress_when(compression_predicate()));

    // CORS
    if let Some(layer) = build_cors_layer(&config.cors) {
        tracing::info!("CORS 已启用");
        app = app.layer(layer);
    }

    // request_id 中间件（最外层）
    app = app.layer(axum::middleware::from_fn(
        crate::request_id::request_id_middleware,
    ));

    app
}

#[cfg(test)]
mod compression_predicate_tests {
    use super::compression_predicate;
    use axum::body::Body;
    use axum::http::{Response as HttpResponse, header};
    use tower_http::compression::predicate::Predicate;

    fn should_compress_for(ct: &str) -> bool {
        let body_bytes = vec![b'x'; 2048];
        let resp = HttpResponse::builder()
            .header(header::CONTENT_TYPE, ct)
            .body(Body::from(body_bytes))
            .unwrap();
        compression_predicate().should_compress(&resp)
    }

    #[test]
    fn compression_predicate_disables_sse() {
        assert!(!should_compress_for("text/event-stream"));
    }

    #[test]
    fn compression_predicate_disables_images_but_allows_svg() {
        assert!(!should_compress_for("image/png"));
        assert!(should_compress_for("image/svg+xml"));
    }

    #[test]
    fn compression_predicate_disables_common_binary_downloads() {
        assert!(!should_compress_for("application/octet-stream"));
        assert!(!should_compress_for("application/zip"));
        assert!(!should_compress_for("application/gzip"));
    }
}
