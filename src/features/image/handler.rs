use std::collections::HashMap;

use axum::body::Bytes;
use axum::{
    Json, Router,
    extract::{Query, State},
    http::{HeaderValue, StatusCode, header},
    response::IntoResponse,
    routing::post,
};
use chrono::{DateTime, Utc};
use std::time::Instant;
use tracing::debug;

use crate::config::AppConfig;
use crate::{
    error::AppError,
    features::{
        image::renderer::{self, PlayerStats, RenderRecord, SongDifficultyScore, SongRenderData},
        save::{
            models::Difficulty,
            provider::{self, SaveSource},
        },
    },
    state::AppState,
};

use super::types::{RenderBnRequest, RenderSongRequest, RenderUserBnRequest};
use serde::Deserialize;

/// 图片输出选项（通过 Query 传入，避免破坏现有 JSON 请求体）
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ImageQueryOpts {
    /// 输出格式：png、jpeg、webp 或 svg（默认 png）
    #[serde(default)]
    format: Option<String>,
    /// 目标宽度：按宽度同比例缩放（可选）
    #[serde(default)]
    width: Option<u32>,
    /// WebP 质量：1-100（仅在 format=webp 时有效，默认 80）
    #[serde(default)]
    webp_quality: Option<u8>,
    /// WebP 无损模式：true=无损，false=有损（仅在 format=webp 时有效，默认 false）
    #[serde(default)]
    webp_lossless: Option<bool>,
}

/// SVG 返回时，曲绘资源的同源访问前缀（由 `src/main.rs` 提供静态目录服务）。
const ILLUSTRATION_PUBLIC_BASE_URL: &str = "/_ill";

fn is_svg_format(q: &ImageQueryOpts) -> bool {
    q.format
        .as_deref()
        .is_some_and(|fmt| fmt.eq_ignore_ascii_case("svg"))
}

fn format_code(q: &ImageQueryOpts) -> &'static str {
    if is_svg_format(q) {
        return "svg";
    }
    match q.format.as_deref() {
        Some("jpeg") | Some("jpg") => "jpg",
        Some("webp") => "webp",
        _ => "png",
    }
}

fn content_type_from_fmt_code(code: &str) -> &'static str {
    match code {
        "svg" => "image/svg+xml; charset=utf-8",
        "jpg" => "image/jpeg",
        "webp" => "image/webp",
        _ => "image/png",
    }
}

