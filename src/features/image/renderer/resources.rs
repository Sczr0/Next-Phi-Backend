use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use lru::LruCache;
use resvg::usvg::fontdb;

use super::super::cover_loader;

const COVER_METADATA_CACHE_SIZE: usize = 10000; // 预热容量（并非运行时 LRU）

/// 曲绘资源索引（启动时预热，运行时只读无锁）。
struct IllustrationIndex {
    /// 全部可用的曲绘/背景文件列表（ill/illLow/illBlur）。
    cover_files: Box<[PathBuf]>,
    /// 仅 illBlur 背景文件列表（用于随机背景）。
    blur_files: Box<[PathBuf]>,
    /// song_id -> 曲绘绝对路径（优先 illLow 覆盖 ill）。
    cover_metadata: HashMap<String, String>,
}

static ILLUSTRATION_INDEX: OnceLock<IllustrationIndex> = OnceLock::new();

#[derive(Clone, Copy)]
enum CoverMetadataMode {
    InsertIfMissing,
    Overwrite,
    Skip,
}

/// 将磁盘图片按给定尺寸进行等比裁剪填充（相当于 xMidYMid slice），再编码为 JPEG 并返回 Data URI。
/// 结果加入 LRU 缓存以避免重复解码与缩放。
pub(super) fn get_scaled_image_data_uri(
    path: &Path,
    target_w: u32,
    target_h: u32,
) -> Option<String> {
    super::resource_scaled::get_scaled_image_data_uri(path, target_w, target_h)
}

/// 获取全局字体数据库
pub(super) fn get_global_font_db() -> Arc<fontdb::Database> {
    super::resource_fonts::get_global_font_db()
}

fn is_supported_image_file(path: &Path) -> bool {
    path.is_file() && super::resource_image::LocalImageKind::from_path(path).is_some()
}

fn insert_cover_metadata(
    cover_metadata: &mut HashMap<String, String>,
    path: &Path,
    #[allow(clippy::trivially_copy_pass_by_ref)] mode: &CoverMetadataMode,
) {
    let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
        return;
    };

    match mode {
        CoverMetadataMode::InsertIfMissing => {
            // 先记录标准封面，后续 illLow 可按需覆盖
            cover_metadata
                .entry(stem.to_string())
                .or_insert_with(|| path.to_string_lossy().to_string());
        }
        CoverMetadataMode::Overwrite => {
            cover_metadata.insert(stem.to_string(), path.to_string_lossy().to_string());
        }
        CoverMetadataMode::Skip => {}
    }
}

fn scan_image_dir(
    dir: &Path,
    cover_files: &mut Vec<PathBuf>,
    blur_files: &mut Vec<PathBuf>,
    cover_metadata: &mut HashMap<String, String>,
    metadata_mode: CoverMetadataMode,
) -> std::io::Result<()> {
    for entry in fs::read_dir(dir)?.flatten() {
        let path = entry.path();
        if !is_supported_image_file(&path) {
            continue;
        }

        insert_cover_metadata(cover_metadata, &path, &metadata_mode);
        if matches!(metadata_mode, CoverMetadataMode::Skip) {
            blur_files.push(path.clone());
        }
        cover_files.push(path);
    }
    Ok(())
}

/// 初始化曲绘资源索引（cover_files / blur_files / metadata_map）。
fn init_illustration_index() -> IllustrationIndex {
    tracing::info!("初始化曲绘资源索引（cover_files/blur_files/metadata_map）");

    // 封面元数据：song_id -> 封面绝对路径
    let mut cover_metadata = HashMap::<String, String>::with_capacity(COVER_METADATA_CACHE_SIZE);

    // 读取封面目录下的所有图片文件（包括 ill / illLow / illBlur 目录）
    let mut cover_files = Vec::new();
    let mut blur_files = Vec::new();

    // 读取 ill 目录（标准封面）
    let cover_base_path = cover_loader::covers_dir().join("ill");
    if let Err(e) = scan_image_dir(
        &cover_base_path,
        &mut cover_files,
        &mut blur_files,
        &mut cover_metadata,
        CoverMetadataMode::InsertIfMissing,
    ) {
        tracing::error!("读取封面目录失败 '{}': {}", cover_base_path.display(), e);
    }

    // 读取 illLow 目录（低分辨率封面，优先级更高：覆盖 ill）
    let cover_low_base_path = cover_loader::covers_dir().join("illLow");
    if let Err(e) = scan_image_dir(
        &cover_low_base_path,
        &mut cover_files,
        &mut blur_files,
        &mut cover_metadata,
        CoverMetadataMode::Overwrite,
    ) {
        tracing::warn!(
            "读取低分辨率封面目录失败 '{}': {}",
            cover_low_base_path.display(),
            e
        );
    }

    // 读取 illBlur 目录（背景图片，只参与随机背景，不写入 cover_metadata）
    let background_base_path = cover_loader::covers_dir().join("illBlur");
    if let Err(e) = scan_image_dir(
        &background_base_path,
        &mut cover_files,
        &mut blur_files,
        &mut cover_metadata,
        CoverMetadataMode::Skip,
    ) {
        tracing::error!(
            "读取背景目录失败 '{}': {}",
            background_base_path.display(),
            e
        );
    }

    tracing::info!(
        "曲绘目录扫描完成: cover_files={}, blur_files={}, metadata={}",
        cover_files.len(),
        blur_files.len(),
        cover_metadata.len()
    );

    IllustrationIndex {
        cover_files: cover_files.into_boxed_slice(),
        blur_files: blur_files.into_boxed_slice(),
        cover_metadata,
    }
}

fn get_illustration_index() -> &'static IllustrationIndex {
    ILLUSTRATION_INDEX.get_or_init(init_illustration_index)
}

/// 获取背景图片缓存
#[allow(dead_code)]
pub(super) fn get_background_cache() -> &'static std::sync::Mutex<LruCache<PathBuf, Arc<str>>> {
    let _ = get_illustration_index();
    super::resource_background::get_background_cache()
}

/// 获取封面文件列表
pub(super) fn get_cover_files() -> &'static [PathBuf] {
    get_illustration_index().cover_files.as_ref()
}

/// 获取封面元数据（只读，无锁）
pub(super) fn get_cover_metadata_map() -> &'static HashMap<String, String> {
    &get_illustration_index().cover_metadata
}

/// 获取 illBlur 背景文件列表（预索引，避免每次渲染 O(n) 扫描）。
pub(super) fn get_blur_background_files() -> &'static [PathBuf] {
    get_illustration_index().blur_files.as_ref()
}

/// 从缓存或磁盘加载背景图片
/// 注意：现在只缓存小图（<256KB），大图直接返回路径
pub(super) fn get_background_image(path: &Path) -> Option<String> {
    let _ = get_illustration_index();
    super::resource_background::get_background_image(path)
}

/// 带缓存的反色计算，避免重复解码大图
pub(super) fn get_inverse_color_from_path_cached(path: &Path) -> Option<String> {
    super::resource_color::get_inverse_color_from_path_cached(path)
}
