//! 存档 API 处理模块（features/save）
use axum::{
    Router,
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Json},
    routing::post,
};
use std::collections::HashMap;

use crate::error::AppError;
use crate::features::rks::engine::{
    PlayerRksResult, calculate_player_rks_simplified as calculate_player_rks,
    calculate_single_chart_rks,
};
use crate::features::stats::storage::SubmissionRecord;
use crate::state::AppState;

use super::{
    models::UnifiedSaveRequest,
    provider::{self, SaveSource},
};

#[utoipa::path(
    post,
    path = "/save",
    summary = "获取并解析玩家存档",
    description = "支持两种认证方式（官方 sessionToken / 外部凭证）。默认仅返回解析后的存档；当 `calculate_rks=true` 时同时返回玩家 RKS 概览。",
    request_body = UnifiedSaveRequest,
    params(
        ("calculate_rks" = Option<bool>, Query, description = "是否计算玩家RKS（true=计算，默认不计算）"),
    ),
    responses(
        (status = 200, description = "成功解析存档，body 为 SaveResponse", body = crate::features::save::models::SaveResponseDoc),
        (status = 200, description = "成功解析存档并计算RKS", body = crate::features::save::models::SaveAndRksResponseDoc),
        (status = 400, description = "请求参数错误", body = AppError),
        (status = 500, description = "服务器内部错误", body = AppError)
    ),
    tag = "Save"
)]
pub async fn get_save_data(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
    Json(payload): Json<UnifiedSaveRequest>,
) -> Result<impl IntoResponse, AppError> {
    // 提前计算用户去敏哈希（避免 move 后不可用）
    let salt = crate::config::AppConfig::global()
        .stats
        .user_hash_salt
        .as_deref();
    let (user_hash, user_kind) =
        crate::features::stats::derive_user_identity_from_auth(salt, &payload);

    let source = validate_and_create_source(payload.clone())?;
    let taptap_version = payload.taptap_version.as_deref();
    let parsed = provider::get_decrypted_save(
        source,
        &state.chart_constants,
        &crate::config::AppConfig::global().taptap,
        taptap_version,
    )
    .await?;

    // 业务打点：成功获取存档
    if let Some(stats) = state.stats.as_ref() {
        let extra = serde_json::json!({ "user_kind": user_kind });
        stats
            .track_feature("save", "get_save", user_hash.clone(), Some(extra))
            .await;
    }
    // 排行榜入库（无论是否返回RKS）
    if let Some(storage) = state.stats_storage.as_ref() {
        if let Some(user_hash_ref) = user_hash.as_ref() {
            let rks_res = calculate_player_rks(&parsed.game_record, &state.chart_constants);
            let total_rks = rks_res.total_rks;
            let (best_top3, ap_top3, rks_comp) =
                compute_textual_details(&parsed.game_record, &state);
            let best_top3_json = serde_json::to_string(&best_top3).ok();
            let ap_top3_json = serde_json::to_string(&ap_top3).ok();
            let rks_comp_json = serde_json::to_string(&rks_comp).ok();
            let now = chrono::Utc::now().to_rfc3339();
            let prev = storage.get_prev_rks(user_hash_ref).await?;
            let prev_rks = prev.as_ref().map(|v| v.0).unwrap_or(0.0);
            let rks_jump = if prev_rks > 0.0 {
                total_rks - prev_rks
            } else {
                0.0
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
            if let Some(kind) = user_kind.as_deref() {
                if kind == "session_token" {
                    suspicion = (suspicion - 0.2).max(0.0);
                }
            }
            let hide = suspicion >= 1.0;
            storage
                .insert_submission(SubmissionRecord {
                    user_hash: user_hash_ref,
                    total_rks,
                    rks_jump,
                    route: "/save",
                    client_ip_hash: None,
                    details_json: None,
                    suspicion_score: suspicion,
                    now_rfc3339: &now,
                })
                .await?;
            storage
                .upsert_leaderboard_rks(
                    user_hash_ref,
                    total_rks,
                    user_kind.as_deref(),
                    suspicion,
                    hide,
                    &now,
                )
                .await?;
            storage
                .upsert_details(
                    user_hash_ref,
                    rks_comp_json.as_deref(),
                    best_top3_json.as_deref(),
                    ap_top3_json.as_deref(),
                    &now,
                )
                .await?;

            // 默认在排行榜上展示：首次保存时创建公开资料
            let cfg = crate::config::AppConfig::global();
            if cfg.leaderboard.allow_public {
                let def_rc = if cfg.leaderboard.default_show_rks_composition {
                    1_i64
                } else {
                    0_i64
                };
                let def_b3 = if cfg.leaderboard.default_show_best_top3 {
                    1_i64
                } else {
                    0_i64
                };
                let def_ap3 = if cfg.leaderboard.default_show_ap_top3 {
                    1_i64
                } else {
                    0_i64
                };
                // 仅当不存在时创建公开行（不覆盖用户后续设置）
                let _ = sqlx::query(
                    "INSERT INTO user_profile(user_hash,is_public,show_rks_composition,show_best_top3,show_ap_top3,user_kind,created_at,updated_at) VALUES(?,?,?,?,?,?,?,?)
                     ON CONFLICT(user_hash) DO NOTHING"
                )
                .bind(user_hash_ref)
                .bind(1_i64)
                .bind(def_rc)
                .bind(def_b3)
                .bind(def_ap3)
                .bind(user_kind.as_deref())
                .bind(&now)
                .bind(&now)
                .execute(&storage.pool)
                .await;
            }
        }
    }

    let calc_rks = params
        .get("calculate_rks")
        .map(|v| v == "true")
        .unwrap_or(false);

    if !calc_rks {
        let value = serde_json::to_value(&parsed)
            .map_err(|e| AppError::Internal(format!("序列化 ParsedSave 失败: {e}")))?;
        let body = serde_json::json!({ "data": value });
        return Ok((StatusCode::OK, Json(body)));
    }

    // 计算 RKS 并返回复合响应
    let rks = calculate_player_rks(&parsed.game_record, &state.chart_constants);
    let save_value = serde_json::to_value(&parsed)
        .map_err(|e| AppError::Internal(format!("序列化 ParsedSave 失败: {e}")))?;
    let resp = SaveAndRksResponse {
        save: save_value,
        rks,
    };
    let body = serde_json::to_value(&resp)
        .map_err(|e| AppError::Internal(format!("序列化 SaveAndRksResponse 失败: {e}")))?;
    Ok((StatusCode::OK, Json(body)))
}

fn validate_and_create_source(payload: UnifiedSaveRequest) -> Result<SaveSource, AppError> {
    match (&payload.session_token, &payload.external_credentials) {
        (Some(token), None) => {
            if token.is_empty() {
                return Err(AppError::SaveHandlerError(
                    "sessionToken 不能为空".to_string(),
                ));
            }
            Ok(SaveSource::official(token))
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

#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct SaveAndRksResponse {
    /// 解析后的存档对象（等价于 SaveResponse.data）
    save: serde_json::Value,
    /// 计算得到的玩家 RKS 概览
    rks: PlayerRksResult,
}

/// 计算用于公开展示的文字详情（BestTop3、APTop3、RKS 构成）
fn compute_textual_details(
    records: &std::collections::HashMap<String, Vec<super::models::DifficultyRecord>>,
    state: &AppState,
) -> (
    Vec<crate::features::leaderboard::models::ChartTextItem>,
    Vec<crate::features::leaderboard::models::ChartTextItem>,
    crate::features::leaderboard::models::RksCompositionText,
) {
    use super::models::Difficulty;
    use crate::features::leaderboard::models::{ChartTextItem, RksCompositionText};
    let chart_constants = &state.chart_constants;

    let mut all_scores: Vec<(String, Difficulty, f64, f64)> = Vec::new(); // (song_id, diff, acc_percent, rks)
    let mut ap_scores: Vec<(String, Difficulty, f64, f64)> = Vec::new();

    for (song_id, diffs) in records.iter() {
        for rec in diffs.iter() {
            let Some(consts) = chart_constants.get(song_id) else {
                continue;
            };
            let level_opt = match rec.difficulty {
                Difficulty::EZ => consts.ez,
                Difficulty::HD => consts.hd,
                Difficulty::IN => consts.in_level,
                Difficulty::AT => consts.at,
            };
            let Some(level) = level_opt else {
                continue;
            };
            let acc_percent = rec.accuracy as f64;
            let acc_decimal = if acc_percent > 1.5 {
                acc_percent / 100.0
            } else {
                acc_percent
            } as f32;
            let rks = calculate_single_chart_rks(acc_decimal, level);
            all_scores.push((song_id.clone(), rec.difficulty.clone(), acc_percent, rks));
            if acc_percent >= 100.0 {
                ap_scores.push((song_id.clone(), rec.difficulty.clone(), acc_percent, rks));
            }
        }
    }

    all_scores.sort_by(|a, b| b.3.partial_cmp(&a.3).unwrap_or(core::cmp::Ordering::Equal));
    ap_scores.sort_by(|a, b| b.3.partial_cmp(&a.3).unwrap_or(core::cmp::Ordering::Equal));

    let name_of = |sid: &str| -> String {
        state
            .song_catalog
            .by_id
            .get(sid)
            .map(|s| s.name.clone())
            .unwrap_or_else(|| sid.to_string())
    };

    let to_text = |v: &[(String, Difficulty, f64, f64)]| -> Vec<ChartTextItem> {
        v.iter()
            .take(3)
            .map(|(sid, d, acc, rks)| ChartTextItem {
                song: name_of(sid),
                difficulty: d.to_string(),
                acc: (*acc),
                rks: (*rks),
            })
            .collect()
    };
    let best_top3 = to_text(&all_scores);
    let ap_top3 = to_text(&ap_scores);

    let best27_sum: f64 = all_scores.iter().take(27).map(|t| t.3).sum();
    let ap3_sum: f64 = ap_scores.iter().take(3).map(|t| t.3).sum();
    let rks_comp = RksCompositionText {
        best27_sum,
        ap_top3_sum: ap3_sum,
    };
    (best_top3, ap_top3, rks_comp)
}
