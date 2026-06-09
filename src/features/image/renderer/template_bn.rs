use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{OnceLock, RwLock};

use rand::prelude::*;
use serde::{Deserialize, Serialize};

use crate::error::AppError;

use super::background_layer::{BACKGROUND_OVERLAY_DARK, BACKGROUND_OVERLAY_WHITE};
use super::bn_header_text::{build_bn_header_text, build_challenge_rank_inner_xml};
use super::engine;
use super::math::{u32_from_usize, usize_from_u32};
use super::resources::{get_blur_background_files, get_scaled_image_data_uri};
use super::score::to_engine_record;
use super::template_bn_card::{BnCardBuildCtx, CardCtx, build_bn_card};
use super::template_shared::{
    BackgroundCtx, ColorsCtx, FontsCtx, JsonOverrideCacheEntry, PageCtx, clamp_template_id,
    read_json_override_cached, render_template, template_base_dir,
};
use super::text::escape_xml;
use super::time::{generated_at_utc8_text, updated_at_utc_text};
use super::urls::get_image_href;
use super::{MAIN_FONT_NAME, PlayerStats, RenderRecord, Theme};

// ---------------- BestN（BN）----------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub(super) struct BnTemplateLayout {
    pub width: u32,
    pub header_height: u32,
    pub footer_height: u32,
    pub columns: u32,
    pub card_gap: u32,
    pub footer_padding: f64,
    pub song_name_max_width: usize,
    pub song_name_max_lines: usize,
}

