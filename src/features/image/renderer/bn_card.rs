use std::fmt::Write;

use crate::error::AppError;
use crate::features::image::Theme;
use crate::rks_contract::engine;

use super::bn_card_acc::format_acc_text;
use super::bn_card_badges::{CardBadgeRenderContext, write_card_badges};
use super::bn_card_cover::{bn_cover_clip_id, resolve_card_cover_href};
use super::bn_card_text::{bn_level_text, bn_rank_text, bn_score_text};
use super::math::round_non_negative_to_u32;
use super::svg_error::svg_fmt_error;
use super::text::{escape_xml, estimate_bn_song_name_width_px};
use super::{COVER_ASPECT_RATIO, RenderRecord};

// 单个成绩卡片的手写 SVG 渲染参数。
pub(super) struct CardRenderInfo<'a> {
    pub(super) svg: &'a mut String,
    pub(super) score: &'a RenderRecord,
    pub(super) index: usize,
    pub(super) card_x: u32,
    pub(super) card_y: u32,
    pub(super) card_width: u32,
    pub(super) is_ap_card: bool,
    pub(super) is_ap_score: bool,
    pub(super) pre_calculated_push_acc: Option<engine::PushAccHint>,
    pub(super) all_engine_records: &'a [engine::RksRecord],
    pub(super) theme: &'a Theme,
    pub(super) is_user_generated: bool,
    pub(super) embed_images: bool,
    /// 若提供，则将曲绘引用改为可被浏览器访问的 URL（例如 `/_ill`）
    pub(super) public_illustration_base_url: Option<&'a str>,
}

