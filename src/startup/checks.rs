use crate::config::AppConfig;
use crate::error::AppError;
use git2::{FetchOptions, Progress, RemoteCallbacks, Repository};
use std::fs;
use std::path::Path;

/// 执行启动检查
///
/// 1. 检查并创建 resources 文件夹
/// 2. 检查并克隆 Phigros 曲绘仓库
pub async fn run_startup_checks(config: &AppConfig) -> Result<(), AppError> {
    tracing::info!("🔍 开始执行启动检查...");

    // 检查并创建 resources 文件夹
    ensure_resources_folder(config)?;

    // 检查并克隆曲绘仓库
    ensure_illustration_repo(config)?;

    // 检查字体资源（仅告警，不阻断启动）
    ensure_font_resources()?;

    // 预热曲绘索引（目录扫描 + 白主题背景反色预计算），降低首个 SVG 请求的长尾延迟。
    let t_prewarm = std::time::Instant::now();
    if let Err(e) =
        tokio::task::spawn_blocking(crate::features::image::prewarm_illustration_assets).await
    {
        tracing::warn!("曲绘索引预热任务失败: {}", e);
    } else {
        tracing::info!("曲绘索引预热完成: {}ms", t_prewarm.elapsed().as_millis());
    }

    tracing::info!("✅ 启动检查完成");
    Ok(())
}

/// 确保 resources 文件夹存在
fn ensure_resources_folder(config: &AppConfig) -> Result<(), AppError> {
    let resources_path = config.resources_path();

    if !resources_path.exists() {
        tracing::warn!("📁 未找到 resources 文件夹，正在创建: {:?}", resources_path);
        fs::create_dir_all(&resources_path)
            .map_err(|e| AppError::Internal(format!("创建 resources 文件夹失败: {e}")))?;
        tracing::info!("✅ resources 文件夹创建成功");
    } else {
        tracing::info!("✅ resources 文件夹已存在");
    }

    Ok(())
}

/// 确保曲绘仓库存在
fn ensure_illustration_repo(config: &AppConfig) -> Result<(), AppError> {
    let illustration_path = config.illustration_path();

    if illustration_path.exists() {
        tracing::info!("✅ Phigros 曲绘仓库已存在: {:?}", illustration_path);

        // 尝试更新仓库
        if let Err(e) = update_repository(&illustration_path) {
            tracing::warn!("⚠️ 更新曲绘仓库失败: {}", e);
            tracing::info!("💡 将继续使用现有仓库");
        } else {
            tracing::info!("✅ 曲绘仓库更新成功");
        }
    } else {
        tracing::info!("📦 正在克隆 Phigros 曲绘仓库...");
        tracing::info!("📍 仓库地址: {}", config.resources.illustration_repo);
        tracing::info!("📂 目标路径: {:?}", illustration_path);

        clone_repository(&config.resources.illustration_repo, &illustration_path)?;

        tracing::info!("✅ Phigros 曲绘仓库克隆成功");
    }

    Ok(())
}

/// 克隆 Git 仓库
fn clone_repository(url: &str, path: &Path) -> Result<(), AppError> {
    // 创建进度回调
    let mut callbacks = RemoteCallbacks::new();
    let mut last_progress = 0;

    callbacks.transfer_progress(|progress: Progress| {
        let current = progress.received_objects();
        let total = progress.total_objects();
        let percentage = if total > 0 {
            (current as f64 / total as f64 * 100.0) as u32
        } else {
            0
        };

        // 每 10% 打印一次进度
        if percentage >= last_progress + 10 {
            tracing::info!("⏬ 克隆进度: {}% ({}/{})", percentage, current, total);
            last_progress = percentage;
        }

        true
    });

    let mut fetch_options = FetchOptions::new();
    fetch_options.remote_callbacks(callbacks);

    let mut builder = git2::build::RepoBuilder::new();
    builder.fetch_options(fetch_options);

    builder
        .clone(url, path)
        .map_err(|e| AppError::Internal(format!("克隆 Git 仓库失败: {e}")))?;

    Ok(())
}

