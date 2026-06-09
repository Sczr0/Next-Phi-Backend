use crate::error::AppError;
use tokio::task::spawn_blocking;

/// 统一的图片编码入口：根据 `format` 选择编码器，并返回字节与 Content-Type。
///
/// 参数：
/// - format: "png" | "jpeg" | "jpg" | "webp"（大小写不敏感）
/// - is_user_generated: 是否用户生成（用于隐式水印）
/// - width: 目标宽度（可选）
/// - webp_quality: WebP 质量（1-100，缺省 80）
/// - webp_lossless: WebP 无损（缺省 false）
pub(super) fn render_svg_unified(
    svg: &str,
    is_user_generated: bool,
    format: Option<&str>,
    width: Option<u32>,
    webp_quality: Option<u8>,
    webp_lossless: Option<bool>,
) -> Result<(Vec<u8>, &'static str), AppError> {
    let fmt = format.unwrap_or("png").to_ascii_lowercase();
    match fmt.as_str() {
        "jpeg" | "jpg" => {
            let bytes = super::raster_jpeg::render_svg_to_jpeg(svg, is_user_generated, width, 85)?;
            Ok((bytes, "image/jpeg"))
        }
        "webp" => {
            let q = webp_quality.unwrap_or(80).clamp(1, 100);
            let lossless = webp_lossless.unwrap_or(false);
            let bytes =
                super::raster_webp::render_svg_to_webp(svg, is_user_generated, width, q, lossless)?;
            Ok((bytes, "image/webp"))
        }
        _ => {
            let bytes = super::raster_png::render_svg_to_png_scaled(svg, is_user_generated, width)?;
            Ok((bytes, "image/png"))
        }
    }
}

/// 异步版本的统一图片编码入口
///
/// 将整个 SVG 解析、栅格化与编码流程放入 Tokio 的阻塞线程池中，避免阻塞异步运行时线程。
pub(super) async fn render_svg_unified_async(
    svg: String,
    is_user_generated: bool,
    format: Option<&str>,
    width: Option<u32>,
    webp_quality: Option<u8>,
    webp_lossless: Option<bool>,
) -> Result<(Vec<u8>, &'static str), AppError> {
    let format_owned = format.map(std::string::ToString::to_string);
    let handle = spawn_blocking(move || {
        render_svg_unified(
            &svg,
            is_user_generated,
            format_owned.as_deref(),
            width,
            webp_quality,
            webp_lossless,
        )
    });

    handle
        .await
        .map_err(|e| AppError::Internal(format!("阻塞渲染任务执行失败: {e}")))?
}
