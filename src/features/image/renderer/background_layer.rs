use std::fmt::Write;

use crate::error::AppError;

use super::svg_error::svg_fmt_error;
use super::text::escape_xml;

pub(super) const BACKGROUND_OVERLAY_WHITE: &str = "rgba(247, 250, 255, 0.78)";
pub(super) const BACKGROUND_OVERLAY_DARK: &str = "rgba(20, 24, 38, 0.7)";
pub(super) const BACKGROUND_FALLBACK_GRADIENT: &str = "url(#bg-gradient)";

pub(super) struct SvgBackgroundLayerRenderContext<'a> {
    pub(super) svg: &'a mut String,
    pub(super) background_image_href: Option<String>,
    pub(super) overlay_fill: &'a str,
    pub(super) fallback_fill: &'a str,
}

pub(super) fn write_svg_background_layer(
    ctx: SvgBackgroundLayerRenderContext<'_>,
) -> Result<(), AppError> {
    let SvgBackgroundLayerRenderContext {
        svg,
        background_image_href,
        overlay_fill,
        fallback_fill,
    } = ctx;

    if let Some(href) = background_image_href {
        let href_xml = escape_xml(&href);
        writeln!(
            svg,
            r#"<image href="{href_xml}" x="0" y="0" width="100%" height="100%" preserveAspectRatio="xMidYMid slice" filter="url(#bg-blur)" />"#
        )
        .map_err(svg_fmt_error)?;
        writeln!(
            svg,
            r#"<rect width="100%" height="100%" fill="{overlay_fill}" />"#
        )
        .map_err(svg_fmt_error)?;
    } else {
        writeln!(
            svg,
            r#"<rect width="100%" height="100%" fill="{fallback_fill}"/>"#
        )
        .map_err(svg_fmt_error)?;
    }

    Ok(())
}
