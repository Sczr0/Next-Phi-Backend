use super::SongRenderData;
use super::math::round_non_negative_to_u32;
use super::resources::get_scaled_image_data_uri;
use super::urls::{build_remote_illustration_url, get_image_href};

pub(super) fn resolve_song_illustration_href(
    data: &SongRenderData,
    embed_images: bool,
    public_illustration_base_url: Option<&str>,
    target_width: f64,
    target_height: f64,
) -> Option<String> {
    if let Some(path) = data.illustration_path.as_ref()
        && let Some(href) = get_image_href(path, embed_images, public_illustration_base_url)
    {
        if embed_images && !href.starts_with("data:") {
            let target_w = round_non_negative_to_u32(target_width.max(1.0));
            let target_h = round_non_negative_to_u32(target_height.max(1.0));
            return get_scaled_image_data_uri(path, target_w, target_h).or(Some(href));
        }
        return Some(href);
    }

    public_illustration_base_url.map(|base| build_remote_illustration_url(base, &data.song_id))
}
