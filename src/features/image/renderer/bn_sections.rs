use std::fmt::Write;

use crate::error::AppError;

use super::PlayerStats;
use super::bn_header_text::{build_bn_header_text, build_challenge_rank_inner_xml};
use super::svg_error::svg_fmt_error;
use super::text::escape_xml;
use super::time::{generated_at_utc8_text, updated_at_utc_text};

pub(super) struct BnHeaderRenderContext<'a> {
    pub(super) svg: &'a mut String,
    pub(super) stats: &'a PlayerStats,
    pub(super) width: u32,
    pub(super) header_height: u32,
    pub(super) card_stroke_color: &'a str,
    pub(super) text_secondary_color: &'a str,
}

pub(super) struct BnFooterRenderContext<'a> {
    pub(super) svg: &'a mut String,
    pub(super) stats: &'a PlayerStats,
    pub(super) width: u32,
    pub(super) total_height: u32,
    pub(super) footer_height: u32,
}

pub(super) fn write_header(ctx: BnHeaderRenderContext<'_>) -> Result<(), AppError> {
    let BnHeaderRenderContext {
        svg,
        stats,
        width,
        header_height,
        card_stroke_color,
        text_secondary_color,
    } = ctx;

    let header_text = build_bn_header_text(stats);
    writeln!(
        svg,
        r#"<text x="40" y="55" class="text-title">{}</text>"#,
        escape_xml(&header_text.player_title),
    )
    .map_err(svg_fmt_error)?;
    writeln!(
        svg,
        r#"<text x="40" y="85" class="text-stat">{}</text>"#,
        header_text.ap_text
    )
    .map_err(svg_fmt_error)?;
    writeln!(
        svg,
        r#"<text x="40" y="110" class="text-stat">{}</text>"#,
        header_text.bn_text
    )
    .map_err(svg_fmt_error)?;

    // 右上角信息块：Data、课题等级和更新时间。
    let mut info_y = 65.0;

    if let Some(data_str) = &stats.data_string {
        writeln!(
            svg,
            r#"<text x="{}" y="{}" class="text-info">{}</text>"#,
            width - 30,
            info_y,
            escape_xml(data_str)
        )
        .map_err(svg_fmt_error)?;
        info_y += 20.0;
    }

    if let Some((color, level)) = &stats.challenge_rank {
        let inner_xml = build_challenge_rank_inner_xml(color, level, text_secondary_color);
        writeln!(
            svg,
            r#"<text x="{}" y="{}" class="text-info">{}</text>"#,
            width - 30,
            info_y,
            inner_xml
        )
        .map_err(svg_fmt_error)?;
        info_y += 20.0;
    }

    let update_time = updated_at_utc_text(&stats.update_time, "%Y/%m/%d %H:%M:%S");
    writeln!(
        svg,
        r#"<text x="{}" y="{}" class="text-time">{}</text>"#,
        width - 30,
        info_y,
        update_time
    )
    .map_err(svg_fmt_error)?;

    writeln!(
        svg,
        "<line x1='40' y1='{}' x2='{}' y2='{}' stroke='{}' stroke-width='1' stroke-opacity='0.7'/>",
        header_height,
        width - 40,
        header_height,
        card_stroke_color
    )
    .map_err(svg_fmt_error)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::*;

    #[test]
    fn write_header_escapes_challenge_rank_text() {
        let stats = PlayerStats {
            ap_top_3_avg: None,
            best_27_avg: None,
            real_rks: Some(15.123_456),
            player_name: Some("Tester".to_string()),
            update_time: Utc::now(),
            n: 1,
            ap_top_3_scores: vec![],
            challenge_rank: Some(("Green<&>\"".to_string(), "Lv<1&\"".to_string())),
            data_string: None,
            custom_footer_text: None,
            is_user_generated: false,
        };
        let mut svg = String::new();

        write_header(BnHeaderRenderContext {
            svg: &mut svg,
            stats: &stats,
            width: 1200,
            header_height: 130,
            card_stroke_color: "#333333",
            text_secondary_color: "#666666",
        })
        .expect("write bn header");

        assert!(svg.contains("Green&lt;&amp;&gt;&quot;"));
        assert!(svg.contains("Lv&lt;1&amp;&quot;"));
        assert!(!svg.contains("Green<&>\""));
        assert!(!svg.contains("Lv<1&\""));
    }
}

pub(super) fn write_footer(ctx: BnFooterRenderContext<'_>) -> Result<(), AppError> {
    let BnFooterRenderContext {
        svg,
        stats,
        width,
        total_height,
        footer_height,
    } = ctx;

    let footer_y = f64::from(total_height - footer_height / 2 + 10);
    let footer_padding = 40.0;

    let generated_text = generated_at_utc8_text();
    writeln!(
        svg,
        r#"<text x="{footer_padding}" y="{footer_y:.1}" class="text-footer" text-anchor="start">{generated_text}</text>"#
    )
    .map_err(svg_fmt_error)?;

    if let Some(custom_text) = &stats.custom_footer_text
        && !custom_text.is_empty()
    {
        writeln!(
            svg,
            r#"<text x="{}" y="{:.1}" class="text-footer" text-anchor="end">{}</text>"#,
            f64::from(width) - footer_padding,
            footer_y,
            escape_xml(custom_text)
        )
        .map_err(svg_fmt_error)?;
    }

    Ok(())
}
