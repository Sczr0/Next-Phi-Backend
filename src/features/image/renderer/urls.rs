use std::fmt::Write;
use std::path::Path;

use crate::config::AppConfig;

use super::super::{cover_loader, signing};

/// 将本地路径转换为对外可访问的 URL（`base_url/relative/path`），用于浏览器渲染 SVG。
pub(super) fn to_public_url_for_base(
    path: &Path,
    base_dir: &Path,
    base_url: &str,
) -> Option<String> {
    let rel = path.strip_prefix(base_dir).ok()?;
    let rel = rel.to_string_lossy().replace('\\', "/");
    Some(format!(
        "{}/{}",
        base_url.trim_end_matches('/'),
        rel.trim_start_matches('/')
    ))
}

/// 将字符串编码为 URL 路径片段（UTF-8 字节逐字节百分号编码），避免空格/Unicode/保留字符导致链接不可用。
fn url_encode_path_segment(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for &b in input.as_bytes() {
        let ch = b as char;
        let is_unreserved = ch.is_ascii_alphanumeric() || matches!(ch, '-' | '.' | '_' | '~');
        if is_unreserved {
            out.push(ch);
        } else {
            let _ = write!(&mut out, "%{b:02X}");
        }
    }
    out
}

fn is_http_url(input: &str) -> bool {
    input.starts_with("http://") || input.starts_with("https://")
}

#[derive(Debug, Clone, Copy)]
pub(super) enum ExternalIllustrationDirMode {
    Legacy,
    Lilith,
}

fn normalize_external_illustration_dir_mode(input: &str) -> ExternalIllustrationDirMode {
    match input.trim().to_ascii_lowercase().as_str() {
        "lilith" => ExternalIllustrationDirMode::Lilith,
        _ => ExternalIllustrationDirMode::Legacy,
    }
}

fn normalize_external_illustration_ext(input: &str) -> &'static str {
    match input.trim().to_ascii_lowercase().as_str() {
        "webp" => "webp",
        "avif" => "avif",
        "jpg" | "jpeg" => "jpg",
        _ => "png",
    }
}

pub(super) fn remote_illustration_dir_for_category(
    category: &str,
    mode: ExternalIllustrationDirMode,
) -> Option<&'static str> {
    match mode {
        ExternalIllustrationDirMode::Legacy => match category {
            "ill" => Some("illustration"),
            "illLow" => Some("illustrationLowRes"),
            "illBlur" => Some("illustrationBlur"),
            _ => None,
        },
        ExternalIllustrationDirMode::Lilith => match category {
            "ill" => Some("ill"),
            "illLow" => Some("illLow"),
            "illBlur" => Some("illBlur"),
            _ => None,
        },
    }
}

fn external_illustration_mode_and_ext() -> (ExternalIllustrationDirMode, &'static str) {
    let resources = &AppConfig::global().resources;
    (
        normalize_external_illustration_dir_mode(&resources.external_illustration_dir_mode),
        normalize_external_illustration_ext(&resources.external_illustration_ext),
    )
}

/// 从URL中提取路径部分（如 `https://example.com/lilith` -> `/lilith`，`https://example.com` -> `""`）
fn url_path_prefix(base_url: &str) -> &str {
    let after_scheme = base_url
        .split_once("://")
        .map_or(base_url, |(_, rest)| rest);
    after_scheme.find('/').map_or("", |i| &after_scheme[i..])
}

fn signed_resource_path(base_url: &str, resource_path: &str) -> String {
    let Some(signing_config) = AppConfig::global()
        .resources
        .illustration_signing
        .as_ref()
        .filter(|signing_config| signing_config.enabled && !signing_config.key.is_empty())
    else {
        return resource_path.to_string();
    };

    // CDN 签名 URL 的 path 需要包含 base_url 中已有的路径前缀。
    let prefix = url_path_prefix(base_url);
    let sign_path = format!("{prefix}{resource_path}");
    let signed = signing::sign_url(signing_config, &sign_path);

    // sign_url 返回带前缀的 path，这里去掉 base_url 前缀后再拼回外部基地址。
    signed.strip_prefix(prefix).unwrap_or(&signed).to_string()
}

