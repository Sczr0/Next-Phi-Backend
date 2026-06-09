//! 存档 API 处理模块（features/save）
use axum::{
    Router,
    body::Bytes,
    extract::{Query, State},
    response::Response,
    routing::post,
};
use moka::future::Cache;
use once_cell::sync::OnceCell;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::error::AppError;
use crate::rks_contract::engine::{PlayerRksResult, calculate_player_rks};
use crate::state::AppState;
use crate::stats_contract::SubmissionRecord;

use super::{
    models::UnifiedSaveRequest,
    provider::{self, SaveSource},
};

mod response;

pub use self::response::{SaveAndRksResponse, SaveApiResponse};
use self::response::{
    build_save_response, build_textual_details_from_rks, serialize_save_data_body,
};

// ── 内部阶段结果结构体 ──

struct SaveAuth {
    user_hash: Option<String>,
    user_kind: Option<String>,
    payload: UnifiedSaveRequest,
    taptap_version: Option<String>,
    auth_ms: i64,
}

pub(super) struct SaveWithCache {
    pub(super) parsed: Arc<provider::ParsedSave>,
    pub(super) data_body: Bytes,
    pub(super) cache_status: &'static str,
    auth_ms: i64,
    source_ms: i64,
    meta_ms: i64,
    cache_lookup_ms: i64,
    decode_ms: i64,
}

pub(super) struct RksComputeResult {
    pub(super) game_record: HashMap<String, Vec<super::models::DifficultyRecord>>,
    pub(super) rks: PlayerRksResult,
    best_top3_json: Option<String>,
    ap_top3_json: Option<String>,
    rks_comp_json: Option<String>,
    pub(super) calc_ms: i64,
}

// ── 内部工具函数 ──

/// /save 缓存项。
#[derive(Clone)]
struct SaveCacheEntry {
    parsed: Arc<provider::ParsedSave>,
    data_body_bytes: Bytes,
}

