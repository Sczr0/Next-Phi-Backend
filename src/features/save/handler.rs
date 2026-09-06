//! 存档 API 处理模块（features/save）
use crate::extract::ValidatedQuery;
use axum::{Router, body::Bytes, extract::State, response::Response, routing::post};
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
    /// 缓存 miss/skip 时已解析的昵称（已烤入 `data_body`）；hit 时为 `None`。
    pub(super) nickname: Option<String>,
    /// hit 时回传未消费的昵称后台任务（仅 calculate_rks 响应需要时才等待；
    /// 丢弃时任务自行完成并温暖昵称缓存，ADR-0004）。
    pub(super) nickname_task: Option<tokio::task::JoinHandle<Option<String>>>,
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

/// 提取可用于 `users/me` 的会话令牌（ADR-0004）：body 官方 sessionToken 优先，
/// 其次 externalCredentials.sessiontoken（其存档流程本就走官方接口）。
fn effective_session_token(payload: &UnifiedSaveRequest) -> Option<&str> {
    if let Some(token) = payload.session_token.as_deref() {
        return (!token.is_empty()).then_some(token);
    }
    payload
        .external_credentials
        .as_ref()
        .and_then(|creds| creds.sessiontoken.as_deref())
        .filter(|token| !token.is_empty())
}