fn remote_illustration_resource_path(remote_dir: &str, song_id: &str, ext: &str) -> String {
    format!("/{remote_dir}/{}.{}", url_encode_path_segment(song_id), ext)
}

fn signed_public_resource_url(base_url: &str, resource_path: &str) -> String {
    let final_path = signed_resource_path(base_url, resource_path);
    format!("{}{}", base_url.trim_end_matches('/'), final_path)
}

pub(super) fn build_remote_illustration_url_with_options(
    public_illustration_base_url: &str,
    song_id: &str,
    mode: ExternalIllustrationDirMode,
    ext: &str,
    low_res: bool,
) -> String {
    let category = if low_res { "illLow" } else { "ill" };
    let remote_dir = remote_illustration_dir_for_category(category, mode).unwrap_or("illustration");
    let resource_path = remote_illustration_resource_path(remote_dir, song_id, ext);

    signed_public_resource_url(public_illustration_base_url, &resource_path)
}

pub(super) fn build_remote_illustration_url(
    public_illustration_base_url: &str,
    song_id: &str,
) -> String {
    let (mode, ext) = external_illustration_mode_and_ext();
    build_remote_illustration_url_with_options(
        public_illustration_base_url,
        song_id,
        mode,
        ext,
        false,
    )
}

pub(super) fn build_remote_illustration_low_res_url(
    public_illustration_base_url: &str,
    song_id: &str,
) -> String {
    let (mode, ext) = external_illustration_mode_and_ext();
    build_remote_illustration_url_with_options(
        public_illustration_base_url,
        song_id,
        mode,
        ext,
        true,
    )
}

pub(super) fn to_somnia_public_url_for_base(
    path: &Path,
    base_dir: &Path,
    base_url: &str,
) -> Option<String> {
    if !is_http_url(base_url) {
        return None;
    }
    let rel = path.strip_prefix(base_dir).ok()?;
    let category = rel.components().next()?.as_os_str().to_string_lossy();
    let song_id = rel.file_stem()?.to_string_lossy();
    let (mode, ext) = external_illustration_mode_and_ext();
    let remote_dir = remote_illustration_dir_for_category(category.as_ref(), mode)?;
    let resource_path = remote_illustration_resource_path(remote_dir, song_id.as_ref(), ext);

    Some(signed_public_resource_url(base_url, &resource_path))
}

pub(super) fn to_public_illustration_url(path: &Path, base_url: &str) -> Option<String> {
    let base_dir = cover_loader::covers_dir();
    to_somnia_public_url_for_base(path, &base_dir, base_url)
        .or_else(|| to_public_url_for_base(path, &base_dir, base_url))
}

/// 返回适合放入 `<image href>` 的引用：
/// - `embed_images=true`：优先 data URI（仅小图）；大图回退为 URL/文件路径
/// - `embed_images=false`：优先 URL（若提供 `public_illustration_base_url`），否则返回文件路径
pub(super) fn get_image_href(
    path: &Path,
    embed_images: bool,
    public_illustration_base_url: Option<&str>,
) -> Option<String> {
    if !embed_images {
        if let Some(base) = public_illustration_base_url {
            // 即使本地不存在，也允许基于路径结构直接生成可访问 URL（用于完全外部化曲绘资源）。
            if let Some(url) = to_public_illustration_url(path, base) {
                return Some(url);
            }
        }
        if path.exists() {
            return Some(path.to_string_lossy().into_owned());
        }
        // 兜底：路径异常时尝试小图内嵌，尽量保证可渲染
        return super::resources::get_background_image(path);
    }

    let href = super::resources::get_background_image(path)?;
    if let Some(base) = public_illustration_base_url
        && !href.starts_with("data:")
    {
        return to_public_illustration_url(Path::new(&href), base).or(Some(href));
    }
    Some(href)
}
