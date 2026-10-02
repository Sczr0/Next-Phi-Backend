use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use resvg::usvg::fontdb;

use super::MAIN_FONT_NAME;
use crate::config::AppConfig;

/// 历史默认字体目录（相对进程工作目录）。
const LEGACY_FONTS_DIR: &str = "resources/fonts";
/// `resources.base_path` 下的字体子目录名。
const FONTS_SUBDIR: &str = "fonts";
/// 支持的字体扩展名（含 TrueType/OpenType 集合——Linux 发行版常以 `.ttc` 分发 CJK 字体）。
const FONT_EXTENSIONS: [&str; 4] = ["ttf", "ttc", "otf", "otc"];

// 全局字体数据库单例
static GLOBAL_FONT_DB: OnceLock<Arc<fontdb::Database>> = OnceLock::new();

/// 自带字体目录候选（按优先级）：
/// 1. 配置 `resources.base_path/fonts`（与曲绘/info 同源，支持绝对路径）；
/// 2. 进程工作目录下的 `resources/fonts`（历史行为）；
/// 3. 可执行文件所在目录下的 `resources/fonts`（单二进制"拷走即用"、容器工作目录差异）。
///
/// 加载时取首个命中的候选目录即停：字体文件较大，不在多个目录重复加载。
fn font_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(config) = AppConfig::try_global() {
        dirs.push(config.resources_path().join(FONTS_SUBDIR));
    }
    dirs.push(PathBuf::from(LEGACY_FONTS_DIR));
    if let Ok(exe) = std::env::current_exe()
        && let Some(parent) = exe.parent()
    {
        dirs.push(parent.join(LEGACY_FONTS_DIR));
    }
    dirs.dedup();
    dirs
}

fn is_supported_font(path: &Path) -> bool {
    let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
        return false;
    };
    let ext = ext.to_ascii_lowercase();
    FONT_EXTENSIONS.contains(&ext.as_str())
}

/// 从首个命中的候选目录加载自带字体，返回加载数量。
fn load_bundled_fonts(font_db: &mut fontdb::Database) -> usize {
    for dir in font_dirs() {
        if !dir.is_dir() {
            continue;
        }
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        let mut loaded = 0usize;
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() || !is_supported_font(&path) {
                continue;
            }
            match font_db.load_font_file(&path) {
                Ok(()) => loaded += 1,
                Err(e) => tracing::error!("加载字体文件失败 '{}': {}", path.display(), e),
            }
        }
        if loaded > 0 {
            tracing::info!("已从 '{}' 加载 {} 个自带字体", dir.display(), loaded);
            return loaded;
        }
    }
    0
}

