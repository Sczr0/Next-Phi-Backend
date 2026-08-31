use crate::error::AppError;

/// 按目标宽度下采样后编码为 WebP（支持透明通道）
/// # 参数
/// * `svg_data` - SVG 字符串数据
/// * `is_user_generated` - 是否用户生成的内容（用于隐式水印）
/// * `target_width` - 目标宽度（可选，按宽度同比例缩放）
/// * `quality` - 有损压缩质量 1-100（默认 80，lossless 模式时无效）
/// * `lossless` - 是否使用无损模式（默认 false）
/// # 返回
/// WebP 格式的图片字节数据
pub(super) fn render_svg_to_webp(
    svg_data: &str,
    is_user_generated: bool,
    target_width: Option<u32>,
    quality: u8,
    lossless: bool,
) -> Result<Vec<u8>, AppError> {
    let surface =
        super::raster_surface::render_scaled_pixmap(svg_data, is_user_generated, target_width)?;

    // WebP 支持透明度通道，直接使用 RGBA 像素数据。
    //
    // 注意：image crate 当前仅支持“无损 WebP”（VP8L）。为了让 `quality/lossless` 参数真实生效，
    // 这里改用基于 libwebp 的 `webp` crate 进行编码。
    let rgba = surface.pixmap.data();

    let encoder = webp::Encoder::from_rgba(rgba, surface.width, surface.height);
    let memory = if lossless {
        encoder.encode_lossless()
    } else {
        encoder.encode(f32::from(quality.clamp(1, 100)))
    };
    Ok(memory.to_vec())
}