/// 规范化 WebP 参数在缓存键中的表达，避免无关/无效参数造成缓存碎片。
///
/// - 非 WebP 输出：`webp_quality/webp_lossless` 一律归零（忽略 query 里多余参数）。
/// - WebP 无损：质量参数无意义，归零（避免 lossless=true 但质量变化导致碎片）。
/// - WebP 有损：质量归一化到 1-100（缺省 80）。
fn normalized_webp_cache_params(fmt_code: &str, q: &ImageQueryOpts) -> (u8, u8) {
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

#[cfg(test)]
mod tests {
    use super::{ImageQueryOpts, content_type_from_fmt_code, format_code};

    #[test]
    fn supports_svg_format_code_and_content_type() {
        let q = ImageQueryOpts {
            format: Some("svg".to_string()),
            width: None,
            webp_quality: None,
            webp_lossless: None,
        };
        assert_eq!(format_code(&q), "svg");
        assert_eq!(
            content_type_from_fmt_code("svg"),
            "image/svg+xml; charset=utf-8"
        );
    }
}

#[utoipa::path(
    post,
    path = "/image/bn",
    summary = "生成 BestN 汇总图片",
    description = "从官方/外部存档解析玩家成绩，按 RKS 值排序取前 N 条生成 BestN 概览（PNG）。可选内嵌封面与主题切换。",
    request_body = RenderBnRequest,
    params(
        ("format" = Option<String>, Query, description = "输出格式：png|jpeg|webp|svg，默认 png"),
        ("width" = Option<u32>, Query, description = "目标宽度像素：按宽度同比例缩放"),
        ("webp_quality" = Option<u8>, Query, description = "WebP 质量：1-100（仅在 format=webp 时有效，默认 80）"),
        ("webp_lossless" = Option<bool>, Query, description = "WebP 无损模式（仅在 format=webp 时有效，默认 false）")
    ),
    responses(
        (status = 200, description = "PNG/JPEG/WebP bytes of BN image"),
        (status = 400, description = "Bad request", body = AppError),
        (status = 500, description = "Renderer error", body = AppError)
    ),
    tag = "Image"
)]
pub async fn render_bn(
    State(state): State<AppState>,
    Query(q): Query<ImageQueryOpts>,
    Json(req): Json<RenderBnRequest>,
) -> Result<impl IntoResponse, AppError> {
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
    let meta = provider::fetch_save_meta(
        source,
        &crate::config::AppConfig::global().taptap,
        taptap_version,
    )
    .await
    .map_err(|e| AppError::Internal(format!("获取存档元信息失败: {e}")))?;
    let updated_for_cache = meta.updated_at.clone().unwrap_or_else(|| "none".into());

    // 参数验证：webp_quality 范围
    if let Some(quality) = q.webp_quality
        && quality > 100
    {
        return Err(AppError::Validation(
            "webp_quality 必须在 1-100 范围内".to_string(),
        ));
    }

    let fmt_code = format_code(&q);
    // SVG 模式下：强制不内嵌图片（避免 data URI 导致体积爆炸），并将曲绘 href 指向外部资源基地址。
    let embed_images_effective = if fmt_code == "svg" {
        false
    } else {
        req.embed_images
    };
    let public_illustration_base_url = if fmt_code == "svg" {
        Some(
            AppConfig::global()
                .resources
                .illustration_external_base_url
                .as_deref()
                .unwrap_or(ILLUSTRATION_PUBLIC_BASE_URL),
        )
    } else {
        None
    };

    // Cache hit/miss 事件 + 快速返回
    let cache_enabled = AppConfig::global().image.cache_enabled;
    let salt = AppConfig::global().stats.user_hash_salt.as_deref();
    let (user_hash_for_cache, user_kind_for_cache) =
        crate::features::stats::derive_user_identity_from_auth(salt, &req.auth);
    if cache_enabled && let Some(user_hash) = user_hash_for_cache.as_ref() {
        let updated = updated_for_cache.clone();
        let theme_code = match req.theme {
            super::types::Theme::White => "w",
            super::types::Theme::Black => "b",
        };
        let width_code = if fmt_code == "svg" {
            0
        } else {
            q.width.unwrap_or(0)
        };
        let (webp_quality_code, webp_lossless_code) = normalized_webp_cache_params(fmt_code, &q);
        let key = format!(
            "{}:bn:{}:{}:{}:{}:{}:{}:{}:{}",
            user_hash,
            req.n.max(1),
            updated,
            theme_code,
            if embed_images_effective { 1 } else { 0 },
            fmt_code,
            width_code,
            webp_quality_code,
            webp_lossless_code
        );
        if let Some(p) = state.bn_image_cache.get(&key).await {
            let _cache_duration = Instant::now().elapsed();
            tracing::info!(target: "bestn_performance", "缓存命中，缓存键: {}", key);

            if let Some(h) = state.stats.as_ref() {
                let total_ms = t_total.elapsed().as_millis() as i64;
                let evt = crate::features::stats::models::EventInsert {
                    ts_utc: chrono::Utc::now(),
                    route: Some("/image/bn".into()),
                    feature: Some("image_cache".into()),
                    action: Some("bn_hit".into()),
                    method: Some("POST".into()),
                    status: None,
                    duration_ms: Some(total_ms),
                    user_hash: Some(user_hash.clone()),
                    client_ip_hash: None,
                    instance: None,
                    extra_json: Some(serde_json::json!({
                        "cached": true,
                        "user_kind": user_kind_for_cache,
                        "fmt": fmt_code,
                        "width": width_code,
                        "webp_quality": webp_quality_code,
                        "webp_lossless": webp_lossless_code
                    })),
                };
                h.track(evt).await;
                // 日志：BestN 缓存命中耗时
                debug!(
                    target: "phi_backend::image::bn",
                    total_ms,
                    fmt = fmt_code,
                    width = width_code,
                    "BestN 图片缓存命中，整体耗时 {total_ms}ms"
                );
            }
            let mut headers = axum::http::HeaderMap::new();
            let content_type = content_type_from_fmt_code(fmt_code);
            headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));

            let total_duration = t_total.elapsed();
            tracing::info!(target: "bestn_performance", "BestN缓存命中完成，总耗时: {:?}ms (缓存命中)", total_duration.as_millis());
            return Ok((StatusCode::OK, headers, p));
        } else {
            let _cache_duration = Instant::now().elapsed();
            tracing::info!(target: "bestn_performance", "缓存未命中，缓存键: {}", key);
        }
        if let Some(h) = state.stats.as_ref() {
            let evt = crate::features::stats::models::EventInsert {
                ts_utc: chrono::Utc::now(),
                route: Some("/image/bn".into()),
                feature: Some("image_cache".into()),
                action: Some("bn_miss".into()),
                method: Some("POST".into()),
                status: None,
                duration_ms: None,
                user_hash: Some(user_hash.clone()),
                client_ip_hash: None,
                instance: None,
                extra_json: Some(
                    serde_json::json!({ "cached": false, "user_kind": user_kind_for_cache }),
                ),
            };
            h.track(evt).await;
        }
    }

    // cache miss：下载/解密/解析存档本体
    let parsed = provider::get_decrypted_save_from_meta(meta, &state.chart_constants)
        .await
        .map_err(|e| AppError::Internal(format!("获取存档失败: {e}")))?;
    let save_ms = t_save.elapsed().as_millis() as i64;

    // 扁平化为渲染记录 + 排序与推分预计算耗时
    let t_flatten = Instant::now();
    let mut all: Vec<RenderRecord> = Vec::new();
    for (song_id, diffs) in parsed.game_record.iter() {
        // 查定数与曲名
        let chart = state.chart_constants.get(song_id);
        let name = state
            .song_catalog
            .by_id
            .get(song_id)
            .map(|s| s.name.clone())
            .unwrap_or_else(|| song_id.clone());

        for rec in diffs {
            let (dv_opt, diff_str) = match rec.difficulty {
                Difficulty::EZ => (chart.and_then(|c| c.ez).map(|v| v as f64), "EZ"),
                Difficulty::HD => (chart.and_then(|c| c.hd).map(|v| v as f64), "HD"),
                Difficulty::IN => (chart.and_then(|c| c.in_level).map(|v| v as f64), "IN"),
                Difficulty::AT => (chart.and_then(|c| c.at).map(|v| v as f64), "AT"),
            };
            let Some(dv) = dv_opt else { continue };

            let mut acc_percent = rec.accuracy as f64;
            if acc_percent <= 1.5 {
                acc_percent *= 100.0;
            }
            let rks = crate::features::rks::engine::calculate_chart_rks(acc_percent, dv);

            all.push(RenderRecord {
                song_id: song_id.clone(),
                song_name: name.clone(),
                difficulty: diff_str.to_string(),
                score: Some(rec.score as f64),
                acc: acc_percent,
                rks,
                difficulty_value: dv,
                is_fc: rec.is_full_combo,
            });
        }
    }

    let data_process_duration = t_flatten.elapsed();
    let data_record_count = all.len();
    tracing::info!(target: "bestn_performance", "数据扁平化完成，记录数: {}, 耗时: {:?}ms", data_record_count, data_process_duration.as_millis());

    let t_sort_start = Instant::now();
    // 按 RKS 降序
    all.sort_by(|a, b| {
        b.rks
            .partial_cmp(&a.rks)
            .unwrap_or(core::cmp::Ordering::Equal)
    });
    let sort_duration = t_sort_start.elapsed();

    let n = req.n.max(1);
    let top: Vec<RenderRecord> = all.iter().take(n as usize).cloned().collect();
    tracing::info!(target: "bestn_performance", "排序完成，目标TopN: {}, 排序耗时: {:?}ms", n, sort_duration.as_millis());

    let t_push_start = Instant::now();
    // 预计算推分 ACC
    let mut push_acc_map: HashMap<String, f64> = HashMap::new();
    let engine_all: Vec<crate::features::rks::engine::RksRecord> =
        all.iter().filter_map(to_engine_record).collect();
    for s in top
        .iter()
        .filter(|s| s.acc < 100.0 && s.difficulty_value > 0.0)
    {
        let key = format!("{}-{}", s.song_id, s.difficulty);
        if let Some(v) = crate::features::rks::engine::calculate_target_chart_push_acc(
            &key,
            s.difficulty_value,
            &engine_all,
        ) {
            push_acc_map.insert(key, v);
        }
    }
    let push_acc_duration = t_push_start.elapsed();
    tracing::info!(target: "bestn_performance", "推分ACC计算完成，计算数量: {}, 耗时: {:?}ms", push_acc_map.len(), push_acc_duration.as_millis());

    let flatten_ms = t_flatten.elapsed().as_millis() as i64;
    let t_stats_start = Instant::now();

    // 统计计算：RKS 详情与平均值
    let (exact_rks, _rounded) =
        crate::features::rks::engine::calculate_player_rks_details(&engine_all);
    let ap_scores: Vec<_> = all.iter().filter(|r| r.acc >= 100.0).take(3).collect();
    let ap_top_3_avg = if ap_scores.len() == 3 {
        Some(ap_scores.iter().map(|r| r.rks).sum::<f64>() / 3.0)
    } else {
        None
    };
    let best_27_avg = if all.is_empty() {
        None
    } else {
        Some(all.iter().take(27).map(|r| r.rks).sum::<f64>() / (all.len().min(27) as f64))
    };
    let stats_duration = t_stats_start.elapsed();
    tracing::info!(target: "bestn_performance", "统计数据计算完成，精确RKS: {:?}, AP Top3: {:?}, Best27: {:?}, 耗时: {:?}ms", 
                   exact_rks, ap_top_3_avg, best_27_avg, stats_duration.as_millis());

    // 课题模式等级（优先使用 summaryParsed，其次使用 gameProgress.challengeModeRank）
    let t_challenge_start = Instant::now();
    let challenge_rank = if let Some(sum) = parsed.summary_parsed.as_ref() {
        Some(sum.challenge_mode_rank as i64)
    } else {
        parsed
            .game_progress
            .get("challengeModeRank")
            .and_then(|v| v.as_i64())
    }
    .and_then(|rank_num| {
        if rank_num <= 0 {
            return None;
        }
        let s = rank_num.to_string();
        if s.is_empty() {
            return None;
        }
        let (color_char, level_str) = s.split_at(1);
        let color = match color_char {
            "1" => "Green",
            "2" => "Blue",
            "3" => "Red",
            "4" => "Gold",
            "5" => "Rainbow",
            _ => return None,
        };
        Some((color.to_string(), level_str.to_string()))
    });
    let challenge_duration = t_challenge_start.elapsed();
    tracing::info!(target: "bestn_performance", "挑战等级解析完成: {:?}, 耗时: {:?}ms", challenge_rank, challenge_duration.as_millis());

    let t_data_string_start = Instant::now();
    // Data 数（money）展示
    let data_string = parsed
        .game_progress
        .get("money")
        .and_then(|v| v.as_array())
        .and_then(|arr| {
            let units = ["KB", "MB", "GB", "TB"];
            let mut parts: Vec<String> = arr
                .iter()
                .zip(units.iter())
                .filter_map(|(val, unit)| {
                    val.as_i64().and_then(|u| {
                        if u > 0 {
                            Some(format!("{u} {unit}"))
                        } else {
                            None
                        }
                    })
                })
                .collect();
            parts.reverse();
            if parts.is_empty() {
                None
            } else {
                Some(format!("Data: {}", parts.join(", ")))
            }
        });
    let data_string_duration = t_data_string_start.elapsed();
    tracing::info!(target: "bestn_performance", "Data字符串解析完成: {:?}, 耗时: {:?}ms", data_string, data_string_duration.as_millis());

    let t_time_start = Instant::now();
    let update_time: DateTime<Utc> = parsed
        .updated_at
        .as_deref()
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(Utc::now);
    let time_parse_duration = t_time_start.elapsed();
    tracing::info!(target: "bestn_performance", "更新时间解析完成, 耗时: {:?}ms", time_parse_duration.as_millis());

    let t_nickname_start = Instant::now();
    // 优先级：请求体昵称 > users/me 昵称 > 默认
    let mut nickname_ms: i64 = 0;
    let display_name = if let Some(n) = req.nickname.clone() {
        n
    } else if let Some(token) = req.auth.session_token.clone() {
        let t_nick = Instant::now();
        let name = fetch_nickname(&token)
            .await
            .unwrap_or_else(|| "Phigros Player".into());
        nickname_ms = t_nick.elapsed().as_millis() as i64;
        name
    } else {
        "Phigros Player".into()
    };
    let nickname_duration = t_nickname_start.elapsed();
    tracing::info!(target: "bestn_performance", "昵称获取完成: {}, 耗时: {:?}ms", display_name, nickname_duration.as_millis());

    let stats = PlayerStats {
        ap_top_3_avg,
        best_27_avg,
        real_rks: Some(exact_rks),
        player_name: Some(display_name),
        update_time,
        n,
        ap_top_3_scores: all
            .iter()
            .filter(|r| r.acc >= 100.0)
            .take(3)
            .cloned()
            .collect(),
        challenge_rank,
        data_string,
        custom_footer_text: Some(AppConfig::global().branding.footer_text.clone()),
        is_user_generated: false,
    };

    // 等待许可与渲染分段计时
    // 统计用：提前提取曲目 ID，避免后续把 `top` move 进阻塞线程后不可用。
    let bestn_song_ids: Vec<String> = top.iter().map(|r| r.song_id.clone()).collect();

    let t_semaphore_start = Instant::now();
    let sem = state.render_semaphore.clone();
    let permits_avail = sem.available_permits() as i64;
    let t_wait = Instant::now();
    let _permit = sem
        .acquire_owned()
        .await
        .map_err(|e| AppError::Internal(format!("获取渲染信号量失败: {e}")))?;
    let wait_ms = t_wait.elapsed().as_millis() as i64;
    let semaphore_duration = t_semaphore_start.elapsed();
    tracing::info!(target: "bestn_performance", "信号量获取完成，可用许可: {}, 等待时间: {:?}ms, 总获取时间: {:?}ms", 
                   permits_avail, wait_ms, semaphore_duration.as_millis());

    let t_svg_start = Instant::now();
    // SVG 生成会触发磁盘 IO/图片解码/目录索引等阻塞操作，必须移出 tokio worker。
    let theme = req.theme;
    let public_base_url = public_illustration_base_url.map(|s| s.to_string());
    let svg = tokio::task::spawn_blocking(move || {
        renderer::generate_svg_string(
            &top,
            &stats,
            Some(&push_acc_map),
            &theme,
            embed_images_effective,
            public_base_url.as_deref(),
        )
    })
    .await
    .map_err(|e| AppError::Internal(format!("阻塞 SVG 生成任务执行失败: {e}")))??;
    let svg_duration = t_svg_start.elapsed();
    let svg_size = svg.len();
    tracing::info!(target: "bestn_performance", "SVG生成完成，SVG大小: {} 字符, 耗时: {:?}ms", svg_size, svg_duration.as_millis());

    let t_render_start = Instant::now();
    // 输出格式与宽度处理（svg 直接返回，不做栅格化渲染）
    let (bytes, content_type) = if fmt_code == "svg" {
        (
            Bytes::from(svg.into_bytes()),
            content_type_from_fmt_code(fmt_code),
        )
    } else {
        let (v, ct) = renderer::render_svg_unified_async(
            svg,
            false,
            q.format.as_deref(),
            q.width,
            q.webp_quality,
            q.webp_lossless,
        )
        .await?;
        (Bytes::from(v), ct)
    };
    let render_duration = t_render_start.elapsed();
    let render_ms = render_duration.as_millis() as i64;
    let bytes_len = bytes.len();
    tracing::info!(target: "bestn_performance", "图片渲染完成，输出格式: {}, 字节大小: {}, 耗时: {:?}ms", 
                   content_type, bytes_len, render_duration.as_millis());

    // 统计：BestN 图片生成（带用户去敏哈希 + 榜单歌曲ID列表 + 用户凭证类型）
    if let Some(stats) = state.stats.as_ref() {
        let salt = crate::config::AppConfig::global()
            .stats
            .user_hash_salt
            .as_deref();
        let (user_hash, user_kind) =
            crate::features::stats::derive_user_identity_from_auth(salt, &req.auth);
        let extra = serde_json::json!({ "bestn_song_ids": bestn_song_ids, "user_kind": user_kind });
        stats
            .track_feature("bestn", "generate_image", user_hash, Some(extra))
            .await;
    }

    let mut headers = axum::http::HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));

    // Cache put
    let mut cache_put_duration = None;
    if AppConfig::global().image.cache_enabled {
        let salt = AppConfig::global().stats.user_hash_salt.as_deref();
        let (uh, _) = crate::features::stats::derive_user_identity_from_auth(salt, &req.auth);
        if let Some(user_hash) = uh.as_ref() {
            let t_cache_put = Instant::now();
            let updated = updated_for_cache.clone();
            let theme_code = match req.theme {
                super::types::Theme::White => "w",
                super::types::Theme::Black => "b",
            };
            let fmt_code = format_code(&q);
            let width_code = if fmt_code == "svg" {
                0
            } else {
                q.width.unwrap_or(0)
            };
            let (webp_quality_code, webp_lossless_code) =
                normalized_webp_cache_params(fmt_code, &q);
            let key = format!(
                "{}:bn:{}:{}:{}:{}:{}:{}:{}:{}",
                user_hash,
                n,
                updated,
                theme_code,
                if embed_images_effective { 1 } else { 0 },
                fmt_code,
                width_code,
                webp_quality_code,
                webp_lossless_code
            );
            state.bn_image_cache.insert(key, bytes.clone()).await;
            cache_put_duration = Some(t_cache_put.elapsed());
        }
    }
    if let Some(cache_dur) = cache_put_duration {
        tracing::info!(target: "bestn_performance", "缓存存储完成，耗时: {:?}ms", cache_dur.as_millis());
    }

    // Basic render metrics (total time and key阶段耗时)
    if let Some(h) = state.stats.as_ref() {
        let total_ms = t_total.elapsed().as_millis() as i64;
        let logic_ms = total_ms.saturating_sub(save_ms).saturating_sub(render_ms);
        let fmt_str = match q.format.as_deref() {
            Some("jpeg") | Some("jpg") => "jpg",
            Some("webp") => "webp",
            _ => "png",
        };
        let evt = crate::features::stats::models::EventInsert {
            ts_utc: chrono::Utc::now(),
            route: Some("/image/bn".into()),
            feature: Some("image_render".into()),
            action: Some("bn".into()),
            method: Some("POST".into()),
            status: None,
            duration_ms: Some(total_ms),
            user_hash: None,
            client_ip_hash: None,
            instance: None,
            extra_json: Some(serde_json::json!({
                "permits_avail": permits_avail,
                "save_ms": save_ms,
                "flatten_ms": flatten_ms,
                "logic_ms": logic_ms,
                "nickname_ms": nickname_ms,
                "wait_ms": wait_ms,
                "render_ms": render_ms,
                "bytes": bytes.len(),
                "fmt": fmt_str,
                "width": q.width,
            })),
        };
        h.track(evt).await;
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
            fmt = fmt_str,
            width = ?q.width,
            "BestN 渲染耗时统计：total={total_ms}ms, save={save_ms}ms, flatten={flatten_ms}ms, logic={logic_ms}ms, wait={wait_ms}ms, render={render_ms}ms"
        );
    }
    Ok((StatusCode::OK, headers, bytes))
}

