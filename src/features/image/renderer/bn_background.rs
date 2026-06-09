use rand::prelude::*;

use crate::features::image::Theme;

use super::resources::{
    get_blur_background_files, get_inverse_color_from_path_cached, get_scaled_image_data_uri,
};
use super::urls::get_image_href;

pub(super) struct BnBackgroundSelection {
    pub(super) image_href: Option<String>,
    pub(super) normal_card_stroke_color: String,
}

pub(super) fn select_random_background(
    theme: &Theme,
    embed_images: bool,
    public_illustration_base_url: Option<&str>,
    width: u32,
    total_height: u32,
    normal_card_stroke_color: String,
) -> BnBackgroundSelection {
    let mut selection = BnBackgroundSelection {
        image_href: None,
        normal_card_stroke_color,
    };

    let filtered_background_files = get_blur_background_files();
    if filtered_background_files.is_empty() {
        tracing::warn!("找不到任何背景文件用于随机背景");
        return selection;
    }

    let mut rng = rand::thread_rng();
    let Some(random_path) = filtered_background_files.choose(&mut rng) else {
        tracing::warn!("无法从背景文件列表中随机选择一个");
        return selection;
    };

    if let Theme::White = theme
        && let Some(inverse_color) = get_inverse_color_from_path_cached(random_path)
    {
        selection.normal_card_stroke_color = inverse_color;
        tracing::info!(
            "使用背景反色作为卡片边框: {}",
            selection.normal_card_stroke_color
        );
    }

    let Some(image_href) = get_image_href(random_path, embed_images, public_illustration_base_url)
    else {
        tracing::error!("获取背景图片失败: {}", random_path.display());
        return selection;
    };

    selection.image_href = Some(image_href);
    if embed_images
        && let Some(ref href_str) = selection.image_href
        && !href_str.starts_with("data:")
        && let Some(uri) = get_scaled_image_data_uri(random_path, width, total_height)
    {
        selection.image_href = Some(uri);
    }
    tracing::info!("使用随机背景图: {}", random_path.display());

    selection
}