pub(super) fn duration_ms_i64(duration: Duration) -> i64 {
    i64::try_from(duration.as_millis()).unwrap_or(i64::MAX)
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

// ── Phase 1: 认证 + 身份推导 ──

async fn authenticate_for_save(
    state: &AppState,
    req: axum::extract::Request,
) -> Result<SaveAuth, AppError> {
    let t_auth_merge = Instant::now();

    let (mut payload, bearer_state) =
        match crate::session_auth::parse_json_with_bearer_state::<UnifiedSaveRequest>(req).await {
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

    let t_auth = Instant::now();
    let salt = crate::config::AppConfig::global()
        .stats
        .user_hash_salt
        .as_deref();
    let (user_hash, user_kind) =
        crate::session_auth::derive_user_identity_with_bearer(salt, &payload, &bearer_state)?;
    if let (Some(storage), Some(user_hash_ref)) =
        (state.stats_storage.as_ref(), user_hash.as_deref())
    {
        storage.ensure_user_not_banned(user_hash_ref).await?;
    }
    let auth_ms = duration_ms_i64(t_auth.elapsed());
    let need_leaderboard = state.stats_storage.is_some() && user_hash.is_some();
    tracing::info!(
        target: "phi_backend::save::performance",
        route = "/save",
        phase = "identity_derive",
        status = "ok",
        need_leaderboard,
        dur_ms = auth_ms,
        "save performance"
    );

    let taptap_version = payload.taptap_version.clone();
    Ok(SaveAuth {
        user_hash,
        user_kind,
        payload,
        taptap_version,
        auth_ms,
    })
}

// ── Phase 3: 元数据获取 + 缓存 ──

async fn fetch_save_with_cache(
    source: SaveSource,
    taptap_version: Option<&str>,
    user_hash: Option<&str>,
    chart_constants: Arc<crate::startup::chart_loader::ChartConstantsMap>,
    stats: Option<&crate::stats_contract::StatsHandle>,
    auth_ms: i64,
    source_ms: i64,
) -> Result<SaveWithCache, AppError> {
    let save_cfg = &crate::config::AppConfig::global().save;

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
    let meta_ms = duration_ms_i64(t_meta.elapsed());
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
        build_save_cache_key(user_hash, meta.updated_at.as_deref(), taptap_version)
    } else {
        None
    };

    let (parsed, data_body, cache_lookup_ms, decode_ms, cache_status) =
        if let Some(key) = cache_key.as_ref() {
            let t_cache = Instant::now();
            if let Some(entry) = save_cache().get(key).await {
                let cache_lookup_ms = duration_ms_i64(t_cache.elapsed());
                if let Some(stats) = stats {
                    let extra = serde_json::json!({
                        "status": "hit",
                        "version": taptap_version.unwrap_or("default")
                    });
                    stats.track_feature(
                        "save_cache",
                        "hit",
                        user_hash.map(str::to_string),
                        Some(extra),
                    );
                }
                let t_decode = Instant::now();
                let parsed = entry.parsed.clone();
                let data_body_bytes = entry.data_body_bytes.clone();
                let save_decode_ms = duration_ms_i64(t_decode.elapsed());
                (
                    parsed,
                    data_body_bytes,
                    cache_lookup_ms,
                    save_decode_ms,
                    "hit",
                )
            } else {
                let cache_lookup_ms = duration_ms_i64(t_cache.elapsed());
                if let Some(stats) = stats {
                    let extra = serde_json::json!({
                        "status": "miss",
                        "version": taptap_version.unwrap_or("default")
                    });
                    stats.track_feature(
                        "save_cache",
                        "miss",
                        user_hash.map(str::to_string),
                        Some(extra),
                    );
                }
                let t_decode = Instant::now();
                let parsed = provider::get_decrypted_save_from_meta(meta, chart_constants).await?;
                let parsed = Arc::new(parsed);
                let data_body_bytes = serialize_save_data_body(parsed.as_ref())?;
                let save_decode_ms = duration_ms_i64(t_decode.elapsed());
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
            if let Some(stats) = stats {
                let extra = serde_json::json!({
                    "status": "skipped",
                    "reason": cache_skip_reason.unwrap_or("unknown"),
                    "version": taptap_version.unwrap_or("default")
                });
                stats.track_feature(
                    "save_cache",
                    "skipped",
                    user_hash.map(str::to_string),
                    Some(extra),
                );
            }
            let t_decode = Instant::now();
            let parsed = provider::get_decrypted_save_from_meta(meta, chart_constants).await?;
            let parsed = Arc::new(parsed);
            let data_body_bytes = serialize_save_data_body(parsed.as_ref())?;
            let save_decode_ms = duration_ms_i64(t_decode.elapsed());
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
        dur_ms = decode_ms,
        "save performance"
    );

    Ok(SaveWithCache {
        parsed,
        data_body,
        cache_status,
        auth_ms,
        source_ms,
        meta_ms,
        cache_lookup_ms,
        decode_ms,
    })
}

// ── Phase 4a: RKS 计算 ──

async fn compute_rks_and_details(
    parsed: Arc<provider::ParsedSave>,
    state: AppState,
    calc_rks: bool,
    need_leaderboard: bool,
) -> Result<RksComputeResult, AppError> {
    let permit = super::save_rks_blocking_semaphore()
        .clone()
        .acquire_owned()
        .await
        .map_err(|e| AppError::Internal(format!("save blocking semaphore closed: {e}")))?;
    let t_calc = Instant::now();
    let join = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let game_record = parsed.game_record.clone();
        if calc_rks {
            let mut game_record = game_record;
            crate::rks_contract::engine::fill_push_acc_for_game_record(&mut game_record);
            let rks_res = calculate_player_rks(&game_record, &state.chart_constants);
            let (best_top3_json, ap_top3_json, rks_comp_json) = if need_leaderboard {
                let (best_top3, ap_top3, rks_comp) =
                    build_textual_details_from_rks(&game_record, &rks_res, &state);
                (
                    serde_json::to_string(&best_top3).ok(),
                    serde_json::to_string(&ap_top3).ok(),
                    serde_json::to_string(&rks_comp).ok(),
                )
            } else {
                (None, None, None)
            };
            (
                game_record,
                rks_res,
                best_top3_json,
                ap_top3_json,
                rks_comp_json,
            )
        } else {
            let rks_res = calculate_player_rks(&game_record, &state.chart_constants);
            let (best_top3_json, ap_top3_json, rks_comp_json) = if need_leaderboard {
                let (best_top3, ap_top3, rks_comp) =
                    build_textual_details_from_rks(&game_record, &rks_res, &state);
                (
                    serde_json::to_string(&best_top3).ok(),
                    serde_json::to_string(&ap_top3).ok(),
                    serde_json::to_string(&rks_comp).ok(),
                )
            } else {
                (None, None, None)
            };
            (
                game_record,
                rks_res,
                best_top3_json,
                ap_top3_json,
                rks_comp_json,
            )
        }
    })
    .await;
    let (game_record, rks, best_top3_json, ap_top3_json, rks_comp_json) = match join {
        Ok(v) => v,
        Err(e) => {
            tracing::info!(
                target: "phi_backend::save::performance",
                route = "/save",
                phase = "calc",
                status = "failed",
                dur_ms = t_calc.elapsed().as_millis(),
                "save performance"
            );
            let e_str = e.to_string();
            if let Ok(panic) = e.try_into_panic() {
                std::panic::resume_unwind(panic);
            }
            return Err(AppError::Internal(format!(
                "spawn_blocking cancelled: {e_str}"
            )));
        }
    };
    let calc_ms = duration_ms_i64(t_calc.elapsed());
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

    Ok(RksComputeResult {
        game_record,
        rks,
        best_top3_json,
        ap_top3_json,
        rks_comp_json,
        calc_ms,
    })
}

