use crate::error::AppError;
use crate::features::image::Theme;
use crate::rks_contract::engine;
use chrono::{DateTime, Utc};
use lru::LruCache;
use resvg::usvg::fontdb;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

mod background_layer;
mod bn;
mod bn_background;
mod bn_card;
mod bn_card_acc;
mod bn_card_badge_style;
mod bn_card_badges;
mod bn_card_cover;
mod bn_card_list;
mod bn_card_text;
mod bn_defs;
mod bn_header_text;
mod bn_layout;
mod bn_sections;
mod bn_theme;
mod leaderboard;
mod math;
mod raster_jpeg;
mod raster_options;
mod raster_png;
mod raster_surface;
mod raster_unified;
mod raster_webp;
mod resource_background;
mod resource_color;
mod resource_fonts;
mod resource_image;
mod resource_scaled;
mod resources;
mod score;
mod song;
mod song_background;
mod song_card;
mod song_defs;
mod song_difficulty;
mod song_illustration;
mod song_layout;
mod song_score_text;
mod song_sections;
mod svg_error;
mod template_bn;
mod template_bn_card;
mod template_shared;
mod template_song;
mod template_song_card;
mod text;
mod time;
mod urls;

#[allow(dead_code)]
pub struct PlayerStats {
    pub ap_top_3_avg: Option<f64>,
    pub best_27_avg: Option<f64>,
    pub real_rks: Option<f64>,
    pub player_name: Option<String>,
    pub update_time: DateTime<Utc>,
    pub n: u32,                                   // 请求的 Best N 数量
    pub ap_top_3_scores: Vec<RenderRecord>,       // AP Top 3 的具体成绩
    pub challenge_rank: Option<(String, String)>, // 课题等级（颜色、等级）
    pub data_string: Option<String>,              // 格式化后的 Data 字符串
    pub custom_footer_text: Option<String>,
    pub is_user_generated: bool, // 标记是否为用户生成
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

// 单曲成绩渲染所需数据结构
#[derive(Debug, Clone)]
pub struct SongDifficultyScore {
    pub score: Option<f64>,
    pub acc: Option<f64>,
    pub rks: Option<f64>,
    pub difficulty_value: Option<f64>,
    pub is_fc: Option<bool>,                          // 可选：是否 Full Combo
    pub is_phi: Option<bool>,                         // 可选：是否 Phi (ACC 100%)
    pub player_push_acc: Option<engine::PushAccHint>, // 玩家总 RKS 推分提示
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
const MAIN_FONT_NAME: &str = "思源黑体 CN";
const DEFAULT_PLAYER_NAME: &str = "Phigros Player";
const COVER_ASPECT_RATIO: f64 = 512.0 / 270.0;

/// 获取全局字体数据库
pub fn get_global_font_db() -> Arc<fontdb::Database> {
    resources::get_global_font_db()
}

/// 获取背景图片缓存
#[allow(dead_code)]
pub fn get_background_cache() -> &'static std::sync::Mutex<LruCache<PathBuf, Arc<str>>> {
    resources::get_background_cache()
}

/// 获取封面文件列表
pub fn get_cover_files() -> &'static [PathBuf] {
    resources::get_cover_files()
}

/// 获取封面元数据（只读，无锁）
pub fn get_cover_metadata_map() -> &'static HashMap<String, String> {
    resources::get_cover_metadata_map()
}

pub fn generate_svg_string<S>(
    scores: &[RenderRecord],
    stats: &PlayerStats,
    push_acc_map: Option<&HashMap<String, engine::PushAccHint, S>>, // 预先计算的推分提示映射，键为"曲目ID-难度"
    theme: &Theme,                                                  // 渲染主题
    embed_images: bool,
    // 若提供，则将曲绘引用改为可被浏览器访问的 URL（例如 `/_ill`）
    public_illustration_base_url: Option<&str>,
    // 外部模板 ID：对应 `resources/templates/image/bn/{id}.svg.jinja`（为空则使用内置手写 SVG 实现）。
    template_id: Option<&str>,
) -> Result<String, AppError>
where
    S: std::hash::BuildHasher,
{
    bn::generate_svg_string(
        scores,
        stats,
        push_acc_map,
        theme,
        embed_images,
        public_illustration_base_url,
        template_id,
    )
}

pub fn render_svg_to_png(svg_data: &str, is_user_generated: bool) -> Result<Vec<u8>, AppError> {
    raster_png::render_svg_to_png_scaled(svg_data, is_user_generated, None)
}

/// 按目标宽度下采样后编码为 PNG（未提供则使用 SVG 原始宽度）
#[allow(dead_code)]
pub fn render_svg_to_png_scaled(
    svg_data: &str,
    is_user_generated: bool,
    target_width: Option<u32>,
) -> Result<Vec<u8>, AppError> {
    raster_png::render_svg_to_png_scaled(svg_data, is_user_generated, target_width)
}

/// 按目标宽度下采样后编码为 JPEG（quality 1-100，建议 80-90）
#[allow(dead_code)]
pub fn render_svg_to_jpeg(
    svg_data: &str,
    is_user_generated: bool,
    target_width: Option<u32>,
    quality: u8,
) -> Result<Vec<u8>, AppError> {
    raster_jpeg::render_svg_to_jpeg(svg_data, is_user_generated, target_width, quality)
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
#[allow(dead_code)]
pub fn render_svg_to_webp(
    svg_data: &str,
    is_user_generated: bool,
    target_width: Option<u32>,
    quality: u8,
    lossless: bool,
) -> Result<Vec<u8>, AppError> {
    raster_webp::render_svg_to_webp(svg_data, is_user_generated, target_width, quality, lossless)
}

/// 统一的图片编码入口：根据 `format` 选择编码器，并返回字节与 Content-Type。
///
/// 参数：
/// - format: "png" | "jpeg" | "jpg" | "webp"（大小写不敏感）
/// - is_user_generated: 是否用户生成（用于隐式水印）
/// - width: 目标宽度（可选）
/// - webp_quality: WebP 质量（1-100，缺省 80）
/// - webp_lossless: WebP 无损（缺省 false）
#[allow(dead_code)]
pub fn render_svg_unified(
    svg: &str,
    is_user_generated: bool,
    format: Option<&str>,
    width: Option<u32>,
    webp_quality: Option<u8>,
    webp_lossless: Option<bool>,
) -> Result<(Vec<u8>, &'static str), AppError> {
    raster_unified::render_svg_unified(
        svg,
        is_user_generated,
        format,
        width,
        webp_quality,
        webp_lossless,
    )
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
    raster_unified::render_svg_unified_async(
        svg,
        is_user_generated,
        format,
        width,
        webp_quality,
        webp_lossless,
    )
    .await
}

pub fn generate_song_svg_string(
    data: &SongRenderData,
    embed_images: bool,
    // 若提供，则将曲绘引用改为可被浏览器访问的 URL（例如 `/_ill`）
    public_illustration_base_url: Option<&str>,
    // 外部模板 ID：对应 `resources/templates/image/song/{id}.svg.jinja`（为空则使用内置手写 SVG 实现）。
    template_id: Option<&str>,
) -> Result<String, AppError> {
    song::generate_song_svg_string(
        data,
        embed_images,
        public_illustration_base_url,
        template_id,
    )
}

/// 生成排行榜SVG字符串
pub fn generate_leaderboard_svg_string(data: &LeaderboardRenderData) -> Result<String, AppError> {
    leaderboard::generate_leaderboard_svg_string(data)
}

#[cfg(test)]
mod tests;
