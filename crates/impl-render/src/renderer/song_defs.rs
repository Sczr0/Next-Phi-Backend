use std::fmt::Write;

use crate::error::AppError;

use super::MAIN_FONT_NAME;
use super::background_layer::{
    BACKGROUND_FALLBACK_GRADIENT, BACKGROUND_OVERLAY_DARK, SvgBackgroundLayerRenderContext,
    write_svg_background_layer,
};
use super::svg_error::svg_fmt_error;

pub(super) struct SongDefsRenderContext<'a> {
    pub(super) svg: &'a mut String,
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) started_at: &'a std::time::Instant,
}

pub(super) struct SongDefsTiming {
    pub(super) defs_elapsed: std::time::Duration,
}

pub(super) struct SongBackgroundLayerRenderContext<'a> {
    pub(super) svg: &'a mut String,
    pub(super) background_image_href: Option<String>,
}

pub(super) fn write_song_defs(ctx: SongDefsRenderContext<'_>) -> Result<SongDefsTiming, AppError> {
    let SongDefsRenderContext {
        svg,
        width,
        height,
        started_at,
    } = ctx;

    writeln!(svg, r#"<svg width="{width}" height="{height}" viewBox="0 0 {width} {height}" xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink">"#).map_err(svg_fmt_error)?;
    writeln!(svg, "<defs>").map_err(svg_fmt_error)?;
    writeln!(svg, "<style>").map_err(svg_fmt_error)?;
    writeln!(
        svg,
        r"
        /* 基本文本样式 */
        .text {{ font-family: '{MAIN_FONT_NAME}', sans-serif; fill: #E0E0E0; }}
        .text-title {{ font-size: 32px; font-weight: bold; fill: #FFFFFF; }}
        .text-subtitle {{ font-size: 18px; fill: #B0B0B0; }}
        .text-label {{ font-size: 28px; font-weight: bold; }} /* 增大难度标签字体 */
        .text-value {{ font-size: 18px; fill: #E0E0E0; }}
        .text-score {{ font-size: 34px; font-weight: bold; }} /* 增大分数字体 */
        .text-acc {{ font-size: 18px; fill: #B0B0B0; }} /* 参考Bn图调整ACC字体 */
        .text-rks {{ font-size: 18px; fill: #E0E0E0; }} /* 参考Bn图调整RKS字体 */
        .text-push-acc {{ font-size: 18px; font-weight: bold; }} /* 参考Bn图调整推分ACC字体 */
        .text-songname {{ font-size: 24px; font-weight: bold; fill: #FFFFFF; text-anchor: middle; }}
        .text-player-info {{ font-size: 22px; font-weight: bold; fill: #FFFFFF; }}
        .text-player-rks {{ font-size: 20px; fill: #E0E0E0; }}
        .text-difficulty-ez {{ fill: #77DD77; }}
        .text-difficulty-hd {{ fill: #87CEEB; }}
        .text-difficulty-in {{ fill: #FFB347; }}
        .text-difficulty-at {{ fill: #FF6961; }}
        .text-footer {{ font-size: 14px; fill: #888888; text-anchor: end; }}
        .text-constants {{ font-size: 18px; fill: #AAAAAA; }}
        .player-info-card {{ fill: rgba(40, 45, 60, 0.8); stroke: rgba(100, 100, 100, 0.4); stroke-width: 1; }}
        .difficulty-card {{ fill: url(#card-gradient); stroke: rgba(120, 120, 120, 0.5); stroke-width: 1.5; }} /* 使用渐变填充 */
        .difficulty-card-inactive {{ fill: rgba(40, 45, 60, 0.5); stroke: rgba(70, 70, 70, 0.3); stroke-width: 1; }}
        .difficulty-card-fc {{ fill: url(#card-gradient); stroke: #87CEEB; stroke-width: 3; }} /* FC卡片使用渐变填充 */
        .difficulty-card-phi {{ fill: url(#card-gradient); stroke: gold; stroke-width: 3; }} /* Phi卡片使用渐变填充 */
        .song-name-card {{ fill: rgba(40, 45, 60, 0.8); stroke: rgba(100, 100, 100, 0.4); stroke-width: 1; }}
        .constants-card {{ fill: rgba(40, 45, 60, 0.8); stroke: rgba(100, 100, 100, 0.4); stroke-width: 1; }}
        .rank-phi {{ fill: gold; }}
        .rank-v {{ fill: silver; }}
        .rank-s {{ fill: #FF6B6B; }}
    "
    )
    .map_err(svg_fmt_error)?;
    writeln!(svg, "</style>").map_err(svg_fmt_error)?;

    writeln!(svg, r#"<linearGradient id="bg-gradient" x1="0%" y1="0%" x2="100%" y2="100%"><stop offset="0%" style="stop-color:#141826" /><stop offset="100%" style="stop-color:#252E48" /></linearGradient>"#).map_err(svg_fmt_error)?;
    writeln!(svg, r#"<filter id="card-shadow" x="-10%" y="-10%" width="120%" height="130%"><feDropShadow dx="0" dy="3" stdDeviation="3" flood-color="rgba(0,0,0,0.25)" flood-opacity="0.25" /></filter>"#).map_err(svg_fmt_error)?;
    writeln!(
        svg,
        r#"<filter id="bg-blur"><feGaussianBlur stdDeviation="10" /></filter>"#
    )
    .map_err(svg_fmt_error)?;
    writeln!(svg, r#"<filter id="illust-shadow" x="-20%" y="-20%" width="140%" height="140%"><feDropShadow dx="0" dy="4" stdDeviation="6" flood-color="rgba(0,0,0,0.3)" flood-opacity="0.3" /></filter>"#).map_err(svg_fmt_error)?;
    writeln!(svg, r#"<linearGradient id="rks-gradient" x1="0%" y1="0%" x2="100%" y2="0%"><stop offset="0%" style="stop-color:#FDC830" /><stop offset="100%" style="stop-color:#F37335" /></linearGradient>"#).map_err(svg_fmt_error)?;
    writeln!(svg, r#"<linearGradient id="card-gradient" x1="0%" y1="0%" x2="100%" y2="100%"><stop offset="0%" style="stop-color:#2D3241" /><stop offset="100%" style="stop-color:#1E2330" /></linearGradient>"#).map_err(svg_fmt_error)?;
    writeln!(svg, r#"<linearGradient id="rks-gradient-ap" x1="0%" y1="0%" x2="100%" y2="0%"><stop offset="0%" style="stop-color:#f6d365" /><stop offset="100%" style="stop-color:#fda085" /></linearGradient>"#).map_err(svg_fmt_error)?;
    writeln!(svg, r#"<linearGradient id="rks-gradient-push" x1="0%" y1="0%" x2="100%" y2="0%"><stop offset="0%" style="stop-color:#a8e063" /><stop offset="100%" style="stop-color:#56ab2f" /></linearGradient>"#).map_err(svg_fmt_error)?;
    writeln!(svg, "</defs>").map_err(svg_fmt_error)?;

    Ok(SongDefsTiming {
        defs_elapsed: started_at.elapsed(),
    })
}

pub(super) fn write_song_background_layer(
    ctx: SongBackgroundLayerRenderContext<'_>,
) -> Result<(), AppError> {
    let SongBackgroundLayerRenderContext {
        svg,
        background_image_href,
    } = ctx;

    write_svg_background_layer(SvgBackgroundLayerRenderContext {
        svg,
        background_image_href,
        overlay_fill: BACKGROUND_OVERLAY_DARK,
        fallback_fill: BACKGROUND_FALLBACK_GRADIENT,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_song_background_layer_escapes_href_attribute() {
        let mut svg = String::new();

        write_song_background_layer(SongBackgroundLayerRenderContext {
            svg: &mut svg,
            background_image_href: Some("https://example.com/song?a=1&b=<x>\"".to_string()),
        })
        .expect("write song background layer");

        assert!(svg.contains("https://example.com/song?a=1&amp;b=&lt;x&gt;&quot;"));
        assert!(!svg.contains("https://example.com/song?a=1&b=<x>\""));
    }
}
