use std::fmt::Write;

use crate::error::AppError;

use super::math::f64_from_usize;
use super::song_difficulty::{
    SONG_DIFFICULTIES, card_class as difficulty_card_class, has_chart, has_score,
    score_for_difficulty,
};
use super::song_score_text::{
    build_song_acc_handwritten_xml, build_song_constant_text, build_song_score_text,
};
use super::svg_error::svg_fmt_error;
use super::{SongDifficultyScore, SongRenderData};

#[derive(Debug, Clone, Copy)]
pub(super) struct SongDifficultyCardRenderLayout {
    pub(super) start_x: f64,
    pub(super) start_y: f64,
    pub(super) card_width: f64,
    pub(super) card_height: f64,
    pub(super) card_spacing: f64,
}

pub(super) fn render_song_difficulty_cards(
    svg: &mut String,
    data: &SongRenderData,
    layout: SongDifficultyCardRenderLayout,
) -> Result<(), AppError> {
    for (i, &diff_key) in SONG_DIFFICULTIES.iter().enumerate() {
        let pos_x = layout.start_x;
        let pos_y = layout.start_y + (layout.card_height + layout.card_spacing) * f64_from_usize(i);
        let score_data = score_for_difficulty(data, diff_key);

        // 检查是否有该难度的数据，决定卡片样式。
        let card_class = difficulty_card_class(score_data);

        writeln!(svg, r#"<rect x="{pos_x}" y="{pos_y}" width="{card_width}" height="{card_height}" rx="8" ry="8" class="{card_class}" filter="url(#card-shadow)" />"#, card_width = layout.card_width, card_height = layout.card_height).map_err(svg_fmt_error)?;

        let content_padding = 25.0;
        let card_middle = pos_x + content_padding + 80.0;
        let diff_label_class = format!(
            "text text-label text-difficulty-{}",
            diff_key.to_lowercase()
        );
        let label_x = pos_x + content_padding + 35.0;
        let label_y = pos_y + layout.card_height / 2.0;

        writeln!(svg, r#"<text x="{label_x}" y="{label_y}" class="{diff_label_class}" text-anchor="middle">{diff_key}</text>"#).map_err(svg_fmt_error)?;

        if let Some(score_data) = score_data
            && let Some(constant_text) = build_song_constant_text(score_data)
        {
            let constant_text_x = label_x;
            let constant_text_y = label_y + 20.0;
            writeln!(svg, r#"<text x="{constant_text_x}" y="{constant_text_y}" class="text-constants" text-anchor="middle">{constant_text}</text>"#).map_err(svg_fmt_error)?;
        }

        let right_area_start = card_middle;
        let right_area_width = layout.card_width - (card_middle - pos_x);
        let right_area_center = right_area_start + right_area_width / 2.0;

        if let Some(score_data) = score_data {
            if has_score(Some(score_data)) {
                write_score_details(
                    svg,
                    score_data,
                    SongScoreTextLayout {
                        text_x: right_area_start + 25.0,
                        score_y: pos_y + 40.0,
                        acc_y: pos_y + 65.0,
                        rks_y: pos_y + 88.0,
                    },
                )?;
            } else if has_chart(Some(score_data)) {
                write_empty_state(
                    svg,
                    right_area_center,
                    pos_y + layout.card_height / 2.0 + 5.0,
                    "无成绩",
                )?;
            }
        } else {
            write_empty_state(
                svg,
                right_area_center,
                pos_y + layout.card_height / 2.0 + 5.0,
                "无谱面",
            )?;
        }
    }

    Ok(())
}

#[derive(Debug, Clone, Copy)]
struct SongScoreTextLayout {
    text_x: f64,
    score_y: f64,
    acc_y: f64,
    rks_y: f64,
}

fn write_score_details(
    svg: &mut String,
    score_data: &SongDifficultyScore,
    layout: SongScoreTextLayout,
) -> Result<(), AppError> {
    let text = build_song_score_text(score_data);
    let acc_text = build_song_acc_handwritten_xml(score_data);

    let SongScoreTextLayout {
        text_x,
        score_y,
        acc_y,
        rks_y,
    } = layout;

    writeln!(
        svg,
        r#"<text x="{text_x}" y="{score_y}" class="text text-score" text-anchor="start">{}</text>"#,
        text.score_text
    )
    .map_err(svg_fmt_error)?;
    writeln!(svg, r#"<text x="{text_x}" y="{acc_y}" class="text text-acc" text-anchor="start">{acc_text}</text>"#).map_err(svg_fmt_error)?;
    writeln!(
        svg,
        r#"<text x="{text_x}" y="{rks_y}" class="text text-rks" text-anchor="start">{}</text>"#,
        text.rks_text
    )
    .map_err(svg_fmt_error)?;

    Ok(())
}

fn write_empty_state(svg: &mut String, x: f64, y: f64, text: &str) -> Result<(), AppError> {
    writeln!(svg, r#"<text x="{x}" y="{y}" class="text text-acc" text-anchor="middle" dominant-baseline="middle">{text}</text>"#)
        .map_err(svg_fmt_error)?;
    Ok(())
}
