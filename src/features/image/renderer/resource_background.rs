use std::fs;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use base64::{Engine as _, engine::general_purpose::STANDARD as base64_engine};
use lru::LruCache;

const BACKGROUND_CACHE_SIZE: usize = 10; // 缓存10张背景图片

static BACKGROUND_CACHE: OnceLock<std::sync::Mutex<LruCache<PathBuf, Arc<str>>>> = OnceLock::new();

/// 获取背景图片缓存
pub(super) fn get_background_cache() -> &'static std::sync::Mutex<LruCache<PathBuf, Arc<str>>> {
    BACKGROUND_CACHE.get_or_init(|| {
        std::sync::Mutex::new(LruCache::new(
            NonZeroUsize::new(BACKGROUND_CACHE_SIZE).unwrap(),
        ))
    })
}

/// 从缓存或磁盘加载背景图片
/// 注意：现在只缓存小图（<256KB），大图直接返回路径
pub(super) fn get_background_image(path: &Path) -> Option<String> {
    let key = path.to_path_buf();

    // 先在锁内做一次快路径读取；缓存值用 Arc，避免在全局锁内复制大段 Data URI。
    let cached_image = if let Ok(mut cache) = get_background_cache().lock() {
        cache.get(&key).cloned()
    } else {
        None
    };
    if let Some(cached_image) = cached_image {
        return Some(cached_image.as_ref().to_owned());
    }

    // 缓存未命中：在锁外做磁盘 IO 与编码，避免长尾延迟放大。
    let data = fs::read(path).ok()?;
    let file_size = data.len();

    // 大图片直接返回文件路径（避免内存膨胀与 base64 成本）。
    if file_size > 256 * 1024 {
        return Some(path.to_string_lossy().into_owned());
    }

    let mime_type = super::resource_image::LocalImageKind::from_path(path)
        .map_or("image/jpeg", |kind| kind.mime_type());
    let base64_encoded = base64_engine.encode(&data);
    let image_data = format!("data:{mime_type};base64,{base64_encoded}");

    // 回写缓存：二次检查，避免并发下重复写入/抖动。
    let cached_image = if let Ok(mut cache) = get_background_cache().lock() {
        let cached_image = cache.get(&key).cloned();
        if cached_image.is_none() {
            cache.put(key, Arc::<str>::from(image_data.as_str()));
        }
        cached_image
    } else {
        None
    };
    if let Some(cached_image) = cached_image {
        return Some(cached_image.as_ref().to_owned());
    }

    Some(image_data)
}
