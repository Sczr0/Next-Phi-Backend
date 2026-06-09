use std::collections::HashMap;

use serde::Serialize;

use super::bn_card_acc::{
    format_plain_acc_text, pre_calculated_push_acc_for_score, resolve_push_acc_hint,
};
use super::bn_card_badge_style::{difficulty_badge_style, fc_ap_badge_style};
use super::bn_card_cover::{bn_cover_clip_id, resolve_card_cover_href};
use super::bn_card_text::{bn_level_text, bn_rank_text, bn_template_score_text};
use super::engine;
use super::math::{f64_from_usize, round_non_negative_to_u32};
use super::template_bn::BnTemplateLayout;
use super::template_shared::{truncate_with_ellipsis, wrap_by_display_width};
use super::text::escape_xml;
use super::{COVER_ASPECT_RATIO, RenderRecord, Theme};

#[derive(Debug, Clone, Serialize)]
struct CoverCtx {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    href_xml: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct BadgeDiffCtx {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    r: f64,
    tx: f64,
    ty: f64,
    fill: String,
    text: String,
}

#[derive(Debug, Clone, Serialize)]
struct BadgeMiniCtx {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    r: f64,
    tx: f64,
    ty: f64,
    fill: String,
    text_fill: String,
    text: String,
}

#[derive(Debug, Clone, Serialize)]
struct BadgeCtx {
    diff: BadgeDiffCtx,
    fc_ap: Option<BadgeMiniCtx>,
}

#[derive(Debug, Clone, Serialize)]
struct CardTextCtx {
    song_x: f64,
    song_y: f64,
    score_x: f64,
    score_y: f64,
    acc_x: f64,
    acc_y: f64,
    level_x: f64,
    level_y: f64,
    rank_x: f64,
    rank_y: f64,
    song_name_xml: String,
    score_text: String,
    acc_text_xml: String,
    level_text_xml: String,
    rank_text: String,
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct CardCtx {
    x: u32,
    pub(super) y: u32,
    w: u32,
    pub(super) h: u32,
    radius: u32,
    class_extra: String,
    clip_id: String,
    cover: CoverCtx,
    badge: BadgeCtx,
    text: CardTextCtx,
}

pub(super) struct BnCardBuildCtx<'a, S>
where
    S: std::hash::BuildHasher,
{
    pub(super) theme: &'a Theme,
    pub(super) embed_images: bool,
    pub(super) public_illustration_base_url: Option<&'a str>,
    pub(super) engine_records: &'a [engine::RksRecord],
    pub(super) push_acc_map: Option<&'a HashMap<String, engine::PushAccHint, S>>,
    pub(super) layout: &'a BnTemplateLayout,
}

pub(super) fn build_bn_card<S>(
    score: &RenderRecord,
    index: usize,
    card_x: u32,
    card_y: u32,
    card_w: u32,
    is_ap_card: bool,
    ctx: &BnCardBuildCtx<'_, S>,
) -> CardCtx
where
    S: std::hash::BuildHasher,
{
    // 内边距与行高：沿用原实现的尺度，便于在模板中替换字段排列时保持可读。
    let card_padding = 10.0;
    let text_line_height_song = 22.0;
    let text_line_height_score = 30.0;
    let text_line_height_acc = 18.0;
    let text_line_height_level = 18.0;
    let text_block_spacing = 4.0;
    let vertical_text_offset = 5.0;

    // 歌名：默认单行 + 省略号，开启多行时按 display width 包裹。
    let song_lines = wrap_by_display_width(
        &score.song_name,
        ctx.layout.song_name_max_width.max(1),
        ctx.layout.song_name_max_lines.max(1),
    );
    let song_name_display = if ctx.layout.song_name_max_lines <= 1 {
        truncate_with_ellipsis(&score.song_name, ctx.layout.song_name_max_width.max(1))
    } else {
        song_lines.join("\n")
    };

    let song_lines_count = f64_from_usize(song_lines.len().max(1));
    let text_block_height = text_line_height_song * song_lines_count
        + text_line_height_score
        + text_line_height_acc
        + text_line_height_level
        + text_block_spacing * (2.0 + song_lines_count);

    let cover_h = text_block_height;
    let cover_w = cover_h * COVER_ASPECT_RATIO;
    let card_h = round_non_negative_to_u32((cover_h + card_padding * 2.0).max(1.0));
    let card_radius = 8u32;

    let cover_x = card_padding;
    let cover_y = card_padding;

    let cover_href = resolve_card_cover_href(
        &score.song_id,
        ctx.embed_images,
        ctx.public_illustration_base_url,
        cover_w,
        cover_h,
    );

    // 文本坐标：仍按“封面右侧文本块”布局计算，模板可自行改。
    let text_x = cover_x + cover_w + 15.0;
    let song_name_y = cover_y + text_line_height_song * 0.75 + vertical_text_offset;
    let score_y =
        song_name_y + (text_line_height_song * song_lines_count) + text_block_spacing + 2.0;
    let acc_y = score_y + text_line_height_acc + text_block_spacing;
    let level_y = acc_y + text_line_height_level + text_block_spacing;

    // 难度徽章。
    let difficulty_badge = difficulty_badge_style(&score.difficulty);
    let badge_w = 36.0;
    let badge_h = 20.0;
    let badge_r = 4.0;
    let badge_x = cover_x + 5.0;
    let badge_y = cover_y + cover_h - badge_h - 5.0;

    // AP/FC 徽章（AP 优先）。
    let fc_ap_badge = fc_ap_badge_style(score, ctx.theme).map(|style| BadgeMiniCtx {
        x: badge_x + badge_w + 5.0,
        y: badge_y,
        w: 30.0,
        h: 20.0,
        r: 4.0,
        tx: badge_x + badge_w + 5.0 + 15.0,
        ty: badge_y + 10.0 + 5.0,
        fill: style.fill.to_string(),
        text_fill: style.text_fill.to_string(),
        text: style.text.to_string(),
    });

    // 推分 ACC（优先使用上游预计算，否则按 engine 计算）。
    let push_hint = resolve_push_acc_hint(
        score,
        pre_calculated_push_acc_for_score(score, ctx.push_acc_map),
        ctx.engine_records,
    );

    let score_text = bn_template_score_text(score);

    let acc_text = format_plain_acc_text(score, push_hint);
    let level_text = bn_level_text(score);

    let class_extra = if is_ap_card {
        "card-ap".to_string()
    } else if score.is_fc {
        "card-fc".to_string()
    } else {
        String::new()
    };

    CardCtx {
        x: card_x,
        y: card_y,
        w: card_w,
        h: card_h,
        radius: card_radius,
        class_extra,
        clip_id: bn_cover_clip_id(is_ap_card, index),
        cover: CoverCtx {
            x: cover_x,
            y: cover_y,
            w: cover_w,
            h: cover_h,
            href_xml: cover_href.map(|s| escape_xml(&s)),
        },
        badge: BadgeCtx {
            diff: BadgeDiffCtx {
                x: badge_x,
                y: badge_y,
                w: badge_w,
                h: badge_h,
                r: badge_r,
                tx: badge_x + badge_w / 2.0,
                ty: badge_y + badge_h / 2.0 + 5.0,
                fill: difficulty_badge.fill.to_string(),
                text: difficulty_badge.text.to_string(),
            },
            fc_ap: fc_ap_badge,
        },
        text: CardTextCtx {
            song_x: text_x,
            song_y: song_name_y,
            score_x: text_x,
            score_y,
            acc_x: text_x,
            acc_y,
            level_x: text_x,
            level_y,
            rank_x: f64::from(card_w) - card_padding,
            rank_y: level_y + 2.0,
            song_name_xml: escape_xml(&song_name_display),
            score_text,
            acc_text_xml: escape_xml(&acc_text),
            level_text_xml: escape_xml(&level_text),
            rank_text: bn_rank_text(index),
        },
    }
}