fn query_face(font_db: &fontdb::Database, families: &[fontdb::Family<'_>]) -> Option<fontdb::ID> {
    font_db.query(&fontdb::Query {
        families,
        weight: fontdb::Weight::NORMAL,
        stretch: fontdb::Stretch::Normal,
        style: fontdb::Style::Normal,
    })
}

/// 选择一个兜底字体面：优先自带主字体，其次名字像 CJK 的面（中文基线更贴近设计），
/// 最后任一面——只要选中任一字体，usvg 的逐字符回退就能补齐缺字，不会再整段丢文本。
fn pick_fallback_face(font_db: &fontdb::Database) -> Option<fontdb::ID> {
    if let Some(id) = query_face(font_db, &[fontdb::Family::Name(MAIN_FONT_NAME)]) {
        return Some(id);
    }
    let cjk = font_db
        .faces()
        .find(|face| {
            face.families
                .iter()
                .any(|(name, _)| looks_like_cjk_family(name))
        })
        .map(|face| face.id);
    cjk.or_else(|| font_db.faces().next().map(|face| face.id))
}

/// 粗糙但零依赖的 CJK 字体识别：仅用于兜底选择，不参与最终字形匹配。
fn looks_like_cjk_family(name: &str) -> bool {
    const HINTS: [&str; 10] = [
        "CJK",
        "Han",
        "SC",
        "思源",
        "黑体",
        "雅黑",
        "YaHei",
        "SimHei",
        "DengXian",
        "Noto Sans SC",
    ];
    let lower = name.to_ascii_lowercase();
    HINTS
        .iter()
        .any(|hint| lower.contains(&hint.to_ascii_lowercase()) || name.contains(hint))
}

fn fallback_family_name(font_db: &fontdb::Database, id: fontdb::ID) -> Option<String> {
    let face = font_db.face(id)?;
    face.families
        .iter()
        .find(|(_, lang)| *lang == fontdb::Language::English_UnitedStates)
        .or_else(|| face.families.first())
        .map(|(name, _)| name.clone())
}

/// 把泛型族（`serif`/`sans-serif`）指向真实加载的字体面。
///
/// 回归背景（"服务端渲染图片字没了"）：resvg/usvg 在给定字体族列表全部落空时
/// **不会**退回到"任意字体"，而是整段文本被丢弃——`layout_text` 只把
/// `select_font` 命中的字体放进 `fonts_cache`，未命中的 span 直接跳过（不是 tofu）。
/// fontdb 的泛型族默认是硬编码的 `"Arial"` / `"Times New Roman"`，在最小化 Linux
/// 容器 / 未装这些字体的宿主机上并不存在；SVG 里 `..., Arial, sans-serif` 的兜底
/// 因此也失效，两图（B27 与单曲）文本整体消失。这里把泛型族重定向到实际加载到的
/// 字体（自带字体优先，其次覆盖 CJK 的面），保证只要字体库非空就一定有可用字体。
fn ensure_generic_families(font_db: &mut fontdb::Database) {
    let sans_missing = query_face(font_db, &[fontdb::Family::SansSerif]).is_none();
    let serif_missing = query_face(font_db, &[fontdb::Family::Serif]).is_none();
    if !sans_missing && !serif_missing {
        return;
    }

    let Some(face_id) = pick_fallback_face(font_db) else {
        tracing::error!(
            "字体数据库为空：服务端渲染图片的文本将整体缺失。\
             请安装 CJK 系统字体，或将自带字体放入 resources/fonts（见 docs/deployment.md 第 7 节）"
        );
        return;
    };
    let Some(name) = fallback_family_name(font_db, face_id) else {
        return;
    };

    if sans_missing {
        font_db.set_sans_serif_family(name.clone());
    }
    if serif_missing {
        font_db.set_serif_family(name.clone());
    }
    tracing::warn!(
        "字体泛型族缺失（fontdb 默认 Arial/Times New Roman 未安装），已回退到 '{name}'；\
         服务端渲染图片的文本依赖此回退，建议为部署补齐自带字体或 CJK 系统字体"
    );
}

/// 初始化全局字体数据库
fn init_global_font_db() -> Arc<fontdb::Database> {
    let mut font_db = fontdb::Database::new();
    font_db.load_system_fonts();

    // 加载自定义字体
    if load_bundled_fonts(&mut font_db) == 0 {
        tracing::warn!(
            "未找到自带字体目录（候选: {}），仅使用系统字体",
            font_dirs()
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }

    ensure_generic_families(&mut font_db);
    tracing::info!("字体数据库初始化完成：{} 个字体面", font_db.len());

    Arc::new(font_db)
}

/// 获取全局字体数据库
pub(super) fn get_global_font_db() -> Arc<fontdb::Database> {
    GLOBAL_FONT_DB.get_or_init(init_global_font_db).clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bundled_font_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(LEGACY_FONTS_DIR)
            .join("Source Han Sans & Saira Hybrid-Regular #5446.ttf")
    }

    fn db_with_bundled_font_only() -> fontdb::Database {
        let mut db = fontdb::Database::new();
        db.load_font_file(bundled_font_path())
            .expect("自带字体应可加载");
        db
    }

    // 自带字体必须能被按 MAIN_FONT_NAME 选中（字体文件随仓库分发）。
    #[test]
    fn bundled_font_is_selectable_by_main_family() {
        let db = db_with_bundled_font_only();
        assert!(
            query_face(&db, &[fontdb::Family::Name(MAIN_FONT_NAME)]).is_some(),
            "自带字体应包含族名 {MAIN_FONT_NAME}"
        );
    }

    // 回归守卫：泛型族默认指向未安装的 Arial/Times 时必须被重定向到真实字体面，
    // 否则 usvg 会把整段文本丢弃（"字没了"）。
    #[test]
    fn generic_families_are_rewired_to_loaded_faces() {
        let mut db = db_with_bundled_font_only();
        assert!(
            query_face(&db, &[fontdb::Family::SansSerif]).is_none(),
            "前置：未加载系统字体时默认 sans-serif(Arial) 应落空"
        );

        ensure_generic_families(&mut db);

        assert!(
            query_face(&db, &[fontdb::Family::SansSerif]).is_some(),
            "sans-serif 应回退到真实字体面"
        );
        assert!(
            query_face(&db, &[fontdb::Family::Serif]).is_some(),
            "serif 应回退到真实字体面"
        );
    }

    // 空字体库只记日志、不 panic（部署缺字体时服务继续运行，渲染降级已被上层日志标记）。
    #[test]
    fn empty_font_db_does_not_panic() {
        let mut db = fontdb::Database::new();
        ensure_generic_families(&mut db);
        assert!(query_face(&db, &[fontdb::Family::SansSerif]).is_none());
    }
}
