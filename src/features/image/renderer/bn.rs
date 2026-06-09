use std::collections::HashMap;
use std::fmt::Write;

use crate::error::AppError;
use crate::features::image::Theme;
use crate::rks_contract::engine;

use super::bn_background::select_random_background;
use super::bn_card_list::{BnCardSectionsRenderContext, write_score_sections};
use super::bn_defs::{
    BnBackgroundLayerRenderContext, BnDefsRenderContext, write_background_layer, write_defs,
    write_svg_open,
};
use super::bn_layout::BnLayout;
use super::bn_sections::{
    BnFooterRenderContext, BnHeaderRenderContext, write_footer, write_header,
};
use super::bn_theme::BnThemePalette;
use super::svg_error::svg_fmt_error;
use super::{PlayerStats, RenderRecord, template_bn};

// --- SVG 生成函数 ---

pub(super) fn generate_svg_string<S>(
    scores: &[RenderRecord],
    stats: &PlayerStats,
    push_acc_map: Option<&HashMap<String, engine::PushAccHint, S>>, // 预先计算的推分提示映射，键为"曲目ID-难度"
    #[allow(clippy::trivially_copy_pass_by_ref)] theme: &Theme,     // 渲染主题
    embed_images: bool,
    // 若提供，则将曲绘引用改为可被浏览器访问的 URL（例如 `/_ill`）
    public_illustration_base_url: Option<&str>,
    // 外部模板 ID：对应 `resources/templates/image/bn/{id}.svg.jinja`（为空则使用内置手写 SVG 实现）。
    template_id: Option<&str>,
) -> Result<String, AppError>
where
    S: std::hash::BuildHasher,
{
    if template_id.is_some() {
        return template_bn::generate_bn_svg_with_template(
            scores,
            stats,
            push_acc_map,
            *theme,
            embed_images,
            public_illustration_base_url,
            template_id,
        );
    }
    let _start_time = std::time::Instant::now();
    let layout = BnLayout::new(scores.len(), stats.ap_top_3_scores.is_empty());
    let width = layout.width;
    let header_height = layout.header_height;
    let footer_height = layout.footer_height;
    let total_height = layout.total_height;

    let palette = BnThemePalette::from_theme(theme);
    // 预分配 SVG 字符串容量，减少多次分配与拷贝
    let mut svg = String::with_capacity(200_000);
    let t0 = std::time::Instant::now();

    let background = select_random_background(
        theme,
        embed_images,
        public_illustration_base_url,
        width,
        total_height,
        palette.normal_card_stroke_color.clone(),
    );
    let background_image_href = background.image_href;
    let normal_card_stroke_color = background.normal_card_stroke_color;

    write_svg_open(&mut svg, width, total_height)?;
    let defs_timing = write_defs(BnDefsRenderContext {
        svg: &mut svg,
        theme,
        palette: &palette,
        normal_card_stroke_color: &normal_card_stroke_color,
        started_at: &t0,
    })?;
    let t_defs = defs_timing.style_elapsed;
    let t_after_defs = defs_timing.defs_elapsed;
    write_background_layer(BnBackgroundLayerRenderContext {
        svg: &mut svg,
        theme,
        background_image_href,
    })?;

    write_header(BnHeaderRenderContext {
        svg: &mut svg,
        stats,
        width,
        header_height,
        card_stroke_color: palette.card_stroke_color,
        text_secondary_color: palette.text_secondary_color,
    })?;

    let card_timings = write_score_sections(BnCardSectionsRenderContext {
        svg: &mut svg,
        scores,
        stats,
        push_acc_map,
        layout: &layout,
        theme,
        embed_images,
        public_illustration_base_url,
        started_at: &t0,
    })?;
    let t_after_ap = card_timings.after_ap_elapsed;
    let t_after_main = card_timings.after_main_elapsed;

    write_footer(BnFooterRenderContext {
        svg: &mut svg,
        stats,
        width,
        total_height,
        footer_height,
    })?;

    writeln!(svg, "</svg>").map_err(svg_fmt_error)?;

    // 分段计时日志：defs/ap/main/total
    tracing::info!(
        "SVG生成分段: defs={:?}, ap={:?}, main={:?}, 总计={:?}",
        t_defs,
        t_after_ap.saturating_sub(t_after_defs),
        t_after_main.saturating_sub(t_after_ap),
        t0.elapsed(),
    );

    Ok(svg)
}
