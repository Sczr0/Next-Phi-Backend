use crate::config::AppConfig;
use crate::error::AppError;
use resvg::usvg::{self, Options as UsvgOptions};

use super::MAIN_FONT_NAME;
use super::resources::get_global_font_db;

/// 构造 SVG 解析选项，统一字体、语言与性能偏好。
pub(super) fn build_usvg_options() -> Result<UsvgOptions<'static>, AppError> {
    let font_db = get_global_font_db();
    let speed = AppConfig::global().image.optimize_speed;

    Ok(UsvgOptions {
        resources_dir: Some(std::env::current_dir().map_err(|e| {
            AppError::ImageRendererError(format!("Failed to get current dir: {e}"))
        })?),
        fontdb: font_db,
        font_family: MAIN_FONT_NAME.to_string(),
        font_size: 16.0,
        languages: vec!["zh-CN".to_string(), "en".to_string()],
        shape_rendering: if speed {
            usvg::ShapeRendering::OptimizeSpeed
        } else {
            usvg::ShapeRendering::GeometricPrecision
        },
        text_rendering: if speed {
            usvg::TextRendering::OptimizeSpeed
        } else {
            usvg::TextRendering::OptimizeLegibility
        },
        image_rendering: if speed {
            usvg::ImageRendering::OptimizeSpeed
        } else {
            usvg::ImageRendering::OptimizeQuality
        },
        ..Default::default()
    })
}
