//! 存档 API 处理模块（features/save）
use axum::{
    Router,
    body::Bytes,
    extract::{Query, State},
    http::{StatusCode, header::CONTENT_TYPE},
    response::{IntoResponse, Response},
    routing::post,
};
use moka::future::Cache;
use once_cell::sync::OnceCell;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::error::AppError;
use crate::features::rks::engine::{ChartRankingScore, PlayerRksResult, calculate_player_rks};
use crate::features::stats::storage::SubmissionRecord;
use crate::state::AppState;

use super::{
    models::{SaveAndRksResponseDoc, SaveResponseDoc, UnifiedSaveRequest},
    provider::{self, SaveSource},
};

/// oneOf 响应：仅解析存档，或解析存档并计算 RKS。
#[derive(serde::Serialize, utoipa::ToSchema)]
#[serde(untagged)]
pub enum SaveApiResponse {
    Save(SaveResponseDoc),
    SaveAndRks(SaveAndRksResponseDoc),
}

/// /save 缓存项。
#[derive(Clone)]
struct SaveCacheEntry {
    parsed: Arc<provider::ParsedSave>,
    data_body_bytes: Bytes,
}

#[derive(serde::Serialize)]
struct SaveDataBody<'a> {
    data: &'a provider::ParsedSave,
}

fn serialize_save_data_body(parsed: &provider::ParsedSave) -> Result<Bytes, AppError> {
    serde_json::to_vec(&SaveDataBody { data: parsed })
        .map(Bytes::from)
        .map_err(|e| AppError::Internal(format!("serialize save data response failed: {e}")))
}

fn serialize_json_bytes<T: serde::Serialize>(value: &T, label: &str) -> Result<Bytes, AppError> {
    serde_json::to_vec(value)
        .map(Bytes::from)
        .map_err(|e| AppError::Internal(format!("serialize {label} failed: {e}")))
}

fn json_bytes_response(body: Bytes) -> Response {
    (StatusCode::OK, [(CONTENT_TYPE, "application/json")], body).into_response()
}

/// /save 缓存 key：同一用户 + 同一 updatedAt + 同一认证版本视为同一份存档结果。
fn build_save_cache_key(
    user_hash: Option<&str>,
    updated_at: Option<&str>,
    taptap_version: Option<&str>,
) -> Option<String> {
    let user_hash = user_hash?;
    let updated_at = updated_at?;
    let ver = taptap_version.unwrap_or("default");
    Some(format!("{user_hash}:{updated_at}:{ver}"))
}

fn save_cache() -> &'static Cache<String, SaveCacheEntry> {
    static CACHE: OnceCell<Cache<String, SaveCacheEntry>> = OnceCell::new();
    CACHE.get_or_init(|| {
        let cfg = &crate::config::AppConfig::global().save;
        Cache::builder()
            .max_capacity(cfg.cache_max_entries.max(1))
            .time_to_live(Duration::from_secs(cfg.cache_ttl_secs.max(1)))
            .time_to_idle(Duration::from_secs(cfg.cache_tti_secs.max(1)))
            .build()
    })
}

