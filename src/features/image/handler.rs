use std::collections::HashMap;

use axum::{
    Router,
    extract::State,
    http::{header, HeaderValue, StatusCode},
    response::IntoResponse,
    routing::post,
    Json,
};
use chrono::{DateTime, Utc};
use std::time::Instant;

use crate::{
    error::AppError,
    features::{
        image::renderer::{self, PlayerStats, RenderRecord, SongDifficultyScore, SongRenderData},
        save::{models::Difficulty, provider::{self, SaveSource}},
    },
    state::AppState,
};
use crate::config::AppConfig;

use super::types::{RenderBnRequest, RenderSongRequest, RenderUserBnRequest};

#[utoipa::path(
    post,
    path = "/image/bn",
    summary = "生成 BestN 汇总图片",
    description = "从官方/外部存档解析玩家成绩，按 RKS 值排序取前 N 条生成 BestN 概览（PNG）。可选内嵌封面与主题切换。",
    request_body = RenderBnRequest,
    responses(
        (status = 200, description = "PNG bytes of BN image"),
        (status = 400, description = "Bad request", body = AppError),
        (status = 500, description = "Renderer error", body = AppError)
    ),
    tag = "Image"
)]
pub async fn render_bn(
    State(state): State<AppState>,
    Json(req): Json<RenderBnRequest>,
) -> Result<impl IntoResponse, AppError> {
    let t_total = std::time::Instant::now();
    let source = to_save_source(&req.auth)?;
    let parsed = provider::get_decrypted_save(source, &state.chart_constants).await
        .map_err(|e| AppError::Internal(format!("获取存档失败: {e}")))?;

    // Cache hit/miss 事件 + 快速返回
    let cache_enabled = AppConfig::global().image.cache_enabled;
    let salt = AppConfig::global().stats.user_hash_salt.as_deref();
    let (user_hash_for_cache, user_kind_for_cache) = crate::features::stats::derive_user_identity_from_auth(salt, &req.auth);
    if cache_enabled {
        if let Some(user_hash) = user_hash_for_cache.as_ref() {
            let updated = parsed.updated_at.clone().unwrap_or_else(|| "none".into());
            let theme_code = match req.theme { super::types::Theme::White => "w", super::types::Theme::Black => "b" };
            let key = format!("{}:bn:{}:{}:{}:{}", user_hash, req.n.max(1), updated, theme_code, if req.embed_images { 1 } else { 0 });
            if let Some(p) = state.bn_image_cache.get(&key).await {
                if let Some(h) = state.stats.as_ref() {
                    let evt = crate::features::stats::models::EventInsert {
                        ts_utc: chrono::Utc::now(),
                        route: Some("/image/bn".into()),
                        feature: Some("image_cache".into()),
                        action: Some("bn_hit".into()),
                        method: Some("POST".into()),
                        status: None,
                        duration_ms: None,
                        user_hash: Some(user_hash.clone()),
                        client_ip_hash: None,
                        instance: None,
                        extra_json: Some(serde_json::json!({ "cached": true, "user_kind": user_kind_for_cache })),
                    };
                    h.track(evt).await;
                }
                let mut headers = axum::http::HeaderMap::new();
                headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("image/png"));
                return Ok((StatusCode::OK, headers, (*p).clone()));
            } else if let Some(h) = state.stats.as_ref() {
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
                    extra_json: Some(serde_json::json!({ "cached": false, "user_kind": user_kind_for_cache })),
                };
                h.track(evt).await;
            }
        }
    }

    // 扁平化为渲染记录
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
            if acc_percent <= 1.5 { acc_percent *= 100.0; }
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

    // 按 RKS 降序
    all.sort_by(|a,b| b.rks.partial_cmp(&a.rks).unwrap_or(core::cmp::Ordering::Equal));
    let n = req.n.max(1);
    let top: Vec<RenderRecord> = all.iter().take(n as usize).cloned().collect();

    // 预计算推分 ACC
    let mut push_acc_map: HashMap<String, f64> = HashMap::new();
    let engine_all: Vec<crate::features::rks::engine::RksRecord> = all.iter().filter_map(to_engine_record).collect();
    for s in top.iter().filter(|s| s.acc < 100.0 && s.difficulty_value > 0.0) {
        let key = format!("{}-{}", s.song_id, s.difficulty);
        if let Some(v) = crate::features::rks::engine::calculate_target_chart_push_acc(&key, s.difficulty_value, &engine_all) {
            push_acc_map.insert(key, v);
        }
    }

    // 统计
    let (exact_rks, _rounded) = crate::features::rks::engine::calculate_player_rks_details(&engine_all);
    let ap_scores: Vec<_> = all.iter().filter(|r| r.acc >= 100.0).take(3).collect();
    let ap_top_3_avg = if ap_scores.len() == 3 { Some(ap_scores.iter().map(|r| r.rks).sum::<f64>()/3.0) } else { None };
    let best_27_avg = if all.is_empty() { None } else { Some(all.iter().take(27).map(|r| r.rks).sum::<f64>() / (all.len().min(27) as f64)) };

    // 课题模式等级（优先使用 summaryParsed，其次使用 gameProgress.challengeModeRank）
    let challenge_rank = if let Some(sum) = parsed.summary_parsed.as_ref() {
        Some(sum.challenge_mode_rank as i64)
    } else {
        parsed
            .game_progress
            .get("challengeModeRank")
            .and_then(|v| v.as_i64())
    }
    .and_then(|rank_num| {
        if rank_num <= 0 { return None; }
        let s = rank_num.to_string();
        if s.is_empty() { return None; }
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
                .filter_map(|(val, unit)| val.as_i64().and_then(|u| if u > 0 { Some(format!("{u} {unit}")) } else { None }))
                .collect();
            parts.reverse();
            if parts.is_empty() { None } else { Some(format!("Data: {}", parts.join(", "))) }
        });

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
        fetch_nickname(&token).await.unwrap_or_else(|| "Phigros Player".into())
    } else {
        "Phigros Player".into()
    };

    let stats = PlayerStats {
        ap_top_3_avg,
        best_27_avg,
        real_rks: Some(exact_rks),
        player_name: Some(display_name),
        update_time,
        n,
        ap_top_3_scores: all.iter().filter(|r| r.acc >= 100.0).take(3).cloned().collect(),
        challenge_rank,
        data_string,
        custom_footer_text: Some(AppConfig::global().branding.footer_text.clone()),
        is_user_generated: false,
    };

    // 等待许可与渲染分段计时
    let sem = state.render_semaphore.clone();
    let permits_avail = sem.available_permits() as i64;
    let t_wait = Instant::now();
    let _permit = sem
        .acquire_owned()
        .await
        .map_err(|e| AppError::Internal(format!("获取渲染信号量失败: {e}")))?;
    let wait_ms = t_wait.elapsed().as_millis() as i64;
    let t_render = Instant::now();
    let svg = renderer::generate_svg_string(&top, &stats, Some(&push_acc_map), &req.theme, req.embed_images)?;
    let png = renderer::render_svg_to_png(svg, false)?;
    let render_ms = t_render.elapsed().as_millis() as i64;

    // 统计：BestN 图片生成（带用户去敏哈希 + 榜单歌曲ID列表 + 用户凭证类型）
    if let Some(stats) = state.stats.as_ref() {
        let salt = crate::config::AppConfig::global().stats.user_hash_salt.as_deref();
        let (user_hash, user_kind) = crate::features::stats::derive_user_identity_from_auth(salt, &req.auth);
        let bestn_song_ids: Vec<String> = top.iter().map(|r| r.song_id.clone()).collect();
        let extra = serde_json::json!({ "bestn_song_ids": bestn_song_ids, "user_kind": user_kind });
        stats.track_feature("bestn", "generate_image", user_hash, Some(extra)).await;
    }

    let mut headers = axum::http::HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("image/png"));

    // Cache put
    if AppConfig::global().image.cache_enabled {
        let salt = AppConfig::global().stats.user_hash_salt.as_deref();
        let (uh, _) = crate::features::stats::derive_user_identity_from_auth(salt, &req.auth);
        if let Some(user_hash) = uh.as_ref() {
            let updated = parsed.updated_at.clone().unwrap_or_else(|| "none".into());
            let theme_code = match req.theme { super::types::Theme::White => "w", super::types::Theme::Black => "b" };
            let key = format!("{}:bn:{}:{}:{}:{}", user_hash, n, updated, theme_code, if req.embed_images { 1 } else { 0 });
            state.bn_image_cache.insert(key, std::sync::Arc::new(png.clone())).await;
        }
    }

    // Basic render metrics (total time and permits)
    if let Some(h) = state.stats.as_ref() {
        let total_ms = t_total.elapsed().as_millis() as i64;
        let evt = crate::features::stats::models::EventInsert{
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
            extra_json: Some(serde_json::json!({"permits_avail": permits_avail, "wait_ms": wait_ms, "render_ms": render_ms, "png_bytes": png.len()})),
        };
        h.track(evt).await;
    }
    Ok((StatusCode::OK, headers, png))
}

