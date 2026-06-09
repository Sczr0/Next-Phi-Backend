use axum::body::Bytes;
use axum::http::{HeaderValue, header};
use serde::Deserialize;

use crate::{config::AppConfig, error::AppError, features::image::renderer};

use super::Theme;

/// 图片输出选项（通过 Query 传入，避免破坏现有 JSON 请求体）
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ImageQueryOpts {
    /// 输出格式：png、jpeg、webp 或 svg（默认 png）
    #[serde(default)]
    pub(super) format: Option<String>,
    /// SVG 模板：对应 `resources/templates/image/{kind}/{template}.svg.jinja`（不传则使用内置手写 SVG 实现）
    #[serde(default)]
    pub(super) template: Option<String>,
    /// 目标宽度：按宽度同比例缩放（可选）
    #[serde(default)]
    pub(super) width: Option<u32>,
    /// WebP 质量：1-100（仅在 format=webp 时有效，默认 80）
    #[serde(default)]
    pub(super) webp_quality: Option<u8>,
    /// WebP 无损模式：true=无损，false=有损（仅在 format=webp 时有效，默认 false）
    #[serde(default)]
    pub(super) webp_lossless: Option<bool>,
}

impl ImageQueryOpts {
    pub(crate) fn into_open_svg_only(mut self) -> Result<Self, AppError> {
        if let Some(fmt) = self.format.as_deref()
            && !fmt.eq_ignore_ascii_case("svg")
        {
            return Err(AppError::Validation(
                "开放平台图片接口仅支持 format=svg".to_string(),
            ));
        }
        self.format = Some("svg".to_string());
        Ok(self)
    }
}

/// SVG 返回时，曲绘资源的同源访问前缀（由 `src/main.rs` 提供静态目录服务）。
const ILLUSTRATION_PUBLIC_BASE_URL: &str = "/_ill";

pub(super) fn is_svg_format(q: &ImageQueryOpts) -> bool {
    q.format
        .as_deref()
        .is_some_and(|fmt| fmt.eq_ignore_ascii_case("svg"))
}

pub(super) fn format_code(q: &ImageQueryOpts) -> &'static str {
    if is_svg_format(q) {
        return "svg";
    }
    match q.format.as_deref() {
        Some("jpeg" | "jpg") => "jpg",
        Some("webp") => "webp",
        _ => "png",
    }
}

pub(super) fn content_type_from_fmt_code(code: &str) -> &'static str {
    match code {
        "svg" => "image/svg+xml; charset=utf-8",
        "jpg" => "image/jpeg",
        "webp" => "image/webp",
        _ => "image/png",
    }
}

pub(super) fn validate_image_query_opts(q: &ImageQueryOpts) -> Result<(), AppError> {
    if let Some(quality) = q.webp_quality
        && quality > 100
    {
        return Err(AppError::Validation(
            "webp_quality 必须在 1-100 范围内".to_string(),
        ));
    }

    Ok(())
}

/// 规范化 WebP 参数在缓存键中的表达，避免无关/无效参数造成缓存碎片。
///
/// - 非 WebP 输出：`webp_quality/webp_lossless` 一律归零（忽略 query 里多余参数）。
/// - WebP 无损：质量参数无意义，归零（避免 lossless=true 但质量变化导致碎片）。
/// - WebP 有损：质量归一化到 1-100（缺省 80）。
pub(super) fn normalized_webp_cache_params(fmt_code: &str, q: &ImageQueryOpts) -> (u8, u8) {
    if fmt_code != "webp" {
        return (0, 0);
    }

    let lossless = q.webp_lossless.unwrap_or(false);
    if lossless {
        return (0, 1);
    }

    let quality = q.webp_quality.unwrap_or(80).clamp(1, 100);
    (quality, 0)
}

