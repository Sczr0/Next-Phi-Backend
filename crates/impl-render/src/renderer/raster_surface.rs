use crate::error::AppError;
use resvg::usvg;
use resvg::{
    render,
    tiny_skia::{Pixmap, Transform},
};

use super::math::scaled_dimensions;

pub(super) struct RasterSurface {
    pub(super) pixmap: Pixmap,
    pub(super) width: u32,
    pub(super) height: u32,
}

/// 将 SVG 解析并按目标宽度栅格化，供不同图片编码器复用。
pub(super) fn render_scaled_pixmap(
    svg_data: &str,
    is_user_generated: bool,
    target_width: Option<u32>,
) -> Result<RasterSurface, AppError> {
    let opts = super::raster_options::build_usvg_options()?;
    let tree = usvg::Tree::from_data(svg_data.as_bytes(), &opts)
        .map_err(|e| AppError::ImageRendererError(format!("Failed to parse SVG: {e}")))?;

    let src_size = tree.size().to_int_size();
    let (width, height, scale) =
        scaled_dimensions(src_size.width(), src_size.height(), target_width);

    let mut pixmap = Pixmap::new(width, height)
        .ok_or_else(|| AppError::ImageRendererError("Failed to create pixmap".to_string()))?;
    render(
        &tree,
        Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );

    // 用户数据添加隐式水印：直接修改未编码像素，避免解/编码开销。
    if is_user_generated && let Some(px) = pixmap.data_mut().get_mut(0..4) {
        px.copy_from_slice(&[0x01, 0x02, 0x03, 0xFF]);
    }

    Ok(RasterSurface {
        pixmap,
        width,
        height,
    })
}