#[utoipa::path(
    post,
    path = "/image/song",
    summary = "生成单曲成绩图片",
    description = "从存档中定位指定歌曲（支持 ID/名称），展示四难度成绩、RKS、推分建议等信息（PNG）。",
    request_body = RenderSongRequest,
    params(
        ("format" = Option<String>, Query, description = "输出格式：png|jpeg|webp|svg，默认 png"),
        ("width" = Option<u32>, Query, description = "目标宽度像素：按宽度同比例缩放"),
        ("webp_quality" = Option<u8>, Query, description = "WebP 质量：1-100（仅在 format=webp 时有效，默认 80）"),
        ("webp_lossless" = Option<bool>, Query, description = "WebP 无损模式（仅在 format=webp 时有效，默认 false）")
    ),
    responses(
        (status = 200, description = "PNG/JPEG/WebP bytes of song image"),
        (status = 400, description = "Bad request", body = AppError),
        (status = 500, description = "Renderer error", body = AppError)
    ),
    tag = "Image"
)]
pub async fn render_song(
    State(state): State<AppState>,
    Query(q): Query<ImageQueryOpts>,
    Json(req): Json<RenderSongRequest>,
) -> Result<impl IntoResponse, AppError> {
    let t_total = std::time::Instant::now();
    let source = to_save_source(&req.auth)?;
    let taptap_version = req.auth.taptap_version.as_deref();
    // 缓存前移：先拿 updatedAt（作为版本号）再决定是否需要下载/解密/解析存档本体。
    let meta = provider::fetch_save_meta(
        source,
        &crate::config::AppConfig::global().taptap,
        taptap_version,
    )
    .await
    .map_err(|e| AppError::Internal(format!("获取存档元信息失败: {e}")))?;
    let updated_for_cache = meta.updated_at.clone().unwrap_or_else(|| "none".into());

    let song = state
        .song_catalog
        .search_unique(&req.song)
        .map_err(AppError::Search)?;

    // 参数验证：webp_quality 范围
    if let Some(quality) = q.webp_quality
        && quality > 100
    {
        return Err(AppError::Validation(
            "webp_quality 必须在 1-100 范围内".to_string(),
        ));
    }

    let fmt_code = format_code(&q);
    // SVG 模式下：强制不内嵌图片（避免 data URI 导致体积爆炸），并将曲绘 href 指向外部资源基地址。
    let embed_images_effective = if fmt_code == "svg" {
        false
    } else {
        req.embed_images
    };
    let public_illustration_base_url = if fmt_code == "svg" {
        Some(
            AppConfig::global()
                .resources
                .illustration_external_base_url
                .as_deref()
                .unwrap_or(ILLUSTRATION_PUBLIC_BASE_URL),
        )
    } else {
        None
    };

    // Cache hit/miss 事件 + 快速返回
    let cache_enabled = AppConfig::global().image.cache_enabled;
    let salt = AppConfig::global().stats.user_hash_salt.as_deref();
    let (user_hash_for_cache, user_kind_for_cache) =
        crate::features::stats::derive_user_identity_from_auth(salt, &req.auth);
    if cache_enabled && let Some(user_hash) = user_hash_for_cache.as_ref() {
        let updated = updated_for_cache.clone();
        let width_code = if fmt_code == "svg" {
            0
        } else {
            q.width.unwrap_or(0)
        };
        let (webp_quality_code, webp_lossless_code) = normalized_webp_cache_params(fmt_code, &q);
        let key = format!(
            "{}:song:{}:{}:{}:{}:{}:{}:{}:{}",
            user_hash,
            song.id,
            updated,
            "d",
            if embed_images_effective { 1 } else { 0 },
            fmt_code,
            width_code,
            webp_quality_code,
            webp_lossless_code
        );
        if let Some(p) = state.song_image_cache.get(&key).await {
            if let Some(h) = state.stats.as_ref() {
                let evt = crate::features::stats::models::EventInsert {
                    ts_utc: chrono::Utc::now(),
                    route: Some("/image/song".into()),
                    feature: Some("image_cache".into()),
                    action: Some("song_hit".into()),
                    method: Some("POST".into()),
                    status: None,
                    duration_ms: None,
                    user_hash: Some(user_hash.clone()),
                    client_ip_hash: None,
                    instance: None,
                    extra_json: Some(
                        serde_json::json!({"cached": true, "user_kind": user_kind_for_cache, "song_id": song.id}),
                    ),
                };
                h.track(evt).await;
            }
            let mut headers = axum::http::HeaderMap::new();
            let content_type = content_type_from_fmt_code(fmt_code);
            headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
            return Ok((StatusCode::OK, headers, p));
        } else if let Some(h) = state.stats.as_ref() {
            let evt = crate::features::stats::models::EventInsert {
                ts_utc: chrono::Utc::now(),
                route: Some("/image/song".into()),
                feature: Some("image_cache".into()),
                action: Some("song_miss".into()),
                method: Some("POST".into()),
                status: None,
                duration_ms: None,
                user_hash: Some(user_hash.clone()),
                client_ip_hash: None,
                instance: None,
                extra_json: Some(
                    serde_json::json!({"cached": false, "user_kind": user_kind_for_cache, "song_id": song.id}),
                ),
            };
            h.track(evt).await;
        }
    }

    // 构建所有引擎记录用于推分
    let mut engine_all: Vec<crate::features::rks::engine::RksRecord> = Vec::new();
    // cache miss：下载/解密/解析存档本体
    let parsed = provider::get_decrypted_save_from_meta(meta, &state.chart_constants)
        .await
        .map_err(|e| AppError::Internal(format!("获取存档失败: {e}")))?;
    for (sid, diffs) in parsed.game_record.iter() {
        let chart = state.chart_constants.get(sid);
        for rec in diffs {
            let cc = match rec.difficulty {
                Difficulty::EZ => chart.and_then(|c| c.ez),
                Difficulty::HD => chart.and_then(|c| c.hd),
                Difficulty::IN => chart.and_then(|c| c.in_level),
                Difficulty::AT => chart.and_then(|c| c.at),
            };
            let Some(cc) = cc else { continue };
            let mut acc_percent = rec.accuracy as f64;
            if acc_percent <= 1.5 {
                acc_percent *= 100.0;
            }
            engine_all.push(crate::features::rks::engine::RksRecord {
                song_id: sid.clone(),
                difficulty: rec.difficulty.clone(),
                score: rec.score,
                acc: acc_percent,
                rks: crate::features::rks::engine::calculate_chart_rks(acc_percent, cc as f64),
                chart_constant: cc as f64,
            });
        }
    }
    engine_all.sort_by(|a, b| {
        b.rks
            .partial_cmp(&a.rks)
            .unwrap_or(core::cmp::Ordering::Equal)
    });

    // 单曲四难度数据
    let mut difficulty_scores: HashMap<String, Option<SongDifficultyScore>> = HashMap::new();
    let song_records = parsed
        .game_record
        .get(&song.id)
        .cloned()
        .unwrap_or_default();

    let levels = &song.chart_constants;
    let map_level = |d: &str| -> Option<f64> {
        match d {
            "EZ" => levels.ez,
            "HD" => levels.hd,
            "IN" => levels.in_level,
            "AT" => levels.at,
            _ => None,
        }
        .map(|v| v as f64)
    };
    for diff in ["EZ", "HD", "IN", "AT"] {
        let dv = map_level(diff);
        let rec = song_records.iter().find(|r| {
            matches!(
                (&r.difficulty, diff),
                (Difficulty::EZ, "EZ")
                    | (Difficulty::HD, "HD")
                    | (Difficulty::IN, "IN")
                    | (Difficulty::AT, "AT")
            )
        });
        let (score, acc, rks, is_fc) = if let Some(r) = rec {
            let mut ap = r.accuracy as f64;
            if ap <= 1.5 {
                ap *= 100.0;
            }
            let rks = dv.map(|v| crate::features::rks::engine::calculate_chart_rks(ap, v));
            (Some(r.score as f64), Some(ap), rks, Some(r.is_full_combo))
        } else {
            (None, None, None, None)
        };

        // 推分 acc
        let player_push_acc = if let (Some(dv), Some(a)) = (dv, acc) {
            if a >= 100.0 {
                Some(100.0)
            } else {
                let key = format!("{}-{}", song.id, diff);
                crate::features::rks::engine::calculate_target_chart_push_acc(&key, dv, &engine_all)
            }
        } else {
            None
        };

        difficulty_scores.insert(
            diff.to_string(),
            Some(SongDifficultyScore {
                score,
                acc,
                rks,
                difficulty_value: dv,
                is_fc,
                is_phi: acc.map(|a| a >= 100.0),
                player_push_acc,
            }),
        );
    }

    // 插画路径
    let ill_png = super::cover_loader::covers_dir()
        .join("ill")
        .join(format!("{}.png", song.id));
    let ill_jpg = super::cover_loader::covers_dir()
        .join("ill")
        .join(format!("{}.jpg", song.id));
    let illustration_path = if ill_png.exists() {
        Some(ill_png)
    } else if ill_jpg.exists() {
        Some(ill_jpg)
    } else {
        None
    };

    let update_time: DateTime<Utc> = parsed
        .updated_at
        .as_deref()
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(Utc::now);

    // 优先级：请求体昵称 > users/me 昵称 > 默认
    let display_name = if let Some(n) = req.nickname.clone() {
        n
    } else if let Some(token) = req.auth.session_token.clone() {
        fetch_nickname(&token)
            .await
            .unwrap_or_else(|| "Phigros Player".into())
    } else {
        "Phigros Player".into()
    };

    let render_data = SongRenderData {
        song_name: song.name.clone(),
        song_id: song.id.clone(),
        player_name: Some(display_name),
        update_time,
        difficulty_scores,
        illustration_path,
        custom_footer_text: Some(AppConfig::global().branding.footer_text.clone()),
    };

    // 等待许可与渲染分段计时
    let sem2 = state.render_semaphore.clone();
    let permits_avail2 = sem2.available_permits() as i64;
    let t_wait2 = Instant::now();
    let _permit2 = sem2
        .acquire_owned()
        .await
        .map_err(|e| AppError::Internal(format!("获取渲染信号量失败: {e}")))?;
    let wait_ms2 = t_wait2.elapsed().as_millis() as i64;
    let t_render2 = Instant::now();
    // SVG 生成会触发磁盘 IO/图片解码/目录索引等阻塞操作，必须移出 tokio worker。
    let public_base_url = public_illustration_base_url.map(|s| s.to_string());
    let svg = tokio::task::spawn_blocking(move || {
        renderer::generate_song_svg_string(
            &render_data,
            embed_images_effective,
            public_base_url.as_deref(),
        )
    })
    .await
    .map_err(|e| AppError::Internal(format!("阻塞 SVG 生成任务执行失败: {e}")))??;
    let (bytes, content_type) = if fmt_code == "svg" {
        (
            Bytes::from(svg.into_bytes()),
            content_type_from_fmt_code(fmt_code),
        )
    } else {
        let (v, ct) = renderer::render_svg_unified_async(
            svg,
            false,
            q.format.as_deref(),
            q.width,
            q.webp_quality,
            q.webp_lossless,
        )
        .await?;
        (Bytes::from(v), ct)
    };
    let render_ms2 = t_render2.elapsed().as_millis() as i64;
    // 统计：单曲查询图片生成（带用户去敏哈希 + song_id + 用户凭证类型）
    if let Some(stats) = state.stats.as_ref() {
        let salt = crate::config::AppConfig::global()
            .stats
            .user_hash_salt
            .as_deref();
        let (user_hash, user_kind) =
            crate::features::stats::derive_user_identity_from_auth(salt, &req.auth);
        let extra = serde_json::json!({ "song_id": song.id, "user_kind": user_kind });
        stats
            .track_feature("single_query", "generate_image", user_hash, Some(extra))
            .await;
    }
    let mut headers = axum::http::HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));

    // Cache put
    if AppConfig::global().image.cache_enabled {
        let salt = AppConfig::global().stats.user_hash_salt.as_deref();
        let (uh, _) = crate::features::stats::derive_user_identity_from_auth(salt, &req.auth);
        if let Some(user_hash) = uh.as_ref() {
            let updated = updated_for_cache.clone();
            let fmt_code = format_code(&q);
            let width_code = if fmt_code == "svg" {
                0
            } else {
                q.width.unwrap_or(0)
            };
            let (webp_quality_code, webp_lossless_code) =
                normalized_webp_cache_params(fmt_code, &q);
            let key = format!(
                "{}:song:{}:{}:{}:{}:{}:{}:{}:{}",
                user_hash,
                song.id,
                updated,
                "d",
                if embed_images_effective { 1 } else { 0 },
                fmt_code,
                width_code,
                webp_quality_code,
                webp_lossless_code
            );
            state.song_image_cache.insert(key, bytes.clone()).await;
        }
    }

    // Basic render metrics (total)
    if let Some(h) = state.stats.as_ref() {
        let total_ms = t_total.elapsed().as_millis() as i64;
        let fmt_str = match q.format.as_deref() {
            Some("jpeg") | Some("jpg") => "jpg",
            Some("webp") => "webp",
            _ => "png",
        };
        let evt = crate::features::stats::models::EventInsert {
            ts_utc: chrono::Utc::now(),
            route: Some("/image/song".into()),
            feature: Some("image_render".into()),
            action: Some("song".into()),
            method: Some("POST".into()),
            status: None,
            duration_ms: Some(total_ms),
            user_hash: None,
            client_ip_hash: None,
            instance: None,
            extra_json: Some(
                serde_json::json!({"permits_avail": permits_avail2, "wait_ms": wait_ms2, "render_ms": render_ms2, "bytes": bytes.len(), "fmt": fmt_str, "width": q.width, "song_id": song.id}),
            ),
        };
        h.track(evt).await;
    }
    Ok((StatusCode::OK, headers, bytes))
}

