use std::fmt::Write;

use crate::error::AppError;

use super::SongRenderData;
use super::song_illustration::resolve_song_illustration_href;
use super::svg_error::svg_fmt_error;
use super::text::escape_xml;
use super::time::format_utc8_datetime;

pub(super) struct SongPlayerInfoRenderContext<'a> {
    pub(super) svg: &'a mut String,
    pub(super) data: &'a SongRenderData,
    pub(super) width: u32,
    pub(super) padding: f64,
    pub(super) player_info_height: f64,
}

pub(super) struct SongIllustrationRenderContext<'a> {
    pub(super) svg: &'a mut String,
    pub(super) data: &'a SongRenderData,
    pub(super) padding: f64,
    pub(super) player_info_height: f64,
    pub(super) illust_height: f64,
    pub(super) illust_width: f64,
    pub(super) song_name_height: f64,
    pub(super) embed_images: bool,
    pub(super) public_illustration_base_url: Option<&'a str>,
}

pub(super) struct SongDifficultyCardsStart {
    pub(super) x: f64,
    pub(super) y: f64,
}

pub(super) struct SongFooterRenderContext<'a> {
    pub(super) svg: &'a mut String,
    pub(super) data: &'a SongRenderData,
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) padding: f64,
}

pub(super) fn write_player_info(ctx: SongPlayerInfoRenderContext<'_>) -> Result<(), AppError> {
    let SongPlayerInfoRenderContext {
        svg,
        data,
        width,
        padding,
        player_info_height,
    } = ctx;

    let player_info_x = padding;
    let player_info_y = padding;
    let player_info_width = f64::from(width) - padding * 2.0;

    writeln!(svg, r#"<rect x="{player_info_x}" y="{player_info_y}" width="{player_info_width}" height="{player_info_height}" rx="8" ry="8" class="player-info-card" filter="url(#card-shadow)" />"#).map_err(svg_fmt_error)?;

    let player_name_display = data.player_name.as_deref().unwrap_or("Player");
    let player_name_display_xml = escape_xml(player_name_display);
    writeln!(
        svg,
        r#"<text x="{}" y="{}" class="text text-player-info">Player: {}</text>"#,
        player_info_x + 20.0,
        player_info_y + 49.0,
        player_name_display_xml
    )
    .map_err(svg_fmt_error)?;

    let time_str = format_utc8_datetime(&data.update_time, "%Y-%m-%d %H:%M:%S");
    writeln!(
        svg,
        r#"<text x="{}" y="{}" class="text text-subtitle" text-anchor="end">{}</text>"#,
        f64::from(width) - padding - 20.0,
        player_info_y + 49.0,
        time_str
    )
    .map_err(svg_fmt_error)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use std::collections::HashMap;

    use super::*;

    #[test]
    fn write_player_info_escapes_player_name() {
        let data = SongRenderData {
            song_name: "Song".to_string(),
            song_id: "song-id".to_string(),
            player_name: Some("玩家<&>\"".to_string()),
            update_time: Utc::now(),
            difficulty_scores: HashMap::new(),
            illustration_path: None,
            custom_footer_text: None,
        };
        let mut svg = String::new();

        write_player_info(SongPlayerInfoRenderContext {
            svg: &mut svg,
            data: &data,
            width: 1400,
            padding: 40.0,
            player_info_height: 78.0,
        })
        .expect("write player info");

        assert!(svg.contains("Player: 玩家&lt;&amp;&gt;&quot;"));
        assert!(!svg.contains("Player: 玩家<&>\""));
    }
}

pub(super) fn write_illustration_and_song_name(
    ctx: SongIllustrationRenderContext<'_>,
) -> Result<SongDifficultyCardsStart, AppError> {
    let SongIllustrationRenderContext {
        svg,
        data,
        padding,
        player_info_height,
        illust_height,
        illust_width,
        song_name_height,
        embed_images,
        public_illustration_base_url,
    } = ctx;

    let illust_x = padding;
    let illust_y = padding + player_info_height + padding;
    let illust_href = resolve_song_illustration_href(
        data,
        embed_images,
        public_illustration_base_url,
        illust_width,
        illust_height,
    );

    let song_name_x = illust_x;
    let song_name_y = illust_y + illust_height + padding / 2.0;
    let song_name_width = illust_width;

    writeln!(svg, r#"<g filter="url(#illust-shadow)">"#).map_err(svg_fmt_error)?;

    let illust_clip_id = "illust-clip";
    writeln!(svg, "<defs><clipPath id=\"{illust_clip_id}\"><rect x=\"{illust_x}\" y=\"{illust_y}\" width=\"{illust_width}\" height=\"{illust_height}\" rx=\"10\" ry=\"10\" /></clipPath></defs>").map_err(svg_fmt_error)?;

    if let Some(href) = illust_href {
        writeln!(svg, r#"<image href="{}" x="{}" y="{}" width="{}" height="{}" clip-path="url(#{})" preserveAspectRatio="xMidYMid slice" />"#,
                 escape_xml(&href), illust_x, illust_y, illust_width, illust_height, illust_clip_id).map_err(svg_fmt_error)?;
    } else {
        writeln!(svg, "<rect x=\"{illust_x}\" y=\"{illust_y}\" width=\"{illust_width}\" height=\"{illust_height}\" fill=\"#333\" rx=\"10\" ry=\"10\" />").map_err(svg_fmt_error)?;
    }

    writeln!(svg, r#"<rect x="{song_name_x}" y="{song_name_y}" width="{song_name_width}" height="{song_name_height}" rx="8" ry="8" class="song-name-card" />"#).map_err(svg_fmt_error)?;
    writeln!(svg, "</g>").map_err(svg_fmt_error)?;

    writeln!(
        svg,
        r#"<text x="{}" y="{}" class="text text-songname">{}</text>"#,
        song_name_x + song_name_width / 2.0,
        song_name_y + song_name_height / 2.0 + 8.0,
        escape_xml(&data.song_name)
    )
    .map_err(svg_fmt_error)?;

    Ok(SongDifficultyCardsStart {
        x: illust_x + illust_width + padding,
        y: illust_y,
    })
}

pub(super) fn write_footer(ctx: SongFooterRenderContext<'_>) -> Result<(), AppError> {
    let SongFooterRenderContext {
        svg,
        data,
        width,
        height,
        padding,
    } = ctx;

    let footer_y = f64::from(height) - padding / 2.0;
    let footer_x = f64::from(width) - padding;
    let time_str = format_utc8_datetime(&data.update_time, "%Y-%m-%d %H:%M:%S UTC+8");
    let right_text = match data.custom_footer_text.as_deref() {
        Some(txt) if !txt.is_empty() => escape_xml(txt),
        _ => format!("Generated by Phi-Backend | {time_str}"),
    };
    writeln!(
        svg,
        r#"<text x="{footer_x}" y="{footer_y}" class="text text-footer">{right_text}</text>"#
    )
    .map_err(svg_fmt_error)?;

    Ok(())
}
