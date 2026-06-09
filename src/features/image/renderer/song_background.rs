use std::path::{Path, PathBuf};

use rand::prelude::*;

use super::resources::{get_cover_files, get_cover_metadata_map, get_scaled_image_data_uri};
use super::urls::get_image_href;

pub(super) fn select_song_background(
    song_id: &str,
    embed_images: bool,
    public_illustration_base_url: Option<&str>,
    target_width: u32,
    target_height: u32,
) -> Option<String> {
    let metadata = get_cover_metadata_map();
    let preferred_cover_path = metadata.get(song_id).cloned().map(PathBuf::from);

    if let Some(path) = preferred_cover_path.as_ref()
        && let Some(image_href) = build_song_background_href(
            path,
            embed_images,
            public_illustration_base_url,
            target_width,
            target_height,
        )
    {
        tracing::info!("使用当前曲目曲绘作为背景: {}", path.display());
        return Some(image_href);
    }

    let cover_files = get_cover_files();
    if cover_files.is_empty() {
        tracing::warn!("找不到任何封面文件用于随机背景");
        return None;
    }

    let mut rng = rand::thread_rng();
    let Some(random_path) = cover_files.choose(&mut rng) else {
        tracing::warn!("无法从封面文件列表中随机选择一个");
        return None;
    };

    let Some(image_href) = build_song_background_href(
        random_path,
        embed_images,
        public_illustration_base_url,
        target_width,
        target_height,
    ) else {
        tracing::error!("获取背景图片失败: {}", random_path.display());
        return None;
    };

    tracing::info!("使用随机背景图: {}", random_path.display());
    Some(image_href)
}

fn build_song_background_href(
    path: &Path,
    embed_images: bool,
    public_illustration_base_url: Option<&str>,
    target_width: u32,
    target_height: u32,
) -> Option<String> {
    let image_href = get_image_href(path, embed_images, public_illustration_base_url)?;
    if embed_images && !image_href.starts_with("data:") {
        return get_scaled_image_data_uri(path, target_width, target_height).or(Some(image_href));
    }
    Some(image_href)
}