#[utoipa::path(
    post,
    path = "/save",
    summary = "获取并解析玩家存档",
    description = "支持两种认证方式（官方 sessionToken / 外部凭证）。默认仅返回解析后的存档；当 `calculate_rks=true` 时同时返回玩家 RKS 概览，并为每个谱面回填推分信息（push_acc + push_acc_hint，用于区分“不可推分/需Phi/已满ACC”等）。",
    request_body = UnifiedSaveRequest,
    params(
        ("calculate_rks" = Option<bool>, Query, description = "是否计算玩家RKS（true=计算，默认不计算）"),
    ),
    responses(
        (
            status = 200,
            description = "成功解析存档；当 calculate_rks=true 时同时包含 rks 字段，并为每个谱面回填 push_acc 与 push_acc_hint（推分提示）",
            body = SaveApiResponse
        ),
        (
            status = 400,
            description = "请求参数错误",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        ),
        (
            status = 401,
            description = "认证失败",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        ),
        (
            status = 422,
            description = "参数校验失败/存档数据无效（解密、校验或解析失败等）",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        ),
        (
            status = 502,
            description = "上游网络错误（非超时）",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        ),
        (
            status = 504,
            description = "上游超时",
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
    tag = "Save"
)]
pub async fn get_save_data(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
    req: axum::extract::Request,
) -> Result<Response, AppError> {
    let t_total = Instant::now();

    let t_auth_merge = Instant::now();
    let (mut payload, bearer_state) =
        match crate::session_auth::parse_json_with_bearer_state::<UnifiedSaveRequest>(req)
            .await
        {
            Ok(v) => v,
            Err(e) => {
                tracing::info!(
                    target: "phi_backend::save::performance",
                    route = "/save",
                    phase = "auth_parse",
                    status = "failed",
                    dur_ms = t_auth_merge.elapsed().as_millis(),
                    "save performance"
                );
                return Err(e);
            }
        };
    if let Err(e) = crate::session_auth::merge_auth_from_bearer_if_missing(
        state.stats_storage.as_ref(),
        &bearer_state,
        &mut payload,
    )
    .await
    {
        tracing::info!(
            target: "phi_backend::save::performance",
            route = "/save",
            phase = "auth_merge",
            status = "failed",
            dur_ms = t_auth_merge.elapsed().as_millis(),
            "save performance"
        );
        return Err(e);
    }
    tracing::info!(
        target: "phi_backend::save::performance",
        route = "/save",
        phase = "auth_merge",
        status = "ok",
        dur_ms = t_auth_merge.elapsed().as_millis(),
        "save performance"
    );

    // 提前计算用户去敏哈希（避免 move 后不可用）
    let t_auth = Instant::now();
    let salt = crate::config::AppConfig::global()
        .stats
        .user_hash_salt
        .as_deref();
    let (user_hash, user_kind) = crate::session_auth::derive_user_identity_with_bearer(
        salt,
        &payload,
        &bearer_state,
    )?;
    if let (Some(storage), Some(user_hash_ref)) =
        (state.stats_storage.as_ref(), user_hash.as_deref())
    {
        storage.ensure_user_not_banned(user_hash_ref).await?;
    }

    let calc_rks = params
        .get("calculate_rks")
        .map(|v| v == "true")
        .unwrap_or(false);
    // 排行榜写入需要：统计存储开启 + 能识别到用户身份
    let need_leaderboard = state.stats_storage.is_some() && user_hash.is_some();
    let auth_ms = t_auth.elapsed().as_millis() as i64;
    tracing::info!(
        target: "phi_backend::save::performance",
        route = "/save",
        phase = "identity_derive",
        status = "ok",
        need_leaderboard,
        dur_ms = auth_ms,
        "save performance"
    );

    let t_source = Instant::now();
    let source = match validate_and_create_source(&payload) {
        Ok(source) => source,
        Err(e) => {
            tracing::info!(
                target: "phi_backend::save::performance",
                route = "/save",
                phase = "validate_source",
                status = "failed",
                dur_ms = t_source.elapsed().as_millis(),
                "save performance"
            );
            return Err(e);
        }
    };
    let source_ms = t_source.elapsed().as_millis() as i64;
    tracing::info!(
        target: "phi_backend::save::performance",
        route = "/save",
        phase = "validate_source",
        status = "ok",
        dur_ms = source_ms,
        "save performance"
    );
    let taptap_version = payload.taptap_version.as_deref();
    let save_cfg = &crate::config::AppConfig::global().save;

    // /save P1：先取 meta，再基于 user_hash + updatedAt 构造缓存键。
    let t_meta = Instant::now();
    let meta = match provider::fetch_save_meta(
        source,
        &crate::config::AppConfig::global().taptap,
        taptap_version,
    )
    .await
    {
        Ok(meta) => meta,
        Err(e) => {
            tracing::info!(
                target: "phi_backend::save::performance",
                route = "/save",
                phase = "fetch_meta",
                status = "failed",
                dur_ms = t_meta.elapsed().as_millis(),
                "save performance"
            );
            return Err(e.into());
        }
    };
    let meta_ms = t_meta.elapsed().as_millis() as i64;
    tracing::info!(
        target: "phi_backend::save::performance",
        route = "/save",
        phase = "fetch_meta",
        status = "ok",
        dur_ms = meta_ms,
        "save performance"
    );
    let mut cache_skip_reason: Option<&'static str> = None;
    if !save_cfg.cache_enabled {
        cache_skip_reason = Some("disabled");
    } else if user_hash.is_none() {
        cache_skip_reason = Some("missing_user_hash");
    } else if meta.updated_at.is_none() {
        cache_skip_reason = Some("missing_updated_at");
    }
    let cache_key = if save_cfg.cache_enabled {
        build_save_cache_key(
            user_hash.as_deref(),
            meta.updated_at.as_deref(),
            taptap_version,
        )
    } else {
        None
    };

    let (parsed_cached, data_body_bytes, cache_lookup_ms, save_decode_ms, cache_status) =
        if let Some(key) = cache_key.as_ref() {
            let t_cache = Instant::now();
            if let Some(entry) = save_cache().get(key).await {
                let cache_lookup_ms = t_cache.elapsed().as_millis() as i64;
                if let Some(stats) = state.stats.as_ref() {
                    let extra = serde_json::json!({
                        "status": "hit",
                        "version": taptap_version.unwrap_or("default")
                    });
                    stats.track_feature("save_cache", "hit", user_hash.clone(), Some(extra));
                }

                let t_decode = Instant::now();
                let parsed = entry.parsed.clone();
                let data_body_bytes = entry.data_body_bytes.clone();
                let save_decode_ms = t_decode.elapsed().as_millis() as i64;
                (
                    parsed,
                    data_body_bytes,
                    cache_lookup_ms,
                    save_decode_ms,
                    "hit",
                )
            } else {
                let cache_lookup_ms = t_cache.elapsed().as_millis() as i64;
                if let Some(stats) = state.stats.as_ref() {
                    let extra = serde_json::json!({
                        "status": "miss",
                        "version": taptap_version.unwrap_or("default")
                    });
                    stats.track_feature("save_cache", "miss", user_hash.clone(), Some(extra));
                }

                let t_decode = Instant::now();
                let parsed =
                    provider::get_decrypted_save_from_meta(meta, state.chart_constants.clone())
                        .await?;
                let parsed = Arc::new(parsed);
                let data_body_bytes = serialize_save_data_body(parsed.as_ref())?;
                let save_decode_ms = t_decode.elapsed().as_millis() as i64;
                save_cache()
                    .insert(
                        key.clone(),
                        SaveCacheEntry {
                            parsed: parsed.clone(),
                            data_body_bytes: data_body_bytes.clone(),
                        },
                    )
                    .await;
                (
                    parsed,
                    data_body_bytes,
                    cache_lookup_ms,
                    save_decode_ms,
                    "miss",
                )
            }
        } else {
            if let Some(stats) = state.stats.as_ref() {
                let extra = serde_json::json!({
                    "status": "skipped",
                    "reason": cache_skip_reason.unwrap_or("unknown"),
                    "version": taptap_version.unwrap_or("default")
                });
                stats.track_feature("save_cache", "skipped", user_hash.clone(), Some(extra));
            }

            let t_decode = Instant::now();
            let parsed =
                provider::get_decrypted_save_from_meta(meta, state.chart_constants.clone()).await?;
            let parsed = Arc::new(parsed);
            let data_body_bytes = serialize_save_data_body(parsed.as_ref())?;
            let save_decode_ms = t_decode.elapsed().as_millis() as i64;
            (parsed, data_body_bytes, 0_i64, save_decode_ms, "skipped")
        };
    let cache_lookup_status = if cache_status == "skipped" {
        "skipped"
    } else {
        "ok"
    };
    tracing::info!(
        target: "phi_backend::save::performance",
        route = "/save",
        phase = "cache_lookup",
        status = cache_lookup_status,
        cache_status,
        dur_ms = cache_lookup_ms,
        "save performance"
    );
    tracing::info!(
        target: "phi_backend::save::performance",
        route = "/save",
        phase = "decode_parse",
        status = "ok",
        cache_status,
        dur_ms = save_decode_ms,
        "save performance"
    );

    // 业务打点：成功获取存档
    if let Some(stats) = state.stats.as_ref() {
        let extra = serde_json::json!({ "user_kind": user_kind });
        stats.track_feature("save", "get_save", user_hash.clone(), Some(extra));
    }

    // 计算 RKS / 文本详情属于 CPU 密集任务：移出 Tokio worker，避免影响吞吐与尾延迟。
    let need_calc = calc_rks || need_leaderboard;
    if !need_calc {
        let t_serialize = Instant::now();
        let body = data_body_bytes.clone();
        let serialize_ms = t_serialize.elapsed().as_millis() as i64;
        tracing::info!(
            target: "phi_backend::save::performance",
            route = "/save",
            phase = "calc",
            status = "skipped",
            calculate_rks = false,
            dur_ms = 0_i64,
            "save performance"
        );
        tracing::info!(
            target: "phi_backend::save::performance",
            route = "/save",
            phase = "serialize",
            status = "ok",
            calculate_rks = false,
            cache_status,
            dur_ms = serialize_ms,
            "save performance"
        );
        if let Some(stats) = state.stats.as_ref() {
            let extra = serde_json::json!({
                "cache_status": cache_status,
                "cache_lookup_ms": cache_lookup_ms,
                "save_decode_ms": save_decode_ms,
                "auth_ms": auth_ms,
                "source_ms": source_ms,
                "meta_ms": meta_ms,
                "calc_ms": 0_i64,
                "serialize_ms": serialize_ms,
                "total_ms": t_total.elapsed().as_millis() as i64,
                "calculate_rks": false,
                "version": taptap_version.unwrap_or("default")
            });
            stats.track_feature("save", "perf", user_hash.clone(), Some(extra));
        }
        tracing::info!(
            target: "phi_backend::save::performance",
            route = "/save",
            phase = "total",
            status = "ok",
            calculate_rks = false,
            cache_status,
            total_dur_ms = t_total.elapsed().as_millis(),
            "save performance"
        );
        return Ok(json_bytes_response(body));
    }

    let parsed_for_calc = Arc::clone(&parsed_cached);
    let permit = super::save_rks_blocking_semaphore()
        .clone()
        .acquire_owned()
        .await
        .map_err(|e| AppError::Internal(format!("save blocking semaphore closed: {e}")))?;
    let t_calc = Instant::now();
    let state_for_calc = state.clone();
    let join = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        if calc_rks {
            let mut game_record = parsed_for_calc.game_record.clone();
            crate::features::rks::engine::fill_push_acc_for_game_record(&mut game_record);
            let rks_res = calculate_player_rks(&game_record, &state_for_calc.chart_constants);
            let (best_top3_json, ap_top3_json, rks_comp_json) = if need_leaderboard {
                let (best_top3, ap_top3, rks_comp) =
                    build_textual_details_from_rks(&game_record, &rks_res, &state_for_calc);
                (
                    serde_json::to_string(&best_top3).ok(),
                    serde_json::to_string(&ap_top3).ok(),
                    serde_json::to_string(&rks_comp).ok(),
                )
            } else {
                (None, None, None)
            };
            (
                Some(game_record),
                rks_res,
                best_top3_json,
                ap_top3_json,
                rks_comp_json,
            )
        } else {
            let game_record = &parsed_for_calc.game_record;
            let rks_res = calculate_player_rks(game_record, &state_for_calc.chart_constants);
            let (best_top3_json, ap_top3_json, rks_comp_json) = if need_leaderboard {
                let (best_top3, ap_top3, rks_comp) =
                    build_textual_details_from_rks(game_record, &rks_res, &state_for_calc);
                (
                    serde_json::to_string(&best_top3).ok(),
                    serde_json::to_string(&ap_top3).ok(),
                    serde_json::to_string(&rks_comp).ok(),
                )
            } else {
                (None, None, None)
            };
            (None, rks_res, best_top3_json, ap_top3_json, rks_comp_json)
        }
    })
    .await;
    let (calc_game_record, rks_res, best_top3_json, ap_top3_json, rks_comp_json) = match join {
        Ok(v) => v,
        Err(e) => {
            let e_str = e.to_string();
            tracing::info!(
                target: "phi_backend::save::performance",
                route = "/save",
                phase = "calc",
                status = "failed",
                dur_ms = t_calc.elapsed().as_millis(),
                "save performance"
            );
            if let Ok(panic) = e.try_into_panic() {
                std::panic::resume_unwind(panic);
            }
            return Err(AppError::Internal(format!(
                "spawn_blocking cancelled: {e_str}"
            )));
        }
    };
    let calc_ms = t_calc.elapsed().as_millis() as i64;
    tracing::info!(
        target: "phi_backend::save::performance",
        route = "/save",
        phase = "calc",
        status = "ok",
        calc_rks,
        need_leaderboard,
        dur_ms = calc_ms,
        "save performance"
    );

    // 排行榜入库（无论是否返回RKS）
    if let Some(storage) = state.stats_storage.as_ref()
        && let Some(user_hash_ref) = user_hash.as_ref()
    {
        let total_rks = rks_res.total_rks;
        let now = chrono::Utc::now().to_rfc3339();
        // (2)B：写库改为 best-effort 后台任务，避免数据库波动影响 /save 主响应。
        let storage = storage.clone();
        let user_hash_s = user_hash_ref.clone();
        let user_kind_s = user_kind.clone();
        tokio::spawn(async move {
            // prev_rks 仅用于计算 rks_jump/suspicion，失败则按 0 处理并告警（不影响主流程）。
            let prev = match storage.get_prev_rks(&user_hash_s).await {
                Ok(v) => v,
                Err(e) => {
                    tracing::warn!(
                        target: "phi_backend::leaderboard",
                        user_hash = %user_hash_s,
                        "get_prev_rks failed (ignored): {e}"
                    );
                    None
                }
            };
            let prev_rks = prev.as_ref().map(|v| v.0).unwrap_or(0.0);
            // rksJump 用于“相对前值的变化量/异常跳变”判断：去除 f64 计算与存储带来的极小噪声。
            // 说明：1e-9 远小于 UI 常见展示精度（0.01），但足以覆盖 1e-15 量级抖动。
            const RKS_JUMP_EPS: f64 = 1e-9;
            let rks_jump = if prev_rks > 0.0 {
                total_rks - prev_rks
            } else {
                0.0
            };
            let rks_jump = if rks_jump.abs() < RKS_JUMP_EPS {
                0.0
            } else {
                rks_jump
            };

            let mut suspicion = 0.0_f64;
            if total_rks > 20.0 {
                suspicion += 0.5;
            }
            if rks_jump > 1.0 {
                suspicion += 0.8;
            } else if rks_jump > 0.5 {
                suspicion += 0.3;
            }
            if let Some(kind) = user_kind_s.as_deref()
                && kind == "session_token"
            {
                suspicion = (suspicion - 0.2).max(0.0);
            }
            let hide = suspicion >= 1.0;

            if let Err(e) = storage
                .insert_submission(SubmissionRecord {
                    user_hash: &user_hash_s,
                    total_rks,
                    rks_jump,
                    route: "/save",
                    client_ip_hash: None,
                    details_json: None,
                    suspicion_score: suspicion,
                    now_rfc3339: &now,
                })
                .await
            {
                tracing::warn!(
                    target: "phi_backend::leaderboard",
                    user_hash = %user_hash_s,
                    "insert_submission failed (ignored): {e}"
                );
            }
            if let Err(e) = storage
                .upsert_leaderboard_rks(
                    &user_hash_s,
                    total_rks,
                    user_kind_s.as_deref(),
                    suspicion,
                    hide,
                    &now,
                )
                .await
            {
                tracing::warn!(
                    target: "phi_backend::leaderboard",
                    user_hash = %user_hash_s,
                    "upsert_leaderboard_rks failed (ignored): {e}"
                );
            }
            if let Err(e) = storage
                .upsert_details(
                    &user_hash_s,
                    rks_comp_json.as_deref(),
                    best_top3_json.as_deref(),
                    ap_top3_json.as_deref(),
                    &now,
                )
                .await
            {
                tracing::warn!(
                    target: "phi_backend::leaderboard",
                    user_hash = %user_hash_s,
                    "upsert_details failed (ignored): {e}"
                );
            }

            // 默认在排行榜上展示：首次保存时创建公开资料（best-effort）
            let cfg = crate::config::AppConfig::global();
            if cfg.leaderboard.allow_public
                && let Err(e) = storage
                    .ensure_default_public_profile(
                        &user_hash_s,
                        user_kind_s.as_deref(),
                        cfg.leaderboard.default_show_rks_composition,
                        cfg.leaderboard.default_show_best_top3,
                        cfg.leaderboard.default_show_ap_top3,
                        &now,
                    )
                    .await
            {
                tracing::warn!(
                    target: "phi_backend::leaderboard",
                    user_hash = %user_hash_s,
                    "ensure_default_public_profile failed (ignored): {e}"
                );
            }
        });

        // 默认公开资料创建已移动到后台 best-effort 任务中。
    }

    if !calc_rks {
        let t_serialize = Instant::now();
        let body = data_body_bytes.clone();
        let serialize_ms = t_serialize.elapsed().as_millis() as i64;
        tracing::info!(
            target: "phi_backend::save::performance",
            route = "/save",
            phase = "calc",
            status = "skipped",
            calculate_rks = false,
            dur_ms = 0_i64,
            "save performance"
        );
        tracing::info!(
            target: "phi_backend::save::performance",
            route = "/save",
            phase = "serialize",
            status = "ok",
            calculate_rks = false,
            cache_status,
            dur_ms = serialize_ms,
            "save performance"
        );
        if let Some(stats) = state.stats.as_ref() {
            let extra = serde_json::json!({
                "cache_status": cache_status,
                "cache_lookup_ms": cache_lookup_ms,
                "save_decode_ms": save_decode_ms,
                "auth_ms": auth_ms,
                "source_ms": source_ms,
                "meta_ms": meta_ms,
                "calc_ms": calc_ms,
                "serialize_ms": serialize_ms,
                "total_ms": t_total.elapsed().as_millis() as i64,
                "calculate_rks": false,
                "version": taptap_version.unwrap_or("default")
            });
            stats.track_feature("save", "perf", user_hash.clone(), Some(extra));
        }
        tracing::info!(
            target: "phi_backend::save::performance",
            route = "/save",
            phase = "total",
            status = "ok",
            calculate_rks = false,
            cache_status,
            total_dur_ms = t_total.elapsed().as_millis(),
            "save performance"
        );
        return Ok(json_bytes_response(body));
    }

    // 计算 RKS 并返回复合响应
    let calc_game_record = calc_game_record.unwrap_or_else(|| parsed_cached.game_record.clone());
    let grade_counts = compute_grade_counts(&calc_game_record);
    let resp = SaveAndRksResponse {
        save: provider::ParsedSave {
            game_record: calc_game_record,
            game_progress: parsed_cached.game_progress.clone(),
            user: parsed_cached.user.clone(),
            settings: parsed_cached.settings.clone(),
            game_key: parsed_cached.game_key.clone(),
            summary_parsed: parsed_cached.summary_parsed.clone(),
            updated_at: parsed_cached.updated_at.clone(),
        },
        rks: rks_res,
        grade_counts,
    };
    let t_serialize = Instant::now();
    let body = serialize_json_bytes(&resp, "save+rks response")?;
    let serialize_ms = t_serialize.elapsed().as_millis() as i64;
    tracing::info!(
        target: "phi_backend::save::performance",
        route = "/save",
        phase = "serialize",
        status = "ok",
        calculate_rks = true,
        cache_status,
        dur_ms = serialize_ms,
        "save performance"
    );
    if let Some(stats) = state.stats.as_ref() {
        let extra = serde_json::json!({
            "cache_status": cache_status,
            "cache_lookup_ms": cache_lookup_ms,
            "save_decode_ms": save_decode_ms,
            "auth_ms": auth_ms,
            "source_ms": source_ms,
            "meta_ms": meta_ms,
            "calc_ms": calc_ms,
            "serialize_ms": serialize_ms,
            "total_ms": t_total.elapsed().as_millis() as i64,
            "calculate_rks": true,
            "version": taptap_version.unwrap_or("default")
        });
        stats.track_feature("save", "perf", user_hash.clone(), Some(extra));
    }
    tracing::info!(
        target: "phi_backend::save::performance",
        route = "/save",
        phase = "total",
        status = "ok",
        calculate_rks = true,
        cache_status,
        total_dur_ms = t_total.elapsed().as_millis(),
        "save performance"
    );
    Ok(json_bytes_response(body))
}

