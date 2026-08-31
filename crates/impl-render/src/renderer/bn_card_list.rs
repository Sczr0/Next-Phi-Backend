use std::collections::HashMap;
use std::fmt::Write;

use crate::error::AppError;
use crate::features::image::Theme;
use crate::rks_contract::engine;

use super::bn_card::{CardRenderInfo, generate_card_svg};
use super::bn_card_acc::pre_calculated_push_acc_for_score;
use super::bn_layout::BnLayout;
use super::math::u32_from_usize;
use super::score::to_engine_record;
use super::svg_error::svg_fmt_error;
use super::{PlayerStats, RenderRecord};

pub(super) struct BnCardSectionsRenderContext<'a, S>
where
    S: std::hash::BuildHasher,
{
    pub(super) svg: &'a mut String,
    pub(super) scores: &'a [RenderRecord],
    pub(super) stats: &'a PlayerStats,
    pub(super) push_acc_map: Option<&'a HashMap<String, engine::PushAccHint, S>>,
    pub(super) layout: &'a BnLayout,
    pub(super) theme: &'a Theme,
    pub(super) embed_images: bool,
    pub(super) public_illustration_base_url: Option<&'a str>,
    pub(super) started_at: &'a std::time::Instant,
}

pub(super) struct BnCardSectionsTiming {
    pub(super) after_ap_elapsed: std::time::Duration,
    pub(super) after_main_elapsed: std::time::Duration,
}

pub(super) fn write_score_sections<S>(
    ctx: BnCardSectionsRenderContext<'_, S>,
) -> Result<BnCardSectionsTiming, AppError>
where
    S: std::hash::BuildHasher,
{
    let BnCardSectionsRenderContext {
        svg,
        scores,
        stats,
        push_acc_map,
        layout,
        theme,
        embed_images,
        public_illustration_base_url,
        started_at,
    } = ctx;

    // 预先构建用于推分计算的 engine 记录，避免在每张卡片中重复转换。
    let engine_records_for_scores: Vec<engine::RksRecord> =
        scores.iter().filter_map(to_engine_record).collect();

    let ap_section_start_y = layout.header_height + 15;
    if !stats.ap_top_3_scores.is_empty() {
        writeln!(
            svg,
            r#"<g id="ap-top-3-section" transform="translate(0, {ap_section_start_y})">"#
        )
        .map_err(svg_fmt_error)?;
        for (idx, score) in stats.ap_top_3_scores.iter().take(3).enumerate() {
            let x_pos = layout.ap_card_padding_outer
                + u32_from_usize(idx) * (layout.main_card_width + layout.ap_card_padding_outer);

            let push_acc = pre_calculated_push_acc_for_score(score, push_acc_map);

            generate_card_svg(CardRenderInfo {
                svg,
                score,
                index: idx,
                card_x: x_pos,
                card_y: layout.ap_card_start_y,
                card_width: layout.main_card_width,
                is_ap_card: true,
                is_ap_score: true,
                pre_calculated_push_acc: push_acc,
                all_engine_records: engine_records_for_scores.as_slice(),
                theme,
                is_user_generated: stats.is_user_generated,
                embed_images,
                public_illustration_base_url,
            })?;
        }
        writeln!(svg, r"</g>").map_err(svg_fmt_error)?;
    }
    let after_ap_elapsed = started_at.elapsed();

    let main_content_start_y = layout.header_height + layout.ap_section_height + 15;
    for (index, score) in scores.iter().enumerate() {
        let idx_u32 = u32_from_usize(index);
        let row = idx_u32 / layout.columns;
        let col = idx_u32 % layout.columns;
        let x = layout.main_card_padding_outer
            + col * (layout.main_card_width + layout.main_card_padding_outer);
        let y = main_content_start_y
            + layout.main_card_padding_outer
            + row * (layout.calculated_card_height + layout.main_card_padding_outer);
        let is_ap_score = score.acc >= 100.0;

        let push_acc = pre_calculated_push_acc_for_score(score, push_acc_map);

        generate_card_svg(CardRenderInfo {
            svg,
            score,
            index,
            card_x: x,
            card_y: y,
            card_width: layout.main_card_width,
            is_ap_card: false,
            is_ap_score,
            pre_calculated_push_acc: push_acc,
            all_engine_records: engine_records_for_scores.as_slice(),
            theme,
            is_user_generated: stats.is_user_generated,
            embed_images,
            public_illustration_base_url,
        })?;
    }
    let after_main_elapsed = started_at.elapsed();

    Ok(BnCardSectionsTiming {
        after_ap_elapsed,
        after_main_elapsed,
    })
}
