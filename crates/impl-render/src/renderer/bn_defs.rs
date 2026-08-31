use std::fmt::Write;

use crate::error::AppError;
use crate::features::image::Theme;

use super::background_layer::{
    BACKGROUND_FALLBACK_GRADIENT, BACKGROUND_OVERLAY_DARK, BACKGROUND_OVERLAY_WHITE,
    SvgBackgroundLayerRenderContext, write_svg_background_layer,
};
use super::svg_error::svg_fmt_error;
use super::{MAIN_FONT_NAME, bn_theme::BnThemePalette};

pub(super) struct BnDefsRenderContext<'a> {
    pub(super) svg: &'a mut String,
    pub(super) theme: &'a Theme,
    pub(super) palette: &'a BnThemePalette,
    pub(super) normal_card_stroke_color: &'a str,
    pub(super) started_at: &'a std::time::Instant,
}

pub(super) struct BnDefsTiming {
    pub(super) style_elapsed: std::time::Duration,
    pub(super) defs_elapsed: std::time::Duration,
}

pub(super) struct BnBackgroundLayerRenderContext<'a> {
    pub(super) svg: &'a mut String,
    pub(super) theme: &'a Theme,
    pub(super) background_image_href: Option<String>,
}

pub(super) fn write_svg_open(
    svg: &mut String,
    width: u32,
    total_height: u32,
) -> Result<(), AppError> {
    writeln!(
        svg,
        r#"<svg width="{width}" height="{total_height}" viewBox="0 0 {width} {total_height}" xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink">"#
    )
    .map_err(svg_fmt_error)
}

