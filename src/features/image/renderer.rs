use super::cover_loader;
use crate::config::AppConfig;
use crate::error::AppError;
use crate::features::image::Theme;
use crate::features::rks::engine;
use crate::features::save::models::Difficulty;
use base64::{Engine as _, engine::general_purpose::STANDARD as base64_engine}; // Added
use chrono::{DateTime, FixedOffset, Utc};
use image::ColorType;
use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use lru::LruCache;
use rand::prelude::*;
use resvg::usvg::{self, Options as UsvgOptions, fontdb};
use resvg::{
    render,
    tiny_skia::{Pixmap, Transform},
};
use std::collections::HashMap;
use std::fmt::Write;
use std::fs;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use tokio::task::spawn_blocking;

#[allow(dead_code)]
pub struct PlayerStats {
    pub ap_top_3_avg: Option<f64>,
    pub best_27_avg: Option<f64>,
    pub real_rks: Option<f64>,
    pub player_name: Option<String>,
    pub update_time: DateTime<Utc>,
    pub n: u32,                                   // 请求的 Best N 数量
    pub ap_top_3_scores: Vec<RenderRecord>,       // 添加 AP Top 3 的具体成绩
    pub challenge_rank: Option<(String, String)>, // 新增：课题等级 (颜色, 等级)
    pub data_string: Option<String>,              // 新增：格式化后的Data字符串
    pub custom_footer_text: Option<String>,
    pub is_user_generated: bool, // 新增：标记是否为用户生成
}

#[derive(Debug, Clone)]
pub struct RenderRecord {
    pub song_id: String,
    pub song_name: String,
    pub difficulty: String,
    pub score: Option<f64>,
    pub acc: f64,
    pub rks: f64,
    pub difficulty_value: f64,
    pub is_fc: bool,
}

// 新增：单曲成绩渲染所需数据结构
#[derive(Debug, Clone)]
pub struct SongDifficultyScore {
    pub score: Option<f64>,
    pub acc: Option<f64>,
    pub rks: Option<f64>,
    pub difficulty_value: Option<f64>,
    pub is_fc: Option<bool>,          // 可选：是否 Full Combo
    pub is_phi: Option<bool>,         // 可选：是否 Phi (ACC 100%)
    pub player_push_acc: Option<f64>, // 新增：玩家总RKS推分ACC
}

#[derive(Debug)]
pub struct SongRenderData {
    pub song_name: String,
    pub song_id: String, // 用于加载封面
    pub player_name: Option<String>,
    pub update_time: DateTime<Utc>,
    // 使用 HashMap 存储不同难度的成绩，Key 为 "EZ", "HD", "IN", "AT"
    pub difficulty_scores: HashMap<String, Option<SongDifficultyScore>>,
    // 歌曲插画路径 (用于渲染)
    pub illustration_path: Option<PathBuf>,
    /// 可选：右下角自定义文字
    pub custom_footer_text: Option<String>,
}

/// 排行榜渲染数据
#[derive(Debug, Clone)]
pub struct LeaderboardEntry {
    pub player_name: String,
    pub rks: f64,
}

#[allow(dead_code)]
pub struct LeaderboardRenderData {
    pub title: String,
    pub update_time: DateTime<Utc>,
    pub entries: Vec<LeaderboardEntry>,
    pub display_count: usize,
}

// 常量定义
const FONTS_DIR: &str = "resources/fonts";
const MAIN_FONT_NAME: &str = "思源黑体 CN";
const COVER_ASPECT_RATIO: f64 = 512.0 / 270.0;
#[allow(dead_code)]
const SONG_ILLUST_ASPECT_RATIO: f64 = 1.0; // 假设单曲图的插画是方形的

// 全局字体数据库单例
static GLOBAL_FONT_DB: OnceLock<Arc<fontdb::Database>> = OnceLock::new();

// 背景图片 LRU 缓存和封面文件列表的组合结构
// 注意：移除了重复的 HashSet，直接使用 HashMap 进行查找
type BackgroundAndCoverCache = (
    std::sync::Mutex<LruCache<PathBuf, String>>,
    Vec<PathBuf>,
    std::sync::Mutex<HashMap<String, String>>,
);
static BACKGROUND_AND_COVER_CACHE: OnceLock<BackgroundAndCoverCache> = OnceLock::new();
const BACKGROUND_CACHE_SIZE: usize = 10; // 缓存10张背景图片
const COVER_METADATA_CACHE_SIZE: usize = 10000; // 缓存封面元数据

// 背景主色反色缓存（避免重复解码大图）
static INVERSE_COLOR_CACHE: OnceLock<std::sync::Mutex<LruCache<PathBuf, String>>> = OnceLock::new();

fn get_inverse_color_cache() -> &'static std::sync::Mutex<LruCache<PathBuf, String>> {
    INVERSE_COLOR_CACHE
        .get_or_init(|| std::sync::Mutex::new(LruCache::new(NonZeroUsize::new(256).unwrap())))
}

// 预缩放图片 Data URI 缓存（键包含源路径与目标尺寸）
#[derive(Hash, Eq, PartialEq, Clone, Debug)]
struct ScaledImageKey {
    path: PathBuf,
    w: u32,
    h: u32,
}

static SCALED_IMAGE_CACHE: OnceLock<std::sync::Mutex<LruCache<ScaledImageKey, String>>> =
    OnceLock::new();
const SCALED_IMAGE_CACHE_SIZE: usize = 256;

fn get_scaled_image_cache() -> &'static std::sync::Mutex<LruCache<ScaledImageKey, String>> {
    SCALED_IMAGE_CACHE.get_or_init(|| {
        std::sync::Mutex::new(LruCache::new(
            NonZeroUsize::new(SCALED_IMAGE_CACHE_SIZE).unwrap(),
        ))
    })
}

/// 将磁盘图片按给定尺寸进行等比裁剪填充（相当于 xMidYMid slice），再编码为 JPEG 并返回 Data URI。
/// 结果加入 LRU 缓存以避免重复解码与缩放。
fn get_scaled_image_data_uri(path: &Path, target_w: u32, target_h: u32) -> Option<String> {
    if target_w == 0 || target_h == 0 {
        return None;
    }
    let key = ScaledImageKey {
        path: path.to_path_buf(),
        w: target_w,
        h: target_h,
    };
    if let Ok(mut cache) = get_scaled_image_cache().lock() {
        if let Some(uri) = cache.get(&key) {
            return Some(uri.clone());
        }
    }

    let speed = AppConfig::global().image.optimize_speed;
    let filter = if speed {
        FilterType::Triangle
    } else {
        FilterType::Lanczos3
    };
    let img = image::open(path).ok()?;
    let rgb = img.resize_to_fill(target_w, target_h, filter).to_rgb8();

    let mut out = Vec::new();
    let mut enc = JpegEncoder::new_with_quality(&mut out, 85);
    if enc
        .encode(&rgb, target_w, target_h, ColorType::Rgb8.into())
        .is_err()
    {
        return None;
    }
    let b64 = base64_engine.encode(out);
    let uri = format!("data:image/jpeg;base64,{b64}");
    if let Ok(mut cache) = get_scaled_image_cache().lock() {
        cache.put(key, uri.clone());
    }
    Some(uri)
}

/// 初始化全局字体数据库
fn init_global_font_db() -> Arc<fontdb::Database> {
    let mut font_db = fontdb::Database::new();
    font_db.load_system_fonts();

    // 加载自定义字体
    let fonts_dir = PathBuf::from(FONTS_DIR);
    if fonts_dir.exists() {
        if let Ok(entries) = fs::read_dir(&fonts_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file()
                    && (path.extension() == Some("ttf".as_ref())
                        || path.extension() == Some("otf".as_ref()))
                {
                    if let Err(e) = font_db.load_font_file(&path) {
                        tracing::error!("加载字体文件失败 '{}': {}", path.display(), e);
                    }
                }
            }
        }
    }

    Arc::new(font_db)
}

/// 获取全局字体数据库
pub fn get_global_font_db() -> Arc<fontdb::Database> {
    GLOBAL_FONT_DB.get_or_init(init_global_font_db).clone()
}

/// 初始化背景图片缓存和封面文件列表
fn init_background_and_cover_cache() -> BackgroundAndCoverCache {
    tracing::info!("初始化背景图片缓存和封面文件列表");

    // 初始 LRU 缓存（用于缓存背景图 data URI）
    let cache = std::sync::Mutex::new(LruCache::new(
        NonZeroUsize::new(BACKGROUND_CACHE_SIZE).unwrap(),
    ));

    // 封面元数据缓存：song_id -> 封面绝对路径
    let mut metadata_map = HashMap::<String, String>::with_capacity(COVER_METADATA_CACHE_SIZE);

    // 读取封面目录下的所有图片文件（包括 ill / illLow / illBlur 目录）
    let mut cover_files = Vec::new();

    // 读取 ill 目录（标准封面）
    let cover_base_path = cover_loader::covers_dir().join("ill");
    match fs::read_dir(&cover_base_path) {
        Ok(entries) => {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file()
                    && (path.extension() == Some("png".as_ref())
                        || path.extension() == Some("jpg".as_ref()))
                {
                    if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                        // 先记录标准封面，后续 illLow 可按需覆盖
                        metadata_map
                            .entry(stem.to_string())
                            .or_insert_with(|| path.to_string_lossy().to_string());
                    }
                    cover_files.push(path);
                }
            }
        }
        Err(e) => {
            tracing::error!("读取封面目录失败 '{}': {}", cover_base_path.display(), e);
        }
    }

    // 读取 illLow 目录（低分辨率封面，优先级更高：覆盖 ill）
    let cover_low_base_path = cover_loader::covers_dir().join("illLow");
    match fs::read_dir(&cover_low_base_path) {
        Ok(entries) => {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file()
                    && (path.extension() == Some("png".as_ref())
                        || path.extension() == Some("jpg".as_ref()))
                {
                    if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                        metadata_map.insert(stem.to_string(), path.to_string_lossy().to_string());
                    }
                    cover_files.push(path);
                }
            }
        }
        Err(e) => {
            tracing::warn!(
                "读取低分辨率封面目录失败 '{}': {}",
                cover_low_base_path.display(),
                e
            );
        }
    }

    // 读取 illBlur 目录（背景图片，只参与随机背景，不写入 metadata_map）
    let background_base_path = cover_loader::covers_dir().join("illBlur");
    match fs::read_dir(&background_base_path) {
        Ok(entries) => {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file()
                    && (path.extension() == Some("png".as_ref())
                        || path.extension() == Some("jpg".as_ref()))
                {
                    cover_files.push(path);
                }
            }
        }
        Err(e) => {
            tracing::error!(
                "读取背景目录失败 '{}': {}",
                background_base_path.display(),
                e
            );
        }
    }

    tracing::info!("初始化完成，共找到 {} 个封面文件", cover_files.len());

    (cache, cover_files, std::sync::Mutex::new(metadata_map))
}

/// 背景和封面缓存的类型别名
type BackgroundAndCoverCacheRefs = (
    &'static std::sync::Mutex<LruCache<PathBuf, String>>,
    &'static Vec<PathBuf>,
    &'static std::sync::Mutex<HashMap<String, String>>,
);

/// 获取背景图片缓存和封面文件列表
fn get_background_and_cover_cache() -> BackgroundAndCoverCacheRefs {
    let (cache, files, metadata) =
        BACKGROUND_AND_COVER_CACHE.get_or_init(init_background_and_cover_cache);
    (cache, files, metadata)
}

/// 获取背景图片缓存
pub fn get_background_cache() -> &'static std::sync::Mutex<LruCache<PathBuf, String>> {
    let (cache, _, _) = get_background_and_cover_cache();
    cache
}

/// 获取封面文件列表
pub fn get_cover_files() -> &'static Vec<PathBuf> {
    let (_, files, _) = get_background_and_cover_cache();
    files
}

/// 获取封面元数据缓存
pub fn get_cover_metadata_cache() -> &'static std::sync::Mutex<HashMap<String, String>> {
    let (_, _, metadata) = get_background_and_cover_cache();
    metadata
}