/// 更新 Git 仓库
fn update_repository(path: &Path) -> Result<(), AppError> {
    let repo = Repository::open(path)
        .map_err(|e| AppError::Internal(format!("打开 Git 仓库失败: {e}")))?;

    // 获取 origin remote
    let mut remote = repo
        .find_remote("origin")
        .map_err(|e| AppError::Internal(format!("查找 remote 失败: {e}")))?;

    // 创建进度回调
    let mut callbacks = RemoteCallbacks::new();
    callbacks.transfer_progress(|progress: Progress| {
        let current = progress.received_objects();
        let total = progress.total_objects();
        if total > 0 {
            let percentage = (current as f64 / total as f64 * 100.0) as u32;
            if percentage > 0 && percentage.is_multiple_of(20) {
                tracing::debug!("⏫ 更新进度: {}%", percentage);
            }
        }
        true
    });

    let mut fetch_options = FetchOptions::new();
    fetch_options.remote_callbacks(callbacks);

    // 执行 fetch
    remote
        .fetch(&["main", "master"], Some(&mut fetch_options), None)
        .map_err(|e| AppError::Internal(format!("Fetch 失败: {e}")))?;

    // 尝试快速前进合并
    let fetch_head = repo
        .find_reference("FETCH_HEAD")
        .map_err(|e| AppError::Internal(format!("查找 FETCH_HEAD 失败: {e}")))?;

    let fetch_commit = repo
        .reference_to_annotated_commit(&fetch_head)
        .map_err(|e| AppError::Internal(format!("获取 commit 失败: {e}")))?;

    let analysis = repo
        .merge_analysis(&[&fetch_commit])
        .map_err(|e| AppError::Internal(format!("合并分析失败: {e}")))?;

    if analysis.0.is_up_to_date() {
        tracing::info!("✅ 仓库已是最新");
    } else if analysis.0.is_fast_forward() {
        tracing::info!("🔁 正在快速前进更新...");
        // 快速前进合并逻辑
        let refname = "refs/heads/main";
        match repo.find_reference(refname) {
            Ok(mut r) => {
                r.set_target(fetch_commit.id(), "Fast-Forward")
                    .map_err(|e| AppError::Internal(format!("设置 target 失败: {e}")))?;
                repo.set_head(refname)
                    .map_err(|e| AppError::Internal(format!("设置 HEAD 失败: {e}")))?;
                repo.checkout_head(Some(git2::build::CheckoutBuilder::default().force()))
                    .map_err(|e| AppError::Internal(format!("Checkout 失败: {e}")))?;
            }
            Err(_) => {
                // 如果 main 不存在，尝试 master
                let refname = "refs/heads/master";
                if let Ok(mut r) = repo.find_reference(refname) {
                    r.set_target(fetch_commit.id(), "Fast-Forward")
                        .map_err(|e| AppError::Internal(format!("设置 target 失败: {e}")))?;
                    repo.set_head(refname)
                        .map_err(|e| AppError::Internal(format!("设置 HEAD 失败: {e}")))?;
                    repo.checkout_head(Some(git2::build::CheckoutBuilder::default().force()))
                        .map_err(|e| AppError::Internal(format!("Checkout 失败: {e}")))?;
                }
            }
        }
    }

    Ok(())
}

/// 确保字体文件存在（必要时仅告警）
fn ensure_font_resources() -> Result<(), AppError> {
    use std::path::PathBuf;
    let font_dir = PathBuf::from("resources/fonts");
    let required_font = "Source Han Sans & Saira Hybrid-Regular #5446.ttf";
    if !font_dir.join(required_font).exists() {
        tracing::warn!("未找到必需字体文件: {}", required_font);
    } else {
        tracing::info!("字体存在: {}", required_font);
    }
    Ok(())
}
