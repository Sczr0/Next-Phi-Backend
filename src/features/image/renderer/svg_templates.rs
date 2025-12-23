use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use chrono::{FixedOffset, Utc};
use minijinja::Environment;
use rand::prelude::*;
use serde::{Deserialize, Serialize};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::config::AppConfig;
use crate::error::AppError;

use super::engine;
use super::{COVER_ASPECT_RATIO, MAIN_FONT_NAME, PlayerStats, RenderRecord, SongRenderData, Theme};

/// SVG 外部模板渲染入口（BestN / Song）。
///
/// 设计原则：
/// - 模板文件位于 `resources/templates/image/{kind}/{id}.svg.jinja`；
/// - Rust 负责：资源 href 选择、基础布局计算（卡片坐标/尺寸）、格式化与转义；
/// - 模板负责：卡片内部布局与字段排列（可自由调整）。
static TEMPLATE_ENV: OnceLock<Environment<'static>> = OnceLock::new();

fn template_base_dir() -> PathBuf {
    AppConfig::global()
        .resources_path()
        .join("templates")
        .join("image")
}

fn get_template_env() -> &'static Environment<'static> {
    TEMPLATE_ENV.get_or_init(|| {
        let mut env = Environment::new();
        env.set_loader(minijinja::path_loader(template_base_dir()));
        env
    })
}

fn render_template<T: Serialize>(template_name: &str, ctx: &T) -> Result<String, AppError> {
    let env = get_template_env();
    let tpl = env.get_template(template_name).map_err(|e| {
        AppError::ImageRendererError(format!("加载 SVG 模板失败（{template_name}）: {e}"))
    })?;
    tpl.render(ctx).map_err(|e| {
        AppError::ImageRendererError(format!("渲染 SVG 模板失败（{template_name}）: {e}"))
    })
}

fn now_utc8_string() -> String {
    let now_utc = Utc::now();
    let offset =
        FixedOffset::east_opt(8 * 3600).unwrap_or_else(|| FixedOffset::east_opt(0).unwrap());
    now_utc
        .with_timezone(&offset)
        .format("%Y/%m/%d %H:%M:%S")
        .to_string()
}

fn clamp_template_id(input: Option<&str>) -> &str {
    // 仅允许安全字符，避免目录穿越与 loader 意外行为。
    let Some(s) = input else { return "default" };
    if s.is_empty()
        || s.len() > 64
        || !s
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
    {
        return "default";
    }
    s
}

fn wrap_by_display_width(text: &str, max_width: usize, max_lines: usize) -> Vec<String> {
    if max_width == 0 || max_lines == 0 {
        return vec![text.to_string()];
    }

    let mut out = Vec::<String>::new();
    let mut current = String::new();
    let mut current_w = 0usize;

    for ch in text.chars() {
        if ch == '\n' {
            out.push(std::mem::take(&mut current));
            current_w = 0;
            if out.len() >= max_lines {
                return out;
            }
            continue;
        }

        let ch_w = UnicodeWidthChar::width(ch).unwrap_or(0).max(1);
        if current_w + ch_w > max_width && !current.is_empty() {
            out.push(std::mem::take(&mut current));
            current_w = 0;
            if out.len() >= max_lines {
                return out;
            }
        }
        current.push(ch);
        current_w += ch_w;
    }
    if !current.is_empty() || out.is_empty() {
        out.push(current);
    }
    out.truncate(max_lines);
    out
}

fn truncate_with_ellipsis(text: &str, max_width: usize) -> String {
    if max_width == 0 {
        return String::new();
    }
    if text.width() <= max_width {
        return text.to_string();
    }
    // 预留省略号宽度（按 1 计）
    let target = max_width.saturating_sub(1);
    let mut acc = String::new();
    let mut w = 0usize;
    for ch in text.chars() {
        let ch_w = UnicodeWidthChar::width(ch).unwrap_or(0).max(1);
        if w + ch_w > target {
            break;
        }
        acc.push(ch);
        w += ch_w;
    }
    acc.push('…');
    acc
}

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
struct PageCtx {
    width: u32,
    height: u32,
}

#[derive(Debug, Clone, Serialize)]
struct FontsCtx {
    main: &'static str,
}