fn validate_and_create_source(payload: &UnifiedSaveRequest) -> Result<SaveSource, AppError> {
    match (&payload.session_token, &payload.external_credentials) {
        (Some(token), None) => {
            if token.is_empty() {
                return Err(AppError::SaveHandlerError(
                    "sessionToken 不能为空".to_string(),
                ));
            }
            Ok(SaveSource::official(token.clone()))
        }
        (None, Some(creds)) => {
            if !creds.is_valid() {
                return Err(AppError::SaveHandlerError(
                    "外部凭证无效：必须提供以下凭证之一：platform + platformId / sessiontoken / apiUserId"
                        .to_string(),
                ));
            }
            Ok(SaveSource::external(creds.clone()))
        }
        (Some(_), Some(_)) => Err(AppError::SaveHandlerError(
            "不能同时提供 sessionToken 和 externalCredentials，请只选择其中一种认证方式"
                .to_string(),
        )),
        (None, None) => Err(AppError::SaveHandlerError(
            "必须提供 sessionToken 或 externalCredentials 中的一项".to_string(),
        )),
    }
}

pub fn create_save_router() -> Router<AppState> {
    Router::<AppState>::new().route("/save", post(get_save_data))
}

#[derive(Debug, serde::Serialize)]
pub struct SaveAndRksResponse {
    /// 解析后的存档对象（等价于 SaveResponse.data）
    save: provider::ParsedSave,
    /// 计算得到的玩家 RKS 概览
    rks: PlayerRksResult,
    /// 按难度统计的 C/FC/P 成绩数量（累计口径）
    #[serde(rename = "gradeCounts")]
    grade_counts: super::models::CfcPCountsByDifficulty,
}

