use std::path::PathBuf;

use once_cell::sync::OnceCell;

use crate::config::AppConfig;

/// 获取曲绘资源目录
pub fn covers_dir() -> PathBuf {
    static COVERS_DIR: OnceCell<PathBuf> = OnceCell::new();
    COVERS_DIR
        .get_or_init(|| AppConfig::global().illustration_path())
        .clone()
}
