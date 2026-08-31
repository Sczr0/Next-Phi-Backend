use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{OnceLock, RwLock};
use std::time::SystemTime;

use minijinja::Environment;
use serde::Serialize;
use serde::de::DeserializeOwned;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::config::AppConfig;
use crate::error::AppError;

#[derive(Debug, Clone, Serialize)]
pub(super) struct PageCtx {
    pub(super) width: u32,
    pub(super) height: u32,
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct FontsCtx {
    pub(super) main: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct ColorsCtx {
    pub(super) bg_grad_0: &'static str,
    pub(super) bg_grad_1: &'static str,
    pub(super) text: &'static str,
    pub(super) text_secondary: &'static str,
    pub(super) card_bg: String,
    pub(super) card_stroke: &'static str,
    pub(super) fc_stroke: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct BackgroundCtx {
    pub(super) href_xml: Option<String>,
    pub(super) overlay_rgba: &'static str,
}

/// SVG 外部模板渲染入口（BestN / Song）。
///
/// 设计原则：
/// - 模板文件位于 `resources/templates/image/{kind}/{id}.svg.jinja`；
/// - Rust 负责：资源 href 选择、基础布局计算（卡片坐标/尺寸）、格式化与转义；
/// - 模板负责：卡片内部布局与字段排列（可自由调整）。
static TEMPLATE_ENV: OnceLock<Environment<'static>> = OnceLock::new();

pub(super) fn template_base_dir() -> PathBuf {
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

pub(super) fn render_template<T: Serialize>(
    template_name: &str,
    ctx: &T,
) -> Result<String, AppError> {
    let env = get_template_env();
    let tpl = env.get_template(template_name).map_err(|e| {
        AppError::ImageRendererError(format!("加载 SVG 模板失败（{template_name}）: {e}"))
    })?;
    tpl.render(ctx).map_err(|e| {
        AppError::ImageRendererError(format!("渲染 SVG 模板失败（{template_name}）: {e}"))
    })
}

pub(super) fn clamp_template_id(input: Option<&str>) -> &str {
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

pub(super) fn wrap_by_display_width(text: &str, max_width: usize, max_lines: usize) -> Vec<String> {
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

pub(super) fn truncate_with_ellipsis(text: &str, max_width: usize) -> String {
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

#[derive(Clone)]
pub(super) struct JsonOverrideCacheEntry<T> {
    mtime: Option<SystemTime>,
    len: u64,
    status: JsonOverrideStatus,
    value: Option<T>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum JsonOverrideStatus {
    Ok,
    ParseError,
    ReadError,
}

pub(super) fn read_json_override_cached<T>(
    cache: &RwLock<HashMap<PathBuf, JsonOverrideCacheEntry<T>>>,
    cfg_path: &Path,
) -> Option<T>
where
    T: DeserializeOwned + Clone,
{
    let Ok(meta) = std::fs::metadata(cfg_path) else {
        let mut map = cache
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        map.remove(cfg_path);
        return None;
    };

    let len = meta.len();
    let mtime = meta.modified().ok();

    {
        let map = cache
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(entry) = map.get(cfg_path)
            && entry.len == len
            && entry.mtime == mtime
        {
            match entry.status {
                JsonOverrideStatus::Ok => return entry.value.clone(),
                JsonOverrideStatus::ParseError => return None,
                JsonOverrideStatus::ReadError => { /* fallthrough: retry */ }
            }
        }
    }

    let Ok(s) = std::fs::read_to_string(cfg_path) else {
        let mut map = cache
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        map.insert(
            cfg_path.to_path_buf(),
            JsonOverrideCacheEntry {
                mtime,
                len,
                status: JsonOverrideStatus::ReadError,
                value: None,
            },
        );
        return None;
    };

    let parsed = serde_json::from_str::<T>(&s).ok();
    let status = if parsed.is_some() {
        JsonOverrideStatus::Ok
    } else {
        JsonOverrideStatus::ParseError
    };

    let mut map = cache
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    map.insert(
        cfg_path.to_path_buf(),
        JsonOverrideCacheEntry {
            mtime,
            len,
            status,
            value: parsed.clone(),
        },
    );
    parsed
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod json_override_cache_tests {
    use super::*;

    use serde::Deserialize;

    #[derive(Clone, Debug, Deserialize)]
    #[serde(default)]
    struct TestBnTemplateLayout {
        width: u32,
        columns: u32,
        card_gap: u32,
        song_name_max_width: usize,
    }

    impl Default for TestBnTemplateLayout {
        fn default() -> Self {
            Self {
                width: 1200,
                columns: 3,
                card_gap: 12,
                song_name_max_width: 28,
            }
        }
    }

    #[derive(Clone, Debug, Deserialize)]
    #[serde(default)]
    struct TestSongTemplateLayout {
        width: u32,
        height: u32,
        padding: f64,
    }

    impl Default for TestSongTemplateLayout {
        fn default() -> Self {
            Self {
                width: 1400,
                height: 800,
                padding: 40.0,
            }
        }
    }

    fn temp_dir(prefix: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("phi-backend-{prefix}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    fn new_cache<T>() -> RwLock<HashMap<PathBuf, JsonOverrideCacheEntry<T>>> {
        RwLock::new(HashMap::new())
    }

    #[test]
    fn bn_layout_override_updates_on_file_change_and_clears_on_delete() {
        let cache = new_cache::<TestBnTemplateLayout>();
        let dir = temp_dir("layout-cache-bn");
        let cfg = dir.join("bn.json");

        std::fs::write(&cfg, r#"{"width":1001,"columns":2}"#).expect("write v1");
        let v1 = read_json_override_cached(&cache, &cfg).expect("read v1");
        assert_eq!(v1.width, 1001);
        assert_eq!(v1.columns, 2);
        // 缺省字段仍保持默认值。
        assert_eq!(v1.card_gap, TestBnTemplateLayout::default().card_gap);

        // 通过 len 变化确保触发失效，避免依赖 mtime 精度。
        std::fs::write(
            &cfg,
            r#"{"width":1002,"columns":4,"song_name_max_width":30}"#,
        )
        .expect("write v2");
        let v2 = read_json_override_cached(&cache, &cfg).expect("read v2");
        assert_eq!(v2.width, 1002);
        assert_eq!(v2.columns, 4);
        assert_eq!(v2.song_name_max_width, 30);

        std::fs::remove_file(&cfg).expect("remove cfg");
        assert!(read_json_override_cached::<TestBnTemplateLayout>(&cache, &cfg).is_none());

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn bn_layout_override_recovers_after_parse_error_on_change() {
        let cache = new_cache::<TestBnTemplateLayout>();
        let dir = temp_dir("layout-cache-bn-invalid");
        let cfg = dir.join("bn.json");

        std::fs::write(&cfg, "{not-json").expect("write invalid");
        assert!(read_json_override_cached::<TestBnTemplateLayout>(&cache, &cfg).is_none());

        std::fs::write(&cfg, r#"{"width":1234,"columns":3}"#).expect("write fixed");
        let v = read_json_override_cached(&cache, &cfg).expect("read fixed");
        assert_eq!(v.width, 1234);
        assert_eq!(v.columns, 3);

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn song_layout_override_updates_on_file_change_and_clears_on_delete() {
        let cache = new_cache::<TestSongTemplateLayout>();
        let dir = temp_dir("layout-cache-song");
        let cfg = dir.join("song.json");

        std::fs::write(&cfg, r#"{"width":701,"height":401}"#).expect("write v1");
        let v1 = read_json_override_cached(&cache, &cfg).expect("read v1");
        assert_eq!(v1.width, 701);
        assert_eq!(v1.height, 401);
        assert_eq!(v1.padding, TestSongTemplateLayout::default().padding);

        std::fs::write(&cfg, r#"{"width":702,"height":402,"padding":12.0}"#).expect("write v2");
        let v2 = read_json_override_cached(&cache, &cfg).expect("read v2");
        assert_eq!(v2.width, 702);
        assert_eq!(v2.height, 402);
        assert_eq!(v2.padding, 12.0);

        std::fs::remove_file(&cfg).expect("remove cfg");
        assert!(read_json_override_cached::<TestSongTemplateLayout>(&cache, &cfg).is_none());

        let _ = std::fs::remove_dir_all(dir);
    }
}