// ── Phase 4b: 排行榜写入（后台 best-effort） ──

fn spawn_leaderboard_write(
    storage: Arc<crate::stats_contract::StatsStorage>,
    user_hash: String,
    user_kind: Option<String>,
    rks_result: &PlayerRksResult,
    best_top3_json: Option<String>,
    ap_top3_json: Option<String>,
    rks_comp_json: Option<String>,
) {
    let total_rks = rks_result.total_rks;
    let now = chrono::Utc::now().to_rfc3339();
    tokio::spawn(async move {
        let prev = match storage.get_prev_rks(&user_hash).await {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(
                    target: "phi_backend::leaderboard",
                    user_hash = %user_hash,
                    "get_prev_rks failed (ignored): {e}"
                );
                None
            }
        };
        let prev_rks = prev.as_ref().map_or(0.0, |v| v.0);
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
        if let Some(kind) = user_kind.as_deref()
            && kind == "session_token"
        {
            suspicion = (suspicion - 0.2).max(0.0);
        }
        let hide = suspicion >= 1.0;

        if let Err(e) = storage
            .insert_submission(SubmissionRecord {
                user_hash: &user_hash,
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
            tracing::warn!(target: "phi_backend::leaderboard", user_hash = %user_hash, "insert_submission failed (ignored): {e}");
        }
        if let Err(e) = storage
            .upsert_leaderboard_rks(
                &user_hash,
                total_rks,
                user_kind.as_deref(),
                suspicion,
                hide,
                &now,
            )
            .await
        {
            tracing::warn!(target: "phi_backend::leaderboard", user_hash = %user_hash, "upsert_leaderboard_rks failed (ignored): {e}");
        }
        if let Err(e) = storage
            .upsert_details(
                &user_hash,
                rks_comp_json.as_deref(),
                best_top3_json.as_deref(),
                ap_top3_json.as_deref(),
                &now,
            )
            .await
        {
            tracing::warn!(target: "phi_backend::leaderboard", user_hash = %user_hash, "upsert_details failed (ignored): {e}");
        }

        let cfg = crate::config::AppConfig::global();
        if cfg.leaderboard.allow_public
            && let Err(e) = storage
                .ensure_default_public_profile(
                    &user_hash,
                    user_kind.as_deref(),
                    cfg.leaderboard.default_show_rks_composition,
                    cfg.leaderboard.default_show_best_top3,
                    cfg.leaderboard.default_show_ap_top3,
                    &now,
                )
                .await
        {
            tracing::warn!(target: "phi_backend::leaderboard", user_hash = %user_hash, "ensure_default_public_profile failed (ignored): {e}");
        }
    });
}

// ── 主 Handler（编排器） ──

