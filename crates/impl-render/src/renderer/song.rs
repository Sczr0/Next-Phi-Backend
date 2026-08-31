use std::fmt::Write;

use crate::error::AppError;

use super::song_background::select_song_background;
use super::song_card::{SongDifficultyCardRenderLayout, render_song_difficulty_cards};
use super::song_defs::{
    SongBackgroundLayerRenderContext, SongDefsRenderContext, write_song_background_layer,
    write_song_defs,
};
use super::song_layout::SongLayout;
use super::song_sections::{
    SongFooterRenderContext, SongIllustrationRenderContext, SongPlayerInfoRenderContext,
    write_footer, write_illustration_and_song_name, write_player_info,
};
use super::svg_error::svg_fmt_error;
use super::{SongRenderData, template_song};

pub(super) fn generate_song_svg_string(
    data: &SongRenderData,
    embed_images: bool,
    public_illustration_base_url: Option<&str>,
    template_id: Option<&str>,
) -> Result<String, AppError> {
    if template_id.is_some() {
        return template_song::generate_song_svg_with_template(
            data,
            embed_images,
            public_illustration_base_url,
            template_id,
        );
    }
    let t0 = std::time::Instant::now();

    let SongLayout {
        width,
        height,
        padding,
        player_info_height,
        illust_height,
        illust_width,
        song_name_height,
        difficulty_card_width,
        difficulty_card_height,
        difficulty_card_spacing,
    } = SongLayout::new();

    // 预分配 SVG 字符串容量
    let mut svg = String::with_capacity(120_000);

    let background_image_href = select_song_background(
        &data.song_id,
        embed_images,
        public_illustration_base_url,
        width,
        height,
    );

    let defs_timing = write_song_defs(SongDefsRenderContext {
        svg: &mut svg,
        width,
        height,
        started_at: &t0,
    })?;
    let t_defs_song = defs_timing.defs_elapsed;
    write_song_background_layer(SongBackgroundLayerRenderContext {
        svg: &mut svg,
        background_image_href,
    })?;

    write_player_info(SongPlayerInfoRenderContext {
        svg: &mut svg,
        data,
        width,
        padding,
        player_info_height,
    })?;
    let cards_start = write_illustration_and_song_name(SongIllustrationRenderContext {
        svg: &mut svg,
        data,
        padding,
        player_info_height,
        illust_height,
        illust_width,
        song_name_height,
        embed_images,
        public_illustration_base_url,
    })?;

    render_song_difficulty_cards(
        &mut svg,
        data,
        SongDifficultyCardRenderLayout {
            start_x: cards_start.x,
            start_y: cards_start.y,
            card_width: difficulty_card_width,
            card_height: difficulty_card_height,
            card_spacing: difficulty_card_spacing,
        },
    )?;

    let t_body_song = t0.elapsed();
    write_footer(SongFooterRenderContext {
        svg: &mut svg,
        data,
        width,
        height,
        padding,
    })?;

    // --- End SVG ---
    writeln!(svg, "</svg>").map_err(svg_fmt_error)?;

    tracing::info!(
        "SVG(单曲)生成分段: defs={:?}, body={:?}, 总计={:?}",
        t_defs_song,
        t_body_song.saturating_sub(t_defs_song),
        t0.elapsed(),
    );

    Ok(svg)
}
