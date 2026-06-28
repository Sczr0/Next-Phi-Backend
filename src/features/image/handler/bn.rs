use std::time::Instant;

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
};
use tracing::debug;

use crate::{
    config::AppConfig,
    error::AppError,
    features::image::{
        renderer::{self, PlayerStats},
        signing,
        types::RenderBnRequest,
    },
    state::AppState,
};

use super::{
    bn_compute::{self, BnComputeInput, BnComputeOutput},
    context::{
        derive_image_user_identity, ensure_image_user_not_banned, image_cache_enabled,
        image_footer_text,
    },
    nickname::resolve_display_name,
    output::{
        ImageOutputCacheSpec, ImageQueryOpts, SvgRenderOptions, image_content_headers,
        render_svg_output_bytes, validate_image_query_opts,
    },
    runtime::{
        acquire_render_permit, blocking_join_error, duration_ms_i64, spawn_blocking_svg_generation,
        track_image_event,
    },
    save_flow::{decrypt_image_save_from_meta, fetch_image_save_meta, to_save_source},
};

#[utoipa::path(
    post,
    path = "/image/bn",
    summary = "生成 BestN 汇总图片",
    description = "从官方/外部存档解析玩家成绩，按 RKS 值排序取前 N 条生成 BestN 概览（PNG）。可选内嵌封面与主题切换。",
    request_body = RenderBnRequest,
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
            description = "请求参数错误/认证缺失",
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
pub async fn render_bn(
    State(state): State<AppState>,
    Query(q): Query<ImageQueryOpts>,
    request: axum::extract::Request,
) -> Result<impl IntoResponse, AppError> {
    let (mut req, bearer_state) =
        crate::session_auth::parse_json_with_bearer_state::<RenderBnRequest>(request).await?;
    crate::session_auth::merge_auth_from_bearer_if_missing(
        state.stats_storage.as_ref(),
        &bearer_state,
        &mut req.auth,
    )
    .await?;

    // 全流程计时：从请求进入到返回响应
    let t_total = Instant::now();
    // 存档获取耗时（含认证源构造 + 解密）
    let t_save = Instant::now();
    let t_auth_start = Instant::now();
    let source = to_save_source(&req.auth)?;
    let auth_duration = t_auth_start.elapsed();
    tracing::info!(target: "bestn_performance", "用户凭证验证完成，耗时: {:?}ms", auth_duration.as_millis());

    let taptap_version = req.auth.taptap_version.as_deref();
    // 缓存前移：先拿 updatedAt（作为版本号）再决定是否需要下载/解密/解析存档本体。
    let (meta, updated_for_cache) = fetch_image_save_meta(source, taptap_version).await?;

    validate_image_query_opts(&q)?;

    // SVG 模式强制外链曲绘且不内嵌图片；缓存维度在同一处归一化，避免 hit/put 分叉。
    let output = ImageOutputCacheSpec::from_query(&q, req.embed_images);
    let fmt_code = output.fmt_code;
    let embed_images_effective = output.embed_images_effective;
    let public_illustration_base_url = output.public_illustration_base_url;

    // Cache hit/miss 事件 + 快速返回
    let cache_enabled = image_cache_enabled();
    let (user_hash_for_cache, user_kind_for_cache) =
        derive_image_user_identity(&req.auth, &bearer_state)?;
    ensure_image_user_not_banned(&state, user_hash_for_cache.as_deref()).await?;
    let cache_key = if cache_enabled {
        user_hash_for_cache
            .as_ref()
            .map(|user_hash| output.bn_cache_key(user_hash, req.n, &updated_for_cache, req.theme))
    } else {
        None
    };
    if let (Some(user_hash), Some(key)) = (user_hash_for_cache.as_ref(), cache_key.as_ref()) {
        if let Some(p) = state.bn_image_cache.get(key).await {
            let _cache_duration = Instant::now().elapsed();
            tracing::info!(target: "bestn_performance", "缓存命中，缓存键: {}", key);

            if let Some(h) = state.stats.as_ref() {
                let total_ms = duration_ms_i64(t_total.elapsed());
                track_image_event(
                    h,
                    "/image/bn",
                    "image_cache",
                    "bn_hit",
                    Some(total_ms),
                    Some(user_hash.clone()),
                    serde_json::json!({
                        "cached": true,
                        "user_kind": user_kind_for_cache.as_deref(),
                        "fmt": fmt_code,
                        "tpl": output.tpl_code.as_str(),
                        "width": output.width_code,
                        "webp_quality": output.webp_quality_code,
                        "webp_lossless": output.webp_lossless_code
                    }),
                );
                // 日志：BestN 缓存命中耗时
                debug!(
                    target: "phi_backend::image::bn",
                    total_ms,
                    fmt = fmt_code,
                    width = output.width_code,
                    "BestN 图片缓存命中，整体耗时 {total_ms}ms"
                );
            }
            let headers = image_content_headers(output.content_type);

            let total_duration = t_total.elapsed();
            tracing::info!(target: "bestn_performance", "BestN缓存命中完成，总耗时: {:?}ms (缓存命中)", total_duration.as_millis());
            return Ok((StatusCode::OK, headers, p));
        }
        let _cache_duration = Instant::now().elapsed();
        tracing::info!(target: "bestn_performance", "缓存未命中，缓存键: {}", key);
        if let Some(h) = state.stats.as_ref() {
            track_image_event(
                h,
                "/image/bn",
                "image_cache",
                "bn_miss",
                None,
                Some(user_hash.clone()),
                serde_json::json!({ "cached": false, "user_kind": user_kind_for_cache.as_deref(), "tpl": output.tpl_code.as_str() }),
            );
        }
    }

    // cache miss：下载/解密/解析存档本体
    let parsed = decrypt_image_save_from_meta(meta, state.chart_constants.clone()).await?;
    let save_ms = duration_ms_i64(t_save.elapsed());

    // 扁平化为渲染记录 + 排序与推分预计算耗时
    let n = req.n.max(1);
    let BnComputeOutput {
        top,
        push_acc_map,
        exact_rks,
        ap_top_3_avg,
        best_27_avg,
        ap_top_3_scores,
        challenge_rank,
        data_string,
        update_time,
        flatten_ms,
    } = {
        // 逻辑阶段（扁平化/排序/推分/统计）属于 CPU 密集 + 大量分配，避免阻塞 Tokio worker。
        let chart_constants = state.chart_constants.clone();
        let song_catalog = state.song_catalog.clone();
        let join = tokio::task::spawn_blocking(move || {
            bn_compute::build_bn_compute_output(BnComputeInput {
                parsed,
                chart_constants,
                song_catalog,
                n,
            })
        })
        .await;
        join.map_err(blocking_join_error)?
    };

    let t_nickname_start = Instant::now();
    // 优先级：请求体昵称 > users/me 昵称 > 默认
    let (display_name, nickname_ms) = resolve_display_name(
        req.nickname.clone(),
        req.auth.session_token.clone(),
        req.auth.taptap_version.as_deref(),
    )
    .await;
    let nickname_duration = t_nickname_start.elapsed();
    tracing::info!(target: "bestn_performance", "昵称获取完成: {}, 耗时: {:?}ms", display_name, nickname_duration.as_millis());

    let stats = PlayerStats {
        ap_top_3_avg,
        best_27_avg,
        real_rks: Some(exact_rks),
        player_name: Some(display_name),
        update_time,
        n,
        ap_top_3_scores,
        challenge_rank,
        data_string,
        custom_footer_text: image_footer_text(),
        is_user_generated: false,
    };

    // 等待许可与渲染分段计时
    // 统计用：提前提取曲目 ID，避免后续把 `top` move 进阻塞线程后不可用。
    let bestn_song_ids: Vec<String> = top.iter().map(|r| r.song_id.clone()).collect();

    let render_permit = acquire_render_permit(&state).await?;
    let permits_avail = render_permit.permits_avail;
    let wait_ms = render_permit.wait_ms;
    tracing::info!(target: "bestn_performance", "信号量获取完成，可用许可: {}, 等待时间: {:?}ms, 总获取时间: {:?}ms",
                   permits_avail, wait_ms, render_permit.wait_elapsed.as_millis());

    let t_svg_start = Instant::now();
    // SVG 生成会触发磁盘 IO/图片解码/目录索引等阻塞操作，必须移出 tokio worker。
    let theme = req.theme;
    let svg_options = SvgRenderOptions::from_query(public_illustration_base_url, &q);
    // 克隆一份 top 用于后续 v4 签名计算 Merkle 树（top 将被 move 入 SVG 生成闭包）。
    let top_for_sig = top.clone();
    let svg = spawn_blocking_svg_generation(move || {
        renderer::generate_svg_string(
            &top,
            &stats,
            Some(&push_acc_map),
            &theme,
            embed_images_effective,
            svg_options.public_base_url(),
            svg_options.template_id(),
        )
    })
    .await?;
    let svg_duration = t_svg_start.elapsed();
    let svg_size = svg.len();
    tracing::info!(target: "bestn_performance", "SVG生成完成，SVG大小: {} 字符, 耗时: {:?}ms", svg_size, svg_duration.as_millis());

    // 签名注入：在 SVG 底部追加签名行，并提取签名字符串用于响应头
    let (signed_svg, sig_header) = {
        let signing_cfg = &AppConfig::global().image.signing;
        let mut maybe_sig: Option<signing::SvgSignature> = None;
        let signed = if signing_cfg.is_v4_usable() {
            let score_tuples: Vec<(&str, &str, f64, f64)> = top_for_sig
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
            if let Some(sig) = signing::sign_svg_v4(
                &svg,
                signing_cfg,
                &score_tuples,
                user_hash_for_cache.as_deref(),
            ) {
                maybe_sig = Some(sig);
                signing::inject_sig_footer(&svg, maybe_sig.as_ref().unwrap())
            } else {
                svg
            }
        } else if signing_cfg.is_usable() {
            if let Some(sig) = signing::sign_svg(&svg, signing_cfg, user_hash_for_cache.as_deref())
            {
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

    let t_render_start = Instant::now();
    // 输出格式与宽度处理（svg 直接返回，不做栅格化渲染）
    let (bytes, content_type) = render_svg_output_bytes(signed_svg, fmt_code, false, &q).await?;
    let render_duration = t_render_start.elapsed();
    let render_ms = duration_ms_i64(render_duration);
    let bytes_len = bytes.len();
    tracing::info!(target: "bestn_performance", "图片渲染完成，输出格式: {}, 字节大小: {}, 耗时: {:?}ms",
                   content_type, bytes_len, render_duration.as_millis());

    // 统计：BestN 图片生成（带用户去敏哈希 + 榜单歌曲ID列表 + 用户凭证类型）
    if let Some(stats) = state.stats.as_ref() {
        let extra = serde_json::json!({ "bestn_song_ids": bestn_song_ids, "user_kind": user_kind_for_cache.as_deref() });
        stats.track_feature(
            "bestn",
            "generate_image",
            user_hash_for_cache.clone(),
            Some(extra),
        );
    }

    let mut headers = image_content_headers(content_type);
    // 签名串放入响应头，便于客户端从任意格式（SVG/PNG/JPEG）直接提取验签字段。
    if let Some(ref line) = sig_header {
        headers.insert("X-Lilith-Sig", line.parse().unwrap());
    }

    // Cache put
    let mut cache_put_duration = None;
    if let Some(key) = cache_key {
        let t_cache_put = Instant::now();
        state.bn_image_cache.insert(key, bytes.clone()).await;
        cache_put_duration = Some(t_cache_put.elapsed());
    }
    if let Some(cache_dur) = cache_put_duration {
        tracing::info!(target: "bestn_performance", "缓存存储完成，耗时: {:?}ms", cache_dur.as_millis());
    }

    // Basic render metrics (total time and key阶段耗时)
    if let Some(h) = state.stats.as_ref() {
        let total_ms = duration_ms_i64(t_total.elapsed());
        let logic_ms = total_ms.saturating_sub(save_ms).saturating_sub(render_ms);
        track_image_event(
            h,
            "/image/bn",
            "image_render",
            "bn",
            Some(total_ms),
            None,
            serde_json::json!({
                "permits_avail": permits_avail,
                "save_ms": save_ms,
                "flatten_ms": flatten_ms,
                "logic_ms": logic_ms,
                "nickname_ms": nickname_ms,
                "wait_ms": wait_ms,
                "render_ms": render_ms,
                "bytes": bytes.len(),
                "fmt": fmt_code,
                "width": q.width,
            }),
        );
        // 日志：BestN 渲染全过程耗时
        debug!(
            target: "phi_backend::image::bn",
            total_ms,
            save_ms,
            flatten_ms,
            logic_ms,
            nickname_ms,
            wait_ms,
            render_ms,
            fmt = fmt_code,
            width = ?q.width,
            "BestN 渲染耗时统计：total={total_ms}ms, save={save_ms}ms, flatten={flatten_ms}ms, logic={logic_ms}ms, wait={wait_ms}ms, render={render_ms}ms"
        );
    }
    Ok((StatusCode::OK, headers, bytes))
}
