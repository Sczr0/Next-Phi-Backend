use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use resvg::usvg::fontdb;

const FONTS_DIR: &str = "resources/fonts";

// 全局字体数据库单例
static GLOBAL_FONT_DB: OnceLock<Arc<fontdb::Database>> = OnceLock::new();

/// 初始化全局字体数据库
fn init_global_font_db() -> Arc<fontdb::Database> {
    let mut font_db = fontdb::Database::new();
    font_db.load_system_fonts();

    // 加载自定义字体
    let fonts_dir = PathBuf::from(FONTS_DIR);
    if fonts_dir.exists()
        && let Ok(entries) = fs::read_dir(&fonts_dir)
    {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file()
                && (path.extension() == Some("ttf".as_ref())
                    || path.extension() == Some("otf".as_ref()))
                && let Err(e) = font_db.load_font_file(&path)
            {
                tracing::error!("加载字体文件失败 '{}': {}", path.display(), e);
            }
        }
    }

    Arc::new(font_db)
}

/// 获取全局字体数据库
pub(super) fn get_global_font_db() -> Arc<fontdb::Database> {
    GLOBAL_FONT_DB.get_or_init(init_global_font_db).clone()
}