fn to_engine_record(r: &RenderRecord) -> Option<crate::features::rks::engine::RksRecord> {
    let diff = match r.difficulty.as_str() {
        "EZ" => Difficulty::EZ,
        "HD" => Difficulty::HD,
        "IN" => Difficulty::IN,
        "AT" => Difficulty::AT,
        _ => return None,
    };
    Some(crate::features::rks::engine::RksRecord {
        song_id: r.song_id.clone(),
        difficulty: diff,
        score: r.score.unwrap_or(0.0) as u32,
        acc: r.acc,
        rks: r.rks,
        chart_constant: r.difficulty_value,
    })
}

fn to_save_source(
    req: &crate::features::save::models::UnifiedSaveRequest,
) -> Result<SaveSource, AppError> {
    match (&req.session_token, &req.external_credentials) {
        (Some(token), None) => Ok(SaveSource::official(token.clone())),
        (None, Some(creds)) => Ok(SaveSource::external(creds.clone())),
        (Some(_), Some(_)) => Err(AppError::SaveHandlerError(
            "不能同时提供 sessionToken 和 externalCredentials".into(),
        )),
        (None, None) => Err(AppError::SaveHandlerError(
            "必须提供 sessionToken 或 externalCredentials 中的一项".into(),
        )),
    }
}

