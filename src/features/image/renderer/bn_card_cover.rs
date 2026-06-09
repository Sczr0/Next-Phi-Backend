use std::path::Path;

use super::math::round_non_negative_to_u32;
use super::resources::{get_cover_metadata_map, get_scaled_image_data_uri};
use super::urls::{build_remote_illustration_low_res_url, to_public_illustration_url};

pub(super) fn bn_cover_clip_id(is_ap_card: bool, index: usize) -> String {
    format!(
        "cover-clip-{}-{}",
        if is_ap_card { "ap" } else { "main" },
        index
    )
}

pub(super) fn resolve_card_cover_href(
    song_id: &str,
    embed_images: bool,
    public_illustration_base_url: Option<&str>,
    cover_width: f64,
    cover_height: f64,
) -> Option<String> {
    let metadata = get_cover_metadata_map();
    if let Some(path) = metadata.get(song_id) {
        let mut href = path.clone();
        if embed_images {
            let pb = Path::new(&href);
            let w = round_non_negative_to_u32(cover_width.max(1.0));
            let h = round_non_negative_to_u32(cover_height.max(1.0));
            href = get_scaled_image_data_uri(pb, w, h).unwrap_or(href);
        }
        if let Some(base) = public_illustration_base_url
            && !href.starts_with("data:")
        {
            let pb = Path::new(&href);
            href = to_public_illustration_url(pb, base).unwrap_or(href);
        }
        return Some(href);
    }

    public_illustration_base_url.map(|base| build_remote_illustration_low_res_url(base, song_id))
}
