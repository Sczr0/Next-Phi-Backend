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

/// 预计算低清与背景目录
pub fn cover_variants() -> (PathBuf, PathBuf, PathBuf) {
    let base = covers_dir();
    let ill = base.join("ill");
    let ill_low = base.join("illLow");
    let ill_blur = base.join("illBlur");
    (ill, ill_low, ill_blur)
}