impl Default for BnTemplateLayout {
    fn default() -> Self {
        Self {
            width: 1200,
            header_height: 120,
            footer_height: 50,
            columns: 3,
            card_gap: 12,
            footer_padding: 40.0,
            song_name_max_width: 28,
            song_name_max_lines: 1,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
struct HeaderRightLineCtx {
    y: f64,
    class: &'static str,
    /// `<text>` 内部内容：允许包含 `<tspan>` 等片段（内部已做 XML 转义）。
    inner_xml: String,
}

#[derive(Debug, Clone, Serialize)]
struct HeaderCtx {
    player_title_xml: String,
    ap_text_xml: String,
    bn_text_xml: String,
    right_lines: Vec<HeaderRightLineCtx>,
}

#[derive(Debug, Clone, Serialize)]
struct FooterCtx {
    y: f64,
    generated_text_xml: String,
    custom_text_xml: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct ApSectionCtx {
    section_y: u32,
    cards: Vec<CardCtx>,
}

#[derive(Debug, Clone, Serialize)]
struct BnTemplateCtx {
    page: PageCtx,
    fonts: FontsCtx,
    colors: ColorsCtx,
    layout: BnTemplateLayout,
    background: BackgroundCtx,
    header: HeaderCtx,
    ap: ApSectionCtx,
    cards: Vec<CardCtx>,
    footer: FooterCtx,
}

fn choose_random_blur_background(
    embed_images: bool,
    public_illustration_base_url: Option<&str>,
    width: u32,
    height: u32,
) -> Option<String> {
    let files = get_blur_background_files();
    if files.is_empty() {
        return None;
    }
    let mut rng = rand::thread_rng();
    let p = files.choose(&mut rng)?.clone();
    let href = get_image_href(&p, embed_images, public_illustration_base_url)?;

    // 若启用内嵌，则对大图做预缩放后再 data-uri（降低 resvg 解码压力）
    if embed_images && !href.starts_with("data:") {
        return get_scaled_image_data_uri(&p, width, height).or(Some(href));
    }
    Some(href)
}

static BN_TEMPLATE_LAYOUT_JSON_CACHE: OnceLock<
    RwLock<HashMap<PathBuf, JsonOverrideCacheEntry<BnTemplateLayout>>>,
> = OnceLock::new();

pub(super) fn read_bn_template_layout_override(cfg_path: &Path) -> Option<BnTemplateLayout> {
    let cache = BN_TEMPLATE_LAYOUT_JSON_CACHE.get_or_init(|| RwLock::new(HashMap::new()));
    read_json_override_cached(cache, cfg_path)
}

pub(super) fn generate_bn_svg_with_template<S>(
    scores: &[RenderRecord],
    stats: &PlayerStats,
    push_acc_map: Option<&HashMap<String, engine::PushAccHint, S>>,
    theme: Theme,
    embed_images: bool,
    public_illustration_base_url: Option<&str>,
    template_id: Option<&str>,
) -> Result<String, AppError>
where
    S: std::hash::BuildHasher,
{
    let template_id = clamp_template_id(template_id);
    let template_name = format!("bn/{template_id}.svg.jinja");

    // 布局参数（可选支持外部 JSON 覆盖：同名 .json）
    let mut layout = BnTemplateLayout::default();
    let cfg_path = template_base_dir()
        .join("bn")
        .join(format!("{template_id}.json"));
    if let Some(v) = read_bn_template_layout_override(&cfg_path) {
        layout = v;
    }

    let width = layout.width.max(1);
    let columns = layout.columns.max(1);
    let card_gap = layout.card_gap;
    let main_card_width = (width - card_gap * (columns + 1)) / columns;

    let engine_records_for_scores: Vec<engine::RksRecord> =
        scores.iter().filter_map(to_engine_record).collect();

    let (bg_grad_0, bg_grad_1, text, card_bg, card_stroke, text_secondary, fc_stroke) = match theme
    {
        Theme::White => (
            "#F7FAFF",
            "#ECEFF4",
            "#000000",
            "#ECEFF4".to_string(),
            "#D0D4DD",
            "#555555",
            "#4682B4",
        ),
        Theme::Black => (
            "#141826",
            "#252E48",
            "#FFFFFF",
            "#1A1E2A".to_string(),
            "#333848",
            "#BBBBBB",
            "#87CEEB",
        ),
    };

    let overlay_rgba = match theme {
        Theme::White => BACKGROUND_OVERLAY_WHITE,
        Theme::Black => BACKGROUND_OVERLAY_DARK,
    };

    let background_href =
        choose_random_blur_background(embed_images, public_illustration_base_url, width, width);

    // 头部文本
    let header_text = build_bn_header_text(stats);

    let mut right_lines = Vec::<HeaderRightLineCtx>::new();
    let mut info_y = 65.0;
    if let Some(data_str) = &stats.data_string {
        right_lines.push(HeaderRightLineCtx {
            y: info_y,
            class: "text-info",
            inner_xml: escape_xml(data_str),
        });
        info_y += 20.0;
    }
    if let Some((color, level)) = &stats.challenge_rank {
        let inner_xml = build_challenge_rank_inner_xml(color, level, text_secondary);
        right_lines.push(HeaderRightLineCtx {
            y: info_y,
            class: "text-info",
            inner_xml,
        });
        info_y += 20.0;
    }
    let update_time = updated_at_utc_text(&stats.update_time, "%Y/%m/%d %H:%M:%S");
    right_lines.push(HeaderRightLineCtx {
        y: info_y,
        class: "text-time",
        inner_xml: escape_xml(&update_time),
    });

    let card_ctx = BnCardBuildCtx {
        theme: &theme,
        embed_images,
        public_illustration_base_url,
        engine_records: engine_records_for_scores.as_slice(),
        push_acc_map,
        layout: &layout,
    };

    // AP Top 3
    let ap_section_start_y = layout.header_height + 15;
    let ap_card_start_y = card_gap;
    let mut ap_cards = Vec::<CardCtx>::new();
    let mut ap_row_max_h = 0u32;
    for (idx, score) in stats.ap_top_3_scores.iter().take(3).enumerate() {
        let x_pos = card_gap + u32_from_usize(idx) * (main_card_width + card_gap);
        let c = build_bn_card(
            score,
            idx,
            x_pos,
            ap_card_start_y,
            main_card_width,
            true,
            &card_ctx,
        );
        ap_row_max_h = ap_row_max_h.max(c.h);
        ap_cards.push(c);
    }
    let ap_section_height = if ap_cards.is_empty() {
        0
    } else {
        ap_card_start_y + ap_row_max_h + card_gap
    };

    // 主列表卡片
    let main_content_start_y = layout.header_height + ap_section_height + 15;
    let mut cards = Vec::<CardCtx>::new();
    let mut next_y = main_content_start_y + card_gap;
    let mut index = 0usize;
    while index < scores.len() {
        let mut row_cards = Vec::<CardCtx>::new();
        let mut row_max_h = 0u32;
        for col in 0..usize_from_u32(columns) {
            let idx = index + col;
            let Some(score) = scores.get(idx) else { break };
            let x = card_gap + u32_from_usize(col) * (main_card_width + card_gap);
            let c = build_bn_card(score, idx, x, 0, main_card_width, false, &card_ctx);
            row_max_h = row_max_h.max(c.h);
            row_cards.push(c);
        }
        for c in &mut row_cards {
            c.y = next_y;
        }
        cards.extend(row_cards);
        next_y = next_y.saturating_add(row_max_h).saturating_add(card_gap);
        index += usize_from_u32(columns);
    }

    let total_height = (next_y + layout.footer_height + 10).max(layout.header_height + 200);
    let footer_y = f64::from(total_height - layout.footer_height / 2 + 10);
    let generated_text = generated_at_utc8_text();
    let custom_text_xml = stats
        .custom_footer_text
        .as_ref()
        .and_then(|s| (!s.is_empty()).then_some(escape_xml(s)));

    let ctx = BnTemplateCtx {
        page: PageCtx {
            width,
            height: total_height,
        },
        fonts: FontsCtx {
            main: MAIN_FONT_NAME,
        },
        colors: ColorsCtx {
            bg_grad_0,
            bg_grad_1,
            text,
            text_secondary,
            card_bg,
            card_stroke,
            fc_stroke,
        },
        layout: layout.clone(),
        background: BackgroundCtx {
            href_xml: background_href.map(|s| escape_xml(&s)),
            overlay_rgba,
        },
        header: HeaderCtx {
            player_title_xml: escape_xml(&header_text.player_title),
            ap_text_xml: escape_xml(&header_text.ap_text),
            bn_text_xml: escape_xml(&header_text.bn_text),
            right_lines,
        },
        ap: ApSectionCtx {
            section_y: ap_section_start_y,
            cards: ap_cards,
        },
        cards,
        footer: FooterCtx {
            y: footer_y,
            generated_text_xml: escape_xml(&generated_text),
            custom_text_xml,
        },
    };

    render_template(&template_name, &ctx)
}
