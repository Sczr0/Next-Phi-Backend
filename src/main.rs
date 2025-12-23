use axum::body::Bytes;
use axum::extract::Request;
use axum::http::{HeaderValue, header};
use axum::middleware::Next;
use axum::response::Response;
use axum::{Router, http::StatusCode, response::Json, routing::get};
use moka::future::Cache;
use phi_backend::features::auth::client::TapTapClient;
use phi_backend::features::leaderboard::handler::create_leaderboard_router;
use phi_backend::features::rks::handler::create_rks_router;
use phi_backend::features::stats::{
    self,
    handler::create_stats_router,
    middleware::{StateWithStats, stats_middleware},
};
use phi_backend::features::{auth, save, song};
use phi_backend::startup::chart_loader::{ChartConstantsMap, load_chart_constants};
use phi_backend::startup::{run_startup_checks, song_loader};
use phi_backend::state::AppState;
use phi_backend::{
    AppError, ShutdownManager, SystemdWatchdog, config::AppConfig, error::SaveProviderError,
};
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Semaphore;
use tower_http::compression::CompressionLayer;
use tower_http::services::ServeDir;
use utoipa::Modify;
use utoipa::OpenApi;
use utoipa::openapi::security::{ApiKey, ApiKeyValue, SecurityScheme};
use utoipa_swagger_ui::SwaggerUi;

fn compression_predicate() -> impl tower_http::compression::predicate::Predicate {
    use tower_http::compression::predicate::{NotForContentType, Predicate, SizeAbove};

    // 压缩策略：从“全局默认”改为“明确排除不该压缩的响应”。
    //
    // 主要考虑：
    // - SSE/流式响应：压缩可能引入缓冲，影响实时性。
    // - 图片/音视频等：本身已压缩或收益极低，反而浪费 CPU。
    // - application/octet-stream/zip/gzip 等：常见二进制下载类型，压缩收益不确定且有额外 CPU 开销。
    //
    // 仍保留默认的最小大小阈值（默认 32B），避免“压缩开销覆盖收益”。
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

#[cfg(test)]
mod compression_predicate_tests {
    use super::compression_predicate;
    use axum::body::Body;
    use axum::http::{Response as HttpResponse, header};
    use tower_http::compression::predicate::Predicate;

    fn should_compress_for(ct: &str) -> bool {
        // 命中 SizeAbove（默认 32B），避免因为 body 太小导致测试不稳定。
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

#[derive(OpenApi)]
#[openapi(
    paths(
        phi_backend::features::save::handler::get_save_data,
        phi_backend::features::auth::handler::get_qrcode,
        phi_backend::features::auth::handler::get_qrcode_status,
        phi_backend::features::auth::handler::post_user_id,
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
        phi_backend::features::rks::handler::post_rks_history,
        health_check,
    ),
    components(
        schemas(
            AppError,
            SaveProviderError,
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
            phi_backend::features::auth::handler::UserIdResponse,
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
            phi_backend::features::rks::handler::RksHistoryRequest,
            phi_backend::features::rks::handler::RksHistoryItem,
            phi_backend::features::rks::handler::RksHistoryResponse,
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
        (name = "RKS", description = "RKS APIs"),
        (name = "Health", description = "Health APIs"),
    ),
    info(
        title = "Phi Backend API",
        version = "0.1.0",
        description = "Phigros backend service (Axum)"
    )
)]
pub struct ApiDoc;

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

#[utoipa::path(
    get,
    path = "/health",
    summary = "健康检查",
    description = "用于探活的健康检查端点，返回服务状态与版本信息。",
    responses((status = 200, description = "服务健康", body = serde_json::Value) ),
    tag = "Health"
)]
async fn health_check() -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::OK,
        Json(json!({
            "status": "healthy",
            "service": "phi-backend",
            "version": env!("CARGO_PKG_VERSION")
        })),
    )
}

