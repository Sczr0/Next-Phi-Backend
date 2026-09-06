use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{OnceLock, RwLock};

use serde::{Deserialize, Serialize};

use crate::error::AppError;

use super::background_layer::BACKGROUND_OVERLAY_DARK;
use super::song_background::select_song_background;
use super::song_illustration::resolve_song_illustration_href;
use super::template_shared::{
    BackgroundCtx, FontsCtx, JsonOverrideCacheEntry, PageCtx, clamp_template_id,
    read_json_override_cached, render_template, template_base_dir,
};
use super::template_song_card::{
    SongDiffCardCtx, SongDifficultyCardsLayout, build_song_difficulty_cards,
};
use super::text::escape_xml;
use super::time::updated_at_utc_text;
use super::{DEFAULT_PLAYER_NAME, MAIN_FONT_NAME, SongRenderData};

// ---------------- 单曲（Song）----------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub(super) struct SongTemplateLayout {
    pub width: u32,
    pub height: u32,
    pub padding: f64,
    pub footer_pad: f64,
}

impl Default for SongTemplateLayout {
    fn default() -> Self {
        // 高度 840 与手写路径一致：底部预留声明行 + generated 行 + 签名行。
        Self {
            width: 1400,
            height: 840,
            padding: 40.0,
            footer_pad: 30.0,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
struct SongCtx {
    page: PageCtx,
    fonts: FontsCtx,
    layout: SongTemplateLayout,
    background: BackgroundCtx,
    illust: IllustCtx,
    player: PlayerInfoCtx,
    song: SongTitleCtx,
    difficulty_cards: Vec<SongDiffCardCtx>,
    footer: SongFooterCtx,
}

#[derive(Debug, Clone, Serialize)]
struct IllustCtx {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    r: f64,
    clip_id: String,
    href_xml: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct PlayerInfoCtx {
    x: f64,
    y_name: f64,
    y_rks: f64,
    name_xml: String,
    rks_xml: String,
}

#[derive(Debug, Clone, Serialize)]
struct SongTitleCtx {
    cx: f64,
    y: f64,
    name_xml: String,
}

#[derive(Debug, Clone, Serialize)]
struct SongFooterCtx {
    /// 非官方声明行 baseline（整幅居中；None=不渲染）
    disclaimer_y: f64,
    disclaimer_xml: Option<String>,
    y: f64,
    pad: f64,
    text_xml: String,
}

static SONG_TEMPLATE_LAYOUT_JSON_CACHE: OnceLock<
    RwLock<HashMap<PathBuf, JsonOverrideCacheEntry<SongTemplateLayout>>>,
> = OnceLock::new();

pub(super) fn read_song_template_layout_override(cfg_path: &Path) -> Option<SongTemplateLayout> {
    let cache = SONG_TEMPLATE_LAYOUT_JSON_CACHE.get_or_init(|| RwLock::new(HashMap::new()));
    read_json_override_cached(cache, cfg_path)
}

pub(super) fn generate_song_svg_with_template(
    data: &SongRenderData,
    embed_images: bool,
    public_illustration_base_url: Option<&str>,
    template_id: Option<&str>,
) -> Result<String, AppError> {
    let template_id = clamp_template_id(template_id);
    let template_name = format!("song/{template_id}.svg.jinja");

    let mut layout = SongTemplateLayout::default();
    let cfg_path = template_base_dir()
        .join("song")
        .join(format!("{template_id}.json"));
    if let Some(v) = read_song_template_layout_override(&cfg_path) {
        layout = v;
    }

    let width = layout.width.max(1);
    let height = layout.height.max(1);

    let background_href = select_song_background(
        &data.song_id,
        embed_images,
        public_illustration_base_url,
        width,
        height,
    );

    // 布局：沿用现有“左曲绘 + 右四张难度卡”结构，但模板可重排。
    let padding = layout.padding;
    let player_info_height = 78.0;
    // 末尾 -120 与手写路径一致：加高的 40px 全部让给底栏（声明 + generated + 签名）。
    let illust_height = f64::from(height) - padding * 3.0 - player_info_height - 120.0;
    let mut illust_width = illust_height * (2048.0 / 1080.0);
    illust_width = illust_width.min(f64::from(width) * 0.60);

    let illust_x = padding;
    let illust_y = padding + player_info_height + padding;
    let illust_r = 18.0;
    let illust_href = resolve_song_illustration_href(
        data,
        embed_images,
        public_illustration_base_url,
        illust_width,
        illust_height,
    );

    // 玩家信息。
    let player_x = padding;
    let player_name = data.player_name.as_deref().unwrap_or(DEFAULT_PLAYER_NAME);
    let player_rks = updated_at_utc_text(&data.update_time, "%Y/%m/%d %H:%M:%S");

    // 曲名居中。
    let song_name_y = padding + player_info_height + 32.0;
    let song_name_cx = illust_x + illust_width / 2.0;

    // 难度卡片（4）。
    let card_area_width = f64::from(width) - illust_width - padding * 3.0;
    let card_w = card_area_width;
    let spacing_total = padding * 0.8 * 3.0;
    let card_h = (illust_height - spacing_total) / 4.0;
    let card_spacing = padding * 0.8;
    let cards_start_x = illust_x + illust_width + padding;
    let cards_start_y = illust_y;

    let difficulty_cards = build_song_difficulty_cards(
        data,
        SongDifficultyCardsLayout {
            start_x: cards_start_x,
            start_y: cards_start_y,
            card_w,
            card_h,
            card_spacing,
        },
    );

    let footer_text = data
        .custom_footer_text
        .as_deref()
        .unwrap_or("Generated by Phi-Backend");
    let disclaimer_text = data
        .disclaimer_text
        .as_deref()
        .filter(|txt| !txt.is_empty())
        .map(escape_xml);

    let ctx = SongCtx {
        page: PageCtx { width, height },
        fonts: FontsCtx {
            main: MAIN_FONT_NAME,
        },
        layout: layout.clone(),
        background: BackgroundCtx {
            href_xml: background_href.map(|s| escape_xml(&s)),
            overlay_rgba: BACKGROUND_OVERLAY_DARK,
        },
        illust: IllustCtx {
            x: illust_x,
            y: illust_y,
            w: illust_width,
            h: illust_height,
            r: illust_r,
            clip_id: "song-illust-clip".to_string(),
            href_xml: illust_href.map(|s| escape_xml(&s)),
        },
        player: PlayerInfoCtx {
            x: player_x,
            y_name: padding + 28.0,
            y_rks: padding + 55.0,
            name_xml: escape_xml(player_name),
            rks_xml: escape_xml(&player_rks),
        },
        song: SongTitleCtx {
            cx: song_name_cx,
            y: song_name_y,
            name_xml: escape_xml(&data.song_name),
        },
        difficulty_cards,
        footer: SongFooterCtx {
            // 与手写路径一致：声明行 height-74，generated 行 height-56，
            // 签名行由注入在 +18/+36 处接续。
            disclaimer_y: f64::from(height) - 74.0,
            disclaimer_xml: disclaimer_text,
            y: f64::from(height) - 56.0,
            pad: layout.footer_pad,
            text_xml: escape_xml(footer_text),
        },
    };

    render_template(&template_name, &ctx)
}