#[utoipa::path(
    post,
    path = "/save",
    summary = "获取并解析玩家存档",
    description = "支持两种认证方式（官方 sessionToken / 外部凭证）。默认仅返回解析后的存档；当 calculate_rks=true 时同时返回玩家 RKS 概览，并为每个谱面回填推分信息（push_acc + push_acc_hint）。",
    request_body = UnifiedSaveRequest,
    params(
        ("calculate_rks" = Option<bool>, Query, description = "是否计算玩家RKS（true=计算，默认不计算）"),
    ),
    responses(
        (status = 200, description = "成功解析存档；当 calculate_rks=true 时同时包含 rks 字段，并为每个谱面回填 push_acc 与 push_acc_hint（推分提示）", body = SaveApiResponse),
        (status = 400, description = "请求参数错误", body = crate::error::ProblemDetails, content_type = "application/problem+json"),
        (status = 401, description = "认证失败", body = crate::error::ProblemDetails, content_type = "application/problem+json"),
        (status = 422, description = "参数校验失败/存档数据无效（解密、校验或解析失败等）", body = crate::error::ProblemDetails, content_type = "application/problem+json"),
        (status = 502, description = "上游网络错误（非超时）", body = crate::error::ProblemDetails, content_type = "application/problem+json"),
        (status = 504, description = "上游超时", body = crate::error::ProblemDetails, content_type = "application/problem+json"),
        (status = 500, description = "服务器内部错误", body = crate::error::ProblemDetails, content_type = "application/problem+json")
    ),
    tag = "Save"
)]
pub async fn get_save_data(
    State(state): State<AppState>,
    Query(params): Query<std::collections::BTreeMap<String, String>>,
    req: axum::extract::Request,
) -> Result<Response, AppError> {
    let t_total = Instant::now();

    // Phase 1: 认证 + 身份推导
    let auth = authenticate_for_save(&state, req).await?;

    // Phase 2: 存档源验证
    let t_source = Instant::now();
    let source = validate_and_create_source(&auth.payload)?;
    let source_ms = duration_ms_i64(t_source.elapsed());
    tracing::info!(
        target: "phi_backend::save::performance",
        route = "/save", phase = "validate_source", status = "ok",
        dur_ms = source_ms, "save performance"
    );

    // Phase 3: 元数据获取 + 缓存
    let data = fetch_save_with_cache(
        source,
        auth.taptap_version.as_deref(),
        auth.user_hash.as_deref(),
        state.chart_constants.clone(),
        state.stats.as_ref(),
        auth.auth_ms,
        source_ms,
    )
    .await?;

    // 业务打点
    if let Some(stats) = state.stats.as_ref() {
        let extra = serde_json::json!({ "user_kind": auth.user_kind });
        stats.track_feature("save", "get_save", auth.user_hash.clone(), Some(extra));
    }

    let calc_rks = params.get("calculate_rks").is_some_and(|v| v == "true");
    let need_leaderboard = state.stats_storage.is_some() && auth.user_hash.is_some();
    let need_calc = calc_rks || need_leaderboard;

    // Phase 4: RKS 计算 + 排行榜写入
    let (rks_opt, calc_ms) = if need_calc {
        let result = compute_rks_and_details(
            data.parsed.clone(),
            state.clone(),
            calc_rks,
            need_leaderboard,
        )
        .await?;

        // 排行榜后台写入
        if let Some(storage) = state.stats_storage.as_ref()
            && let Some(ref user_hash_ref) = auth.user_hash
        {
            spawn_leaderboard_write(
                storage.clone(),
                user_hash_ref.clone(),
                auth.user_kind.clone(),
                &result.rks,
                result.best_top3_json.clone(),
                result.ap_top3_json.clone(),
                result.rks_comp_json.clone(),
            );
        }
        let calc_ms = result.calc_ms;
        (Some(result), calc_ms)
    } else {
        tracing::info!(
            target: "phi_backend::save::performance",
            route = "/save", phase = "calc", status = "skipped",
            calculate_rks = false, dur_ms = 0_i64, "save performance"
        );
        (None, 0_i64)
    };

    // Phase 5: 构建响应
    let response = if let Some(ref rks_result) = rks_opt {
        if calc_rks {
            // 包含 RKS 的复合响应
            build_save_response(&data, Some((rks_result, data.parsed.as_ref())))?
        } else {
            // need_leaderboard 但不需要 RKS 响应
            build_save_response(&data, None)?
        }
    } else {
        build_save_response(&data, None)?
    };

    // 最终性能统计
    if let Some(stats) = state.stats.as_ref() {
        let extra = serde_json::json!({
            "cache_status": data.cache_status,
            "cache_lookup_ms": data.cache_lookup_ms,
            "save_decode_ms": data.decode_ms,
            "auth_ms": data.auth_ms,
            "source_ms": data.source_ms,
            "meta_ms": data.meta_ms,
            "calc_ms": calc_ms,
            "serialize_ms": 0_i64,
            "total_ms": duration_ms_i64(t_total.elapsed()),
            "calculate_rks": calc_rks,
            "version": auth.taptap_version.as_deref().unwrap_or("default")
        });
        stats.track_feature("save", "perf", auth.user_hash.clone(), Some(extra));
    }
    tracing::info!(
        target: "phi_backend::save::performance",
        route = "/save", phase = "total", status = "ok",
        calculate_rks = calc_rks,
        cache_status = data.cache_status,
        total_dur_ms = t_total.elapsed().as_millis(),
        "save performance"
    );

    Ok(response)
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

#[cfg(test)]
mod tests {
    use super::build_save_cache_key;

    #[test]
    fn build_save_cache_key_requires_user_and_updated_at() {
        assert!(build_save_cache_key(None, Some("2026-02-10T00:00:00Z"), None).is_none());
        assert!(build_save_cache_key(Some("u1"), None, None).is_none());

        let key = build_save_cache_key(Some("u1"), Some("2026-02-10T00:00:00Z"), Some("global"))
            .expect("cache key");
        assert_eq!(key, "u1:2026-02-10T00:00:00Z:global");
    }
}
