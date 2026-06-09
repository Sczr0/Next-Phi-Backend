use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use lru::LruCache;

// 反色计算兜底 LRU：只在预热未覆盖的路径上触发（极少数场景）。
static INVERSE_COLOR_DYNAMIC_CACHE: OnceLock<std::sync::Mutex<LruCache<PathBuf, String>>> =
    OnceLock::new();

fn get_inverse_color_dynamic_cache() -> &'static std::sync::Mutex<LruCache<PathBuf, String>> {
    INVERSE_COLOR_DYNAMIC_CACHE
        .get_or_init(|| std::sync::Mutex::new(LruCache::new(NonZeroUsize::new(256).unwrap())))
}

/// 从图片路径计算主色的反色
/// 优化：使用缩略图（100x100）计算颜色，而不是全尺寸图片
fn calculate_inverse_color_from_path(path: &Path) -> Option<String> {
    // 使用 image crate 打开图片
    let img = image::open(path).ok()?;

    // 缩小到 100x100 计算颜色，大幅减少内存使用和计算时间
    let thumbnail = img.thumbnail(100, 100);
    let pixels = thumbnail.to_rgba8().into_raw();

    if pixels.is_empty() {
        return None;
    }

    let mut total_r: u64 = 0;
    let mut total_g: u64 = 0;
    let mut total_b: u64 = 0;

    // 像素数据是扁平的 [R, G, B, A, R, G, B, A, ...] 数组
    for chunk in pixels.chunks_exact(4) {
        total_r += u64::from(chunk[0]);
        total_g += u64::from(chunk[1]);
        total_b += u64::from(chunk[2]);
    }

    let num_pixels = u64::try_from(pixels.len() / 4).unwrap_or(u64::MAX);
    if num_pixels == 0 {
        return None;
    }

    let avg_r = u8::try_from(total_r / num_pixels).unwrap_or(u8::MAX);
    let avg_g = u8::try_from(total_g / num_pixels).unwrap_or(u8::MAX);
    let avg_b = u8::try_from(total_b / num_pixels).unwrap_or(u8::MAX);

    // 计算反色
    let inv_r = 255 - avg_r;
    let inv_g = 255 - avg_g;
    let inv_b = 255 - avg_b;

    Some(format!("#{inv_r:02X}{inv_g:02X}{inv_b:02X}"))
}

/// 带缓存的反色计算，避免重复解码大图
pub(super) fn get_inverse_color_from_path_cached(path: &Path) -> Option<String> {
    let key = PathBuf::from(path);
    {
        let mut cache = get_inverse_color_dynamic_cache().lock().ok()?;
        if let Some(c) = cache.get(&key) {
            return Some(c.clone());
        }
    }

    let color = calculate_inverse_color_from_path(path)?;
    if let Ok(mut cache) = get_inverse_color_dynamic_cache().lock() {
        cache.put(key, color.clone());
    }
    Some(color)
}