#[derive(Debug, Clone, Serialize)]
struct ColorsCtx {
    bg_grad_0: &'static str,
    bg_grad_1: &'static str,
    text: &'static str,
    text_secondary: &'static str,
    card_bg: String,
    card_stroke: &'static str,
    fc_stroke: &'static str,
}

#[derive(Debug, Clone, Serialize)]
struct BackgroundCtx {
    href_xml: Option<String>,
    overlay_rgba: &'static str,
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
struct CardCtx {
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    radius: u32,
    class_extra: String,
    clip_id: String,
    cover: CoverCtx,
    badge: BadgeCtx,
    text: CardTextCtx,
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

fn resolve_cover_href_for_card(
    song_id: &str,
    embed_images: bool,
    public_illustration_base_url: Option<&str>,
    cover_w: f64,
    cover_h: f64,
) -> Option<String> {
    // 先走本地索引（启动期预热、O(1)）
    let metadata = super::get_cover_metadata_map();
    if let Some(path) = metadata.get(song_id) {
        let mut href = path.clone();
        if embed_images {
            let pb = PathBuf::from(&href);
            let w = cover_w.max(1.0).round() as u32;
            let h = cover_h.max(1.0).round() as u32;
            href = super::get_scaled_image_data_uri(&pb, w, h).unwrap_or(href);
        }
        if let Some(base) = public_illustration_base_url
            && !href.starts_with("data:")
        {
            let pb = PathBuf::from(&href);
            href = super::to_public_illustration_url(&pb, base).unwrap_or(href);
        }
        return Some(href);
    }

    // 本地缺失且启用了“外部曲绘基址”时，按约定生成远端 low-res URL
    public_illustration_base_url
        .map(|base| super::build_remote_illustration_low_res_url(base, song_id))
}

struct BnCardBuildCtx<'a> {
    theme: &'a Theme,
    embed_images: bool,
    public_illustration_base_url: Option<&'a str>,
    engine_records: &'a [engine::RksRecord],
    push_acc_map: Option<&'a HashMap<String, f64>>,
    layout: &'a BnTemplateLayout,
}

fn build_bn_card(
    score: &RenderRecord,
    index: usize,
    card_x: u32,
    card_y: u32,
    card_w: u32,
    is_ap_card: bool,
    ctx: &BnCardBuildCtx<'_>,
) -> CardCtx {
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

    let song_lines_count = song_lines.len().max(1) as f64;
    let text_block_height = text_line_height_song * song_lines_count
        + text_line_height_score
        + text_line_height_acc
        + text_line_height_level
        + text_block_spacing * (2.0 + song_lines_count);

    let cover_h = text_block_height;
    let cover_w = cover_h * COVER_ASPECT_RATIO;
    let card_h = (cover_h + card_padding * 2.0).round().max(1.0) as u32;
    let card_radius = 8u32;

    let cover_x = card_padding;
    let cover_y = card_padding;

    let cover_href = resolve_cover_href_for_card(
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

    // 难度徽章
    let (difficulty_text, difficulty_color) = match score.difficulty.as_str() {
        diff if diff.eq_ignore_ascii_case("EZ") => ("EZ", "#51AF44"),
        diff if diff.eq_ignore_ascii_case("HD") => ("HD", "#3173B3"),
        diff if diff.eq_ignore_ascii_case("IN") => ("IN", "#BE2D23"),
        diff if diff.eq_ignore_ascii_case("AT") => ("AT", "#383838"),
        _ => ("??", "#888888"),
    };
    let badge_w = 36.0;
    let badge_h = 20.0;
    let badge_r = 4.0;
    let badge_x = cover_x + 5.0;
    let badge_y = cover_y + cover_h - badge_h - 5.0;

    // AP/FC 徽章（AP 优先）
    let (ap_fill, fc_fill, ap_text_fill, fc_text_fill) = match ctx.theme {
        Theme::White => ("url(#ap-gradient-white)", "#4682B4", "white", "white"),
        Theme::Black => ("url(#ap-gradient)", "#87CEEB", "white", "white"),
    };
    let fc_ap_badge = if (score.acc - 100.0).abs() < f64::EPSILON {
        Some(BadgeMiniCtx {
            x: badge_x + badge_w + 5.0,
            y: badge_y,
            w: 30.0,
            h: 20.0,
            r: 4.0,
            tx: badge_x + badge_w + 5.0 + 15.0,
            ty: badge_y + 10.0 + 5.0,
            fill: ap_fill.to_string(),
            text_fill: ap_text_fill.to_string(),
            text: "AP".to_string(),
        })
    } else if score.is_fc {
        Some(BadgeMiniCtx {
            x: badge_x + badge_w + 5.0,
            y: badge_y,
            w: 30.0,
            h: 20.0,
            r: 4.0,
            tx: badge_x + badge_w + 5.0 + 15.0,
            ty: badge_y + 10.0 + 5.0,
            fill: fc_fill.to_string(),
            text_fill: fc_text_fill.to_string(),
            text: "FC".to_string(),
        })
    } else {
        None
    };

    // 推分 ACC（优先使用上游预计算，否则按 engine 计算）
    let push_acc = ctx
        .push_acc_map
        .and_then(|map| {
            let key = format!("{}-{}", score.song_id, score.difficulty);
            map.get(&key).copied()
        })
        .or_else(|| {
            let chart_id = format!("{}-{}", score.song_id, score.difficulty);
            super::calculate_push_acc(&chart_id, score.difficulty_value, ctx.engine_records)
        });

    let score_text = score
        .score
        .map(|s| format!("{:.0}", s.max(0.0).round()))
        .unwrap_or_else(|| "N/A".to_string());

    let mut acc_text = format!("Acc: {:.2}%", score.acc);
    if let Some(p) = push_acc {
        acc_text.push_str(&format!(" -> {p:.2}%"));
    }
    let level_text = format!("Lv.{:.1} -> {:.2}", score.difficulty_value, score.rks);

    let class_extra = if is_ap_card {
        "card-ap".to_string()
    } else if score.is_fc {
        "card-fc".to_string()
    } else {
        "".to_string()
    };

    CardCtx {
        x: card_x,
        y: card_y,
        w: card_w,
        h: card_h,
        radius: card_radius,
        class_extra,
        clip_id: format!(
            "cover-clip-{}-{}",
            if is_ap_card { "ap" } else { "main" },
            index
        ),
        cover: CoverCtx {
            x: cover_x,
            y: cover_y,
            w: cover_w,
            h: cover_h,
            href_xml: cover_href.map(|s| super::escape_xml(&s)),
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
                fill: difficulty_color.to_string(),
                text: difficulty_text.to_string(),
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
            rank_x: (card_w as f64) - card_padding,
            rank_y: level_y + 2.0,
            song_name_xml: super::escape_xml(&song_name_display),
            score_text,
            acc_text_xml: super::escape_xml(&acc_text),
            level_text_xml: super::escape_xml(&level_text),
            rank_text: format!("#{}", index + 1),
        },
    }
}

fn choose_random_blur_background(
    embed_images: bool,
    public_illustration_base_url: Option<&str>,
    width: u32,
    height: u32,
) -> Option<String> {
    let files = super::get_blur_background_files();
    if files.is_empty() {
        return None;
    }
    let mut rng = rand::thread_rng();
    let p = files.choose(&mut rng)?.clone();
    let href = super::get_image_href(&p, embed_images, public_illustration_base_url)?;

    // 若启用内嵌，则对大图做预缩放后再 data-uri（降低 resvg 解码压力）
    if embed_images && !href.starts_with("data:") {
        let pb = Path::new(&href);
        return super::get_scaled_image_data_uri(pb, width, height).or(Some(href));
    }
    Some(href)
}

pub(super) fn generate_bn_svg_with_template(
    scores: &[RenderRecord],
    stats: &PlayerStats,
    push_acc_map: Option<&HashMap<String, f64>>,
    theme: &Theme,
    embed_images: bool,
    public_illustration_base_url: Option<&str>,
    template_id: Option<&str>,
) -> Result<String, AppError> {
    let template_id = clamp_template_id(template_id);
    let template_name = format!("bn/{template_id}.svg.jinja");

    // 布局参数（可选支持外部 JSON 覆盖：同名 .json）
    let mut layout = BnTemplateLayout::default();
    let cfg_path = template_base_dir()
        .join("bn")
        .join(format!("{template_id}.json"));
    if cfg_path.exists()
        && let Ok(s) = std::fs::read_to_string(&cfg_path)
        && let Ok(v) = serde_json::from_str::<BnTemplateLayout>(&s)
    {
        layout = v;
    }

    let width = layout.width.max(1);
    let columns = layout.columns.max(1);
    let card_gap = layout.card_gap;
    let main_card_width = (width - card_gap * (columns + 1)) / columns;

    let engine_records_for_scores: Vec<engine::RksRecord> =
        scores.iter().filter_map(super::to_engine_record).collect();

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
        Theme::White => "rgba(247, 250, 255, 0.78)",
        Theme::Black => "rgba(20, 24, 38, 0.7)",
    };

    let background_href =
        choose_random_blur_background(embed_images, public_illustration_base_url, width, width);

    // 头部文本
    let player_name = stats.player_name.as_deref().unwrap_or("Phigros Player");
    let real_rks = stats.real_rks.unwrap_or(0.0);
    let player_title = format!("{}({:.6})", player_name, real_rks);
    let ap_text = match stats.ap_top_3_avg {
        Some(avg) => format!("AP Top 3 Avg: {avg:.4}"),
        None => "AP Top 3 Avg: N/A".to_string(),
    };
    let b27_avg_str = stats
        .best_27_avg
        .map_or("N/A".to_string(), |avg| format!("{avg:.4}"));
    let bn_text = format!("Best 27 Avg: {b27_avg_str}");

    let mut right_lines = Vec::<HeaderRightLineCtx>::new();
    let mut info_y = 65.0;
    if let Some(data_str) = &stats.data_string {
        right_lines.push(HeaderRightLineCtx {
            y: info_y,
            class: "text-info",
            inner_xml: super::escape_xml(data_str),
        });
        info_y += 20.0;
    }
    if let Some((color, level)) = &stats.challenge_rank {
        let color_hex = match color.as_str() {
            "Green" => "#51AF44",
            "Blue" => "#3173B3",
            "Red" => "#BE2D23",
            "Gold" => "#D1913C",
            "Rainbow" => "url(#ap-gradient)",
            _ => text_secondary,
        };
        let inner_xml = format!(
            "Challenge: <tspan fill=\"{}\">{}</tspan> {}",
            color_hex,
            super::escape_xml(color),
            super::escape_xml(level)
        );
        right_lines.push(HeaderRightLineCtx {
            y: info_y,
            class: "text-info",
            inner_xml,
        });
        info_y += 20.0;
    }
    let update_time = format!(
        "Updated at {} UTC",
        stats.update_time.format("%Y/%m/%d %H:%M:%S")
    );
    right_lines.push(HeaderRightLineCtx {
        y: info_y,
        class: "text-time",
        inner_xml: super::escape_xml(&update_time),
    });

    let card_ctx = BnCardBuildCtx {
        theme,
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
        let x_pos = card_gap + idx as u32 * (main_card_width + card_gap);
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
        for col in 0..columns as usize {
            let idx = index + col;
            let Some(score) = scores.get(idx) else { break };
            let x = card_gap + (col as u32) * (main_card_width + card_gap);
            let c = build_bn_card(score, idx, x, 0, main_card_width, false, &card_ctx);
            row_max_h = row_max_h.max(c.h);
            row_cards.push(c);
        }
        for c in row_cards.iter_mut() {
            c.y = next_y;
        }
        cards.extend(row_cards);
        next_y = next_y.saturating_add(row_max_h).saturating_add(card_gap);
        index += columns as usize;
    }

    let total_height = (next_y + layout.footer_height + 10).max(layout.header_height + 200);
    let footer_y = (total_height - layout.footer_height / 2 + 10) as f64;
    let generated_text = format!("Generated by Phi-Backend at {} UTC+8", now_utc8_string());
    let custom_text_xml = stats
        .custom_footer_text
        .as_ref()
        .and_then(|s| (!s.is_empty()).then_some(super::escape_xml(s)));

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
            href_xml: background_href.map(|s| super::escape_xml(&s)),
            overlay_rgba,
        },
        header: HeaderCtx {
            player_title_xml: super::escape_xml(&player_title),
            ap_text_xml: super::escape_xml(&ap_text),
            bn_text_xml: super::escape_xml(&bn_text),
            right_lines,
        },
        ap: ApSectionCtx {
            section_y: ap_section_start_y,
            cards: ap_cards,
        },
        cards,
        footer: FooterCtx {
            y: footer_y,
            generated_text_xml: super::escape_xml(&generated_text),
            custom_text_xml,
        },
    };

    render_template(&template_name, &ctx)
}

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
        Self {
            width: 1400,
            height: 800,
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
struct SongDiffCardCtx {
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

#[derive(Debug, Clone, Serialize)]
struct SongFooterCtx {
    y: f64,
    pad: f64,
    text_xml: String,
}

fn resolve_song_illust_href(
    data: &SongRenderData,
    embed_images: bool,
    public_illustration_base_url: Option<&str>,
    target_w: u32,
    target_h: u32,
) -> Option<String> {
    if let Some(p) = data.illustration_path.as_ref() {
        let href = super::get_image_href(p, embed_images, public_illustration_base_url)?;
        if embed_images && !href.starts_with("data:") {
            let pb = Path::new(&href);
            return super::get_scaled_image_data_uri(pb, target_w, target_h).or(Some(href));
        }
        return Some(href);
    }
    // 外部曲绘：按约定生成远端标准图（非 low-res）
    public_illustration_base_url
        .map(|base| super::build_remote_illustration_url(base, &data.song_id))
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
    if cfg_path.exists()
        && let Ok(s) = std::fs::read_to_string(&cfg_path)
        && let Ok(v) = serde_json::from_str::<SongTemplateLayout>(&s)
    {
        layout = v;
    }

    let width = layout.width.max(1);
    let height = layout.height.max(1);

    // 背景：优先用当前曲目曲绘（或随机封面）作为背景（与现实现接近）
    let cover_files = super::get_cover_files();
    let metadata = super::get_cover_metadata_map();
    let mut background_href = None::<String>;
    if let Some(path) = metadata.get(&data.song_id).cloned().map(PathBuf::from) {
        background_href = super::get_image_href(&path, embed_images, public_illustration_base_url);
    }
    if background_href.is_none() && !cover_files.is_empty() {
        let mut rng = rand::thread_rng();
        if let Some(p) = cover_files.choose(&mut rng) {
            background_href = super::get_image_href(p, embed_images, public_illustration_base_url);
        }
    }

    // 布局：沿用现有“左曲绘 + 右四张难度卡”结构，但模板可重排。
    let padding = layout.padding;
    let player_info_height = 78.0;
    let illust_height = (height as f64) - padding * 3.0 - player_info_height - 80.0;
    let mut illust_width = illust_height * (2048.0 / 1080.0);
    illust_width = illust_width.min((width as f64) * 0.60);

    let illust_x = padding;
    let illust_y = padding + player_info_height + padding;
    let illust_r = 18.0;
    let illust_target_w = illust_width.max(1.0).round() as u32;
    let illust_target_h = illust_height.max(1.0).round() as u32;
    let illust_href = resolve_song_illust_href(
        data,
        embed_images,
        public_illustration_base_url,
        illust_target_w,
        illust_target_h,
    );

    // 玩家信息
    let player_x = padding;
    let player_name = data.player_name.as_deref().unwrap_or("Phigros Player");
    let player_rks = format!(
        "Updated at {} UTC",
        data.update_time.format("%Y/%m/%d %H:%M:%S")
    );

    // 曲名居中
    let song_name_y = padding + player_info_height + 32.0;
    let song_name_cx = illust_x + illust_width / 2.0;

    // 难度卡片（4）
    let card_area_width = (width as f64) - illust_width - padding * 3.0;
    let card_w = card_area_width;
    let spacing_total = padding * 0.8 * 3.0;
    let card_h = (illust_height - spacing_total) / 4.0;
    let card_spacing = padding * 0.8;
    let cards_start_x = illust_x + illust_width + padding;
    let cards_start_y = illust_y;

    let mut difficulty_cards = Vec::<SongDiffCardCtx>::new();
    for (idx, key) in ["EZ", "HD", "IN", "AT"].into_iter().enumerate() {
        let pos_x = cards_start_x;
        let pos_y = cards_start_y + idx as f64 * (card_h + card_spacing);
        let r = 18.0;

        let content_padding = 18.0;
        let label_x = pos_x + content_padding + 35.0;
        let label_y = pos_y + card_h / 2.0;
        let constant_y = label_y + 20.0;

        let right_area_start = pos_x + 90.0;
        let text_x = right_area_start + 25.0;
        let score_y = pos_y + 40.0;
        let acc_y = pos_y + 65.0;
        let rks_y = pos_y + 88.0;

        let (score_text, acc_text, rks_text, constant_text, card_class) =
            match data.difficulty_scores.get(key).and_then(|o| o.as_ref()) {
                Some(score_data) if score_data.acc.is_some() => {
                    let score_text = score_data
                        .score
                        .map(|s| format!("{s:.0}"))
                        .unwrap_or_else(|| "N/A".to_string());
                    let acc_value = score_data.acc.unwrap_or(0.0);
                    let rks_value = score_data.rks.unwrap_or(0.0);
                    let dv_value = score_data.difficulty_value.unwrap_or(0.0);

                    let mut acc_text = format!("Acc: {acc_value:.2}%");
                    if let Some(push_acc) = score_data.player_push_acc {
                        acc_text.push_str(&format!(" -> {push_acc:.2}%"));
                    }
                    let rks_text = format!("Lv.{dv_value:.1} -> {rks_value:.2}");

                    let constant_text =
                        score_data.difficulty_value.map(|dv| format!("Lv. {dv:.1}"));

                    let card_class = if score_data.is_phi == Some(true) {
                        "difficulty-card-phi"
                    } else if score_data.is_fc == Some(true) {
                        "difficulty-card-fc"
                    } else {
                        "difficulty-card"
                    };

                    (
                        Some(score_text),
                        acc_text,
                        rks_text,
                        constant_text,
                        card_class.to_string(),
                    )
                }
                Some(score_data) => {
                    let constant_text =
                        score_data.difficulty_value.map(|dv| format!("Lv. {dv:.1}"));
                    (
                        None,
                        "Acc: N/A".to_string(),
                        "Lv.?? -> ??".to_string(),
                        constant_text,
                        "difficulty-card-inactive".to_string(),
                    )
                }
                None => (
                    None,
                    "Acc: N/A".to_string(),
                    "Lv.?? -> ??".to_string(),
                    None,
                    "difficulty-card-inactive".to_string(),
                ),
            };

        difficulty_cards.push(SongDiffCardCtx {
            key: key.to_string(),
            key_lower: key.to_lowercase(),
            x: pos_x,
            y: pos_y,
            w: card_w,
            h: card_h,
            r,
            card_class,
            label_x,
            label_y,
            constant_y,
            constant_text_xml: constant_text.map(|s| super::escape_xml(&s)),
            text_x,
            score_y,
            acc_y,
            rks_y,
            score_text,
            acc_text_xml: super::escape_xml(&acc_text),
            rks_text_xml: super::escape_xml(&rks_text),
            no_data_x: pos_x + card_w / 2.0,
            no_data_y: pos_y + card_h / 2.0,
            no_data_text: "无谱面".to_string(),
        });
    }

    let footer_text = data
        .custom_footer_text
        .as_deref()
        .unwrap_or("Generated by Phi-Backend");

    let ctx = SongCtx {
        page: PageCtx { width, height },
        fonts: FontsCtx {
            main: MAIN_FONT_NAME,
        },
        layout: layout.clone(),
        background: BackgroundCtx {
            href_xml: background_href.map(|s| super::escape_xml(&s)),
            overlay_rgba: "rgba(20, 24, 38, 0.7)",
        },
        illust: IllustCtx {
            x: illust_x,
            y: illust_y,
            w: illust_width,
            h: illust_height,
            r: illust_r,
            clip_id: "song-illust-clip".to_string(),
            href_xml: illust_href.map(|s| super::escape_xml(&s)),
        },
        player: PlayerInfoCtx {
            x: player_x,
            y_name: padding + 28.0,
            y_rks: padding + 55.0,
            name_xml: super::escape_xml(player_name),
            rks_xml: super::escape_xml(&player_rks),
        },
        song: SongTitleCtx {
            cx: song_name_cx,
            y: song_name_y,
            name_xml: super::escape_xml(&data.song_name),
        },
        difficulty_cards,
        footer: SongFooterCtx {
            y: (height as f64) - 18.0,
            pad: layout.footer_pad,
            text_xml: super::escape_xml(footer_text),
        },
    };

    render_template(&template_name, &ctx)
}