/// 从缓存或磁盘加载背景图片
/// 注意：现在只缓存小图（<256KB），大图直接返回路径
fn get_background_image(path: &PathBuf) -> Option<String> {
    let mut cache = get_background_cache().lock().unwrap();

    // 尝试从缓存中获取
    if let Some(cached_image) = cache.get(path) {
        return Some(cached_image.clone());
    }

    // 缓存未命中，从磁盘加载
    if let Ok(data) = fs::read(path) {
        let file_size = data.len();

        // 只对小于 256KB 的图片进行 Base64 编码并缓存
        // 大图片直接返回路径，避免内存膨胀
        if file_size <= 256 * 1024 {
            let mime_type = if path.extension().is_some_and(|ext| ext == "png") {
                "image/png"
            } else {
                "image/jpeg"
            };
            let base64_encoded = base64_engine.encode(&data);
            let image_data = format!("data:{mime_type};base64,{base64_encoded}");
            cache.put(path.clone(), image_data.clone());
            return Some(image_data);
        }

        // 大图片直接返回文件路径
        return Some(path.to_string_lossy().into_owned());
    }

    None
}

/// 返回适合放入 <image href> 的引用：小图返回 data URI，大图返回文件路径
fn get_image_href(path: &PathBuf, embed_images: bool) -> Option<String> {
    // 当 embed_images=false 时，总是返回文件路径，避免小图内嵌 data URI（减轻 usvg 解析和内存压力）
    if !embed_images {
        if path.exists() {
            return Some(path.to_string_lossy().into_owned());
        } else {
            // 兜底：路径异常时尝试小图内嵌，尽量保证可渲染
            return get_background_image(path);
        }
    }
    // 需要显式内嵌时，复用小图内嵌逻辑（大图直接返回路径）
    get_background_image(path)
}

fn to_engine_difficulty(code: &str) -> Option<Difficulty> {
    match code {
        "EZ" | "ez" => Some(Difficulty::EZ),
        "HD" | "hd" => Some(Difficulty::HD),
        "IN" | "in" => Some(Difficulty::IN),
        "AT" | "at" => Some(Difficulty::AT),
        _ => None,
    }
}

fn to_engine_record(record: &RenderRecord) -> Option<engine::RksRecord> {
    let difficulty = to_engine_difficulty(&record.difficulty)?;
    let score = record
        .score
        .map(|s| s.max(0.0).round() as u32)
        .unwrap_or_default();
    Some(engine::RksRecord {
        song_id: record.song_id.clone(),
        difficulty,
        score,
        acc: record.acc,
        rks: record.rks,
        chart_constant: record.difficulty_value,
    })
}

fn calculate_push_acc(
    target_chart_id: &str,
    difficulty_value: f64,
    engine_records: &[engine::RksRecord],
) -> Option<f64> {
    if engine_records.is_empty() {
        return None;
    }
    engine::calculate_target_chart_push_acc(target_chart_id, difficulty_value, engine_records)
}

// Helper function to generate a single score card SVG group
struct CardRenderInfo<'a> {
    svg: &'a mut String,
    score: &'a RenderRecord,
    index: usize,
    card_x: u32,
    card_y: u32,
    card_width: u32,
    is_ap_card: bool,
    is_ap_score: bool,
    pre_calculated_push_acc: Option<f64>,
    all_engine_records: &'a [engine::RksRecord],
    theme: &'a Theme,
    is_user_generated: bool, // 新增
    embed_images: bool,
}

