#![allow(
    clippy::similar_names,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::too_many_lines,
    clippy::doc_markdown,
    clippy::struct_excessive_bools,
    clippy::items_after_statements,
    clippy::module_name_repetitions
)]

use axum::body::Bytes;
use moka::future::Cache;
use phi_backend::features::auth::client::TapTapClient;
use phi_backend::features::stats;
use phi_backend::router::build_app;
use phi_backend::startup::chart_loader::{ChartConstantsMap, load_chart_constants};
use phi_backend::startup::{run_startup_checks, song_loader};
use phi_backend::state::AppState;
use phi_backend::{ShutdownManager, SystemdWatchdog, config::AppConfig};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Semaphore;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "phi_backend=info,tower_http=info".into()),
        )
        .init();

    let shutdown_manager = ShutdownManager::new();

    if let Err(e) = AppConfig::init_global() {
        tracing::error!("Config init failed: {}", e);
        std::process::exit(1);
    }
    let config = AppConfig::global();

    if let Err(e) = shutdown_manager.start_signal_handler().await {
        tracing::error!("信号处理器启动失败: {}", e);
        std::process::exit(1);
    }

    let watchdog = SystemdWatchdog::new(config.shutdown.watchdog.clone(), &shutdown_manager);
    if let Err(e) = watchdog.validate_config() {
        tracing::error!("看门狗配置验证失败: {}", e);
        std::process::exit(1);
    }

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
                        last.clone_from(&code);
                        tracing::info!(
                            "watermark_unlock_code = {} (valid for ~{}s)",
                            code,
                            wm.dynamic_ttl_secs
                        );
                    }
                } else {
                    break;
                }
            }
        });
    }

    if let Err(e) = run_startup_checks(config).await {
        tracing::error!("Startup checks failed: {}", e);
        std::process::exit(1);
    }

    // 加载 difficulty.csv
    let info_dir = config.info_path();
    let csv_path = info_dir.join("difficulty.csv");
    let mut chart_map: ChartConstantsMap = load_chart_constants(&csv_path).unwrap_or_else(|e| {
        tracing::error!("Failed to load difficulty.csv: {}", e);
        panic!("missing or invalid difficulty.csv");
    });

    // 加载歌曲目录
    let mut song_catalog = song_loader::load_song_catalog(&info_dir).unwrap_or_else(|e| {
        tracing::error!("Failed to load info.csv or nicklist.yaml: {}", e);
        panic!("missing or invalid info.csv/nicklist.yaml");
    });

    // 尝试从远端加载更新的 info 文件
    if let Some(ref base_url) = config.resources.info_base_url {
        match phi_backend::startup::remote_info::try_load_remote_info(base_url, &info_dir).await {
            Ok(Some(remote)) => {
                tracing::info!("远端 info 版本更新，已切换至远端数据");
                chart_map = remote.chart_constants;
                song_catalog = remote.song_catalog;
            }
            Ok(None) => {
                tracing::info!("远端 info 无更新或不可达，继续使用本地数据");
            }
            Err(e) => {
                tracing::warn!("远端 info 加载出错，继续使用本地数据: {e}");
            }
        }
    }

    // 构建共享状态
    let taptap_client = match TapTapClient::new(&config.taptap) {
        Ok(c) => Arc::new(c),
        Err(e) => {
            tracing::error!("TapTap client init failed: {}", e);
            std::process::exit(1);
        }
    };
    let qrcode_service =
        Arc::new(phi_backend::features::auth::qrcode_service::QrCodeService::new());

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

    if config.open_platform.enabled {
        let op_storage = match phi_backend::features::open_platform::storage::OpenPlatformStorage::connect_sqlite(
            &config.open_platform.sqlite_path,
            config.open_platform.sqlite_wal,
        )
        .await
        {
            Ok(s) => s,
            Err(e) => {
                tracing::error!("开放平台存储初始化失败: {}", e);
                std::process::exit(1);
            }
        };
        if let Err(e) = op_storage.init_schema().await {
            tracing::error!("开放平台存储建表失败: {}", e);
            std::process::exit(1);
        }
        if let Err(e) =
            phi_backend::features::open_platform::storage::init_global(Arc::new(op_storage))
        {
            tracing::error!("开放平台存储注册失败: {}", e);
            std::process::exit(1);
        }
        if let Err(e) =
            phi_backend::features::open_platform::auth::init_global(&config.open_platform)
        {
            tracing::error!("开放平台鉴权服务初始化失败: {}", e);
            std::process::exit(1);
        }
    }

    let bn_image_cache: Cache<String, Bytes> = {
        let img = &config.image;
        Cache::builder()
            .weigher(|_k, v: &Bytes| u32::try_from(v.len()).unwrap_or(u32::MAX))
            .max_capacity(img.cache_max_bytes)
            .time_to_live(Duration::from_secs(img.cache_ttl_secs))
            .time_to_idle(Duration::from_secs(img.cache_tti_secs))
            .build()
    };
    let song_image_cache: Cache<String, Bytes> = {
        let img = &config.image;
        Cache::builder()
            .weigher(|_k, v: &Bytes| u32::try_from(v.len()).unwrap_or(u32::MAX))
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

    // 构建路由（含中间件）
    let app = build_app(app_state, config, stats_handle_opt.as_ref());

    if let Err(e) = watchdog.start_watchdog_task() {
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

    if let Err(e) = watchdog.notify_ready() {
        tracing::warn!("发送ready信号失败: {}", e);
    }

    // 优雅退出
    let shutdown_config = &config.shutdown;
    let shutdown_timeout = shutdown_config.timeout_duration();
    let stats_handle_for_cleanup = stats_handle_opt.clone();
    let watchdog_for_shutdown = watchdog.clone();
    let shutdown_signal = async move {
        let reason = shutdown_manager.wait_for_shutdown().await;
        tracing::info!("接收到退出信号: {:?}，开始优雅退出...", reason);

        if let Err(e) = watchdog_for_shutdown.notify_stopping() {
            tracing::warn!("发送stopping信号失败: {}", e);
        }

        if let Ok(()) = tokio::time::timeout(shutdown_timeout, async move {
            tracing::info!("优雅退出超时时间: {}秒", shutdown_config.timeout_secs);

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

            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        })
        .await
        {
            tracing::info!("优雅退出完成");
        } else {
            tracing::warn!("优雅退出超时，强制退出");
            if shutdown_config.force_quit {
                tracing::info!("等待 {} 秒后强制退出", shutdown_config.force_delay_secs);
                tokio::time::sleep(shutdown_config.force_delay_duration()).await;
            }
        }
    };

    let graceful = axum::serve(listener, app).with_graceful_shutdown(async {
        shutdown_signal.await;
        tracing::info!("开始优雅关闭HTTP服务器...");
    });

    if let Err(e) = graceful.await {
        tracing::error!("服务器运行错误: {}", e);
        std::process::exit(1);
    }

    tracing::info!("服务器已优雅关闭");
}
