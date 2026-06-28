use axum::{
    Json,
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
};
use chrono::Utc;

use crate::{
    config::AppConfig,
    error::AppError,
    features::image::{
        renderer::{self, PlayerStats},
        signing,
        types::RenderUserBnRequest,
    },
    state::AppState,
};

use super::{
    context::image_footer_text,
    output::{
        ImageOutputCacheSpec, ImageQueryOpts, SvgRenderOptions, image_content_headers,
        render_svg_output_bytes, validate_image_query_opts,
    },
    runtime::{acquire_render_permit, blocking_join_error, spawn_blocking_svg_generation},
    score::u32_from_usize,
    user_bn_compute::{self, UserBnComputeOutput},
    usize_from_u32,
};

#[utoipa::path(
    post,
    path = "/image/bn/user",
    summary = "生成用户自报成绩的 BestN 图片",
    description = "无需存档，直接提交若干条用户自报成绩，计算 RKS 排序并生成 BestN 图片；支持水印解除口令。",
    request_body = RenderUserBnRequest,
    params(
        ("format" = Option<String>, Query, description = "输出格式：png|jpeg|webp|svg，默认 png"),
        ("template" = Option<String>, Query, description = "SVG 模板 ID：对应 resources/templates/image/bn/{id}.svg.jinja（不传则使用内置手写 SVG）"),
        ("width" = Option<u32>, Query, description = "目标宽度像素：按宽度同比例缩放"),
        ("webp_quality" = Option<u8>, Query, description = "WebP 质量：1-100（仅在 format=webp 时有效，默认 80）"),
        ("webp_lossless" = Option<bool>, Query, description = "WebP 无损模式（仅在 format=webp 时有效，默认 false）")
    ),
    responses(
        (
            status = 200,
            description = "图片（由 query format 决定）",
            content(
                (crate::features::image::types::BinaryImage = "image/png"),
                (crate::features::image::types::BinaryImage = "image/jpeg"),
                (crate::features::image::types::BinaryImage = "image/webp"),
                (String = "image/svg+xml")
            )
        ),
        (
            status = 400,
            description = "请求参数错误",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        ),
        (
            status = 404,
            description = "歌曲未找到（unique search）",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        ),
        (
            status = 409,
            description = "歌曲结果不唯一（unique search）",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        ),
        (
            status = 422,
            description = "参数校验失败/渲染错误",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        ),
        (
            status = 500,
            description = "服务器内部错误",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        )
    ),
    tag = "Image"
)]
pub async fn render_bn_user(
    State(state): State<AppState>,
    Query(q): Query<ImageQueryOpts>,
    Json(req): Json<RenderUserBnRequest>,
) -> Result<impl IntoResponse, AppError> {
    validate_image_query_opts(&q)?;

    let RenderUserBnRequest {
        theme,
        nickname,
        unlock_password,
        scores,
    } = req;

    // 限制 user 自报成绩条数，避免大输入放大 CPU/内存（排序/推分求解均会随条数线性/超线性增长）。
    let cfg = AppConfig::global();
    let max_scores = usize_from_u32(cfg.image.max_user_scores);
    if max_scores > 0 && scores.len() > max_scores {
        return Err(AppError::Validation(format!(
            "scores 条数超过上限: {} > {}",
            scores.len(),
            max_scores
        )));
    }

    let records_len = scores.len();
    let n = records_len.max(1);

    // 解析成绩、排序、推分与统计属于 CPU 密集任务：移出 Tokio worker，避免影响吞吐与尾延迟。
    let UserBnComputeOutput {
        records,
        push_acc_map,
        exact_rks,
        ap_top_3_avg,
        best_27_avg,
        ap_top_3_scores,
    } = {
        let song_catalog = state.song_catalog.clone();
        let join = tokio::task::spawn_blocking(move || {
            user_bn_compute::build_user_bn_compute_output(scores, song_catalog)
        })
        .await;
        join.map_err(blocking_join_error)??
    };

    // 昵称
    let display_name = nickname.unwrap_or_else(|| "Phigros Player".into());

    // 水印控制：默认启用配置中的显式/隐式；若提供了正确的解除口令，则同时关闭二者
    let unlocked = cfg.watermark.is_unlock_valid(unlock_password.as_deref());
    let explicit = if unlocked {
        false
    } else {
        cfg.watermark.explicit_badge
    };
    let implicit = if unlocked {
        false
    } else {
        cfg.watermark.implicit_pixel
    };

    let stats = PlayerStats {
        ap_top_3_avg,
        best_27_avg,
        real_rks: Some(exact_rks),
        player_name: Some(display_name),
        update_time: Utc::now(),
        n: u32_from_usize(n),
        ap_top_3_scores,
        challenge_rank: None,
        data_string: None,
        custom_footer_text: image_footer_text(),
        is_user_generated: explicit,
    };

    let output = ImageOutputCacheSpec::from_query(&q, false);
    let fmt_code = output.fmt_code;
    let public_illustration_base_url = output.public_illustration_base_url;
    // 等待许可与渲染分段计时
    let _render_permit = acquire_render_permit(&state).await?;

    // SVG 生成会触发磁盘 IO/图片解码/目录索引等阻塞操作，必须移出 tokio worker。
    let svg_options = SvgRenderOptions::from_query(public_illustration_base_url, &q);
    let records_for_sig = records.clone();
    let svg = spawn_blocking_svg_generation(move || {
        renderer::generate_svg_string(
            &records,
            &stats,
            Some(&push_acc_map),
            &theme,
            false,
            svg_options.public_base_url(),
            svg_options.template_id(),
        )
    })
    .await?;

    // 签名注入：用户自报成绩无 user_hash，uid 标记为 anon
    let (signed_svg, sig_header) = {
        let signing_cfg = &AppConfig::global().image.signing;
        let mut maybe_sig: Option<signing::SvgSignature> = None;
        let signed = if signing_cfg.is_v4_usable() {
            let score_tuples: Vec<(&str, &str, f64, f64)> = records_for_sig
                .iter()
                .map(|r| {
                    (
                        r.song_id.as_str(),
                        r.difficulty.as_str(),
                        r.score.unwrap_or(0.0),
                        r.acc,
                    )
                })
                .collect();
            if let Some(sig) = signing::sign_svg_v4(&svg, signing_cfg, &score_tuples, None) {
                maybe_sig = Some(sig);
                signing::inject_sig_footer(&svg, maybe_sig.as_ref().unwrap())
            } else {
                svg
            }
        } else if signing_cfg.is_usable() {
            if let Some(sig) = signing::sign_svg(&svg, signing_cfg, None) {
                maybe_sig = Some(sig);
                signing::inject_sig_footer(&svg, maybe_sig.as_ref().unwrap())
            } else {
                svg
            }
        } else {
            svg
        };
        let line = maybe_sig.map(|s| signing::build_sig_line_any(&s));
        (signed, line)
    };

    let (bytes, content_type) = render_svg_output_bytes(signed_svg, fmt_code, implicit, &q).await?;

    // 统计：用户自报 BestN 图片生成
    if let Some(stats_handle) = state.stats.as_ref() {
        let extra = serde_json::json!({
            "scores_len": records_len,
            "unlocked": unlocked
        });
        stats_handle.track_feature("bestn_user", "generate_image", None, Some(extra));
    }

    let mut headers = image_content_headers(content_type);
    if let Some(ref line) = sig_header {
        headers.insert("X-Lilith-Sig", line.parse().unwrap());
    }
    Ok((StatusCode::OK, headers, bytes))
}