fn generate_card_svg(info: CardRenderInfo) -> Result<(), AppError> {
    let fmt_err = |e| AppError::ImageRendererError(format!("SVG formatting error: {e}"));

    // Destructure for convenience
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
    } = info;

    // --- Card Dimensions & Layout ---
    let card_padding = 10.0; // Inner padding
    let text_line_height_song = 22.0;
    let text_line_height_score = 30.0;
    let text_line_height_acc = 18.0;
    let text_line_height_level = 18.0;
    let text_block_spacing = 4.0; // Spacing between text lines

    // Calculate text block height (approximate)
    let text_block_height = text_line_height_song
        + text_line_height_score
        + text_line_height_acc
        + text_line_height_level
        + text_block_spacing * 3.0;

    let cover_size_h = text_block_height;
    let cover_size_w = cover_size_h * COVER_ASPECT_RATIO;
    let card_height = (cover_size_h + card_padding * 2.0) as u32;
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

    writeln!(svg, r#"<g transform="translate({card_x}, {card_y})">"#).map_err(fmt_err)?;

    // Card background rectangle
    writeln!(svg, r#"<rect width="{card_width}" height="{card_height}" rx="{card_radius}" ry="{card_radius}" class="{card_class}" />"#).map_err(fmt_err)?;

    // --- Card Content ---
    // Define clip path for rounded cover
    let clip_path_id = format!(
        "cover-clip-{}-{}",
        if is_ap_card { "ap" } else { "main" },
        index
    );
    writeln!(svg, "<defs><clipPath id=\"{clip_path_id}\"><rect x=\"{cover_x}\" y=\"{cover_y}\" width=\"{cover_size_w:.1}\" height=\"{cover_size_h:.1}\" rx=\"4\" ry=\"4\" /></clipPath></defs>").map_err(fmt_err)?;

    // Cover Image or Placeholder
    // 使用预构建的封面元数据缓存，避免运行时文件系统调用
    let metadata_cache = get_cover_metadata_cache();
    let cover_href = {
        let cache = metadata_cache.lock().unwrap();
        cache.get(&score.song_id).cloned()
    };

    if let Some(href) = cover_href {
        let final_href = if embed_images {
            let pb = PathBuf::from(&href);
            let w = cover_size_w.max(1.0).round() as u32;
            let h = cover_size_h.max(1.0).round() as u32;
            get_scaled_image_data_uri(&pb, w, h).unwrap_or(href)
        } else {
            href
        };
        let escaped_href = escape_xml(&final_href);
        writeln!(svg, r#"<image href="{escaped_href}" x="{cover_x}" y="{cover_y}" width="{cover_size_w:.1}" height="{cover_size_h:.1}" clip-path="url(#{clip_path_id})" />"#).map_err(fmt_err)?;
    }

    // Text content positioning
    let text_x = cover_x + cover_size_w + 15.0; // Padding between cover and text
    let text_width = (card_width as f64) - text_x - card_padding; // Available width for text

    // 新增一个垂直偏移量，用于微调文本块的整体位置
    // 可以调整这个值，直到视觉效果满意为止。数值越大，文本越往下。
    let vertical_text_offset = 5.0;

    // Calculate Y positions for text lines to align with cover
    let song_name_y = cover_y + text_line_height_song * 0.75 + vertical_text_offset;
    let score_y = song_name_y + text_line_height_score * 0.8 + text_block_spacing + 2.0; // 分数部分向下移动2像素
    let acc_y = score_y + text_line_height_acc + text_block_spacing;
    let level_y = acc_y + text_line_height_level + text_block_spacing;

    // --- Song Name (智能判断是否需要压缩) ---

    // 1. 定义一个简单的函数来判断字符是否为全角（主要针对中日韩字符）
    fn is_full_width(ch: char) -> bool {
        // 这个范围覆盖了常见的中日韩统一表意文字、平假名、片假名和全角符号
        ('\u{4E00}'..='\u{9FFF}').contains(&ch) || // CJK Unified Ideographs
    ('\u{3040}'..='\u{30FF}').contains(&ch) || // Hiragana and Katakana
    ('\u{FF00}'..='\u{FFEF}').contains(&ch) // Full-width forms
    }

    // 2. 估算文本渲染后的大致宽度
    let mut estimated_width = 0.0;
    // 根据CSS样式，.text-songname 的 font-size 是 19px。
    // 全角字符宽度约等于字号，半角字符宽度约为一半。这里我们用稍大的值做估算。
    let full_width_char_px = 19.0;
    let half_width_char_px = 10.5; // 英文、数字等半角字符的平均宽度估值

    for ch in score.song_name.chars() {
        if is_full_width(ch) {
            estimated_width += full_width_char_px;
        } else {
            estimated_width += half_width_char_px;
        }
    }

    // 3. 根据估算结果，决定是否启用SVG压缩
    let song_name_escaped = escape_xml(&score.song_name);

    if estimated_width > text_width {
        // 估算宽度超过了可用空间，启用 textLength 进行压缩
        writeln!(
        svg,
        r#"<text x="{text_x}" y="{song_name_y:.1}" class="text-songname" textLength="{text_width:.1}" lengthAdjust="spacingAndGlyphs">{song_name_escaped}</text>"#
    ).map_err(fmt_err)?;
    } else {
        // 估算宽度足够，正常渲染，不压缩也不拉伸
        writeln!(
        svg,
        r#"<text x="{text_x}" y="{song_name_y:.1}" class="text-songname">{song_name_escaped}</text>"#
    ).map_err(fmt_err)?;
    }

    // Score
    let score_text = score.score.map_or("N/A".to_string(), |s| format!("{s:.0}"));
    writeln!(
        svg,
        r#"<text x="{text_x}" y="{score_y:.1}" class="text-score">{score_text}</text>"#
    )
    .map_err(fmt_err)?;

    // 如果是用户生成的数据，在分数旁边添加 "U" 标签
    if is_user_generated {
        // 方案: 将 "U" 标签放在序号的左边
        let u_badge_width = 18.0;
        let u_badge_height = 18.0;
        let u_badge_radius = 4.0;

        // 序号的 x 坐标是 card_width - card_padding
        // 我们将 U 标签放在序号左边，并留出一些间距
        let rank_text_approx_width = 30.0; // 估算 "#10" 这种文本的宽度
        let u_badge_x =
            (card_width as f64) - card_padding - rank_text_approx_width - u_badge_width - 5.0;
        let u_badge_y = level_y - u_badge_height + 4.0; // 与序号的基线对齐 (向下微调2px)

        writeln!(svg, r#"<rect x='{u_badge_x}' y='{u_badge_y}' width='{u_badge_width}' height='{u_badge_height}' rx='{u_badge_radius}' ry='{u_badge_radius}' fill='#888888' />"#).map_err(fmt_err)?;
        writeln!(svg, r#"<text x="{}" y="{}" class="text-fc-ap-badge" text-anchor="middle" fill="white">U</text>"#, u_badge_x + u_badge_width / 2.0, u_badge_y + u_badge_height / 2.0 + 4.0).map_err(fmt_err)?;
    }

    // Accuracy (带推分acc)
    let acc_text = if !is_ap_score && score.acc < 100.0 && score.difficulty_value > 0.0 {
        // 只有定数>0时才显示推分
        // 如果有预计算的推分ACC，优先使用
        let push_acc = if let Some(pa) = pre_calculated_push_acc {
            pa
        } else {
            // 否则使用新算法计算
            let target_chart_id = format!("{}-{}", score.song_id, score.difficulty);
            calculate_push_acc(&target_chart_id, score.difficulty_value, all_engine_records)
                .unwrap_or(100.0) // 如果计算失败（比如格式错误），则默认为100
        };

        // 如果推分acc非常接近100，直接显示 -> 100.00%
        if push_acc > 99.995 {
            format!(
                "Acc: {:.2}% <tspan class='push-acc'>-> 100.00%</tspan>",
                score.acc
            )
        }
        // 如果两者差值非常小(小于0.005，对应四舍五入后两位不变)，则展示三位小数
        else if (push_acc - score.acc).abs() < 0.005 {
            format!(
                "Acc: {:.2}% <tspan class='push-acc'>-> {:.3}%</tspan>",
                score.acc, push_acc
            )
        } else {
            format!(
                "Acc: {:.2}% <tspan class='push-acc'>-> {:.2}%</tspan>",
                score.acc, push_acc
            )
        }
    } else {
        // AP或者已满分或者定数为0，只显示当前acc
        format!("Acc: {:.2}%", score.acc)
    };
    writeln!(
        svg,
        r#"<text x="{text_x}" y="{acc_y:.1}" class="text-acc">{acc_text}</text>"#
    )
    .map_err(fmt_err)?;

    // Level & RKS
    // 获取难度标签文本和颜色
    let (difficulty_text, difficulty_color) = match &score.difficulty {
        diff if diff.eq_ignore_ascii_case("EZ") => ("EZ", "#51AF44"), // 绿色
        diff if diff.eq_ignore_ascii_case("HD") => ("HD", "#3173B3"), // 蓝色
        diff if diff.eq_ignore_ascii_case("IN") => ("IN", "#BE2D23"), // 红色
        diff if diff.eq_ignore_ascii_case("AT") => ("AT", "#383838"), // 深灰色
        _ => ("??", "#888888"),                                       // 默认灰色
    };

    // 难度标签尺寸
    let badge_width = 36.0;
    let badge_height = 20.0;
    let badge_radius = 4.0;
    // 将标签放置在曲绘左下角
    let badge_x = cover_x + 5.0; // 曲绘左侧留出5px边距
    let badge_y = cover_y + cover_size_h - badge_height - 5.0; // 曲绘底部留出5px边距

    // 绘制难度标签背景
    writeln!(svg, r#"<rect x="{badge_x}" y="{badge_y:.1}" width="{badge_width:.1}" height="{badge_height:.1}" rx="{badge_radius:.1}" ry="{badge_radius:.1}" fill="{difficulty_color}" />"#).map_err(fmt_err)?;

    // 绘制难度标签文本
    let badge_text_x = badge_x + badge_width / 2.0;
    let badge_text_y = badge_y + badge_height / 2.0 + 5.0; // 垂直居中
    writeln!(svg, r#"<text x="{badge_text_x:.1}" y="{badge_text_y:.1}" class="text-difficulty-badge" text-anchor="middle" fill="white">{difficulty_text}</text>"#).map_err(fmt_err)?;

    // FC/AP标签尺寸（AP 与 FC 互斥：有 AP 仅显示 AP，否则若 FC 则显示 FC）
    let fc_ap_badge_width = 30.0;
    let fc_ap_badge_height = 20.0;
    let fc_ap_badge_radius = 4.0;
    let fc_ap_badge_spacing = 5.0;
    let (ap_badge_fill, fc_badge_fill, ap_text_fill, fc_text_fill) = match theme {
        Theme::White => ("url(#ap-gradient-white)", "#4682B4", "white", "white"),
        Theme::Black => ("url(#ap-gradient)", "#87CEEB", "white", "white"),
    };

    if score.acc == 100.0 {
        // 仅显示 AP
        let ap_badge_x = badge_x + badge_width + fc_ap_badge_spacing;
        let ap_badge_y = badge_y;
        let ap_badge_color = ap_badge_fill;
        writeln!(svg, r#"<rect x="{ap_badge_x}" y="{ap_badge_y:.1}" width="{fc_ap_badge_width:.1}" height="{fc_ap_badge_height:.1}" rx="{fc_ap_badge_radius:.1}" ry="{fc_ap_badge_radius:.1}" fill="{ap_badge_color}" />"#).map_err(fmt_err)?;
        let ap_badge_text_x = ap_badge_x + fc_ap_badge_width / 2.0;
        let ap_badge_text_y = ap_badge_y + fc_ap_badge_height / 2.0 + 5.0;
        writeln!(svg, r#"<text x="{ap_badge_text_x:.1}" y="{ap_badge_text_y:.1}" class="text-fc-ap-badge" text-anchor="middle" fill="{ap_text_fill}">AP</text>"#).map_err(fmt_err)?;
    } else if score.is_fc {
        // 仅显示 FC
        let fc_badge_x = badge_x + badge_width + fc_ap_badge_spacing;
        let fc_badge_y = badge_y;
        let fc_badge_color = fc_badge_fill;
        writeln!(svg, r#"<rect x="{fc_badge_x}" y="{fc_badge_y:.1}" width="{fc_ap_badge_width:.1}" height="{fc_ap_badge_height:.1}" rx="{fc_ap_badge_radius:.1}" ry="{fc_ap_badge_radius:.1}" fill="{fc_badge_color}" />"#).map_err(fmt_err)?;
        let fc_badge_text_x = fc_badge_x + fc_ap_badge_width / 2.0;
        let fc_badge_text_y = fc_badge_y + fc_ap_badge_height / 2.0 + 5.0;
        writeln!(svg, r#"<text x="{fc_badge_text_x:.1}" y="{fc_badge_text_y:.1}" class="text-fc-ap-badge" text-anchor="middle" fill="{fc_text_fill}">FC</text>"#).map_err(fmt_err)?;
    }

    // 恢复等级和RKS的简单字符串拼接
    // 修复定数显示的浮点误差：限定为 1 位小数
    let level_text = format!("Lv.{:.1} -> {:.2}", score.difficulty_value, score.rks);
    writeln!(
        svg,
        r#"<text x="{text_x}" y="{level_y:.1}" class="text-level">{level_text}</text>"#
    )
    .map_err(fmt_err)?;

    // Rank (Only for main scores, not AP)
    if !is_ap_card {
        let rank_text = format!("#{}", index + 1);
        writeln!(
            svg,
            r#"<text x="{}" y="{:.1}" class="text-rank">{}</text>"#,
            (card_width as f64) - card_padding,
            level_y + 2.0,
            rank_text
        )
        .map_err(fmt_err)?;
    }

    writeln!(svg, "</g>").map_err(fmt_err)?; // End card group
    Ok(())
}

// --- SVG 生成函数 ---

pub fn generate_svg_string(
    scores: &[RenderRecord],
    stats: &PlayerStats,
    push_acc_map: Option<&HashMap<String, f64>>, // 新增：预先计算的推分ACC映射，键为"曲目ID-难度"
    theme: &Theme,                               // 新增：主题参数
    embed_images: bool,
) -> Result<String, AppError> {
    let _start_time = std::time::Instant::now();
    // ... (width, height calculations etc. - keep these as they were) ...
    let width = 1200;
    let header_height = 120;
    let _ap_title_height = 50; // Prefix unused variable
    let footer_height = 50;
    let main_card_padding_outer = 12;
    let ap_card_padding_outer = 12;
    let columns = 3;

    let main_card_width = (width - main_card_padding_outer * (columns + 1)) / columns;
    let card_padding_inner = 10.0;
    let text_line_height_song = 22.0;
    let text_line_height_score = 30.0;
    let text_line_height_acc = 18.0;
    let text_line_height_level = 18.0;
    let text_block_spacing = 4.0;
    let text_block_height = text_line_height_song
        + text_line_height_score
        + text_line_height_acc
        + text_line_height_level
        + text_block_spacing * 3.0;
    let calculated_card_height = (text_block_height + card_padding_inner * 2.0) as u32;
    let ap_card_start_y = ap_card_padding_outer;
    let ap_section_height = if !stats.ap_top_3_scores.is_empty() {
        ap_card_start_y + calculated_card_height + ap_card_padding_outer
    } else {
        0
    };
    let rows = (scores.len() as u32).div_ceil(columns);
    let content_height = (calculated_card_height + main_card_padding_outer) * rows.max(1);
    let total_height = header_height + ap_section_height + content_height + footer_height + 10;

    // 预先构建用于推分计算的 engine 记录，避免在每张卡片中重复转换
    let engine_records_for_scores: Vec<engine::RksRecord> =
        scores.iter().filter_map(to_engine_record).collect();

    // 根据主题定义颜色变量
    let (
        bg_color,
        text_color,
        card_bg_color,
        card_stroke_color,
        text_secondary_color,
        fc_stroke_color,
        ap_stroke_color,
    ) = match theme {
        Theme::White => (
            "#F7FAFF",
            "#000000",
            "#ECEFF4",
            "#D0D4DD",
            "#555555",
            "#4682B4",
            "url(#ap-gradient)",
        ),
        Theme::Black => (
            "#141826",
            "#FFFFFF",
            "#1A1E2A",
            "#333848",
            "#BBBBBB",
            "#87CEEB",
            "url(#ap-gradient)",
        ),
    };
    let (ap_card_fill, fc_card_fill) = match theme {
        Theme::White => ("#FFFBEB".to_string(), "#E6F2FF".to_string()),
        Theme::Black => (card_bg_color.to_string(), card_bg_color.to_string()),
    };

    let mut normal_card_stroke_color = match theme {
        Theme::White => "url(#normal-card-stroke-gradient)".to_string(),
        Theme::Black => "#252A38".to_string(), // Weaker border for black theme
    };
    // 预分配 SVG 字符串容量，减少多次分配与拷贝
    let mut svg = String::with_capacity(200_000);
    let t0 = std::time::Instant::now();
    let fmt_err = |e| AppError::ImageRendererError(format!("SVG formatting error: {e}"));

    // --- 获取随机背景图 ---
    let mut background_image_href = None;
    let _background_fill = "url(#bg-gradient)".to_string(); // Prefix unused variable

    // 使用预先缓存的封面文件列表来获取背景图片，避免重复读取目录
    let background_files = get_cover_files();
    let background_base_path = cover_loader::covers_dir().join("illBlur");
    let filtered_background_files: Vec<&PathBuf> = background_files
        .iter()
        .filter(|path| {
            // 检查路径是否在 illBlur 目录下且是图片文件
            path.starts_with(&background_base_path)
                && (path.extension() == Some("png".as_ref())
                    || path.extension() == Some("jpg".as_ref()))
        })
        .collect();

    if !filtered_background_files.is_empty() {
        let mut rng = rand::thread_rng();
        if let Some(random_path) = filtered_background_files.choose(&mut rng) {
            // 随机选择一个路径
            // --- 新增：计算背景主色的反色 ---
            if let Theme::White = theme {
                if let Some(inverse_color) = get_inverse_color_from_path_cached(random_path) {
                    normal_card_stroke_color = inverse_color;
                    tracing::info!("使用背景反色作为卡片边框: {normal_card_stroke_color}");
                }
            }
            // --- 结束新增 ---

            // 使用缓存函数获取背景图片
            if let Some(image_href) = get_image_href(random_path, embed_images) {
                background_image_href = Some(image_href);
                // 若需内嵌，则将背景预缩放为 Data URI，避免 resvg 进行大图缩放
                if embed_images {
                    if let Some(ref href_str) = background_image_href {
                        if !href_str.starts_with("data:") {
                            if let Some(uri) =
                                get_scaled_image_data_uri(random_path, width, total_height)
                            {
                                background_image_href = Some(uri);
                            }
                        }
                    }
                }
                tracing::info!("使用随机背景图: {}", random_path.display());
            } else {
                tracing::error!("获取背景图片失败: {}", random_path.display());
                // 获取失败则回退到渐变
            }
        } else {
            tracing::warn!("无法从背景文件列表中随机选择一个");
            // Fallback to gradient if choose fails (shouldn't happen with non-empty list)
        }
    } else {
        tracing::warn!("找不到任何背景文件用于随机背景");
        // Fallback to gradient if directory is empty or read failed
    }
    // --- 背景图获取结束 ---

    writeln!(
        svg,
        r#"<svg width="{width}" height="{total_height}" viewBox="0 0 {width} {total_height}" xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink">"#
    ).map_err(fmt_err)?;

    // --- Definitions (Styles, Gradients, Filters, Font) ---
    writeln!(svg, "<defs>").map_err(fmt_err)?;

    // Background Gradient (Fallback)
    match theme {
        Theme::White => {
            writeln!(svg, r#"<linearGradient id="bg-gradient" x1="0%" y1="0%" x2="100%" y2="100%"><stop offset="0%" style="stop-color:#F7FAFF" /><stop offset="100%" style="stop-color:#ECEFF4" /></linearGradient>"#).map_err(fmt_err)?;
        }
        Theme::Black => {
            writeln!(svg, r#"<linearGradient id="bg-gradient" x1="0%" y1="0%" x2="100%" y2="100%"><stop offset="0%" style="stop-color:#141826" /><stop offset="100%" style="stop-color:#252E48" /></linearGradient>"#).map_err(fmt_err)?;
        }
    }

    // Shadow Filter Definition
    writeln!(svg, r#"<filter id="card-shadow" x="-10%" y="-10%" width="120%" height="130%"><feDropShadow dx="0" dy="3" stdDeviation="3" flood-color="rgba(0,0,0,0.25)" flood-opacity="0.25" /></filter>"#).map_err(fmt_err)?;

    // FC Glow Filter Definition
    writeln!(svg, r#"<filter id="fc-glow" x="-50%" y="-50%" width="200%" height="200%"><feDropShadow dx="0" dy="0" stdDeviation="4" flood-color="{fc_stroke_color}" flood-opacity="0.8" /></filter>"#).map_err(fmt_err)?;

    writeln!(svg, r#"<filter id="ap-glow" x="-50%" y="-50%" width="200%" height="200%"><feDropShadow dx="0" dy="0" stdDeviation="4" flood-color="{fc_stroke_color}" flood-opacity="0.8" /></filter>"#).map_err(fmt_err)?;

    // AP Text Shadow Filter Definition (Removed)

    // Gaussian Blur Filter Definition
    writeln!(svg, r#"<filter id="bg-blur">"#).map_err(fmt_err)?;
    // 调整 stdDeviation 控制模糊程度, 10 是一个比较强的模糊效果
    writeln!(svg, r#"<feGaussianBlur stdDeviation="10" />"#).map_err(fmt_err)?;
    writeln!(svg, r#"</filter>"#).map_err(fmt_err)?;

    // Font style ... (保持不变) ...
    writeln!(svg, "<style>").map_err(fmt_err)?;
    write!(
        svg,
        r#"
        /* <![CDATA[ */
        svg {{ background-color: {bg_color}; /* Fallback background color */ }}
        .card {{
            fill: {card_bg_color};
            stroke: {normal_card_stroke_color};
            stroke-width: 1.5;
            filter: url(#card-shadow);
            transition: all 0.3s ease;
        }}
        .card-ap {{
          fill: {ap_card_fill};
          stroke: {ap_stroke_color};
          stroke-width: 2.5;
          filter: url(#ap-glow);
        }}
        .card-fc {{
          fill: {fc_card_fill};
          stroke: {fc_stroke_color}; /* Light Sky Blue */
          stroke-width: 2.5;
          filter: url(#fc-glow);
        }}
        /* ... (其他样式保持不变) ... */
        .text-title {{ font-size: 34px; fill: {text_color}; /* font-weight: bold; */ text-shadow: 0px 2px 4px rgba(0, 0, 0, 0.4); }}
        .text-stat {{ font-size: 21px; fill: {text_color}; }}
        .text-info {{ font-size: 16px; fill: {text_secondary_color}; text-anchor: end; }} /* For new info */
        .text-time {{ font-size: 15px; fill: {text_secondary_color}; text-anchor: end; }}
        .text-footer {{ font-size: 14px; fill: {text_secondary_color}; }}
        .text-songname {{ font-size: 20px; fill: {text_color}; font-weight: 600; }}
        .text-score {{ font-size: 30px; fill: {text_color}; font-weight: 700; }}
        .text-acc {{ font-size: 14px; fill: {text_secondary_color}; font-weight: 400; }}
        .text-level {{ font-size: 14px; fill: {text_secondary_color}; font-weight: 400; }}
        .text-rank {{ font-size: 15px; fill: {text_secondary_color}; font-weight: 500; text-anchor: end; }}
        .text-difficulty-badge {{ font-size: 12px; font-weight: 700; }} /* 难度标签文本样式 */
        .text-fc-ap-badge {{ font-size: 11px; font-weight: 700; }} /* FC/AP标签文本样式 */
        .push-acc {{ fill: #4CAF50; font-weight: 600; }}
        .text-rank-tag {{ font-size: 13px; fill: {text_secondary_color}; text-anchor: end; font-weight: 700; }}
        .text-section-title {{ font-size: 21px; fill: {text_color}; /* font-weight: bold; */ }}
        * {{ font-family: "{MAIN_FONT_NAME}", "Microsoft YaHei", "SimHei", "DengXian", Arial, sans-serif; }}
        /* ]]> */
        "#
    ).map_err(fmt_err)?;
    writeln!(svg, "</style>").map_err(fmt_err)?;
    let t_defs = t0.elapsed();

    // Define normal card stroke gradient
    writeln!(
        svg,
        r#"<linearGradient id="normal-card-stroke-gradient" x1="0%" y1="0%" x2="100%" y2="100%">"#
    )
    .map_err(fmt_err)?;
    writeln!(svg, "<stop offset=\"0%\" style=\"stop-color:#555868\" />").map_err(fmt_err)?; // 深灰色
    writeln!(svg, "<stop offset=\"100%\" style=\"stop-color:#333848\" />").map_err(fmt_err)?; // 更深的灰色
    writeln!(svg, r#"</linearGradient>"#).map_err(fmt_err)?;

    // Define AP card stroke gradient
    writeln!(
        svg,
        r#"<linearGradient id="ap-gradient" x1="0%" y1="0%" x2="100%" y2="100%">"#
    )
    .map_err(fmt_err)?;
    writeln!(svg, "<stop offset=\"0%\" style=\"stop-color:#FFDA63\" />").map_err(fmt_err)?;
    writeln!(svg, "<stop offset=\"100%\" style=\"stop-color:#D1913C\" />").map_err(fmt_err)?;
    writeln!(svg, r#"</linearGradient>"#).map_err(fmt_err)?;

    // 暂时不为白色主题定义更暗的AP渐变
    writeln!(
        svg,
        r#"<linearGradient id="ap-gradient-white" x1="0%" y1="0%" x2="100%" y2="100%">"#
    )
    .map_err(fmt_err)?;
    writeln!(svg, "<stop offset=\"0%\" style=\"stop-color:#D4A017\" />").map_err(fmt_err)?; // 更暗的金色
    writeln!(svg, "<stop offset=\"100%\" style=\"stop-color:#B8860B\" />").map_err(fmt_err)?; // 更暗的金色
    writeln!(svg, r#"</linearGradient>"#).map_err(fmt_err)?;

    // Gradients for white theme are now solid colors.

    writeln!(svg, "</defs>").map_err(fmt_err)?;
    let t_after_defs = t0.elapsed();

    // --- Background ---
    // 如果找到了背景图，则使用<image>并应用模糊，否则使用原来的<rect>和渐变
    if let Some(href) = background_image_href {
        // 预缩放并内嵌背景，减少 resvg 的解码与缩放开销
        let href = if embed_images && !href.starts_with("data:") {
            let p = Path::new(&href);
            get_scaled_image_data_uri(p, width, total_height).unwrap_or(href)
        } else {
            href
        };
        writeln!(svg,
            // 使用 href (Base64 data URI), preserveAspectRatio 保证图片覆盖并居中裁剪, filter 应用模糊
            r#"<image href="{href}" x="0" y="0" width="100%" height="100%" preserveAspectRatio="xMidYMid slice" filter="url(#bg-blur)" />"#
        ).map_err(fmt_err)?;
        // 可选：在模糊背景上加一层半透明叠加层，使前景文字更清晰
        // 调整 rgba 最后一个值 (alpha) 控制透明度, 0.7 = 70% 不透明
        match theme {
            Theme::White => {
                writeln!(
                    svg,
                    r#"<rect width="100%" height="100%" fill="rgba(247, 250, 255, 0.78)" />"#
                )
                .map_err(fmt_err)?;
            }
            Theme::Black => {
                writeln!(
                    svg,
                    r#"<rect width="100%" height="100%" fill="rgba(20, 24, 38, 0.7)" />"#
                )
                .map_err(fmt_err)?;
            }
        }
    } else {
        // 回退到渐变背景
        writeln!(
            svg,
            r#"<rect width="100%" height="100%" fill="url(#bg-gradient)"/>"#
        )
        .map_err(fmt_err)?;
    }
    // --- 背景结束 ---

    // --- Header ---
    let player_name = stats.player_name.as_deref().unwrap_or("Phigros Player");
    let real_rks = stats.real_rks.unwrap_or(0.0);
    writeln!(
        svg,
        r#"<text x="40" y="55" class="text-title">{}({:.6})</text>"#,
        escape_xml(player_name),
        real_rks
    )
    .map_err(fmt_err)?;
    let ap_text = match stats.ap_top_3_avg {
        Some(avg) => format!("AP Top 3 Avg: {avg:.4}"),
        None => "AP Top 3 Avg: N/A".to_string(),
    };
    writeln!(
        svg,
        r#"<text x="40" y="85" class="text-stat">{ap_text}</text>"#
    )
    .map_err(fmt_err)?;
    let b27_avg_str = stats
        .best_27_avg
        .map_or("N/A".to_string(), |avg| format!("{avg:.4}"));
    let bn_text = format!("Best 27 Avg: {b27_avg_str}");
    writeln!(
        svg,
        r#"<text x="40" y="110" class="text-stat">{bn_text}</text>"#
    )
    .map_err(fmt_err)?;

    // --- Right-aligned info (Data, Challenge, Time) ---
    let mut info_y = 65.0; // Starting Y position for the top-right info block

    // Data String
    if let Some(data_str) = &stats.data_string {
        writeln!(
            svg,
            r#"<text x="{}" y="{}" class="text-info">{}</text>"#,
            width - 30,
            info_y,
            escape_xml(data_str)
        )
        .map_err(fmt_err)?;
        info_y += 20.0; // Increment Y for the next line
    }

    // Challenge Rank
    if let Some((color, level)) = &stats.challenge_rank {
        let color_hex = match color.as_str() {
            "Green" => "#51AF44",
            "Blue" => "#3173B3",
            "Red" => "#BE2D23",
            "Gold" => "#D1913C",
            "Rainbow" => "url(#ap-gradient)", // Use existing gold gradient for rainbow for now
            _ => text_secondary_color,
        };
        writeln!(svg, r#"<text x="{}" y="{}" class="text-info">Challenge: <tspan fill="{}">{}</tspan> {}</text>"#,
                 width - 30, info_y, color_hex, color, level).map_err(fmt_err)?;
        info_y += 20.0; // Increment Y for the next line
    }

    // Update Time (always displayed)
    let update_time = format!(
        "Updated at {} UTC",
        stats.update_time.format("%Y/%m/%d %H:%M:%S")
    );
    writeln!(
        svg,
        r#"<text x="{}" y="{}" class="text-time">{}</text>"#,
        width - 30,
        info_y,
        update_time
    )
    .map_err(fmt_err)?;

    writeln!(
        svg,
        "<line x1='40' y1='{}' x2='{}' y2='{}' stroke='{}' stroke-width='1' stroke-opacity='0.7'/>",
        header_height,
        width - 40,
        header_height,
        card_stroke_color
    )
    .map_err(fmt_err)?;

    // --- AP Top 3 Section --- (保持不变) ...
    let ap_section_start_y = header_height + 15;
    if !stats.ap_top_3_scores.is_empty() {
        writeln!(
            svg,
            r#"<g id="ap-top-3-section" transform="translate(0, {ap_section_start_y})">"#
        )
        .map_err(fmt_err)?;
        for (idx, score) in stats.ap_top_3_scores.iter().take(3).enumerate() {
            let x_pos =
                ap_card_padding_outer + idx as u32 * (main_card_width + ap_card_padding_outer);

            // AP Top 3 卡片可能不需要推分ACC（因为已经是100%），但为了统一处理，也获取一下
            let push_acc = push_acc_map.and_then(|map| {
                let key = format!("{}-{}", score.song_id, score.difficulty);
                map.get(&key).copied()
            });

            generate_card_svg(CardRenderInfo {
                svg: &mut svg,
                score,
                index: idx,
                card_x: x_pos,
                card_y: ap_card_start_y,
                card_width: main_card_width,
                is_ap_card: true,
                is_ap_score: true,
                pre_calculated_push_acc: push_acc,
                all_engine_records: engine_records_for_scores.as_slice(),
                theme,
                is_user_generated: stats.is_user_generated,
                embed_images,
            })?
        }
        writeln!(svg, r#"</g>"#).map_err(fmt_err)?;
    }
    let t_after_ap = t0.elapsed();

    // --- Main Score Cards Section --- (保持不变) ...
    let main_content_start_y = header_height + ap_section_height + 15;
    for (index, score) in scores.iter().enumerate() {
        let row = index as u32 / columns;
        let col = index as u32 % columns;
        let x = main_card_padding_outer + col * (main_card_width + main_card_padding_outer);
        let y = main_content_start_y
            + main_card_padding_outer
            + row * (calculated_card_height + main_card_padding_outer);
        let is_ap_score = score.acc >= 100.0;

        // 获取预计算的推分ACC（如果有）
        let push_acc = push_acc_map.and_then(|map| {
            let key = format!("{}-{}", score.song_id, score.difficulty);
            map.get(&key).copied()
        });

        generate_card_svg(CardRenderInfo {
            svg: &mut svg,
            score,
            index,
            card_x: x,
            card_y: y,
            card_width: main_card_width,
            is_ap_card: false,
            is_ap_score,
            pre_calculated_push_acc: push_acc,
            all_engine_records: engine_records_for_scores.as_slice(),
            theme,
            is_user_generated: stats.is_user_generated,
            embed_images,
        })?
    }
    let t_after_main = t0.elapsed();

    // --- Footer ---
    let footer_y = (total_height - footer_height / 2 + 10) as f64;
    let footer_padding = 40.0;

    // 左下角文本
    let now_utc = Utc::now();
    let offset = FixedOffset::east_opt(8 * 3600).unwrap();
    let now_utc8 = now_utc.with_timezone(&offset);
    let generated_text = format!(
        "Generated by Phi-Backend at {} UTC+8",
        now_utc8.format("%Y/%m/%d %H:%M:%S")
    );
    writeln!(svg, r#"<text x="{footer_padding}" y="{footer_y:.1}" class="text-footer" text-anchor="start">{generated_text}</text>"#).map_err(fmt_err)?;

    // 右下角自定义文本
    if let Some(custom_text) = &stats.custom_footer_text {
        if !custom_text.is_empty() {
            writeln!(
                svg,
                r#"<text x="{}" y="{:.1}" class="text-footer" text-anchor="end">{}</text>"#,
                width as f64 - footer_padding,
                footer_y,
                escape_xml(custom_text)
            )
            .map_err(fmt_err)?;
        }
    }

    writeln!(svg, "</svg>").map_err(fmt_err)?;

    // 分段计时日志：defs/ap/main/total
    tracing::info!(
        "SVG生成分段: defs={:?}, ap={:?}, main={:?}, 总计={:?}",
        t_defs,
        t_after_ap - t_after_defs,
        t_after_main - t_after_ap,
        t0.elapsed(),
    );

    Ok(svg)
}

// ... (render_svg_to_png function - unchanged) ...
pub fn render_svg_to_png(svg_data: String, is_user_generated: bool) -> Result<Vec<u8>, AppError> {
    // 分段计时，定位瓶颈
    let t0 = std::time::Instant::now();

    // 字体数据库（全局复用）
    let font_db = get_global_font_db();

    let speed = AppConfig::global().image.optimize_speed;
    let opts = UsvgOptions {
        resources_dir: Some(std::env::current_dir().map_err(|e| {
            AppError::ImageRendererError(format!("Failed to get current dir: {e}"))
        })?),
        fontdb: font_db,
        font_family: MAIN_FONT_NAME.to_string(),
        font_size: 16.0,
        languages: vec!["zh-CN".to_string(), "en".to_string()],
        shape_rendering: if speed {
            usvg::ShapeRendering::OptimizeSpeed
        } else {
            usvg::ShapeRendering::GeometricPrecision
        },
        text_rendering: if speed {
            usvg::TextRendering::OptimizeSpeed
        } else {
            usvg::TextRendering::OptimizeLegibility
        },
        image_rendering: if speed {
            usvg::ImageRendering::OptimizeSpeed
        } else {
            usvg::ImageRendering::OptimizeQuality
        },
        ..Default::default()
    };

    let tree = usvg::Tree::from_data(svg_data.as_bytes(), &opts)
        .map_err(|e| AppError::ImageRendererError(format!("Failed to parse SVG: {e}")))?;
    let t_parse = t0.elapsed();

    let pixmap_size = tree.size().to_int_size();
    let mut pixmap = Pixmap::new(pixmap_size.width(), pixmap_size.height())
        .ok_or_else(|| AppError::ImageRendererError("Failed to create pixmap".to_string()))?;

    render(&tree, Transform::default(), &mut pixmap.as_mut());
    let t_raster = t0.elapsed();

    // 用户数据添加隐式水印：直接修改未编码像素，避免解/编码开销
    if is_user_generated {
        if let Some(px) = pixmap.data_mut().get_mut(0..4) {
            px.copy_from_slice(&[0x01, 0x02, 0x03, 0xFF]);
        }
    }

    // 使用 png crate 进行快速编码
    let mut out = Vec::with_capacity((pixmap_size.width() * pixmap_size.height() * 4) as usize);
    {
        let mut encoder = png::Encoder::new(&mut out, pixmap_size.width(), pixmap_size.height());
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        if AppConfig::global().image.optimize_speed {
            encoder.set_compression(png::Compression::Fast);
            encoder.set_filter(png::FilterType::NoFilter);
        } else {
            encoder.set_compression(png::Compression::Default);
            encoder.set_filter(png::FilterType::Paeth);
        }
        // 如需更小体积可改为 Adaptive，但会更慢
        let mut writer = encoder
            .write_header()
            .map_err(|e| AppError::ImageRendererError(format!("PNG write_header error: {e}")))?;
        writer.write_image_data(pixmap.data()).map_err(|e| {
            AppError::ImageRendererError(format!("PNG write_image_data error: {e}"))
        })?;
        writer
            .finish()
            .map_err(|e| AppError::ImageRendererError(format!("PNG finish error: {e}")))?;
    }
    let t_encode = t0.elapsed();

    tracing::info!(
        "PNG渲染内部分段: 解析={:?}, 栅格化={:?}, 编码={:?}, 总计={:?}",
        t_parse,
        t_raster - t_parse,
        t_encode - t_raster,
        t_encode
    );

    Ok(out)
}

/// 按目标宽度下采样后编码为 PNG（未提供则使用 SVG 原始宽度）
pub fn render_svg_to_png_scaled(
    svg_data: String,
    is_user_generated: bool,
    target_width: Option<u32>,
) -> Result<Vec<u8>, AppError> {
    // 字体数据库（全局复用）
    let font_db = get_global_font_db();
    let speed = AppConfig::global().image.optimize_speed;
    let opts = UsvgOptions {
        resources_dir: Some(std::env::current_dir().map_err(|e| {
            AppError::ImageRendererError(format!("Failed to get current dir: {e}"))
        })?),
        fontdb: font_db,
        font_family: MAIN_FONT_NAME.to_string(),
        font_size: 16.0,
        languages: vec!["zh-CN".to_string(), "en".to_string()],
        shape_rendering: if speed {
            usvg::ShapeRendering::OptimizeSpeed
        } else {
            usvg::ShapeRendering::GeometricPrecision
        },
        text_rendering: if speed {
            usvg::TextRendering::OptimizeSpeed
        } else {
            usvg::TextRendering::OptimizeLegibility
        },
        image_rendering: if speed {
            usvg::ImageRendering::OptimizeSpeed
        } else {
            usvg::ImageRendering::OptimizeQuality
        },
        ..Default::default()
    };

    let tree = usvg::Tree::from_data(svg_data.as_bytes(), &opts)
        .map_err(|e| AppError::ImageRendererError(format!("Failed to parse SVG: {e}")))?;

    let src_size = tree.size().to_int_size();
    let (dst_w, dst_h, scale) = if let Some(tw) = target_width {
        if tw > 0 && tw != src_size.width() {
            let s = tw as f32 / src_size.width() as f32;
            (
                (src_size.width() as f32 * s).round() as u32,
                (src_size.height() as f32 * s).round() as u32,
                s,
            )
        } else {
            (src_size.width(), src_size.height(), 1.0)
        }
    } else {
        (src_size.width(), src_size.height(), 1.0)
    };

    let mut pixmap = Pixmap::new(dst_w, dst_h)
        .ok_or_else(|| AppError::ImageRendererError("Failed to create pixmap".to_string()))?;
    render(
        &tree,
        Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );

    // 隐式水印
    if is_user_generated {
        if let Some(px) = pixmap.data_mut().get_mut(0..4) {
            px.copy_from_slice(&[0x01, 0x02, 0x03, 0xFF]);
        }
    }

    // 编码 PNG
    let mut out = Vec::with_capacity((dst_w * dst_h * 4) as usize);
    {
        let mut encoder = png::Encoder::new(&mut out, dst_w, dst_h);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        if AppConfig::global().image.optimize_speed {
            encoder.set_compression(png::Compression::Fast);
            encoder.set_filter(png::FilterType::NoFilter);
        } else {
            encoder.set_compression(png::Compression::Default);
            encoder.set_filter(png::FilterType::Paeth);
        }
        let mut writer = encoder
            .write_header()
            .map_err(|e| AppError::ImageRendererError(format!("PNG write_header error: {e}")))?;
        writer.write_image_data(pixmap.data()).map_err(|e| {
            AppError::ImageRendererError(format!("PNG write_image_data error: {e}"))
        })?;
        writer
            .finish()
            .map_err(|e| AppError::ImageRendererError(format!("PNG finish error: {e}")))?;
    }
    Ok(out)
}

/// 按目标宽度下采样后编码为 JPEG（quality 1-100，建议 80-90）
pub fn render_svg_to_jpeg(
    svg_data: String,
    is_user_generated: bool,
    target_width: Option<u32>,
    quality: u8,
) -> Result<Vec<u8>, AppError> {
    // 字体数据库（全局复用）
    let font_db = get_global_font_db();
    let speed = AppConfig::global().image.optimize_speed;
    let opts = UsvgOptions {
        resources_dir: Some(std::env::current_dir().map_err(|e| {
            AppError::ImageRendererError(format!("Failed to get current dir: {e}"))
        })?),
        fontdb: font_db,
        font_family: MAIN_FONT_NAME.to_string(),
        font_size: 16.0,
        languages: vec!["zh-CN".to_string(), "en".to_string()],
        shape_rendering: if speed {
            usvg::ShapeRendering::OptimizeSpeed
        } else {
            usvg::ShapeRendering::GeometricPrecision
        },
        text_rendering: if speed {
            usvg::TextRendering::OptimizeSpeed
        } else {
            usvg::TextRendering::OptimizeLegibility
        },
        image_rendering: if speed {
            usvg::ImageRendering::OptimizeSpeed
        } else {
            usvg::ImageRendering::OptimizeQuality
        },
        ..Default::default()
    };

    let tree = usvg::Tree::from_data(svg_data.as_bytes(), &opts)
        .map_err(|e| AppError::ImageRendererError(format!("Failed to parse SVG: {e}")))?;

    let src_size = tree.size().to_int_size();
    let (dst_w, dst_h, scale) = if let Some(tw) = target_width {
        if tw > 0 && tw != src_size.width() {
            let s = tw as f32 / src_size.width() as f32;
            (
                (src_size.width() as f32 * s).round() as u32,
                (src_size.height() as f32 * s).round() as u32,
                s,
            )
        } else {
            (src_size.width(), src_size.height(), 1.0)
        }
    } else {
        (src_size.width(), src_size.height(), 1.0)
    };

    let mut pixmap = Pixmap::new(dst_w, dst_h)
        .ok_or_else(|| AppError::ImageRendererError("Failed to create pixmap".to_string()))?;
    render(
        &tree,
        Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );

    // 隐式水印
    if is_user_generated {
        if let Some(px) = pixmap.data_mut().get_mut(0..4) {
            px.copy_from_slice(&[0x01, 0x02, 0x03, 0xFF]);
        }
    }

    // 将 RGBA 像素扁平化到黑色背景（JPEG 无透明通道）
    let rgba = pixmap.data();
    let mut rgb: Vec<u8> = Vec::with_capacity((dst_w as usize) * (dst_h as usize) * 3);
    let mut i = 0;
    while i + 3 < rgba.len() {
        let r = rgba[i] as u16;
        let g = rgba[i + 1] as u16;
        let b = rgba[i + 2] as u16;
        let a = rgba[i + 3] as u16; // 0..255
        // 过黑底合成：c' = c * a/255
        let r_out = ((r * a) / 255) as u8;
        let g_out = ((g * a) / 255) as u8;
        let b_out = ((b * a) / 255) as u8;
        rgb.push(r_out);
        rgb.push(g_out);
        rgb.push(b_out);
        i += 4;
    }

    let mut out = Vec::new();
    let mut enc = JpegEncoder::new_with_quality(&mut out, quality.clamp(1, 100));
    enc.encode(&rgb, dst_w, dst_h, ColorType::Rgb8.into())
        .map_err(|e| AppError::ImageRendererError(format!("JPEG encode error: {e}")))?;
    Ok(out)
}

/// 按目标宽度下采样后编码为 WebP（支持透明通道）
/// # 参数
/// * `svg_data` - SVG 字符串数据
/// * `is_user_generated` - 是否用户生成的内容（用于隐式水印）
/// * `target_width` - 目标宽度（可选，按宽度同比例缩放）
/// * `quality` - 有损压缩质量 1-100（默认 80，lossless 模式时无效）
/// * `lossless` - 是否使用无损模式（默认 false）
/// # 返回
/// WebP 格式的图片字节数据
pub fn render_svg_to_webp(
    svg_data: String,
    is_user_generated: bool,
    target_width: Option<u32>,
    _quality: u8,
    _lossless: bool,
) -> Result<Vec<u8>, AppError> {
    // 字体数据库（全局复用）
    let font_db = get_global_font_db();
    let speed = AppConfig::global().image.optimize_speed;
    let opts = UsvgOptions {
        resources_dir: Some(std::env::current_dir().map_err(|e| {
            AppError::ImageRendererError(format!("Failed to get current dir: {e}"))
        })?),
        fontdb: font_db,
        font_family: MAIN_FONT_NAME.to_string(),
        font_size: 16.0,
        languages: vec!["zh-CN".to_string(), "en".to_string()],
        shape_rendering: if speed {
            usvg::ShapeRendering::OptimizeSpeed
        } else {
            usvg::ShapeRendering::GeometricPrecision
        },
        text_rendering: if speed {
            usvg::TextRendering::OptimizeSpeed
        } else {
            usvg::TextRendering::OptimizeLegibility
        },
        image_rendering: if speed {
            usvg::ImageRendering::OptimizeSpeed
        } else {
            usvg::ImageRendering::OptimizeQuality
        },
        ..Default::default()
    };

    let tree = usvg::Tree::from_data(svg_data.as_bytes(), &opts)
        .map_err(|e| AppError::ImageRendererError(format!("Failed to parse SVG: {e}")))?;

    let src_size = tree.size().to_int_size();
    let (dst_w, dst_h, scale) = if let Some(tw) = target_width {
        if tw > 0 && tw != src_size.width() {
            let s = tw as f32 / src_size.width() as f32;
            (
                (src_size.width() as f32 * s).round() as u32,
                (src_size.height() as f32 * s).round() as u32,
                s,
            )
        } else {
            (src_size.width(), src_size.height(), 1.0)
        }
    } else {
        (src_size.width(), src_size.height(), 1.0)
    };

    let mut pixmap = Pixmap::new(dst_w, dst_h)
        .ok_or_else(|| AppError::ImageRendererError("Failed to create pixmap".to_string()))?;
    render(
        &tree,
        Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );

    // 隐式水印
    if is_user_generated {
        if let Some(px) = pixmap.data_mut().get_mut(0..4) {
            px.copy_from_slice(&[0x01, 0x02, 0x03, 0xFF]);
        }
    }

    // WebP 支持透明度通道，直接使用 RGBA 像素数据
    let _rgba = pixmap.data();

    // 使用 image crate 的 WebPEncoder
    use image::codecs::webp::WebPEncoder;

    let mut out = Vec::new();
    let enc = WebPEncoder::new_lossless(&mut out);
    enc.encode(_rgba, dst_w, dst_h, ColorType::Rgba8.into())
        .map_err(|e| AppError::ImageRendererError(format!("WebP encode error: {e}")))?;
    Ok(out)
}

/// 统一的图片编码入口：根据 `format` 选择编码器，并返回字节与 Content-Type。
///
/// 参数：
/// - format: "png" | "jpeg" | "jpg" | "webp"（大小写不敏感）
/// - is_user_generated: 是否用户生成（用于隐式水印）
/// - width: 目标宽度（可选）
/// - webp_quality: WebP 质量（1-100，缺省 80）
/// - webp_lossless: WebP 无损（缺省 false）
pub fn render_svg_unified(
    svg: String,
    is_user_generated: bool,
    format: Option<&str>,
    width: Option<u32>,
    webp_quality: Option<u8>,
    webp_lossless: Option<bool>,
) -> Result<(Vec<u8>, &'static str), AppError> {
    let fmt = format.unwrap_or("png").to_ascii_lowercase();
    match fmt.as_str() {
        "jpeg" | "jpg" => {
            let bytes = render_svg_to_jpeg(svg, is_user_generated, width, 85)?;
            Ok((bytes, "image/jpeg"))
        }
        "webp" => {
            let q = webp_quality.unwrap_or(80).clamp(1, 100);
            let lossless = webp_lossless.unwrap_or(false);
            let bytes = render_svg_to_webp(svg, is_user_generated, width, q, lossless)?;
            Ok((bytes, "image/webp"))
        }
        _ => {
            let bytes = render_svg_to_png_scaled(svg, is_user_generated, width)?;
            Ok((bytes, "image/png"))
        }
    }
}

/// 异步版本的统一图片编码入口
///
/// 将整个 SVG 解析、栅格化与编码流程放入 Tokio 的阻塞线程池中，避免阻塞异步运行时线程。
pub async fn render_svg_unified_async(
    svg: String,
    is_user_generated: bool,
    format: Option<&str>,
    width: Option<u32>,
    webp_quality: Option<u8>,
    webp_lossless: Option<bool>,
) -> Result<(Vec<u8>, &'static str), AppError> {
    let format_owned = format.map(|s| s.to_string());
    let handle = spawn_blocking(move || {
        render_svg_unified(
            svg,
            is_user_generated,
            format_owned.as_deref(),
            width,
            webp_quality,
            webp_lossless,
        )
    });

    handle
        .await
        .map_err(|e| AppError::Internal(format!("阻塞渲染任务执行失败: {e}")))?
}

// ... (escape_xml function - unchanged) ...
fn escape_xml(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// 从图片路径计算主色的反色
/// 优化：使用缩略图（100x100）计算颜色，而不是全尺寸图片
fn calculate_inverse_color_from_path(path: &Path) -> Option<String> {
    // 使用 image crate 打开图片
    let img = image::open(path).ok()?;

    // 缩小到 100x100 计算颜色，大幅减少内存使用和计算时间
    let thumbnail = img.thumbnail(100, 100);
    let pixels = thumbnail.to_rgba8().into_raw();

    if pixels.is_empty() {
        return None;
    }

    let mut total_r: u64 = 0;
    let mut total_g: u64 = 0;
    let mut total_b: u64 = 0;

    // 像素数据是扁平的 [R, G, B, A, R, G, B, A, ...] 数组
    for chunk in pixels.chunks_exact(4) {
        total_r += u64::from(chunk[0]);
        total_g += u64::from(chunk[1]);
        total_b += u64::from(chunk[2]);
    }

    let num_pixels = (pixels.len() / 4) as u64;
    if num_pixels == 0 {
        return None;
    }

    let avg_r = (total_r / num_pixels) as u8;
    let avg_g = (total_g / num_pixels) as u8;
    let avg_b = (total_b / num_pixels) as u8;

    // 计算反色
    let inv_r = 255 - avg_r;
    let inv_g = 255 - avg_g;
    let inv_b = 255 - avg_b;

    Some(format!("#{inv_r:02X}{inv_g:02X}{inv_b:02X}"))
}

/// 带缓存的反色计算，避免重复解码大图
fn get_inverse_color_from_path_cached(path: &Path) -> Option<String> {
    let key = PathBuf::from(path);
    {
        let mut cache = get_inverse_color_cache().lock().ok()?;
        if let Some(c) = cache.get(&key) {
            return Some(c.clone());
        }
    }

    let color = calculate_inverse_color_from_path(path)?;
    if let Ok(mut cache) = get_inverse_color_cache().lock() {
        cache.put(key, color.clone());
    }
    Some(color)
}

// --- 新增：生成单曲成绩 SVG ---
pub fn generate_song_svg_string(
    data: &SongRenderData,
    embed_images: bool,
) -> Result<String, AppError> {
    let fmt_err = |e| AppError::ImageRendererError(format!("SVG formatting error: {e}"));
    let t0 = std::time::Instant::now();

    // --- 整体布局与尺寸（横版）---
    let width = 1400; // 图片宽度，从1200增加到1400
    let height = 800; // 图片高度，从720增加到800
    let padding = 40.0; // 内边距，从30.0增加到40.0

    // 玩家信息区域高度
    let player_info_height = 78.0; // 原来是70.0，增加8px (上下各4px)

    // 曲绘尺寸 - 保持2048x1080的比例，但整体缩小
    let illust_height = height as f64 - padding * 3.0 - player_info_height - 80.0; // 给标题、页脚和曲目名称留出空间
    let illust_width = illust_height * (2048.0 / 1080.0); // 保持2048x1080的比例

    // 确保曲绘不会超过整体宽度的60%
    let illust_width = (illust_width).min(width as f64 * 0.60);

    // 曲目名称区域高度
    let song_name_height = 50.0;

    let _difficulty_info_height = 40.0; // Prefix unused variable

    // 成绩卡尺寸 - 调整为与曲绘总高度一致
    let card_area_width = width as f64 - illust_width - padding * 3.0;
    let difficulty_card_width = card_area_width;
    // 总共4张卡片，高度加上3个间距等于曲绘高度
    let difficulty_spacing_total = padding * 0.8 * 3.0; // 3个间距，增加间距
    let difficulty_card_height = (illust_height - difficulty_spacing_total) / 4.0; // 每张卡片高度
    let difficulty_card_spacing = padding * 0.8; // 卡片间距增加，从0.6调整为0.8

    // 预分配 SVG 字符串容量
    let mut svg = String::with_capacity(120_000);

    // --- 获取随机背景图 ---
    let mut background_image_href = None;
    let cover_files = get_cover_files();

    // 优先尝试使用当前曲目的曲绘作为背景
    let current_song_ill_path_png = cover_loader::covers_dir()
        .join("ill")
        .join(format!("{}.png", data.song_id));
    let current_song_ill_path_jpg = cover_loader::covers_dir()
        .join("ill")
        .join(format!("{}.jpg", data.song_id));

    // 优先尝试使用当前曲目的曲绘作为背景
    // 使用预先缓存的封面文件列表来检查文件是否存在，避免重复的文件系统调用
    if cover_files.contains(&current_song_ill_path_png) {
        if let Some(image_href) = get_image_href(&current_song_ill_path_png, embed_images) {
            background_image_href = Some(image_href);
            tracing::info!(
                "使用当前曲目曲绘作为背景: {}",
                current_song_ill_path_png.display()
            );
        }
    } else if cover_files.contains(&current_song_ill_path_jpg) {
        if let Some(image_href) = get_image_href(&current_song_ill_path_jpg, embed_images) {
            background_image_href = Some(image_href);
            tracing::info!(
                "使用当前曲目曲绘作为背景: {}",
                current_song_ill_path_jpg.display()
            );
        }
    } else {
        // 如果找不到当前曲目的曲绘，则随机选一个
        if !cover_files.is_empty() {
            let mut rng = rand::thread_rng();
            if let Some(random_path) = cover_files.as_slice().choose(&mut rng) {
                if let Some(image_href) = get_image_href(random_path, embed_images) {
                    background_image_href = Some(image_href);
                    tracing::info!("使用随机背景图: {}", random_path.display());
                } else {
                    tracing::error!("获取背景图片失败: {}", random_path.display());
                    // 获取失败则回退到渐变
                }
            } else {
                tracing::warn!("无法从封面文件列表中随机选择一个");
            }
        } else {
            tracing::warn!("找不到任何封面文件用于随机背景");
        }
    }

    // --- SVG 头部和 Defs ---
    writeln!(svg, r#"<svg width="{width}" height="{height}" viewBox="0 0 {width} {height}" xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink">"#).map_err(fmt_err)?;
    writeln!(svg, "<defs>").map_err(fmt_err)?;
    // Style
    writeln!(svg, "<style>").map_err(fmt_err)?;
    writeln!(svg, r#"
        /* 基本文本样式 */
        .text {{ font-family: '{MAIN_FONT_NAME}', sans-serif; fill: #E0E0E0; }}
        .text-title {{ font-size: 32px; font-weight: bold; fill: #FFFFFF; }}
        .text-subtitle {{ font-size: 18px; fill: #B0B0B0; }}
        .text-label {{ font-size: 28px; font-weight: bold; }} /* 增大难度标签字体 */
        .text-value {{ font-size: 18px; fill: #E0E0E0; }}
        .text-score {{ font-size: 34px; font-weight: bold; }} /* 增大分数字体 */
        .text-acc {{ font-size: 18px; fill: #B0B0B0; }} /* 参考Bn图调整ACC字体 */
        .text-rks {{ font-size: 18px; fill: #E0E0E0; }} /* 参考Bn图调整RKS字体 */
        .text-push-acc {{ font-size: 18px; font-weight: bold; }} /* 参考Bn图调整推分ACC字体 */
        .text-songname {{ font-size: 24px; font-weight: bold; fill: #FFFFFF; text-anchor: middle; }}
        .text-player-info {{ font-size: 22px; font-weight: bold; fill: #FFFFFF; }}
        .text-player-rks {{ font-size: 20px; fill: #E0E0E0; }}
        .text-difficulty-ez {{ fill: #77DD77; }}
        .text-difficulty-hd {{ fill: #87CEEB; }}
        .text-difficulty-in {{ fill: #FFB347; }}
        .text-difficulty-at {{ fill: #FF6961; }}
        .text-footer {{ font-size: 14px; fill: #888888; text-anchor: end; }}
        .text-constants {{ font-size: 18px; fill: #AAAAAA; }}
        .player-info-card {{ fill: rgba(40, 45, 60, 0.8); stroke: rgba(100, 100, 100, 0.4); stroke-width: 1; }}
        .difficulty-card {{ fill: url(#card-gradient); stroke: rgba(120, 120, 120, 0.5); stroke-width: 1.5; }} /* 使用渐变填充 */
        .difficulty-card-inactive {{ fill: rgba(40, 45, 60, 0.5); stroke: rgba(70, 70, 70, 0.3); stroke-width: 1; }}
        .difficulty-card-fc {{ fill: url(#card-gradient); stroke: #87CEEB; stroke-width: 3; }} /* FC卡片使用渐变填充 */
        .difficulty-card-phi {{ fill: url(#card-gradient); stroke: gold; stroke-width: 3; }} /* Phi卡片使用渐变填充 */
        .song-name-card {{ fill: rgba(40, 45, 60, 0.8); stroke: rgba(100, 100, 100, 0.4); stroke-width: 1; }}
        .constants-card {{ fill: rgba(40, 45, 60, 0.8); stroke: rgba(100, 100, 100, 0.4); stroke-width: 1; }}
        .rank-phi {{ fill: gold; }}
        .rank-v {{ fill: silver; }}
        .rank-s {{ fill: #FF6B6B; }}
    "#).map_err(fmt_err)?;
    writeln!(svg, "</style>").map_err(fmt_err)?;

    // ... existing gradient and filter definitions ...
    writeln!(svg, r#"<linearGradient id="bg-gradient" x1="0%" y1="0%" x2="100%" y2="100%"><stop offset="0%" style="stop-color:#141826" /><stop offset="100%" style="stop-color:#252E48" /></linearGradient>"#).map_err(fmt_err)?;
    writeln!(svg, r#"<filter id="card-shadow" x="-10%" y="-10%" width="120%" height="130%"><feDropShadow dx="0" dy="3" stdDeviation="3" flood-color="rgba(0,0,0,0.25)" flood-opacity="0.25" /></filter>"#).map_err(fmt_err)?;
    writeln!(
        svg,
        r#"<filter id="bg-blur"><feGaussianBlur stdDeviation="10" /></filter>"#
    )
    .map_err(fmt_err)?;
    // 添加曲绘阴影效果
    writeln!(svg, r#"<filter id="illust-shadow" x="-20%" y="-20%" width="140%" height="140%"><feDropShadow dx="0" dy="4" stdDeviation="6" flood-color="rgba(0,0,0,0.3)" flood-opacity="0.3" /></filter>"#).map_err(fmt_err)?;
    writeln!(svg, r#"<linearGradient id="rks-gradient" x1="0%" y1="0%" x2="100%" y2="0%"><stop offset="0%" style="stop-color:#FDC830" /><stop offset="100%" style="stop-color:#F37335" /></linearGradient>"#).map_err(fmt_err)?;
    // 添加卡片渐变效果
    writeln!(svg, r#"<linearGradient id="card-gradient" x1="0%" y1="0%" x2="100%" y2="100%"><stop offset="0%" style="stop-color:#2D3241" /><stop offset="100%" style="stop-color:#1E2330" /></linearGradient>"#).map_err(fmt_err)?;
    writeln!(svg, r#"<linearGradient id="rks-gradient-ap" x1="0%" y1="0%" x2="100%" y2="0%"><stop offset="0%" style="stop-color:#f6d365" /><stop offset="100%" style="stop-color:#fda085" /></linearGradient>"#).map_err(fmt_err)?;
    writeln!(svg, r#"<linearGradient id="rks-gradient-push" x1="0%" y1="0%" x2="100%" y2="0%"><stop offset="0%" style="stop-color:#a8e063" /><stop offset="100%" style="stop-color:#56ab2f" /></linearGradient>"#).map_err(fmt_err)?;
    writeln!(svg, "</defs>").map_err(fmt_err)?;
    let t_defs_song = t0.elapsed();

    // --- Background ---
    if let Some(href) = background_image_href {
        writeln!(svg, r#"<image href="{href}" x="0" y="0" width="100%" height="100%" preserveAspectRatio="xMidYMid slice" filter="url(#bg-blur)" />"#).map_err(fmt_err)?;
        writeln!(
            svg,
            r#"<rect width="100%" height="100%" fill="rgba(20, 24, 38, 0.7)" />"#
        )
        .map_err(fmt_err)?;
    } else {
        writeln!(
            svg,
            r#"<rect width="100%" height="100%" fill="url(#bg-gradient)"/>"#
        )
        .map_err(fmt_err)?;
    }

    // --- 玩家信息区域（顶部） ---
    let player_info_x = padding;
    let player_info_y = padding;
    let player_info_width = width as f64 - padding * 2.0;

    // 玩家信息卡片
    writeln!(svg, r#"<rect x="{player_info_x}" y="{player_info_y}" width="{player_info_width}" height="{player_info_height}" rx="8" ry="8" class="player-info-card" filter="url(#card-shadow)" />"#).map_err(fmt_err)?;

    // 玩家名称 - 加前缀"Player："并移除歌曲名
    let player_name_display = data.player_name.as_deref().unwrap_or("Player");
    writeln!(
        svg,
        r#"<text x="{}" y="{}" class="text text-player-info">Player: {}</text>"#,
        player_info_x + 20.0,
        player_info_y + 49.0,
        player_name_display
    )
    .map_err(fmt_err)?;

    // 时间戳放在右侧
    let shanghai_offset = FixedOffset::east_opt(8 * 3600).unwrap();
    let local_time = data.update_time.with_timezone(&shanghai_offset);
    let time_str = local_time.format("%Y-%m-%d %H:%M:%S").to_string();
    writeln!(
        svg,
        r#"<text x="{}" y="{}" class="text text-subtitle" text-anchor="end">{}</text>"#,
        width as f64 - padding - 20.0,
        player_info_y + 49.0,
        time_str
    )
    .map_err(fmt_err)?;

    // --- 曲绘和曲目名称（左侧）---
    let illust_x = padding;
    let illust_y = player_info_y + player_info_height + padding; // 在玩家信息区域下方
    let illust_href = data
        .illustration_path
        .as_ref()
        .map(|p| p.to_string_lossy().into_owned());

    // 曲目名称位置
    let song_name_x = illust_x;
    let song_name_y = illust_y + illust_height + padding / 2.0;
    let song_name_width = illust_width;

    // --- 方案: 使用 <g> 包裹并应用滤镜 ---
    let _left_card_x = illust_x;
    let _left_card_y = illust_y;
    let _left_card_width = illust_width;
    let _left_card_height = illust_height + padding / 2.0 + song_name_height;

    // 开始一个组，并对组应用阴影
    writeln!(svg, r#"<g filter="url(#illust-shadow)">"#).map_err(fmt_err)?;

    // 曲绘裁剪路径（圆角矩形）
    let illust_clip_id = "illust-clip";
    writeln!(svg, "<defs><clipPath id=\"{illust_clip_id}\"><rect x=\"{illust_x}\" y=\"{illust_y}\" width=\"{illust_width}\" height=\"{illust_height}\" rx=\"10\" ry=\"10\" /></clipPath></defs>").map_err(fmt_err)?;

    // 曲绘图片或占位矩形（移除单独的阴影）
    if let Some(href) = illust_href {
        // 预缩放并内嵌曲绘，减少 resvg 的解码与缩放开销
        let final_href = if embed_images && !href.starts_with("data:") {
            let pb = PathBuf::from(&href);
            get_scaled_image_data_uri(&pb, illust_width as u32, illust_height as u32)
                .unwrap_or(href)
        } else {
            href
        };
        let href = final_href;
        writeln!(svg, r#"<image href="{}" x="{}" y="{}" width="{}" height="{}" clip-path="url(#{})" preserveAspectRatio="xMidYMid slice" />"#,
                 escape_xml(&href), illust_x, illust_y, illust_width, illust_height, illust_clip_id).map_err(fmt_err)?;
    } else {
        writeln!(svg, "<rect x=\"{illust_x}\" y=\"{illust_y}\" width=\"{illust_width}\" height=\"{illust_height}\" fill=\"#333\" rx=\"10\" ry=\"10\" />").map_err(fmt_err)?;
    }

    // 曲目名称背景卡片（移除单独的阴影）
    writeln!(svg, r#"<rect x="{song_name_x}" y="{song_name_y}" width="{song_name_width}" height="{song_name_height}" rx="8" ry="8" class="song-name-card" />"#).map_err(fmt_err)?;

    // 结束组
    writeln!(svg, "</g>").map_err(fmt_err)?;

    // 曲目名称文字（居中）
    writeln!(
        svg,
        r#"<text x="{}" y="{}" class="text text-songname">{}</text>"#,
        song_name_x + song_name_width / 2.0,
        song_name_y + song_name_height / 2.0 + 8.0,
        escape_xml(&data.song_name)
    )
    .map_err(fmt_err)?;

    // --- 难度卡片（右侧垂直排列）---
    let difficulties = ["EZ", "HD", "IN", "AT"]; // 难度顺序

    // 计算右侧卡片区域的起始位置
    let cards_start_x = illust_x + illust_width + padding;
    let cards_start_y = illust_y; // 与曲绘顶部对齐

    // 渲染四个难度卡片
    for (i, &diff_key) in difficulties.iter().enumerate() {
        let pos_x = cards_start_x;
        let pos_y = cards_start_y + (difficulty_card_height + difficulty_card_spacing) * i as f64;

        // 检查是否有该难度的数据，决定卡片样式
        let has_difficulty_data = data
            .difficulty_scores
            .get(diff_key)
            .is_some_and(|opt| opt.as_ref().is_some_and(|score| score.acc.is_some()));

        // 判断是否是FC或Phi，选择相应的卡片样式
        let card_class = if has_difficulty_data {
            if let Some(Some(score_data)) = data.difficulty_scores.get(diff_key) {
                if score_data.is_phi == Some(true) {
                    "difficulty-card-phi" // Phi/AP成绩使用金色边框
                } else if score_data.is_fc == Some(true) {
                    "difficulty-card-fc" // FC成绩使用蓝色边框
                } else {
                    "difficulty-card" // 普通成绩使用默认边框
                }
            } else {
                "difficulty-card"
            }
        } else {
            "difficulty-card-inactive" // 无数据使用灰色卡片
        };

        // 绘制卡片背景 (添加圆角)
        writeln!(svg, r#"<rect x="{pos_x}" y="{pos_y}" width="{difficulty_card_width}" height="{difficulty_card_height}" rx="8" ry="8" class="{card_class}" filter="url(#card-shadow)" />"#).map_err(fmt_err)?;

        // 卡片内容边距
        let content_padding = 25.0; // 从20.0增加到25.0

        // 计算卡片中央分隔线 - 将卡片分为左右两部分
        let card_middle = pos_x + content_padding + 80.0; // 难度标签占用左侧区域，宽度从70px增加到80px

        // 难度标签 - 垂直居中位置，仅显示在左侧
        let diff_label_class = format!(
            "text text-label text-difficulty-{}",
            diff_key.to_lowercase()
        );
        let label_x = pos_x + content_padding + 35.0; // 左侧居中
        let label_y = pos_y + difficulty_card_height / 2.0; // 垂直居中位置

        writeln!(svg, r#"<text x="{label_x}" y="{label_y}" class="{diff_label_class}" text-anchor="middle">{diff_key}</text>"#).map_err(fmt_err)?;

        // 在难度标签下方显示定数值
        if let Some(Some(score_data)) = data.difficulty_scores.get(diff_key) {
            if let Some(dv) = score_data.difficulty_value {
                let constant_text_x = label_x; // 与难度标签X轴对齐
                // 调整Y坐标，让它位于难度标签下方
                let constant_text_y = label_y + 20.0;
                writeln!(svg, r#"<text x="{constant_text_x}" y="{constant_text_y}" class="text-constants" text-anchor="middle">Lv. {dv:.1}</text>"#).map_err(fmt_err)?;
            }
        }

        // 判断是否有该难度的谱面数据
        let has_difficulty_chart = data.difficulty_scores.get(diff_key).is_some_and(|opt| {
            opt.as_ref()
                .is_some_and(|score| score.difficulty_value.is_some())
        });

        // 该难度的成绩信息 - 放在右侧区域
        let right_area_start = card_middle;
        let right_area_width = difficulty_card_width - (card_middle - pos_x);
        let right_area_center = right_area_start + right_area_width / 2.0;

        if let Some(Some(score_data)) = data.difficulty_scores.get(diff_key) {
            // 有成绩数据
            if score_data.acc.is_some() {
                // 有ACC记录，显示完整成绩信息
                let score_text = score_data
                    .score
                    .map_or("N/A".to_string(), |s| format!("{s:.0}"));
                let acc_value = score_data.acc.unwrap_or(0.0);
                let rks_value = score_data.rks.unwrap_or(0.0);
                let dv_value = score_data.difficulty_value.unwrap_or(0.0);

                // 左对齐
                let text_x = right_area_start + 25.0;

                // Y 坐标
                let score_y = pos_y + 40.0;
                let acc_y = pos_y + 65.0;
                let rks_y = pos_y + 88.0;

                // 分数
                writeln!(svg, r#"<text x="{text_x}" y="{score_y}" class="text text-score" text-anchor="start">{score_text}</text>"#).map_err(fmt_err)?;

                // ACC -> 推分
                let mut acc_text = format!("Acc: {acc_value:.2}%");
                if let Some(push_acc) = score_data.player_push_acc {
                    let push_acc_display = if push_acc >= 100.0 {
                        if score_data.is_phi == Some(true) {
                            "<tspan class='text-push-acc' fill='gold'> (已 Phi)</tspan>".to_string()
                        } else {
                            "<tspan class='text-push-acc' fill='gold'> -> 100.00%</tspan>"
                                .to_string()
                        }
                    } else {
                        format!(
                            r#"<tspan class='text-push-acc' fill='url(#rks-gradient-push)'> -> {push_acc:.2}%</tspan>"#
                        )
                    };
                    acc_text.push_str(&push_acc_display);
                }
                writeln!(svg, r#"<text x="{text_x}" y="{acc_y}" class="text text-acc" text-anchor="start">{acc_text}</text>"#).map_err(fmt_err)?;

                // Lv. -> RKS
                let rks_text = format!("Lv.{dv_value:.1} -> {rks_value:.2}");
                writeln!(svg, r#"<text x="{text_x}" y="{rks_y}" class="text text-rks" text-anchor="start">{rks_text}</text>"#).map_err(fmt_err)?;
            } else if has_difficulty_chart {
                // 有难度定数但无成绩，显示"无成绩"
                let no_data_x = right_area_center;
                let no_data_y = pos_y + difficulty_card_height / 2.0 + 5.0; // 垂直居中
                writeln!(svg, r#"<text x="{no_data_x}" y="{no_data_y}" class="text text-acc" text-anchor="middle" dominant-baseline="middle">无成绩</text>"#).map_err(fmt_err)?;
            }
        } else {
            // 没有数据时，显示"无谱面"
            let no_data_x = right_area_center;
            let no_data_y = pos_y + difficulty_card_height / 2.0 + 5.0; // 垂直居中
            writeln!(svg, r#"<text x="{no_data_x}" y="{no_data_y}" class="text text-acc" text-anchor="middle" dominant-baseline="middle">无谱面</text>"#).map_err(fmt_err)?;
        }
    }

    // --- Footer ---
    let t_body_song = t0.elapsed();
    let footer_y = height as f64 - padding / 2.0;
    let footer_x = width as f64 - padding;
    let time_str = local_time.format("%Y-%m-%d %H:%M:%S UTC+8").to_string(); // 使用UTC+8表示时区
    let right_text = if let Some(txt) = &data.custom_footer_text {
        if !txt.is_empty() {
            escape_xml(txt)
        } else {
            format!("Generated by Phi-Backend | {time_str}")
        }
    } else {
        format!("Generated by Phi-Backend | {time_str}")
    };
    writeln!(
        svg,
        r#"<text x="{footer_x}" y="{footer_y}" class="text text-footer">{right_text}</text>"#
    )
    .map_err(fmt_err)?;

    // --- End SVG ---
    writeln!(svg, "</svg>").map_err(fmt_err)?;

    tracing::info!(
        "SVG(单曲)生成分段: defs={:?}, body={:?}, 总计={:?}",
        t_defs_song,
        t_body_song - t_defs_song,
        t0.elapsed(),
    );

    Ok(svg)
}

/// 生成排行榜SVG字符串
pub fn generate_leaderboard_svg_string(data: &LeaderboardRenderData) -> Result<String, AppError> {
    // -- 定义 fmt_err 闭包 --
    let fmt_err = |e| AppError::ImageRendererError(format!("SVG formatting error: {e}"));
    // -- 结束定义 --

    let width = 1200;
    let row_height = 60;
    let header_height = 120;
    let footer_height = 40;
    let total_height = header_height + (data.entries.len() as i32 * row_height) + footer_height;

    let mut svg = String::with_capacity(20000);
    svg.push_str(&format!(r#"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{total_height}" viewBox="0 0 {width} {total_height}">"#));

    // 添加渐变背景和样式
    // 使用 r##"..."## 来避免 # 颜色值与原始字符串分隔符冲突
    svg.push_str(r##"
    <defs>
        <linearGradient id="bg-gradient" x1="0%" y1="0%" x2="100%" y2="100%">
            <stop offset="0%" stop-color="#1a1a2e" />
            <stop offset="100%" stop-color="#16213e" />
        </linearGradient>
        <style>
            @font-face {
                font-family: 'NotoSansSC';
                src: url('https://fonts.gstatic.com/s/notosanssc/v36/k3kXo84MPvpLmixcA63oeALhLIiP-Q-87KaAavc.woff2') format('woff2');
            }
            .header-text {
                font-family: 'NotoSansSC', sans-serif;
                font-size: 48px;
                fill: white;
                text-anchor: middle;
                font-weight: bold; /* 加粗标题 */
            }
            .rank-text {
                font-family: 'NotoSansSC', sans-serif;
                font-size: 32px;
                fill: white;
                text-anchor: middle;
                font-weight: bold;
            }
            .name-text {
                font-family: 'NotoSansSC', sans-serif;
                font-size: 32px;
                fill: white;
                text-anchor: start;
            }
            .rks-text {
                font-family: 'NotoSansSC', sans-serif;
                font-size: 32px;
                fill: white;
                text-anchor: end;
                font-weight: bold;
            }
            .footer-text {
                font-family: 'NotoSansSC', sans-serif;
                font-size: 20px;
                fill: #aaaaaa;
                text-anchor: end;
            }
        </style>
    </defs>
"##); // <--- 修正结束符的位置，紧跟在 </defs> 之后

    // 绘制背景
    svg.push_str(&format!(
        r#"<rect width="{width}" height="{total_height}" fill="url(#bg-gradient)" />"#
    ));

    // 绘制标题
    svg.push_str(&format!(
        r#"<text x="{}" y="{}" class="header-text">{}</text>"#,
        width / 2,
        header_height / 2 + 16,
        data.title
    ));

    // 绘制表头分隔线
    write!(
        svg,
        r##"<line x1="20" y1="{}" x2="{}" y2="{}" stroke="#4a5568" stroke-width="2" />"##,
        header_height,
        width - 20,
        header_height
    )
    .map_err(fmt_err)?;

    // 绘制排行榜条目
    for (i, entry) in data.entries.iter().enumerate() {
        let y_pos = header_height + (i as i32 * row_height);

        // 绘制排名
        write!(
            svg,
            r##"<text x="60" y="{}" class="rank-text">#{}</text>"##,
            y_pos + (row_height / 2) + 10,
            i + 1
        )
        .map_err(fmt_err)?;

        // 绘制玩家名
        let name_display = if entry.player_name.len() > 20 {
            format!("{}...", &entry.player_name[0..17])
        } else {
            entry.player_name.clone()
        };
        write!(
            svg,
            r##"<text x="120" y="{}" class="name-text">{}</text>"##,
            y_pos + (row_height / 2) + 10,
            name_display
        )
        .map_err(fmt_err)?;

        // 绘制RKS
        write!(
            svg,
            r##"<text x="{}" y="{}" class="rks-text">{:.2}</text>"##,
            width - 60,
            y_pos + (row_height / 2) + 10,
            entry.rks
        )
        .map_err(fmt_err)?;

        // 如果不是最后一行，绘制分隔线
        if i < data.entries.len() - 1 {
            let line_y = y_pos + row_height; // Cast here
            write!(
                svg,
                r##"<line x1="100" y1="{}" x2="{}" y2="{}" stroke="#2d3748" stroke-width="1" />"##,
                line_y,
                width - 100,
                line_y
            )
            .map_err(fmt_err)?;
        }
    }

    // 绘制底部更新时间
    let time_str = data.update_time.format("%Y-%m-%d %H:%M:%S").to_string();
    svg.push_str(&format!(
        r#"<text x="{}" y="{}" class="footer-text">更新时间: {} UTC</text>"#,
        width - 60,
        total_height - 15,
        time_str
    ));

    svg.push_str("</svg>");
    Ok(svg)
}