// 参数均为调用点就地可得的上下文（认证产物/共享状态/计时打点/昵称任务），
// 拆结构体只会增加一层转发；与本 crate 对形状类 lint 的整体宽容一致（lib.rs allow 列表）。
#[allow(clippy::too_many_arguments)]
async fn fetch_save_with_cache(
    source: SaveSource,
    taptap_version: Option<&str>,
    user_hash: Option<&str>,
    chart_constants: Arc<crate::startup::chart_loader::ChartConstantsMap>,
    stats: Option<&crate::stats_contract::StatsHandle>,
    auth_ms: i64,
    source_ms: i64,
    nickname_task: Option<tokio::task::JoinHandle<Option<String>>>,
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

    let (parsed, data_body, cache_lookup_ms, decode_ms, cache_status, nickname, nickname_task) =
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
                // hit：正文已带 miss 时烤入的昵称；后台任务原样回传，
                // 仅 calculate_rks 响应需要时才由调用方等待（ADR-0004）。
                (
                    parsed,
                    data_body_bytes,
                    cache_lookup_ms,
                    save_decode_ms,
                    "hit",
                    None,
                    nickname_task,
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
                // 昵称任务与存档下载/解密并发执行，此处只等剩余时间（ADR-0004）。
                let nickname = match nickname_task {
                    Some(handle) => handle.await.ok().flatten(),
                    None => None,
                };
                let data_body_bytes =
                    serialize_save_data_body(parsed.as_ref(), nickname.as_deref())?;
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
                    nickname,
                    None,
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
            let nickname = match nickname_task {
                Some(handle) => handle.await.ok().flatten(),
                None => None,
            };
            let data_body_bytes = serialize_save_data_body(parsed.as_ref(), nickname.as_deref())?;
            let save_decode_ms = duration_ms_i64(t_decode.elapsed());
            (
                parsed,
                data_body_bytes,
                0_i64,
                save_decode_ms,
                "skipped",
                nickname,
                None,
            )
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
        nickname,
        nickname_task,
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
    description = "支持三种认证方式（官方 sessionToken / 外部凭证 / Authorization: Bearer 内嵌凭证）。默认仅返回解析后的存档；当 calculate_rks=true 时同时返回玩家 RKS 概览，并为每个谱面回填推分信息（push_acc + push_acc_hint）。",
    request_body = UnifiedSaveRequest,
    params(
        ("calculate_rks" = Option<bool>, Query, description = "是否计算玩家RKS（true=计算，默认不计算）"),
    ),
    responses(
        (status = 200, description = "成功解析存档；当 calculate_rks=true 时同时包含 rks 字段，并为每个谱面回填 push_acc 与 push_acc_hint（推分提示）。响应顶层在能解析会话令牌（sessionToken / Bearer 内嵌凭证 / externalCredentials.sessiontoken）时附带 nickname 字段（ADR-0004）；无令牌或解析失败时该字段整体省略", body = SaveApiResponse),
        (status = 400, description = "请求参数错误", body = crate::error::ProblemDetails, content_type = "application/problem+json"),
        (status = 401, description = "认证失败", body = crate::error::ProblemDetails, content_type = "application/problem+json"),
        (status = 403, description = "用户已被封禁", body = crate::error::ProblemDetails, content_type = "application/problem+json"),
        (status = 422, description = "参数校验失败/存档数据无效（解密、校验或解析失败等）", body = crate::error::ProblemDetails, content_type = "application/problem+json"),
        (status = 502, description = "上游网络错误（非超时）", body = crate::error::ProblemDetails, content_type = "application/problem+json"),
        (status = 504, description = "上游超时", body = crate::error::ProblemDetails, content_type = "application/problem+json"),
        (status = 500, description = "服务器内部错误", body = crate::error::ProblemDetails, content_type = "application/problem+json")
    ),
    tag = "Save"
)]
pub async fn get_save_data(
    State(state): State<AppState>,
    ValidatedQuery(params): ValidatedQuery<std::collections::BTreeMap<String, String>>,
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

    // Phase 2.5: 昵称解析（ADR-0004）。有会话令牌（直接传入 / Bearer 合并 /
    // external.sessiontoken）即后台起任务，与存档元信息获取及下载并发执行；
    // 失败/超时降级为字段省略，绝不影响 /save 成功。
    let nickname_task = effective_session_token(&auth.payload).map(|token| {
        let token = token.to_owned();
        let taptap_version = auth.taptap_version.clone();
        tokio::spawn(async move {
            super::nickname::resolve_session_nickname(&token, taptap_version.as_deref()).await
        })
    });

    // Phase 3: 元数据获取 + 缓存
    let mut data = fetch_save_with_cache(
        source,
        auth.taptap_version.as_deref(),
        auth.user_hash.as_deref(),
        state.chart_constants.clone(),
        state.stats.as_ref(),
        auth.auth_ms,
        source_ms,
        nickname_task,
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
    // 昵称（ADR-0004）：纯存档响应的正文已在缓存填充阶段烤入昵称（miss/skip
    // 解析、hit 沿用），无需在此等待；仅 calculate_rks 的复合响应需要显式
    // nickname 字段——miss 时 `data.nickname` 已就绪，hit 时才等待后台任务
    // （昵称缓存 TTL 内为纯内存命中）。
    let nickname = if calc_rks {
        if data.nickname.is_some() {
            data.nickname.clone()
        } else {
            match data.nickname_task.take() {
                Some(handle) => handle.await.ok().flatten(),
                None => None,
            }
        }
    } else {
        None
    };
    let response = if let Some(ref rks_result) = rks_opt {
        if calc_rks {
            // 包含 RKS 的复合响应
            build_save_response(&data, nickname, Some((rks_result, data.parsed.as_ref())))?
        } else {
            // need_leaderboard 但不需要 RKS 响应
            build_save_response(&data, None, None)?
        }
    } else {
        build_save_response(&data, None, None)?
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
    use super::{build_save_cache_key, effective_session_token};
    use crate::features::save::client::ExternalApiCredentials;
    use crate::features::save::models::UnifiedSaveRequest;

    #[test]
    fn build_save_cache_key_requires_user_and_updated_at() {
        assert!(build_save_cache_key(None, Some("2026-02-10T00:00:00Z"), None).is_none());
        assert!(build_save_cache_key(Some("u1"), None, None).is_none());

        let key = build_save_cache_key(Some("u1"), Some("2026-02-10T00:00:00Z"), Some("global"))
            .expect("cache key");
        assert_eq!(key, "u1:2026-02-10T00:00:00Z:global");
    }

    /// ADR-0004：会话令牌提取矩阵。
    #[test]
    fn effective_session_token_covers_all_token_paths() {
        let token_of =
            |payload: &UnifiedSaveRequest| effective_session_token(payload).map(str::to_owned);

        // 官方 sessionToken 优先。
        let direct = UnifiedSaveRequest {
            session_token: Some("r:direct".to_string()),
            external_credentials: None,
            taptap_version: None,
        };
        assert_eq!(token_of(&direct).as_deref(), Some("r:direct"));

        // Bearer 合并后等价于直接 sessionToken（payload 已被填充），此处验证即可。
        // external.sessiontoken 作为回退。
        let external = UnifiedSaveRequest {
            session_token: None,
            external_credentials: Some(ExternalApiCredentials {
                platform: None,
                platform_id: None,
                sessiontoken: Some("r:external".to_string()),
                api_user_id: None,
                api_token: None,
            }),
            taptap_version: None,
        };
        assert_eq!(token_of(&external).as_deref(), Some("r:external"));

        // 无令牌外部凭证 → None。
        let external_no_token = UnifiedSaveRequest {
            session_token: None,
            external_credentials: Some(ExternalApiCredentials {
                platform: Some("TapTap".to_string()),
                platform_id: Some("12345".to_string()),
                sessiontoken: None,
                api_user_id: None,
                api_token: None,
            }),
            taptap_version: None,
        };
        assert_eq!(token_of(&external_no_token), None);

        // 空串按无令牌处理。
        let empty_token = UnifiedSaveRequest {
            session_token: Some(String::new()),
            external_credentials: None,
            taptap_version: None,
        };
        assert_eq!(token_of(&empty_token), None);

        // 两者皆无 → None。
        let none = UnifiedSaveRequest {
            session_token: None,
            external_credentials: None,
            taptap_version: None,
        };
        assert_eq!(token_of(&none), None);
    }
}
