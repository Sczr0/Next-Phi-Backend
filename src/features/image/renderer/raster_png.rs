use crate::config::AppConfig;
use crate::error::AppError;

use super::math::usize_from_u32;

/// 按目标宽度下采样后编码为 PNG（未提供则使用 SVG 原始宽度）
pub(super) fn render_svg_to_png_scaled(
    svg_data: &str,
    is_user_generated: bool,
    target_width: Option<u32>,
) -> Result<Vec<u8>, AppError> {
    let surface =
        super::raster_surface::render_scaled_pixmap(svg_data, is_user_generated, target_width)?;

    // 编码 PNG
    let out_cap = usize_from_u32(surface.width)
        .saturating_mul(usize_from_u32(surface.height))
        .saturating_mul(4);
    let mut out = Vec::with_capacity(out_cap);
    {
        let mut encoder = png::Encoder::new(&mut out, surface.width, surface.height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        if AppConfig::global().image.optimize_speed {
            encoder.set_compression(png::Compression::Fast);
            encoder.set_filter(png::FilterType::NoFilter);
        } else {
            encoder.set_compression(png::Compression::Default);
            encoder.set_filter(png::FilterType::Paeth);
        }
        let mut writer = encoder
            .write_header()
            .map_err(|e| AppError::ImageRendererError(format!("PNG write_header error: {e}")))?;
        writer
            .write_image_data(surface.pixmap.data())
            .map_err(|e| {
                AppError::ImageRendererError(format!("PNG write_image_data error: {e}"))
            })?;
        writer
            .finish()
            .map_err(|e| AppError::ImageRendererError(format!("PNG finish error: {e}")))?;
    }
    Ok(out)
}