pub(super) fn normalized_template_cache_code(q: &ImageQueryOpts) -> String {
    // 缓存键需要与 renderer 的模板选择语义一致：
    // - 未指定模板：走 legacy（内置手写 SVG）
    // - 指定模板但非法：归一到 default（避免缓存碎片 + 避免路径穿越）
    match q.template.as_deref() {
        None => "legacy".to_string(),
        Some(s) => {
            if s.is_empty()
                || s.len() > 64
                || !s
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
            {
                "default".to_string()
            } else {
                s.to_string()
            }
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct ImageOutputCacheSpec {
    pub(super) fmt_code: &'static str,
    pub(super) content_type: &'static str,
    pub(super) embed_images_effective: bool,
    pub(super) public_illustration_base_url: Option<&'static str>,
    pub(super) width_code: u32,
    pub(super) tpl_code: String,
    pub(super) webp_quality_code: u8,
    pub(super) webp_lossless_code: u8,
}

impl ImageOutputCacheSpec {
    pub(super) fn from_query(q: &ImageQueryOpts, requested_embed_images: bool) -> Self {
        let fmt_code = format_code(q);
        let is_svg = fmt_code == "svg";
        let embed_images_effective = !is_svg && requested_embed_images;
        let public_illustration_base_url = is_svg.then(|| {
            AppConfig::global()
                .resources
                .illustration_external_base_url
                .as_deref()
                .unwrap_or(ILLUSTRATION_PUBLIC_BASE_URL)
        });
        let width_code = if is_svg { 0 } else { q.width.unwrap_or(0) };
        let tpl_code = normalized_template_cache_code(q);
        let (webp_quality_code, webp_lossless_code) = normalized_webp_cache_params(fmt_code, q);

        Self {
            fmt_code,
            content_type: content_type_from_fmt_code(fmt_code),
            embed_images_effective,
            public_illustration_base_url,
            width_code,
            tpl_code,
            webp_quality_code,
            webp_lossless_code,
        }
    }

    pub(super) fn bn_cache_key(
        &self,
        user_hash: &str,
        n: u32,
        updated: &str,
        theme: Theme,
    ) -> String {
        format!(
            "{}:bn:{}:{}:{}:{}:{}:{}:{}:{}:{}",
            user_hash,
            n.max(1),
            updated,
            theme_cache_code(theme),
            i32::from(self.embed_images_effective),
            self.tpl_code,
            self.fmt_code,
            self.width_code,
            self.webp_quality_code,
            self.webp_lossless_code
        )
    }

    pub(super) fn song_cache_key(&self, user_hash: &str, song_id: &str, updated: &str) -> String {
        format!(
            "{}:song:{}:{}:{}:{}:{}:{}:{}:{}:{}",
            user_hash,
            song_id,
            updated,
            "d",
            i32::from(self.embed_images_effective),
            self.tpl_code,
            self.fmt_code,
            self.width_code,
            self.webp_quality_code,
            self.webp_lossless_code
        )
    }
}

fn theme_cache_code(theme: Theme) -> &'static str {
    match theme {
        Theme::White => "w",
        Theme::Black => "b",
    }
}

pub(super) fn image_content_headers(content_type: &'static str) -> axum::http::HeaderMap {
    let mut headers = axum::http::HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    headers
}

pub(super) struct SvgRenderOptions {
    public_base_url: Option<String>,
    template_id: Option<String>,
}

impl SvgRenderOptions {
    pub(super) fn from_query(
        public_illustration_base_url: Option<&str>,
        q: &ImageQueryOpts,
    ) -> Self {
        Self {
            public_base_url: public_illustration_base_url.map(std::string::ToString::to_string),
            template_id: q.template.clone(),
        }
    }

    pub(super) fn public_base_url(&self) -> Option<&str> {
        self.public_base_url.as_deref()
    }

    pub(super) fn template_id(&self) -> Option<&str> {
        self.template_id.as_deref()
    }
}

pub(super) async fn render_svg_output_bytes(
    svg: String,
    fmt_code: &str,
    is_user_generated: bool,
    q: &ImageQueryOpts,
) -> Result<(Bytes, &'static str), AppError> {
    // SVG 直接返回文本字节；其他格式进入统一栅格化与编码入口。
    if fmt_code == "svg" {
        return Ok((
            Bytes::from(svg.into_bytes()),
            content_type_from_fmt_code(fmt_code),
        ));
    }

    let (bytes, content_type) = renderer::render_svg_unified_async(
        svg,
        is_user_generated,
        q.format.as_deref(),
        q.width,
        q.webp_quality,
        q.webp_lossless,
    )
    .await?;
    Ok((Bytes::from(bytes), content_type))
}