#[utoipa::path(
    post,
    path = "/image/song",
    summary = "生成单曲成绩图片",
    description = "从存档中定位指定歌曲（支持 ID/名称），展示四难度成绩、RKS、推分建议等信息（PNG）。",
    request_body = RenderSongRequest,
    responses(
        (status = 200, description = "PNG bytes of song image"),
        (status = 400, description = "Bad request", body = AppError),
        (status = 500, description = "Renderer error", body = AppError)
    ),
    tag = "Image"
)]
pub async fn render_song(
    State(state): State<AppState>,
    Json(req): Json<RenderSongRequest>,
) -> Result<impl IntoResponse, AppError> {
    let t_total = std::time::Instant::now();
    let source = to_save_source(&req.auth)?;
    let parsed = provider::get_decrypted_save(source, &state.chart_constants).await
        .map_err(|e| AppError::Internal(format!("获取存档失败: {e}")))?;

    let song = state
        .song_catalog
        .search_unique(&req.song)
        .map_err(AppError::Search)?;

    // Cache hit/miss 事件 + 快速返回
    let cache_enabled = AppConfig::global().image.cache_enabled;
    let salt = AppConfig::global().stats.user_hash_salt.as_deref();
    let (user_hash_for_cache, user_kind_for_cache) = crate::features::stats::derive_user_identity_from_auth(salt, &req.auth);
    if cache_enabled {
        if let Some(user_hash) = user_hash_for_cache.as_ref() {
            let updated = parsed.updated_at.clone().unwrap_or_else(|| "none".into());
            let key = format!("{}:song:{}:{}:{}:{}", user_hash, song.id, updated, "d", if req.embed_images { 1 } else { 0 });
            if let Some(p) = state.song_image_cache.get(&key).await {
                if let Some(h) = state.stats.as_ref() {
                    let evt = crate::features::stats::models::EventInsert{
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
                        extra_json: Some(serde_json::json!({"cached": true, "user_kind": user_kind_for_cache, "song_id": song.id})),
                    };
                    h.track(evt).await;
                }
                let mut headers = axum::http::HeaderMap::new();
                headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("image/png"));
                return Ok((StatusCode::OK, headers, (*p).clone()));
            } else if let Some(h) = state.stats.as_ref() {
                let evt = crate::features::stats::models::EventInsert{
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
                    extra_json: Some(serde_json::json!({"cached": false, "user_kind": user_kind_for_cache, "song_id": song.id})),
                };
                h.track(evt).await;
            }
        }
    }

    // 构建所有引擎记录用于推分
    let mut engine_all: Vec<crate::features::rks::engine::RksRecord> = Vec::new();
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
            let mut acc_percent = rec.accuracy as f64; if acc_percent <= 1.5 { acc_percent *= 100.0; }
            engine_all.push(crate::features::rks::engine::RksRecord{
                song_id: sid.clone(),
                difficulty: rec.difficulty.clone(),
                score: rec.score,
                acc: acc_percent,
                rks: crate::features::rks::engine::calculate_chart_rks(acc_percent, cc as f64),
                chart_constant: cc as f64,
            });
        }
    }
    engine_all.sort_by(|a,b| b.rks.partial_cmp(&a.rks).unwrap_or(core::cmp::Ordering::Equal));

    // 单曲四难度数据
    let mut difficulty_scores: HashMap<String, Option<SongDifficultyScore>> = HashMap::new();
    let song_records = parsed.game_record.get(&song.id).cloned().unwrap_or_default();

    let levels = &song.chart_constants;
    let map_level = |d: &str| -> Option<f64> {
        match d {"EZ"=>levels.ez, "HD"=>levels.hd, "IN"=>levels.in_level, "AT"=>levels.at, _=>None}.map(|v| v as f64)
    };
    for diff in ["EZ","HD","IN","AT"] {
        let dv = map_level(diff);
        let rec = song_records.iter().find(|r| match (&r.difficulty, diff) {
            (Difficulty::EZ, "EZ")|(Difficulty::HD, "HD")|(Difficulty::IN, "IN")|(Difficulty::AT, "AT") => true,
            _ => false,
        });
        let (score, acc, rks, is_fc) = if let Some(r) = rec { 
            let mut ap = r.accuracy as f64; if ap <= 1.5 { ap *= 100.0; }
            let rks = dv.map(|v| crate::features::rks::engine::calculate_chart_rks(ap, v));
            (Some(r.score as f64), Some(ap), rks, Some(r.is_full_combo))
        } else { (None, None, None, None) };

        // 推分 acc
        let player_push_acc = if let (Some(dv), Some(a)) = (dv, acc) {
            if a >= 100.0 { Some(100.0) } else {
                let key = format!("{}-{}", song.id, diff);
                crate::features::rks::engine::calculate_target_chart_push_acc(&key, dv, &engine_all)
            }
        } else { None };

        difficulty_scores.insert(diff.to_string(), Some(SongDifficultyScore{
            score,
            acc,
            rks,
            difficulty_value: dv,
            is_fc,
            is_phi: acc.map(|a| a>=100.0),
            player_push_acc,
        }));
    }

    // 插画路径
    let ill_png = super::cover_loader::covers_dir().join("ill").join(format!("{}.png", song.id));
    let ill_jpg = super::cover_loader::covers_dir().join("ill").join(format!("{}.jpg", song.id));
    let illustration_path = if ill_png.exists() { Some(ill_png) } else if ill_jpg.exists() { Some(ill_jpg) } else { None };

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
        fetch_nickname(&token).await.unwrap_or_else(|| "Phigros Player".into())
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
    let svg = renderer::generate_song_svg_string(&render_data, req.embed_images)?;
    let png = renderer::render_svg_to_png(svg, false)?;
    let render_ms2 = t_render2.elapsed().as_millis() as i64;
    // 统计：单曲查询图片生成（带用户去敏哈希 + song_id + 用户凭证类型）
    if let Some(stats) = state.stats.as_ref() {
        let salt = crate::config::AppConfig::global().stats.user_hash_salt.as_deref();
        let (user_hash, user_kind) = crate::features::stats::derive_user_identity_from_auth(salt, &req.auth);
        let extra = serde_json::json!({ "song_id": song.id, "user_kind": user_kind });
        stats.track_feature("single_query", "generate_image", user_hash, Some(extra)).await;
    }
    let mut headers = axum::http::HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("image/png"));

    // Cache put
    if AppConfig::global().image.cache_enabled {
        let salt = AppConfig::global().stats.user_hash_salt.as_deref();
        let (uh, _) = crate::features::stats::derive_user_identity_from_auth(salt, &req.auth);
        if let Some(user_hash) = uh.as_ref() {
            let updated = parsed.updated_at.clone().unwrap_or_else(|| "none".into());
            let key = format!("{}:song:{}:{}:{}:{}", user_hash, song.id, updated, "d", if req.embed_images { 1 } else { 0 });
            state.song_image_cache.insert(key, std::sync::Arc::new(png.clone())).await;
        }
    }

    // Basic render metrics (total)
    if let Some(h) = state.stats.as_ref() {
        let total_ms = t_total.elapsed().as_millis() as i64;
        let evt = crate::features::stats::models::EventInsert{
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
            extra_json: Some(serde_json::json!({"permits_avail": permits_avail2, "wait_ms": wait_ms2, "render_ms": render_ms2, "png_bytes": png.len(), "song_id": song.id})),
        };
        h.track(evt).await;
    }
    Ok((StatusCode::OK, headers, png))
}

