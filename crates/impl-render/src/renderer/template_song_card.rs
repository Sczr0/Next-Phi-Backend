use serde::Serialize;

use super::SongRenderData;
use super::math::f64_from_usize;
use super::song_difficulty::{
    SONG_DIFFICULTIES, card_class as difficulty_card_class, score_for_difficulty,
};
use super::song_score_text::{
    INACTIVE_ACC_TEXT, INACTIVE_RKS_TEXT, build_song_constant_text, build_song_score_text,
};
use super::text::escape_xml;

#[derive(Debug, Clone, Copy)]
pub(super) struct SongDifficultyCardsLayout {
    pub(super) start_x: f64,
    pub(super) start_y: f64,
    pub(super) card_w: f64,
    pub(super) card_h: f64,
    pub(super) card_spacing: f64,
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct SongDiffCardCtx {
    key: String,
    key_lower: String,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    r: f64,
    card_class: String,
    label_x: f64,
    label_y: f64,
    constant_y: f64,
    constant_text_xml: Option<String>,
    text_x: f64,
    score_y: f64,
    acc_y: f64,
    rks_y: f64,
    score_text: Option<String>,
    acc_text_xml: String,
    rks_text_xml: String,
    no_data_x: f64,
    no_data_y: f64,
    no_data_text: String,
}

pub(super) fn build_song_difficulty_cards(
    data: &SongRenderData,
    layout: SongDifficultyCardsLayout,
) -> Vec<SongDiffCardCtx> {
    let mut difficulty_cards = Vec::<SongDiffCardCtx>::new();
    for (idx, key) in SONG_DIFFICULTIES.into_iter().enumerate() {
        let pos_x = layout.start_x;
        let pos_y = layout.start_y + f64_from_usize(idx) * (layout.card_h + layout.card_spacing);
        let r = 18.0;

        let content_padding = 18.0;
        let label_x = pos_x + content_padding + 35.0;
        let label_y = pos_y + layout.card_h / 2.0;
        let constant_y = label_y + 20.0;

        let right_area_start = pos_x + 90.0;
        let text_x = right_area_start + 25.0;
        let score_y = pos_y + 40.0;
        let acc_y = pos_y + 65.0;
        let rks_y = pos_y + 88.0;

        let (score_text, acc_text, rks_text, constant_text, card_class) =
            match score_for_difficulty(data, key) {
                Some(score_data) if score_data.acc.is_some() => {
                    let text = build_song_score_text(score_data);

                    (
                        Some(text.score_text),
                        text.acc_text,
                        text.rks_text,
                        text.constant_text,
                        difficulty_card_class(Some(score_data)).to_string(),
                    )
                }
                Some(score_data) => {
                    let constant_text = build_song_constant_text(score_data);
                    (
                        None,
                        INACTIVE_ACC_TEXT.to_string(),
                        INACTIVE_RKS_TEXT.to_string(),
                        constant_text,
                        "difficulty-card-inactive".to_string(),
                    )
                }
                None => (
                    None,
                    INACTIVE_ACC_TEXT.to_string(),
                    INACTIVE_RKS_TEXT.to_string(),
                    None,
                    "difficulty-card-inactive".to_string(),
                ),
            };

        difficulty_cards.push(SongDiffCardCtx {
            key: key.to_string(),
            key_lower: key.to_lowercase(),
            x: pos_x,
            y: pos_y,
            w: layout.card_w,
            h: layout.card_h,
            r,
            card_class,
            label_x,
            label_y,
            constant_y,
            constant_text_xml: constant_text.map(|s| escape_xml(&s)),
            text_x,
            score_y,
            acc_y,
            rks_y,
            score_text,
            acc_text_xml: escape_xml(&acc_text),
            rks_text_xml: escape_xml(&rks_text),
            no_data_x: pos_x + layout.card_w / 2.0,
            no_data_y: pos_y + layout.card_h / 2.0,
            no_data_text: "无谱面".to_string(),
        });
    }
    difficulty_cards
}
