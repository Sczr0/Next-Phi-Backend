use crate::error::AppError;
use image::ColorType;
use image::codecs::jpeg::JpegEncoder;

use super::math::usize_from_u32;

/// 按目标宽度下采样后编码为 JPEG（quality 1-100，建议 80-90）
pub(super) fn render_svg_to_jpeg(
    svg_data: &str,
    is_user_generated: bool,
    target_width: Option<u32>,
    quality: u8,
) -> Result<Vec<u8>, AppError> {
    let surface =
        super::raster_surface::render_scaled_pixmap(svg_data, is_user_generated, target_width)?;

    // 将 RGBA 像素扁平化到黑色背景（JPEG 无透明通道）
    let rgba = surface.pixmap.data();
    let rgb_cap = usize_from_u32(surface.width)
        .saturating_mul(usize_from_u32(surface.height))
        .saturating_mul(3);
    let mut rgb: Vec<u8> = Vec::with_capacity(rgb_cap);
    let mut rgba_index = 0;
    while rgba_index + 3 < rgba.len() {
        let red = u16::from(rgba[rgba_index]);
        let green = u16::from(rgba[rgba_index + 1]);
        let blue = u16::from(rgba[rgba_index + 2]);
        let alpha = u16::from(rgba[rgba_index + 3]); // 0..255
        // 过黑底合成：c' = c * a/255
        let red_out = u8::try_from((red * alpha) / 255).unwrap_or(u8::MAX);
        let green_out = u8::try_from((green * alpha) / 255).unwrap_or(u8::MAX);
        let blue_out = u8::try_from((blue * alpha) / 255).unwrap_or(u8::MAX);
        rgb.push(red_out);
        rgb.push(green_out);
        rgb.push(blue_out);
        rgba_index += 4;
    }

    let mut out = Vec::new();
    let mut enc = JpegEncoder::new_with_quality(&mut out, quality.clamp(1, 100));
    enc.encode(&rgb, surface.width, surface.height, ColorType::Rgb8.into())
        .map_err(|e| AppError::ImageRendererError(format!("JPEG encode error: {e}")))?;
    Ok(out)
}
