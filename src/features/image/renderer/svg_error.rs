use crate::error::AppError;

/// 将 SVG 字符串写入错误转换为图片渲染错误，保持手写 SVG 路径的错误文案一致。
pub(super) fn svg_fmt_error(e: std::fmt::Error) -> AppError {
    AppError::ImageRendererError(format!("SVG formatting error: {e}"))
}