/// 统计每个难度下的 C/FC/P 成绩数量（累计口径）。
fn compute_grade_counts(
    records: &std::collections::HashMap<String, Vec<super::models::DifficultyRecord>>,
) -> super::models::CfcPCountsByDifficulty {
    use super::models::{CfcPCounts, CfcPCountsByDifficulty, Difficulty};

    #[derive(Debug, Clone, Copy)]
    enum Tier {
        C,
        FC,
        P,
    }

    let mut counts = CfcPCountsByDifficulty::default();

    for (_song_id, diffs) in records.iter() {
        for rec in diffs.iter() {
            let tier = if rec.score == 1_000_000 {
                Tier::P
            } else if rec.is_full_combo {
                Tier::FC
            } else if rec.score > 700_000 {
                Tier::C
            } else {
                continue;
            };

            let bucket: &mut CfcPCounts = match &rec.difficulty {
                Difficulty::EZ => &mut counts.ez,
                Difficulty::HD => &mut counts.hd,
                Difficulty::IN => &mut counts.in_,
                Difficulty::AT => &mut counts.at,
            };

            match tier {
                Tier::P => {
                    bucket.p += 1;
                    bucket.fc += 1;
                    bucket.c += 1;
                }
                Tier::FC => {
                    bucket.fc += 1;
                    bucket.c += 1;
                }
                Tier::C => {
                    bucket.c += 1;
                }
            }
        }
    }

    counts
}