pub(super) fn write_defs(ctx: BnDefsRenderContext<'_>) -> Result<BnDefsTiming, AppError> {
    let BnDefsRenderContext {
        svg,
        theme,
        palette,
        normal_card_stroke_color,
        started_at,
    } = ctx;

    writeln!(svg, "<defs>").map_err(svg_fmt_error)?;

    // 背景渐变作为图片背景不可用时的兜底。
    match theme {
        Theme::White => {
            writeln!(svg, r#"<linearGradient id="bg-gradient" x1="0%" y1="0%" x2="100%" y2="100%"><stop offset="0%" style="stop-color:#F7FAFF" /><stop offset="100%" style="stop-color:#ECEFF4" /></linearGradient>"#).map_err(svg_fmt_error)?;
        }
        Theme::Black => {
            writeln!(svg, r#"<linearGradient id="bg-gradient" x1="0%" y1="0%" x2="100%" y2="100%"><stop offset="0%" style="stop-color:#141826" /><stop offset="100%" style="stop-color:#252E48" /></linearGradient>"#).map_err(svg_fmt_error)?;
        }
    }

    writeln!(svg, r#"<filter id="card-shadow" x="-10%" y="-10%" width="120%" height="130%"><feDropShadow dx="0" dy="3" stdDeviation="3" flood-color="rgba(0,0,0,0.25)" flood-opacity="0.25" /></filter>"#).map_err(svg_fmt_error)?;
    writeln!(svg, r#"<filter id="fc-glow" x="-50%" y="-50%" width="200%" height="200%"><feDropShadow dx="0" dy="0" stdDeviation="4" flood-color="{}" flood-opacity="0.8" /></filter>"#, palette.fc_stroke_color).map_err(svg_fmt_error)?;
    writeln!(svg, r#"<filter id="ap-glow" x="-50%" y="-50%" width="200%" height="200%"><feDropShadow dx="0" dy="0" stdDeviation="4" flood-color="{}" flood-opacity="0.8" /></filter>"#, palette.fc_stroke_color).map_err(svg_fmt_error)?;

    writeln!(svg, r#"<filter id="bg-blur">"#).map_err(svg_fmt_error)?;
    // 调整 stdDeviation 控制模糊程度，10 是一个比较强的模糊效果。
    writeln!(svg, r#"<feGaussianBlur stdDeviation="10" />"#).map_err(svg_fmt_error)?;
    writeln!(svg, r"</filter>").map_err(svg_fmt_error)?;

    writeln!(svg, "<style>").map_err(svg_fmt_error)?;
    write!(
        svg,
        r#"
        /* <![CDATA[ */
        svg {{ background-color: {}; /* Fallback background color */ }}
        .card {{
            fill: {};
            stroke: {};
            stroke-width: 1.5;
            filter: url(#card-shadow);
            transition: all 0.3s ease;
        }}
        .card-ap {{
          fill: {};
          stroke: {};
          stroke-width: 2.5;
          filter: url(#ap-glow);
        }}
        .card-fc {{
          fill: {};
          stroke: {}; /* Light Sky Blue */
          stroke-width: 2.5;
          filter: url(#fc-glow);
        }}
        /* 基础卡片与文本样式 */
        .text-title {{ font-size: 34px; fill: {}; /* font-weight: bold; */ text-shadow: 0px 2px 4px rgba(0, 0, 0, 0.4); }}
        .text-stat {{ font-size: 21px; fill: {}; }}
        .text-info {{ font-size: 16px; fill: {}; text-anchor: end; }} /* For new info */
        .text-time {{ font-size: 15px; fill: {}; text-anchor: end; }}
        .text-footer {{ font-size: 14px; fill: {}; }}
        .text-songname {{ font-size: 20px; fill: {}; font-weight: 600; }}
        .text-score {{ font-size: 30px; fill: {}; font-weight: 700; }}
        .text-acc {{ font-size: 14px; fill: {}; font-weight: 400; }}
        .text-level {{ font-size: 14px; fill: {}; font-weight: 400; }}
        .text-rank {{ font-size: 15px; fill: {}; font-weight: 500; text-anchor: end; }}
        .text-difficulty-badge {{ font-size: 12px; font-weight: 700; }} /* 难度标签文本样式 */
        .text-fc-ap-badge {{ font-size: 11px; font-weight: 700; }} /* FC/AP标签文本样式 */
        .push-acc {{ fill: #4CAF50; font-weight: 600; }}
        .push-acc-phi-only {{ fill: #FFC107; }}
        .push-acc-unreachable {{ fill: #9E9E9E; }}
        .text-rank-tag {{ font-size: 13px; fill: {}; text-anchor: end; font-weight: 700; }}
        .text-section-title {{ font-size: 21px; fill: {}; /* font-weight: bold; */ }}
        * {{ font-family: "{MAIN_FONT_NAME}", "Microsoft YaHei", "SimHei", "DengXian", Arial, sans-serif; }}
        /* ]]> */
        "#,
        palette.bg_color,
        palette.card_bg_color,
        normal_card_stroke_color,
        palette.ap_card_fill,
        palette.ap_stroke_color,
        palette.fc_card_fill,
        palette.fc_stroke_color,
        palette.text_color,
        palette.text_color,
        palette.text_secondary_color,
        palette.text_secondary_color,
        palette.text_secondary_color,
        palette.text_color,
        palette.text_color,
        palette.text_secondary_color,
        palette.text_secondary_color,
        palette.text_secondary_color,
        palette.text_secondary_color,
        palette.text_color,
    )
    .map_err(svg_fmt_error)?;
    writeln!(svg, "</style>").map_err(svg_fmt_error)?;
    let style_elapsed = started_at.elapsed();

    writeln!(
        svg,
        r#"<linearGradient id="normal-card-stroke-gradient" x1="0%" y1="0%" x2="100%" y2="100%">"#
    )
    .map_err(svg_fmt_error)?;
    writeln!(svg, "<stop offset=\"0%\" style=\"stop-color:#555868\" />").map_err(svg_fmt_error)?;
    writeln!(svg, "<stop offset=\"100%\" style=\"stop-color:#333848\" />")
        .map_err(svg_fmt_error)?;
    writeln!(svg, r"</linearGradient>").map_err(svg_fmt_error)?;

    writeln!(
        svg,
        r#"<linearGradient id="ap-gradient" x1="0%" y1="0%" x2="100%" y2="100%">"#
    )
    .map_err(svg_fmt_error)?;
    writeln!(svg, "<stop offset=\"0%\" style=\"stop-color:#FFDA63\" />").map_err(svg_fmt_error)?;
    writeln!(svg, "<stop offset=\"100%\" style=\"stop-color:#D1913C\" />")
        .map_err(svg_fmt_error)?;
    writeln!(svg, r"</linearGradient>").map_err(svg_fmt_error)?;

    writeln!(
        svg,
        r#"<linearGradient id="ap-gradient-white" x1="0%" y1="0%" x2="100%" y2="100%">"#
    )
    .map_err(svg_fmt_error)?;
    writeln!(svg, "<stop offset=\"0%\" style=\"stop-color:#D4A017\" />").map_err(svg_fmt_error)?;
    writeln!(svg, "<stop offset=\"100%\" style=\"stop-color:#B8860B\" />")
        .map_err(svg_fmt_error)?;
    writeln!(svg, r"</linearGradient>").map_err(svg_fmt_error)?;

    writeln!(svg, "</defs>").map_err(svg_fmt_error)?;
    let defs_elapsed = started_at.elapsed();

    Ok(BnDefsTiming {
        style_elapsed,
        defs_elapsed,
    })
}

pub(super) fn write_background_layer(
    ctx: BnBackgroundLayerRenderContext<'_>,
) -> Result<(), AppError> {
    let BnBackgroundLayerRenderContext {
        svg,
        theme,
        background_image_href,
    } = ctx;

    let overlay_fill = match theme {
        Theme::White => BACKGROUND_OVERLAY_WHITE,
        Theme::Black => BACKGROUND_OVERLAY_DARK,
    };

    write_svg_background_layer(SvgBackgroundLayerRenderContext {
        svg,
        background_image_href,
        overlay_fill,
        fallback_fill: BACKGROUND_FALLBACK_GRADIENT,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_background_layer_escapes_href_attribute() {
        let mut svg = String::new();

        write_background_layer(BnBackgroundLayerRenderContext {
            svg: &mut svg,
            theme: &Theme::Black,
            background_image_href: Some("https://example.com/bg?a=1&b=<x>\"".to_string()),
        })
        .expect("write bn background layer");

        assert!(svg.contains("https://example.com/bg?a=1&amp;b=&lt;x&gt;&quot;"));
        assert!(!svg.contains("https://example.com/bg?a=1&b=<x>\""));
    }
}