pub fn create_image_router() -> Router<AppState> {
    Router::new()
        .route("/image/bn", post(render_bn))
        .route("/image/song", post(render_song))
        .route("/image/bn/user", post(render_bn_user))
}

/// 从 LeanCloud users/me 获取昵称（复用 phigros.cxx 的请求头部）
async fn fetch_nickname(session_token: &str) -> Option<String> {
    #[derive(serde::Deserialize)]
    struct UserMe {
        nickname: Option<String>,
    }
    const LC_ID: &str = "rAK3FfdieFob2Nn8Am";
    const LC_KEY: &str = "Qr9AEqtuoSVS3zeD6iVbM4ZC0AtkJcQ89tywVyi0";
    let url = "https://rak3ffdi.cloud.tds1.tapapis.cn/1.1/users/me";
    let client = reqwest::Client::new();
    let resp = client
        .get(url)
        .header("X-LC-Id", LC_ID)
        .header("X-LC-Key", LC_KEY)
        .header("X-LC-Session", session_token)
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let me: UserMe = resp.json().await.ok()?;
    me.nickname
}

#[utoipa::path(
    post,
    path = "/image/bn/user",
    summary = "生成用户自报成绩的 BestN 图片",
    description = "无需存档，直接提交若干条用户自报成绩，计算 RKS 排序并生成 BestN 图片；支持水印解除口令。",
    request_body = RenderUserBnRequest,
    params(
        ("format" = Option<String>, Query, description = "输出格式：png|jpeg|webp|svg，默认 png"),
        ("width" = Option<u32>, Query, description = "目标宽度像素：按宽度同比例缩放"),
        ("webp_quality" = Option<u8>, Query, description = "WebP 质量：1-100（仅在 format=webp 时有效，默认 80）"),
        ("webp_lossless" = Option<bool>, Query, description = "WebP 无损模式（仅在 format=webp 时有效，默认 false）")
    ),
    responses(
        (status = 200, description = "PNG/JPEG/WebP bytes of user BN image"),
        (status = 400, description = "Bad request", body = AppError),
        (status = 500, description = "Renderer error", body = AppError)
    ),
    tag = "Image"
)]
pub async fn render_bn_user(
    State(state): State<AppState>,
    Query(q): Query<ImageQueryOpts>,
    Json(req): Json<RenderUserBnRequest>,
) -> Result<impl IntoResponse, AppError> {
    // 参数验证：webp_quality 范围
    if let Some(quality) = q.webp_quality
        && quality > 100
    {
        return Err(AppError::Validation(
            "webp_quality 必须在 1-100 范围内".to_string(),
        ));
    }

    // 解析成绩并计算 RKS
    let mut records: Vec<RenderRecord> = Vec::with_capacity(req.scores.len());
    for (idx, item) in req.scores.iter().enumerate() {
        // 找歌
        let info = state
            .song_catalog
            .search_unique(&item.song)
            .map_err(AppError::Search)?;
        // 定数
        let dv_opt = match item.difficulty.as_str() {
            "EZ" | "ez" => info.chart_constants.ez,
            "HD" | "hd" => info.chart_constants.hd,
            "IN" | "in" => info.chart_constants.in_level,
            "AT" | "at" => info.chart_constants.at,
            _ => None,
        };
        let Some(dv) = dv_opt.map(|v| v as f64) else {
            return Err(AppError::ImageRendererError(format!(
                "第{}条成绩难度无效或无定数: {} {}",
                idx + 1,
                info.name,
                item.difficulty
            )));
        };
        // ACC 统一百分比
        let acc = item.acc;
        // RKS
        let rks = crate::features::rks::engine::calculate_chart_rks(acc, dv);
        records.push(RenderRecord {
            song_id: info.id.clone(),
            song_name: info.name.clone(),
            difficulty: item.difficulty.to_uppercase(),
            score: item.score.map(|v| v as f64),
            acc,
            rks,
            difficulty_value: dv,
            is_fc: (item.score.unwrap_or_default() == 1_000_000) || (acc >= 100.0),
        });
    }

    // 排序、截取 N（按传入成绩数量）
    records.sort_by(|a, b| {
        b.rks
            .partial_cmp(&a.rks)
            .unwrap_or(core::cmp::Ordering::Equal)
    });
    let n = records.len().max(1);
    let top: Vec<RenderRecord> = records.iter().take(n).cloned().collect();

    // 推分 ACC
    let mut push_acc_map: HashMap<String, f64> = HashMap::new();
    let engine_all: Vec<crate::features::rks::engine::RksRecord> = records
        .iter()
        .filter_map(|r| {
            let diff = match r.difficulty.as_str() {
                "EZ" => Difficulty::EZ,
                "HD" => Difficulty::HD,
                "IN" => Difficulty::IN,
                "AT" => Difficulty::AT,
                _ => return None,
            };
            Some(crate::features::rks::engine::RksRecord {
                song_id: r.song_id.clone(),
                difficulty: diff,
                score: r.score.unwrap_or(0.0) as u32,
                acc: r.acc,
                rks: r.rks,
                chart_constant: r.difficulty_value,
            })
        })
        .collect();
    for r in top
        .iter()
        .filter(|s| s.acc < 100.0 && s.difficulty_value > 0.0)
    {
        let key = format!("{}-{}", r.song_id, r.difficulty);
        if let Some(v) = crate::features::rks::engine::calculate_target_chart_push_acc(
            &key,
            r.difficulty_value,
            &engine_all,
        ) {
            push_acc_map.insert(key, v);
        }
    }

    // 统计项
    let (exact_rks, _rounded) =
        crate::features::rks::engine::calculate_player_rks_details(&engine_all);
    let ap_scores: Vec<_> = records.iter().filter(|r| r.acc >= 100.0).take(3).collect();
    let ap_top_3_avg = if ap_scores.len() == 3 {
        Some(ap_scores.iter().map(|r| r.rks).sum::<f64>() / 3.0)
    } else {
        None
    };
    let best_27_avg = if records.is_empty() {
        None
    } else {
        Some(records.iter().take(27).map(|r| r.rks).sum::<f64>() / (records.len().min(27) as f64))
    };

    // 昵称
    let display_name = req
        .nickname
        .clone()
        .unwrap_or_else(|| "Phigros Player".into());

    // 水印控制：默认启用配置中的显式/隐式；若提供了正确的解除口令，则同时关闭二者
    let cfg = AppConfig::global();
    let unlocked = cfg
        .watermark
        .is_unlock_valid(req.unlock_password.as_deref());
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
        n: n as u32,
        ap_top_3_scores: records
            .iter()
            .filter(|r| r.acc >= 100.0)
            .take(3)
            .cloned()
            .collect(),
        challenge_rank: None,
        data_string: None,
        custom_footer_text: Some(cfg.branding.footer_text.clone()),
        is_user_generated: explicit,
    };

    let fmt_code = format_code(&q);
    let public_illustration_base_url = if fmt_code == "svg" {
        Some(
            AppConfig::global()
                .resources
                .illustration_external_base_url
                .as_deref()
                .unwrap_or(ILLUSTRATION_PUBLIC_BASE_URL),
        )
    } else {
        None
    };
    // 等待许可与渲染分段计时
    let sem = state.render_semaphore.clone();
    let _permit = sem
        .acquire_owned()
        .await
        .map_err(|e| AppError::Internal(format!("获取渲染信号量失败: {e}")))?;

    // SVG 生成会触发磁盘 IO/图片解码/目录索引等阻塞操作，必须移出 tokio worker。
    let theme = req.theme;
    let public_base_url = public_illustration_base_url.map(|s| s.to_string());
    let svg = tokio::task::spawn_blocking(move || {
        renderer::generate_svg_string(
            &top,
            &stats,
            Some(&push_acc_map),
            &theme,
            false,
            public_base_url.as_deref(),
        )
    })
    .await
    .map_err(|e| AppError::Internal(format!("阻塞 SVG 生成任务执行失败: {e}")))??;
    let (bytes, content_type) = if fmt_code == "svg" {
        (
            Bytes::from(svg.into_bytes()),
            content_type_from_fmt_code(fmt_code),
        )
    } else {
        let (v, ct) = renderer::render_svg_unified_async(
            svg,
            implicit,
            q.format.as_deref(),
            q.width,
            q.webp_quality,
            q.webp_lossless,
        )
        .await?;
        (Bytes::from(v), ct)
    };

    // 统计：用户自报 BestN 图片生成
    if let Some(stats_handle) = state.stats.as_ref() {
        let extra = serde_json::json!({
            "scores_len": records.len(),
            "unlocked": unlocked
        });
        stats_handle
            .track_feature("bestn_user", "generate_image", None, Some(extra))
            .await;
    }

    let mut headers = axum::http::HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    Ok((StatusCode::OK, headers, bytes))
}