fn normalize_accuracy_percent(acc: f32) -> f64 {
    let raw = acc as f64;
    if raw <= 1.5 { raw * 100.0 } else { raw }
}

fn build_chart_acc_index(
    records: &std::collections::HashMap<String, Vec<super::models::DifficultyRecord>>,
) -> (HashMap<String, f64>, usize, usize) {
    let mut acc_by_chart = HashMap::new();
    let mut valid_count = 0usize;
    let mut ap_count = 0usize;

    for (song_id, diffs) in records {
        for rec in diffs {
            // 与 calculate_player_rks 的口径保持一致，仅统计存在定数的谱面。
            if rec.chart_constant.is_none() {
                continue;
            }
            let acc = normalize_accuracy_percent(rec.accuracy);
            let key = format!("{song_id}-{}", rec.difficulty);
            acc_by_chart.insert(key, acc);
            valid_count = valid_count.saturating_add(1);
            if acc >= 100.0 {
                ap_count = ap_count.saturating_add(1);
            }
        }
    }

    (acc_by_chart, valid_count, ap_count)
}

/// 基于已计算完成的 RKS 结果构建文本详情，避免重复计算每个谱面的 RKS。
fn build_textual_details_from_rks(
    records: &std::collections::HashMap<String, Vec<super::models::DifficultyRecord>>,
    rks_result: &PlayerRksResult,
    state: &AppState,
) -> (
    Vec<crate::leaderboard_contract::ChartTextItem>,
    Vec<crate::leaderboard_contract::ChartTextItem>,
    crate::leaderboard_contract::RksCompositionText,
) {
    use crate::leaderboard_contract::{ChartTextItem, RksCompositionText};
    let (acc_by_chart, valid_count, ap_count) = build_chart_acc_index(records);
    let total_charts = rks_result.b30_charts.len();
    let best27_len = valid_count.min(27).min(total_charts);
    let ap3_len = ap_count.min(3).min(total_charts.saturating_sub(best27_len));

    let best_slice = &rks_result.b30_charts[..best27_len];
    let ap_slice = &rks_result.b30_charts[best27_len..best27_len + ap3_len];

    let name_of = |sid: &str| -> String {
        state
            .song_catalog
            .by_id
            .get(sid)
            .map(|s| s.name.clone())
            .unwrap_or_else(|| sid.to_string())
    };

    let to_text = |v: &[ChartRankingScore]| -> Vec<ChartTextItem> {
        v.iter()
            .take(3)
            .map(|score| {
                let key = format!("{}-{}", score.song_id, score.difficulty);
                let acc = acc_by_chart.get(&key).copied().unwrap_or(0.0);
                ChartTextItem {
                    song: name_of(&score.song_id),
                    difficulty: score.difficulty.to_string(),
                    acc,
                    rks: score.rks,
                }
            })
            .collect()
    };
    let best_top3 = to_text(best_slice);
    let ap_top3 = to_text(ap_slice);
    let best27_sum: f64 = best_slice.iter().map(|v| v.rks).sum();
    let ap3_sum: f64 = ap_slice.iter().map(|v| v.rks).sum();

    (
        best_top3,
        ap_top3,
        RksCompositionText {
            best27_sum,
            ap_top3_sum: ap3_sum,
        },
    )
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::{build_save_cache_key, compute_grade_counts};
    use crate::features::save::models::{Difficulty, DifficultyRecord};

    fn rec(difficulty: Difficulty, score: u32, is_full_combo: bool) -> DifficultyRecord {
        DifficultyRecord {
            difficulty,
            score,
            // 本测试仅关注 score/fc 统计逻辑，accuracy 等字段填充占位值即可。
            accuracy: 0.0,
            is_full_combo,
            chart_constant: None,
            push_acc: None,
            push_acc_hint: None,
        }
    }

    #[test]
    fn compute_grade_counts_counts_c_fc_p_with_cumulative_rule() {
        let mut records: HashMap<String, Vec<DifficultyRecord>> = HashMap::new();
        records.insert(
            "song_a".to_string(),
            vec![
                rec(Difficulty::EZ, 700_001, false),   // C
                rec(Difficulty::HD, 500_000, true),    // FC（计入 C）
                rec(Difficulty::IN, 1_000_000, false), // P（即使 fc=false，也计入 FC/C）
                rec(Difficulty::AT, 700_000, false),   // 不计入任何等级
            ],
        );
        records.insert(
            "song_b".to_string(),
            vec![
                rec(Difficulty::EZ, 1_000_000, true), // P
                rec(Difficulty::AT, 700_000, true),   // FC（边界分数也要计入 C）
            ],
        );

        let counts = compute_grade_counts(&records);

        // EZ：1 个 C + 1 个 P（P 同时计入 FC/C）
        assert_eq!(counts.ez.c, 2);
        assert_eq!(counts.ez.fc, 1);
        assert_eq!(counts.ez.p, 1);

        // HD：1 个 FC（同时计入 C）
        assert_eq!(counts.hd.c, 1);
        assert_eq!(counts.hd.fc, 1);
        assert_eq!(counts.hd.p, 0);

        // IN：1 个 P（同时计入 FC/C）
        assert_eq!(counts.in_.c, 1);
        assert_eq!(counts.in_.fc, 1);
        assert_eq!(counts.in_.p, 1);

        // AT：1 个 FC（score=700000，但 fc=true 仍计入 FC/C）
        assert_eq!(counts.at.c, 1);
        assert_eq!(counts.at.fc, 1);
        assert_eq!(counts.at.p, 0);
    }

    #[test]
    fn grade_counts_serializes_with_expected_keys() {
        let records: HashMap<String, Vec<DifficultyRecord>> = HashMap::new();
        let counts = compute_grade_counts(&records);
        let v = serde_json::to_value(counts).expect("serialize");

        // 难度键名（大写）
        assert!(v.get("EZ").is_some());
        assert!(v.get("HD").is_some());
        assert!(v.get("IN").is_some());
        assert!(v.get("AT").is_some());

        // 成绩键名（大写）
        let ez = v.get("EZ").expect("EZ exists");
        assert!(ez.get("C").is_some());
        assert!(ez.get("FC").is_some());
        assert!(ez.get("P").is_some());
    }

    #[test]
    fn build_save_cache_key_requires_user_and_updated_at() {
        assert!(build_save_cache_key(None, Some("2026-02-10T00:00:00Z"), None).is_none());
        assert!(build_save_cache_key(Some("u1"), None, None).is_none());

        let key = build_save_cache_key(Some("u1"), Some("2026-02-10T00:00:00Z"), Some("global"))
            .expect("cache key");
        assert_eq!(key, "u1:2026-02-10T00:00:00Z:global");
    }
}
