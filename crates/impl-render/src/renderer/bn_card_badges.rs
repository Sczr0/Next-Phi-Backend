use std::fmt::Write;

use crate::error::AppError;
use crate::features::image::Theme;

use super::RenderRecord;
use super::bn_card_badge_style::{difficulty_badge_style, fc_ap_badge_style};
use super::svg_error::svg_fmt_error;

pub(super) struct CardBadgeRenderContext<'a> {
    pub(super) svg: &'a mut String,
    pub(super) score: &'a RenderRecord,
    pub(super) theme: &'a Theme,
    pub(super) cover_x: f64,
    pub(super) cover_y: f64,
    pub(super) cover_size_h: f64,
}

pub(super) fn write_card_badges(ctx: CardBadgeRenderContext<'_>) -> Result<(), AppError> {
    let CardBadgeRenderContext {
        svg,
        score,
        theme,
        cover_x,
        cover_y,
        cover_size_h,
    } = ctx;

    let difficulty_badge = difficulty_badge_style(&score.difficulty);

    let badge_width = 36.0;
    let badge_height = 20.0;
    let badge_radius = 4.0;
    let badge_x = cover_x + 5.0;
    let badge_y = cover_y + cover_size_h - badge_height - 5.0;

    writeln!(svg, r#"<rect x="{badge_x}" y="{badge_y:.1}" width="{badge_width:.1}" height="{badge_height:.1}" rx="{badge_radius:.1}" ry="{badge_radius:.1}" fill="{}" />"#, difficulty_badge.fill).map_err(svg_fmt_error)?;

    let badge_text_x = badge_x + badge_width / 2.0;
    let badge_text_y = badge_y + badge_height / 2.0 + 5.0;
    writeln!(svg, r#"<text x="{badge_text_x:.1}" y="{badge_text_y:.1}" class="text-difficulty-badge" text-anchor="middle" fill="white">{}</text>"#, difficulty_badge.text).map_err(svg_fmt_error)?;

    let fc_ap_badge_width = 30.0;
    let fc_ap_badge_height = 20.0;
    let fc_ap_badge_radius = 4.0;
    let fc_ap_badge_spacing = 5.0;

    if let Some(fc_ap_badge) = fc_ap_badge_style(score, theme) {
        let fc_ap_badge_x = badge_x + badge_width + fc_ap_badge_spacing;
        let fc_ap_badge_y = badge_y;
        writeln!(svg, r#"<rect x="{fc_ap_badge_x}" y="{fc_ap_badge_y:.1}" width="{fc_ap_badge_width:.1}" height="{fc_ap_badge_height:.1}" rx="{fc_ap_badge_radius:.1}" ry="{fc_ap_badge_radius:.1}" fill="{}" />"#, fc_ap_badge.fill).map_err(svg_fmt_error)?;
        let fc_ap_badge_text_x = fc_ap_badge_x + fc_ap_badge_width / 2.0;
        let fc_ap_badge_text_y = fc_ap_badge_y + fc_ap_badge_height / 2.0 + 5.0;
        writeln!(svg, r#"<text x="{fc_ap_badge_text_x:.1}" y="{fc_ap_badge_text_y:.1}" class="text-fc-ap-badge" text-anchor="middle" fill="{}">{}</text>"#, fc_ap_badge.text_fill, fc_ap_badge.text).map_err(svg_fmt_error)?;
    }

    Ok(())
}