fn to_engine_record(r: &RenderRecord) -> Option<crate::features::rks::engine::RksRecord> {
    let diff = match r.difficulty.as_str() {"EZ"=>Difficulty::EZ, "HD"=>Difficulty::HD, "IN"=>Difficulty::IN, "AT"=>Difficulty::AT, _=>return None};
    Some(crate::features::rks::engine::RksRecord{
        song_id: r.song_id.clone(),
        difficulty: diff,
        score: r.score.unwrap_or(0.0) as u32,
        acc: r.acc,
        rks: r.rks,
        chart_constant: r.difficulty_value,
    })
}

fn to_save_source(req: &crate::features::save::models::UnifiedSaveRequest) -> Result<SaveSource, AppError> {
    
    match (&req.session_token, &req.external_credentials) {
        (Some(token), None) => Ok(SaveSource::official(token.clone())),
        (None, Some(creds)) => Ok(SaveSource::external(creds.clone())),
        (Some(_), Some(_)) => Err(AppError::SaveHandlerError("不能同时提供 sessionToken 和 externalCredentials".into())),
        (None, None) => Err(AppError::SaveHandlerError("必须提供 sessionToken 或 externalCredentials 中的一项".into())),
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
    struct UserMe { nickname: Option<String> }
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
    if !resp.status().is_success() { return None; }
    let me: UserMe = resp.json().await.ok()?;
    me.nickname
}

#[utoipa::path(
    post,
    path = "/image/bn/user",
    summary = "生成用户自报成绩的 BestN 图片",
    description = "无需存档，直接提交若干条用户自报成绩，计算 RKS 排序并生成 BestN 图片；支持水印解除口令。",
    request_body = RenderUserBnRequest,
    responses(
        (status = 200, description = "PNG bytes of user BN image"),
        (status = 400, description = "Bad request", body = AppError),
        (status = 500, description = "Renderer error", body = AppError)
    ),
    tag = "Image"
)]
pub async fn render_bn_user(
    State(state): State<AppState>,
    Json(req): Json<RenderUserBnRequest>,
) -> Result<impl IntoResponse, AppError> {
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
            "EZ"|"ez" => info.chart_constants.ez,
            "HD"|"hd" => info.chart_constants.hd,
            "IN"|"in" => info.chart_constants.in_level,
            "AT"|"at" => info.chart_constants.at,
            _ => None,
        };
        let Some(dv) = dv_opt.map(|v| v as f64) else {
            return Err(AppError::ImageRendererError(format!("第{}条成绩难度无效或无定数: {} {}", idx+1, info.name, item.difficulty)));
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
    records.sort_by(|a,b| b.rks.partial_cmp(&a.rks).unwrap_or(core::cmp::Ordering::Equal));
    let n = records.len().max(1);
    let top: Vec<RenderRecord> = records.iter().take(n).cloned().collect();

    // 推分 ACC
    let mut push_acc_map: HashMap<String, f64> = HashMap::new();
    let engine_all: Vec<crate::features::rks::engine::RksRecord> = records.iter().filter_map(|r| {
        let diff = match r.difficulty.as_str() {"EZ"=>Difficulty::EZ, "HD"=>Difficulty::HD, "IN"=>Difficulty::IN, "AT"=>Difficulty::AT, _=>return None};
        Some(crate::features::rks::engine::RksRecord{
            song_id: r.song_id.clone(), difficulty: diff, score: r.score.unwrap_or(0.0) as u32,
            acc: r.acc, rks: r.rks, chart_constant: r.difficulty_value,
        })
    }).collect();
    for r in top.iter().filter(|s| s.acc < 100.0 && s.difficulty_value > 0.0) {
        let key = format!("{}-{}", r.song_id, r.difficulty);
        if let Some(v) = crate::features::rks::engine::calculate_target_chart_push_acc(&key, r.difficulty_value, &engine_all) {
            push_acc_map.insert(key, v);
        }
    }

    // 统计项
    let (exact_rks, _rounded) = crate::features::rks::engine::calculate_player_rks_details(&engine_all);
    let ap_scores: Vec<_> = records.iter().filter(|r| r.acc >= 100.0).take(3).collect();
    let ap_top_3_avg = if ap_scores.len() == 3 { Some(ap_scores.iter().map(|r| r.rks).sum::<f64>()/3.0) } else { None };
    let best_27_avg = if records.is_empty() { None } else { Some(records.iter().take(27).map(|r| r.rks).sum::<f64>() / (records.len().min(27) as f64)) };

    // 昵称
    let display_name = req.nickname.clone().unwrap_or_else(|| "Phigros Player".into());

    // 水印控制：默认启用配置中的显式/隐式；若提供了正确的解除口令，则同时关闭二者
    let cfg = AppConfig::global();
    let unlocked = cfg.watermark.is_unlock_valid(req.unlock_password.as_deref());
    let explicit = if unlocked { false } else { cfg.watermark.explicit_badge };
    let implicit = if unlocked { false } else { cfg.watermark.implicit_pixel };

    let stats = PlayerStats {
        ap_top_3_avg,
        best_27_avg,
        real_rks: Some(exact_rks),
        player_name: Some(display_name),
        update_time: Utc::now(),
        n: n as u32,
        ap_top_3_scores: records.iter().filter(|r| r.acc >= 100.0).take(3).cloned().collect(),
        challenge_rank: None,
        data_string: None,
        custom_footer_text: Some(cfg.branding.footer_text.clone()),
        is_user_generated: explicit,
    };

    let svg = renderer::generate_svg_string(&top, &stats, Some(&push_acc_map), &req.theme, false)?;
    let png = renderer::render_svg_to_png(svg, implicit)?;

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
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("image/png"));
    Ok((StatusCode::OK, headers, png))
}
