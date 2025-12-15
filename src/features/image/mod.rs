mod cover_loader;
pub mod handler;
mod renderer;
mod service;
mod types;

/// 启动期预热曲绘索引（目录扫描 + 白主题背景反色预计算）。
///
/// 注意：该预热只用于降低首个 SVG 请求的长尾延迟，不参与请求处理路径。
pub(crate) fn prewarm_illustration_assets() {
    let _ = renderer::get_global_font_db();
    let _ = renderer::get_cover_files();
    let _ = renderer::get_cover_metadata_map();
}

pub use handler::create_image_router;
pub use renderer::{
    LeaderboardRenderData, PlayerStats, RenderRecord, SongDifficultyScore, SongRenderData,
    generate_leaderboard_svg_string, generate_song_svg_string, generate_svg_string,
    render_svg_to_png,
};
pub use service::ImageService;
pub use types::{RenderBnRequest, RenderSongRequest, Theme};