/// 为曲绘静态资源（`/_ill/*`）添加缓存头，降低 SVG 引用大量图片时的后端压力。
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

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "phi_backend=info,tower_http=info".into()),
        )
        .init();

    // 创建优���退出管理器
    let shutdown_manager = ShutdownManager::new();

    // Load config
    if let Err(e) = AppConfig::init_global() {
        tracing::error!("Config init failed: {}", e);
        std::process::exit(1);
    }
    let config = AppConfig::global();

    // 启动信号处理器
    if let Err(e) = shutdown_manager.start_signal_handler().await {
        tracing::error!("信号处理器启动失败: {}", e);
        std::process::exit(1);
    }

    // 创建并启动看门狗
    let watchdog = SystemdWatchdog::new(config.shutdown.watchdog.clone(), &shutdown_manager);
    if let Err(e) = watchdog.validate_config() {
        tracing::error!("看门狗配置验证失败: {}", e);
        std::process::exit(1);
    }

    // 通知systemd服务正在启动
    if let Err(e) = watchdog.notify_reloading() {
        tracing::warn!("发送reloading信号失败: {}", e);
    }
    if let Some(code) = config.watermark.current_dynamic_code() {
        tracing::info!(
            "watermark_unlock_code = {} (valid for ~{}s)",
            code,
            config.watermark.dynamic_ttl_secs
        );
    }
    // 周期性打印动态口令（仅当启用动态口令时）
    if config.watermark.unlock_dynamic {
        let wm = config.watermark.clone();
        tokio::spawn(async move {
            use tokio::time::{Duration, interval};
            let ttl = wm.dynamic_ttl_secs.max(1);
            let period = std::cmp::max(1, ttl / 4);
            let mut ticker = interval(Duration::from_secs(period));
            let mut last = String::new();
            loop {
                ticker.tick().await;
                if let Some(code) = wm.current_dynamic_code() {
                    if code != last {
                        last = code.clone();
                        tracing::info!(
                            "watermark_unlock_code = {} (valid for ~{}s)",
                            code,
                            wm.dynamic_ttl_secs
                        );
                    }
                } else {
                    // 未启用动态口令，停止任务
                    break;
                }
            }
        });
    }

    // Run startup checks
    if let Err(e) = run_startup_checks(config).await {
        tracing::error!("Startup checks failed: {}", e);
        std::process::exit(1);
    }

    // Load difficulty.csv
    let info_dir = config.info_path();
    let csv_path = info_dir.join("difficulty.csv");
    let chart_map: ChartConstantsMap = load_chart_constants(&csv_path).unwrap_or_else(|e| {
        tracing::error!("Failed to load difficulty.csv: {}", e);
        panic!("missing or invalid difficulty.csv");
    });

    // Load song catalog and nicknames
    let song_catalog = song_loader::load_song_catalog(&info_dir).unwrap_or_else(|e| {
        tracing::error!("Failed to load info.csv or nicklist.yaml: {}", e);
        panic!("missing or invalid info.csv/nicklist.yaml");
    });

    // Shared state
    let taptap_client = match TapTapClient::new(&config.taptap) {
        Ok(c) => Arc::new(c),
        Err(e) => {
            tracing::error!("TapTap client init failed: {}", e);
            std::process::exit(1);
        }
    };
    let qrcode_service =
        Arc::new(phi_backend::features::auth::qrcode_service::QrCodeService::new());

    // 初始化统计
    let (stats_handle_opt, stats_storage_opt) = if config.stats.enabled {
        match stats::init_stats(config).await {
            Ok((h, storage)) => (Some(h), Some(storage)),
            Err(e) => {
                tracing::warn!("统计初始化失败：{}（将继续运行）", e);
                (None, None)
            }
        }
    } else {
        (None, None)
    };

    // 初始化图片缓存（容量按总字节数加权）
    let bn_image_cache: Cache<String, Bytes> = {
        let img = &config.image;
        Cache::builder()
            .weigher(|_k, v: &Bytes| v.len() as u32)
            .max_capacity(img.cache_max_bytes)
            .time_to_live(Duration::from_secs(img.cache_ttl_secs))
            .time_to_idle(Duration::from_secs(img.cache_tti_secs))
            .build()
    };
    let song_image_cache: Cache<String, Bytes> = {
        let img = &config.image;
        Cache::builder()
            .weigher(|_k, v: &Bytes| v.len() as u32)
            .max_capacity(img.cache_max_bytes)
            .time_to_live(Duration::from_secs(img.cache_ttl_secs))
            .time_to_idle(Duration::from_secs(img.cache_tti_secs))
            .build()
    };

    let app_state = AppState {
        chart_constants: Arc::new(chart_map),
        song_catalog: Arc::new(song_catalog),
        taptap_client,
        qrcode_service,
        stats: stats_handle_opt.clone(),
        stats_storage: stats_storage_opt.clone(),
        render_semaphore: Arc::new(Semaphore::new({
            let m = config.image.max_parallel as usize;
            if m == 0 { num_cpus::get() } else { m }
        })),
        bn_image_cache,
        song_image_cache,
    };

    // Routes
    let mut api_router = Router::<AppState>::new()
        .nest("/auth", auth::create_auth_router())
        .merge(save::create_save_router())
        .merge(song::create_song_router())
        .merge(phi_backend::features::image::create_image_router())
        .merge(create_leaderboard_router())
        .merge(create_rks_router());
    api_router = api_router.merge(create_stats_router());

    let ill_root = config.illustration_path();
    let mut app = Router::<AppState>::new()
        .route("/health", get(health_check))
        // 给 SVG 渲染提供同源曲绘访问路径：只暴露实际用到的三个目录，避免误暴露 `.git` 等敏感文件。
        .nest_service("/_ill/ill", ServeDir::new(ill_root.join("ill")))
        .nest_service("/_ill/illLow", ServeDir::new(ill_root.join("illLow")))
        .nest_service("/_ill/illBlur", ServeDir::new(ill_root.join("illBlur")))
        .nest(&config.api.prefix, api_router)
        .merge(SwaggerUi::new("/docs").url("/api-docs/openapi.json", ApiDoc::openapi()))
        .with_state(app_state);
    // 为 /_ill 静态资源加缓存头（仅匹配 /_ill/* 路径）。
    app = app.layer(axum::middleware::from_fn(ill_cache_control_middleware));

    // 全局请求采集中间件
    if let Some(ref stats_handle) = stats_handle_opt {
        let s = StateWithStats {
            stats: stats_handle.clone(),
        };
        app = app.layer(axum::middleware::from_fn_with_state(s, stats_middleware));
    }

    // 应用内响应压缩：对 SVG/JSON/文本等内容启用 gzip/brotli，降低带宽占用（默认不会压缩 png/jpg/webp 等图片）。
    // 压缩策略：从“全局默认”改为“明确排除不该压缩的响应”。
    //
    // - SSE/流式响应：压缩容易引入缓冲，影响实时性
    // - 图片/音视频等：本身已压缩或压缩收益极低，反而浪费 CPU
    // - application/octet-stream/zip/gzip：通常是二进制下载，压缩收益不确定且可能造成额外 CPU
    app = app.layer(CompressionLayer::new().compress_when(compression_predicate()));

    // 启动看门狗任务
    if let Err(e) = watchdog.start_watchdog_task().await {
        tracing::warn!("看门狗任务启动失败: {}", e);
    }

    let addr = config.server_addr();
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .unwrap_or_else(|e| {
            tracing::error!("Bind address failed {}: {}", addr, e);
            std::process::exit(1);
        });

    tracing::info!("Server: http://{}", addr);
    tracing::info!("Docs: http://{}/docs", addr);
    tracing::info!("Health: http://{}/health", addr);
    tracing::info!("Save API: http://{}{}/save", addr, config.api.prefix);
    tracing::info!("Auth API: http://{}{}/auth", addr, config.api.prefix);
    tracing::info!("Illustrations: {:?}", config.illustration_path());

    // 通知systemd服务已准备就绪
    if let Err(e) = watchdog.notify_ready() {
        tracing::warn!("发送ready信号失败: {}", e);
    }

    // 启动服务器并等待优雅退出信号
    let shutdown_config = &config.shutdown;
    let shutdown_timeout = shutdown_config.timeout_duration();

    // 创建graceful shutdown future
    let stats_handle_for_cleanup = stats_handle_opt.clone();
    let watchdog_for_shutdown = watchdog.clone();
    let shutdown_signal = async move {
        let reason = shutdown_manager.wait_for_shutdown().await;
        tracing::info!("接收到退出信号: {:?}，开始优雅退出...", reason);

        // 通知systemd服务正在停止
        if let Err(e) = watchdog_for_shutdown.notify_stopping() {
            tracing::warn!("发送stopping信号失败: {}", e);
        }

        // 设置优雅退出超时
        match tokio::time::timeout(shutdown_timeout, async move {
            tracing::info!("优雅退出超时时间: {}秒", shutdown_config.timeout_secs);

            // 清理统计服务
            if let Some(stats_handle) = stats_handle_for_cleanup.clone() {
                tracing::info!("开始关闭统计服务...");
                if let Err(e) = stats_handle
                    .graceful_shutdown(std::time::Duration::from_secs(10))
                    .await
                {
                    tracing::warn!("统计服务关闭失败: {}", e);
                } else {
                    tracing::info!("统计服务已优雅关闭");
                }
            }

            // 等待一小段时间确保其他资源清理完成
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        })
        .await
        {
            Ok(_) => {
                tracing::info!("优雅退出完成");
            }
            Err(_) => {
                tracing::warn!("优雅退出超时，强制退出");
                if shutdown_config.force_quit {
                    tracing::info!("等待 {} 秒后强制退出", shutdown_config.force_delay_secs);
                    tokio::time::sleep(shutdown_config.force_delay_duration()).await;
                }
            }
        }
    };

    // 运行服务器直到收到退出信号
    let graceful = axum::serve(listener, app).with_graceful_shutdown(async {
        // 等待退出信号
        shutdown_signal.await;
        tracing::info!("开始优雅关闭HTTP服务器...");
    });

    if let Err(e) = graceful.await {
        tracing::error!("服务器运行错误: {}", e);
        std::process::exit(1);
    }

    tracing::info!("服务器已优雅关闭");
}