pub(super) fn generate_card_svg(info: CardRenderInfo) -> Result<(), AppError> {
    let CardRenderInfo {
        svg,
        score,
        index,
        card_x,
        card_y,
        card_width,
        is_ap_card,
        is_ap_score,
        pre_calculated_push_acc,
        all_engine_records,
        theme,
        is_user_generated,
        embed_images,
        public_illustration_base_url,
    } = info;

    // 卡片尺寸与内部布局。
    let card_padding = 10.0; // 内边距
    let text_line_height_song = 22.0;
    let text_line_height_score = 30.0;
    let text_line_height_acc = 18.0;
    let text_line_height_level = 18.0;
    let text_block_spacing = 4.0; // 文本行间距

    // 估算文本块高度。
    let text_block_height = text_line_height_song
        + text_line_height_score
        + text_line_height_acc
        + text_line_height_level
        + text_block_spacing * 3.0;

    let cover_size_h = text_block_height;
    let cover_size_w = cover_size_h * COVER_ASPECT_RATIO;
    let card_height = round_non_negative_to_u32(cover_size_h + card_padding * 2.0);
    let card_radius = 8;

    let cover_x = card_padding;
    let cover_y = card_padding;

    let card_class = if is_ap_score {
        "card card-ap"
    } else if score.is_fc {
        "card card-fc"
    } else {
        "card"
    };

    writeln!(svg, r#"<g transform="translate({card_x}, {card_y})">"#).map_err(svg_fmt_error)?;

    // 卡片背景。
    writeln!(svg, r#"<rect width="{card_width}" height="{card_height}" rx="{card_radius}" ry="{card_radius}" class="{card_class}" />"#).map_err(svg_fmt_error)?;

    // 卡片内容：先定义圆角封面的裁剪路径。
    let clip_path_id = bn_cover_clip_id(is_ap_card, index);
    writeln!(svg, "<defs><clipPath id=\"{clip_path_id}\"><rect x=\"{cover_x}\" y=\"{cover_y}\" width=\"{cover_size_w:.1}\" height=\"{cover_size_h:.1}\" rx=\"4\" ry=\"4\" /></clipPath></defs>").map_err(svg_fmt_error)?;

    if let Some(href) = resolve_card_cover_href(
        &score.song_id,
        embed_images,
        public_illustration_base_url,
        cover_size_w,
        cover_size_h,
    ) {
        let escaped_href = escape_xml(&href);
        writeln!(
            svg,
            r#"<image href="{escaped_href}" x="{cover_x}" y="{cover_y}" width="{cover_size_w:.1}" height="{cover_size_h:.1}" clip-path="url(#{clip_path_id})" />"#
        )
        .map_err(svg_fmt_error)?;
    }

    // 文本内容坐标。
    let text_x = cover_x + cover_size_w + 15.0; // 封面与文本之间的间距
    let text_width = f64::from(card_width) - text_x - card_padding; // 文本可用宽度

    // 用于微调文本块的整体垂直位置。
    // 可以调整这个值，直到视觉效果满意为止。数值越大，文本越往下。
    let vertical_text_offset = 5.0;

    // 文本各行纵向位置，与封面对齐。
    let song_name_y = cover_y + text_line_height_song * 0.75 + vertical_text_offset;
    let score_y = song_name_y + text_line_height_score * 0.8 + text_block_spacing + 2.0; // 分数部分向下移动2像素
    let acc_y = score_y + text_line_height_acc + text_block_spacing;
    let level_y = acc_y + text_line_height_level + text_block_spacing;

    // --- Song Name (智能判断是否需要压缩) ---
    let estimated_width = estimate_bn_song_name_width_px(&score.song_name);
    let song_name_escaped = escape_xml(&score.song_name);
    write_song_name_text(
        svg,
        text_x,
        song_name_y,
        text_width,
        estimated_width,
        &song_name_escaped,
    )
    .map_err(svg_fmt_error)?;

    // 分数。
    let score_text = bn_score_text(score);
    writeln!(
        svg,
        r#"<text x="{text_x}" y="{score_y:.1}" class="text-score">{score_text}</text>"#
    )
    .map_err(svg_fmt_error)?;

    // 如果是用户生成的数据，在分数旁边添加 "U" 标签
    if is_user_generated {
        write_user_generated_badge(svg, card_width, card_padding, level_y)
            .map_err(svg_fmt_error)?;
    }

    let acc_text = format_acc_text(
        score,
        is_ap_score,
        pre_calculated_push_acc,
        all_engine_records,
    );
    writeln!(
        svg,
        r#"<text x="{text_x}" y="{acc_y:.1}" class="text-acc">{acc_text}</text>"#
    )
    .map_err(svg_fmt_error)?;

    write_card_badges(CardBadgeRenderContext {
        svg,
        score,
        theme,
        cover_x,
        cover_y,
        cover_size_h,
    })?;

    let level_text = bn_level_text(score);
    writeln!(
        svg,
        r#"<text x="{text_x}" y="{level_y:.1}" class="text-level">{level_text}</text>"#
    )
    .map_err(svg_fmt_error)?;

    // 仅主列表显示排名，AP Top 3 不显示。
    if !is_ap_card {
        write_main_rank_text(svg, index, card_width, card_padding, level_y)
            .map_err(svg_fmt_error)?;
    }

    writeln!(svg, "</g>").map_err(svg_fmt_error)?;
    Ok(())
}

fn write_song_name_text(
    svg: &mut String,
    text_x: f64,
    song_name_y: f64,
    text_width: f64,
    estimated_width: f64,
    song_name_escaped: &str,
) -> std::fmt::Result {
    if estimated_width > text_width {
        writeln!(
            svg,
            r#"<text x="{text_x}" y="{song_name_y:.1}" class="text-songname" textLength="{text_width:.1}" lengthAdjust="spacingAndGlyphs">{song_name_escaped}</text>"#
        )
    } else {
        writeln!(
            svg,
            r#"<text x="{text_x}" y="{song_name_y:.1}" class="text-songname">{song_name_escaped}</text>"#
        )
    }
}

fn write_user_generated_badge(
    svg: &mut String,
    card_width: u32,
    card_padding: f64,
    level_y: f64,
) -> std::fmt::Result {
    // 方案: 将 "U" 标签放在序号的左边
    let u_badge_width = 18.0;
    let u_badge_height = 18.0;
    let u_badge_radius = 4.0;

    // 序号的 x 坐标是 card_width - card_padding
    // 我们将 U 标签放在序号左边，并留出一些间距
    let rank_text_approx_width = 30.0; // 估算 "#10" 这种文本的宽度
    let u_badge_x =
        f64::from(card_width) - card_padding - rank_text_approx_width - u_badge_width - 5.0;
    let u_badge_y = level_y - u_badge_height + 4.0; // 与序号的基线对齐 (向下微调2px)

    writeln!(
        svg,
        r"<rect x='{u_badge_x}' y='{u_badge_y}' width='{u_badge_width}' height='{u_badge_height}' rx='{u_badge_radius}' ry='{u_badge_radius}' fill='#888888' />"
    )?;
    writeln!(
        svg,
        r#"<text x="{}" y="{}" class="text-fc-ap-badge" text-anchor="middle" fill="white">U</text>"#,
        u_badge_x + u_badge_width / 2.0,
        u_badge_y + u_badge_height / 2.0 + 4.0
    )
}

fn write_main_rank_text(
    svg: &mut String,
    index: usize,
    card_width: u32,
    card_padding: f64,
    level_y: f64,
) -> std::fmt::Result {
    let rank_text = bn_rank_text(index);
    writeln!(
        svg,
        r#"<text x="{}" y="{:.1}" class="text-rank">{}</text>"#,
        f64::from(card_width) - card_padding,
        level_y + 2.0,
        rank_text
    )
}
