use std::time::Instant;

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
};

use crate::{
    config::AppConfig,
    error::AppError,
    features::image::{
        renderer::{self, SongRenderData},
        signing,
        types::RenderSongRequest,
    },
    state::AppState,
};

use super::{
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
    song_compute::{self, SongComputeInput, SongComputeOutput},
};

#[utoipa::path(
    post,
    path = "/image/song",
    summary = "生成单曲成绩图片",
    description = "从存档中定位指定歌曲（支持 ID/名称），展示四难度成绩、RKS、推分建议等信息（PNG）。",
    request_body = RenderSongRequest,
    params(
        ("format" = Option<String>, Query, description = "输出格式：png|jpeg|webp|svg，默认 png"),
        ("template" = Option<String>, Query, description = "SVG 模板 ID：对应 resources/templates/image/song/{id}.svg.jinja（不传则使用内置手写 SVG）"),
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
pub async fn render_song(
    State(state): State<AppState>,
    Query(q): Query<ImageQueryOpts>,
    request: axum::extract::Request,
) -> Result<impl IntoResponse, AppError> {
    let (mut req, bearer_state) =
        crate::session_auth::parse_json_with_bearer_state::<RenderSongRequest>(request).await?;
    crate::session_auth::merge_auth_from_bearer_if_missing(
        state.stats_storage.as_ref(),
        &bearer_state,
        &mut req.auth,
    )
    .await?;
    let t_total = std::time::Instant::now();
    let source = to_save_source(&req.auth)?;
    let taptap_version = req.auth.taptap_version.as_deref();
    // 缓存前移：先拿 updatedAt（作为版本号）再决定是否需要下载/解密/解析存档本体。
    let (meta, updated_for_cache) = fetch_image_save_meta(source, taptap_version).await?;

    let song = state
        .song_catalog
        .search_unique(&req.song)
        .map_err(AppError::Search)?;

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
            .map(|user_hash| output.song_cache_key(user_hash, &song.id, &updated_for_cache))
    } else {
        None
    };
    if let (Some(user_hash), Some(key)) = (user_hash_for_cache.as_ref(), cache_key.as_ref()) {
        if let Some(p) = state.song_image_cache.get(key).await {
            if let Some(h) = state.stats.as_ref() {
                track_image_event(
                    h,
                    "/image/song",
                    "image_cache",
                    "song_hit",
                    None,
                    Some(user_hash.clone()),
                    serde_json::json!({"cached": true, "user_kind": user_kind_for_cache.as_deref(), "song_id": song.id.as_str(), "tpl": output.tpl_code.as_str()}),
                );
            }
            let headers = image_content_headers(output.content_type);
            return Ok((StatusCode::OK, headers, p));
        } else if let Some(h) = state.stats.as_ref() {
            track_image_event(
                h,
                "/image/song",
                "image_cache",
                "song_miss",
                None,
                Some(user_hash.clone()),
                serde_json::json!({"cached": false, "user_kind": user_kind_for_cache.as_deref(), "song_id": song.id.as_str(), "tpl": output.tpl_code.as_str()}),
            );
        }
    }

    // cache miss：下载/解密/解析存档本体
    let parsed = decrypt_image_save_from_meta(meta, state.chart_constants.clone()).await?;
    // 单曲成绩聚合、排序、推分求解与文件存在性检查均为同步 CPU/FS 工作，移出 Tokio worker。
    let SongComputeOutput {
        difficulty_scores,
        illustration_path,
        update_time,
    } = {
        let chart_constants = state.chart_constants.clone();
        let song_id = song.id.clone();
        let song_chart_constants = song.chart_constants.clone();
        let join = tokio::task::spawn_blocking(move || {
            song_compute::build_song_compute_output(SongComputeInput {
                parsed,
                chart_constants,
                song_id,
                song_chart_constants,
            })
        })
        .await;
        join.map_err(blocking_join_error)??
    };

    // 优先级：请求体昵称 > users/me 昵称 > 默认
    let (display_name, _) = resolve_display_name(
        req.nickname.clone(),
        req.auth.session_token.clone(),
        req.auth.taptap_version.as_deref(),
    )
    .await;

    let render_data = SongRenderData {
        song_name: song.name.clone(),
        song_id: song.id.clone(),
        player_name: Some(display_name),
        update_time,
        difficulty_scores,
        illustration_path,
        custom_footer_text: image_footer_text(),
    };

    // 等待许可与渲染分段计时
    let render_permit = acquire_render_permit(&state).await?;
    let permits_avail2 = render_permit.permits_avail;
    let wait_ms2 = render_permit.wait_ms;
    let t_render2 = Instant::now();
    // SVG 生成会触发磁盘 IO/图片解码/目录索引等阻塞操作，必须移出 tokio worker。
    let svg_options = SvgRenderOptions::from_query(public_illustration_base_url, &q);
    let svg = spawn_blocking_svg_generation(move || {
        renderer::generate_song_svg_string(
            &render_data,
            embed_images_effective,
            svg_options.public_base_url(),
            svg_options.template_id(),
        )
    })
    .await?;

    // 签名注入：在 SVG 底部追加签名行
    let signed_svg = {
        let signing_cfg = &AppConfig::global().image.signing;
        if signing_cfg.is_usable() {
            if let Some(sig) = signing::sign_svg(&svg, signing_cfg, user_hash_for_cache.as_deref())
            {
                signing::inject_sig_footer(&svg, &sig)
            } else {
                svg
            }
        } else {
            svg
        }
    };

    let (bytes, content_type) = render_svg_output_bytes(signed_svg, fmt_code, false, &q).await?;
    let render_ms2 = duration_ms_i64(t_render2.elapsed());
    // 统计：单曲查询图片生成（带用户去敏哈希 + song_id + 用户凭证类型）
    if let Some(stats) = state.stats.as_ref() {
        let extra = serde_json::json!({ "song_id": song.id.as_str(), "user_kind": user_kind_for_cache.as_deref() });
        stats.track_feature(
            "single_query",
            "generate_image",
            user_hash_for_cache.clone(),
            Some(extra),
        );
    }
    let headers = image_content_headers(content_type);

    // Cache put
    if let Some(key) = cache_key {
        state.song_image_cache.insert(key, bytes.clone()).await;
    }

    // Basic render metrics (total)
    if let Some(h) = state.stats.as_ref() {
        let total_ms = duration_ms_i64(t_total.elapsed());
        track_image_event(
            h,
            "/image/song",
            "image_render",
            "song",
            Some(total_ms),
            None,
            serde_json::json!({"permits_avail": permits_avail2, "wait_ms": wait_ms2, "render_ms": render_ms2, "bytes": bytes.len(), "fmt": fmt_code, "width": q.width, "song_id": song.id.as_str()}),
        );
    }
    Ok((StatusCode::OK, headers, bytes))
}
