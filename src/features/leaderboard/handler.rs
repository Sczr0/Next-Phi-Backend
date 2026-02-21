use axum::http::HeaderMap;
use axum::{
    Router,
    extract::{Path, Query, State},
    response::Json,
    routing::{get, post, put},
};
use serde::{Deserialize, Serialize};
use sqlx::Row;
use std::collections::HashMap;
use std::time::Instant;

use crate::{error::AppError, state::AppState};

use super::models::{
    AliasRequest, ChartTextItem, LeaderboardTopItem, LeaderboardTopResponse, MeResponse,
    ProfileUpdateRequest, PublicProfileResponse, RksCompositionText,
};

#[derive(Deserialize)]
pub struct TopQuery {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
    pub after_score: Option<f64>,
    pub after_updated: Option<String>,
    pub after_user: Option<String>,
    /// 精简模式：不返回 BestTop3/APTop3（默认 false）
    pub lite: Option<bool>,
}

#[derive(Deserialize)]
pub struct RankQuery {
    /// 精简模式：不返回 BestTop3/APTop3（默认 false）
    pub lite: Option<bool>,
    /// 单个排名（1-based）。与 start/end/count 互斥
    pub rank: Option<i64>,
    /// 起始排名（1-based）
    pub start: Option<i64>,
    /// 结束排名（包含，1-based）
    pub end: Option<i64>,
    /// 返回数量（与 start 组合使用）
    pub count: Option<i64>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(example = json!({"ok": true}))]
pub struct OkResponse {
    pub ok: bool,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(example = json!({"ok": true, "alias": "Alice"}))]
pub struct OkAliasResponse {
    pub ok: bool,
    pub alias: String,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AdminUsersQuery {
    /// 页码（从 1 开始）
    pub page: Option<i64>,
    /// 每页条数（1-200）
    pub page_size: Option<i64>,
    /// 可选状态筛选：active|approved|shadow|banned|rejected
    pub status: Option<String>,
    /// 可选别名模糊搜索
    pub alias: Option<String>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AdminLeaderboardUserItem {
    pub user_hash: String,
    pub alias: Option<String>,
    pub score: f64,
    pub suspicion: f64,
    pub is_hidden: bool,
    pub status: String,
    pub updated_at: String,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AdminLeaderboardUsersResponse {
    pub items: Vec<AdminLeaderboardUserItem>,
    pub total: i64,
    pub page: i64,
    pub page_size: i64,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AdminUserStatusQuery {
    pub user_hash: String,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AdminUserStatusResponse {
    pub user_hash: String,
    pub status: String,
    pub reason: Option<String>,
    pub updated_by: Option<String>,
    pub updated_at: Option<String>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AdminSetUserStatusRequest {
    pub user_hash: String,
    pub status: String,
    pub reason: Option<String>,
}

fn normalize_moderation_status(raw: &str) -> Result<(&'static str, i64), AppError> {
    let st = raw.trim().to_lowercase();
    let mapped = match st.as_str() {
        "active" | "approved" => ("active", 0_i64),
        "shadow" => ("shadow", 1_i64),
        "banned" => ("banned", 1_i64),
        "rejected" => ("rejected", 1_i64),
        _ => {
            return Err(AppError::Validation(
                "status 必须为 active|approved|shadow|banned|rejected".into(),
            ));
        }
    };
    Ok(mapped)
}

async fn ensure_not_banned(
    storage: &crate::stats_contract::StatsStorage,
    user_hash: &str,
) -> Result<(), AppError> {
    storage.ensure_user_not_banned(user_hash).await
}

async fn apply_user_status(
    storage: &crate::stats_contract::StatsStorage,
    user_hash: &str,
    status_raw: &str,
    reason: Option<&str>,
    admin: &str,
    now: &str,
) -> Result<String, AppError> {
    let (status, hide) = normalize_moderation_status(status_raw)?;
    storage.set_leaderboard_hidden(user_hash, hide != 0).await?;
    storage
        .set_user_moderation_status(user_hash, status, reason, admin, now)
        .await?;
    Ok(status.to_string())
}

fn mask_user_prefix(hash: &str) -> String {
    let prefix_end = hash
        .char_indices()
        .nth(4)
        .map(|(i, _)| i)
        .unwrap_or(hash.len());

    let mut out = String::with_capacity(prefix_end + 4);
    out.push_str(&hash[..prefix_end]);
    out.push_str("****");
    out
}

/// 批量查询 BestTop3/APTop3 文本详情，避免 N+1 往返。
///
/// 行为保持：详情查询失败时，仍然返回排行榜主数据（详情字段为 None）。
async fn fetch_top3_details_map(
    storage: &crate::stats_contract::StatsStorage,
    user_hashes: &[String],
) -> HashMap<String, (Option<String>, Option<String>)> {
    match storage.fetch_top3_details_for_users(user_hashes).await {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(target: "phi_backend::leaderboard", "batch query leaderboard_details failed (ignored): {e}");
            HashMap::new()
        }
    }
}

/// 检查字符是否为中日韩（CJK）字符
/// 包括：CJK统一汉字、扩展区A/B、兼容汉字、日文平假名/片假名、韩文音节
fn is_cjk_char(c: char) -> bool {
    matches!(c,
        '\u{4E00}'..='\u{9FFF}'   // CJK 统一汉字
        | '\u{3400}'..='\u{4DBF}' // CJK 扩展 A
        | '\u{20000}'..='\u{2A6DF}' // CJK 扩展 B
        | '\u{F900}'..='\u{FAFF}' // CJK 兼容汉字
        | '\u{3040}'..='\u{309F}' // 日文平假名
        | '\u{30A0}'..='\u{30FF}' // 日文片假名
        | '\u{AC00}'..='\u{D7AF}' // 韩文音节
    )
}

#[utoipa::path(
    get,
    path = "/leaderboard/rks/top",
    summary = "排行榜TOP（按RKS）",
    description = "返回公开玩家的RKS排行榜。若玩家开启展示，将在条目中附带BestTop3/APTop3文字数据。",
    params(
        ("limit" = Option<i64>, Query, description = "每页数量，默认50；普通模式最大200，lite=true时最大1000"),
        ("offset" = Option<i64>, Query, description = "偏移量"),
        ("lite" = Option<bool>, Query, description = "精简模式：不返回 bestTop3/apTop3（默认 false）")
    ),
    responses(
        (status = 200, description = "排行榜 TOP", body = LeaderboardTopResponse),
        (
            status = 500,
            description = "统计存储未初始化/查询失败",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        )
    ),
    tag = "Leaderboard"
)]
pub async fn get_top(
    State(state): State<AppState>,
    Query(q): Query<TopQuery>,
) -> Result<Json<LeaderboardTopResponse>, AppError> {
    let t_total = Instant::now();
    let storage = state
        .stats_storage
        .as_ref()
        .ok_or_else(|| AppError::Internal("统计存储未初始化".into()))?;
    let lite = q.lite.unwrap_or(false);
    let max_limit = if lite { 1000 } else { 200 };
    let limit = q.limit.unwrap_or(50).clamp(1, max_limit);
    let offset = q.offset.unwrap_or(0).max(0);

    let total = storage.count_public_leaderboard_total().await?;

    let rows = if let (Some(sc), Some(upd), Some(usr)) =
        (q.after_score, q.after_updated.clone(), q.after_user.clone())
    {
        storage
            .query_leaderboard_top_seek(sc, &upd, &usr, limit)
            .await?
    } else {
        storage.query_leaderboard_top_offset(limit, offset).await?
    };

    let mut items: Vec<LeaderboardTopItem> = Vec::with_capacity(rows.len());
    // capture last row tokens if has more
    let has_more = if q.after_score.is_some() && q.after_updated.is_some() && q.after_user.is_some()
    {
        (rows.len() as i64) == limit
    } else {
        (offset + rows.len() as i64) < total
    };
    let (mut last_score, mut last_updated, mut last_user_hash) =
        (None::<f64>, None::<String>, None::<String>);
    if has_more && let Some(r) = rows.last() {
        last_score = r.try_get::<f64, _>("total_rks").ok();
        last_updated = r.try_get::<String, _>("updated_at").ok();
        last_user_hash = r.try_get::<String, _>("user_hash").ok();
    }

    let details_map = if !lite {
        let mut detail_users: Vec<String> = Vec::new();
        for r in rows.iter() {
            let sbt: i64 = r.try_get("sbt").unwrap_or(0);
            let sat: i64 = r.try_get("sat").unwrap_or(0);
            if (sbt != 0 || sat != 0)
                && let Ok(uh) = r.try_get::<String, _>("user_hash")
            {
                detail_users.push(uh);
            }
        }
        fetch_top3_details_map(storage, &detail_users).await
    } else {
        HashMap::new()
    };

    for (idx, r) in rows.into_iter().enumerate() {
        let user_hash: String = r.try_get("user_hash").unwrap_or_default();
        let alias: Option<String> = r.try_get("alias").ok();
        let score: f64 = r.try_get("total_rks").unwrap_or(0.0);
        let updated_at: String = r.try_get("updated_at").unwrap_or_default();
        let sbt: i64 = r.try_get("sbt").unwrap_or(0);
        let sat: i64 = r.try_get("sat").unwrap_or(0);

        let mut best_top3: Option<Vec<ChartTextItem>> = None;
        let mut ap_top3: Option<Vec<ChartTextItem>> = None;
        if !lite
            && (sbt != 0 || sat != 0)
            && let Some((best_json, ap_json)) = details_map.get(&user_hash)
        {
            if sbt != 0
                && let Some(j) = best_json.as_deref()
            {
                best_top3 = serde_json::from_str::<Vec<ChartTextItem>>(j).ok();
            }
            if sat != 0
                && let Some(j) = ap_json.as_deref()
            {
                ap_top3 = serde_json::from_str::<Vec<ChartTextItem>>(j).ok();
            }
        }

        items.push(LeaderboardTopItem {
            rank: (offset + idx as i64 + 1),
            alias,
            user: mask_user_prefix(&user_hash),
            score,
            updated_at,
            best_top3,
            ap_top3,
        });
    }

    tracing::info!(
        target: "phi_backend::leaderboard::performance",
        route = "/leaderboard/rks/top",
        phase = "total",
        status = "ok",
        lite,
        items = items.len(),
        total,
        total_dur_ms = t_total.elapsed().as_millis(),
        "leaderboard performance"
    );
    Ok(Json(LeaderboardTopResponse {
        items,
        total,
        next_after_score: if has_more { last_score } else { None },
        next_after_updated: if has_more { last_updated } else { None },
        next_after_user: if has_more {
            last_user_hash.as_deref().map(mask_user_prefix)
        } else {
            None
        },
    }))
}

#[utoipa::path(
    get,
    path = "/leaderboard/rks/by-rank",
    summary = "按排名区间获取玩家（按RKS）",
    description = "可传入单个 rank，或 [start,end] / [start,count] 区间获取玩家信息。采用与 TOP 相同的稳定排序与公开过滤。",
    params(
        ("rank" = Option<i64>, Query, description = "单个排名（1-based）"),
        ("start" = Option<i64>, Query, description = "起始排名（1-based）"),
        ("end" = Option<i64>, Query, description = "结束排名（包含）"),
        ("count" = Option<i64>, Query, description = "返回数量（与 start 组合使用）"),
        ("lite" = Option<bool>, Query, description = "精简模式：不返回 bestTop3/apTop3（默认 false）")
    ),
    responses(
        (status = 200, description = "区间结果", body = LeaderboardTopResponse),
        (
            status = 422,
            description = "参数校验失败（缺少 rank/start 等）",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        ),
        (
            status = 500,
            description = "统计存储未初始化/查询失败",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        )
    ),
    tag = "Leaderboard"
)]
pub async fn get_by_rank(
    State(state): State<AppState>,
    Query(q): Query<RankQuery>,
) -> Result<Json<LeaderboardTopResponse>, AppError> {
    let t_total = Instant::now();
    let storage = state
        .stats_storage
        .as_ref()
        .ok_or_else(|| AppError::Internal("统计存储未初始化".into()))?;

    // 解析区间
    let (start_rank, count) = if let Some(r) = q.rank {
        (r.max(1), 1_i64)
    } else if let (Some(s), Some(e)) = (q.start, q.end) {
        let s = s.max(1);
        let e = e.max(s);
        (s, (e - s + 1).min(200))
    } else if let (Some(s), Some(c)) = (q.start, q.count) {
        let s = s.max(1);
        let c = c.clamp(1, 200);
        (s, c)
    } else {
        return Err(AppError::Validation(
            "必须提供 rank 或 (start,end)/(start,count)".into(),
        ));
    };

    let offset = start_rank - 1;
    let limit = count;
    let lite = q.lite.unwrap_or(false);

    let total = storage.count_public_leaderboard_total().await?;
    let rows = storage.query_leaderboard_by_rank(limit, offset).await?;

    let mut items: Vec<LeaderboardTopItem> = Vec::with_capacity(rows.len());
    let has_more = ((start_rank - 1) + rows.len() as i64) < total;
    let (mut last_score, mut last_updated, mut last_user_hash) =
        (None::<f64>, None::<String>, None::<String>);
    if has_more && let Some(r) = rows.last() {
        last_score = r.try_get::<f64, _>("total_rks").ok();
        last_updated = r.try_get::<String, _>("updated_at").ok();
        last_user_hash = r.try_get::<String, _>("user_hash").ok();
    }

    let details_map = if !lite {
        let mut detail_users: Vec<String> = Vec::new();
        for r in rows.iter() {
            let sbt: i64 = r.try_get("sbt").unwrap_or(0);
            let sat: i64 = r.try_get("sat").unwrap_or(0);
            if (sbt != 0 || sat != 0)
                && let Ok(uh) = r.try_get::<String, _>("user_hash")
            {
                detail_users.push(uh);
            }
        }
        fetch_top3_details_map(storage, &detail_users).await
    } else {
        HashMap::new()
    };

    for (i, r) in rows.into_iter().enumerate() {
        let user_hash: String = r.try_get("user_hash").unwrap_or_default();
        let alias: Option<String> = r.try_get("alias").ok();
        let score: f64 = r.try_get("total_rks").unwrap_or(0.0);
        let updated_at: String = r.try_get("updated_at").unwrap_or_default();
        let sbt: i64 = r.try_get("sbt").unwrap_or(0);
        let sat: i64 = r.try_get("sat").unwrap_or(0);

        let mut best_top3: Option<Vec<ChartTextItem>> = None;
        let mut ap_top3: Option<Vec<ChartTextItem>> = None;
        if !lite
            && (sbt != 0 || sat != 0)
            && let Some((best_json, ap_json)) = details_map.get(&user_hash)
        {
            if sbt != 0
                && let Some(j) = best_json.as_deref()
            {
                best_top3 = serde_json::from_str::<Vec<ChartTextItem>>(j).ok();
            }
            if sat != 0
                && let Some(j) = ap_json.as_deref()
            {
                ap_top3 = serde_json::from_str::<Vec<ChartTextItem>>(j).ok();
            }
        }

        items.push(LeaderboardTopItem {
            rank: start_rank + i as i64,
            alias,
            user: mask_user_prefix(&user_hash),
            score,
            updated_at,
            best_top3,
            ap_top3,
        });
    }

    tracing::info!(
        target: "phi_backend::leaderboard::performance",
        route = "/leaderboard/rks/by-rank",
        phase = "total",
        status = "ok",
        lite,
        items = items.len(),
        total,
        total_dur_ms = t_total.elapsed().as_millis(),
        "leaderboard performance"
    );
    Ok(Json(LeaderboardTopResponse {
        items,
        total,
        next_after_score: if has_more { last_score } else { None },
        next_after_updated: if has_more { last_updated } else { None },
        next_after_user: if has_more {
            last_user_hash.as_deref().map(mask_user_prefix)
        } else {
            None
        },
    }))
}

#[utoipa::path(
    post,
    path = "/leaderboard/rks/me",
    summary = "我的名次（按RKS）",
    description = "通过认证信息推导用户身份，返回名次、分数、总量与百分位（竞争排名）",
    request_body = crate::auth_contract::UnifiedSaveRequest,
    responses(
        (status = 200, description = "查询成功", body = MeResponse),
        (
            status = 500,
            description = "统计存储未初始化/查询失败/无法识别用户",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        )
    ),
    tag = "Leaderboard"
)]
pub async fn post_me(
    State(state): State<AppState>,
    request: axum::extract::Request,
) -> Result<Json<MeResponse>, AppError> {
    let (mut auth, bearer_state) = crate::session_auth::parse_json_with_bearer_state::<
        crate::auth_contract::UnifiedSaveRequest,
    >(request)
    .await?;
    crate::session_auth::merge_auth_from_bearer_if_missing(
        state.stats_storage.as_ref(),
        &bearer_state,
        &mut auth,
    )
    .await?;

    let storage = state
        .stats_storage
        .as_ref()
        .ok_or_else(|| AppError::Internal("统计存储未初始化".into()))?;
    let salt = crate::config::AppConfig::global()
        .stats
        .user_hash_salt
        .as_deref();
    let (user_hash_opt, _kind) =
        crate::session_auth::derive_user_identity_with_bearer(salt, &auth, &bearer_state)?;
    let user_hash =
        user_hash_opt.ok_or_else(|| AppError::Internal("无法识别用户（缺少可用凭证）".into()))?;
    ensure_not_banned(storage, &user_hash).await?;

    let (my_score, my_updated) =
        if let Some((score, updated)) = storage.get_prev_rks(&user_hash).await? {
            (score, updated)
        } else {
            (0.0, String::from(""))
        };

    let total = storage.count_public_leaderboard_total().await?;

    if total == 0 || my_score <= 0.0 {
        return Ok(Json(MeResponse {
            rank: 0,
            score: 0.0,
            total,
            percentile: 0.0,
        }));
    }

    let higher = storage
        .count_public_leaderboard_higher(my_score, &my_updated, &user_hash)
        .await?;
    let rank = higher + 1;
    let percentile = 100.0 * (1.0 - ((rank - 1) as f64 / total as f64));
    Ok(Json(MeResponse {
        rank,
        score: my_score,
        total,
        percentile,
    }))
}

#[utoipa::path(
    put,
    path = "/leaderboard/alias",
    summary = "设置/更新公开别名（幂等）",
    request_body = AliasRequest,
    responses(
        (status = 200, description = "设置成功", body = OkAliasResponse),
        (
            status = 409,
            description = "别名被占用",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        ),
        (
            status = 422,
            description = "别名非法",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        ),
        (
            status = 500,
            description = "统计存储未初始化/写入失败/无法识别用户",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        )
    ),
    tag = "Leaderboard"
)]
pub async fn put_alias(
    State(state): State<AppState>,
    request: axum::extract::Request,
) -> Result<Json<OkAliasResponse>, AppError> {
    let (mut req, bearer_state) =
        crate::session_auth::parse_json_with_bearer_state::<AliasRequest>(request).await?;
    crate::session_auth::merge_auth_from_bearer_if_missing(
        state.stats_storage.as_ref(),
        &bearer_state,
        &mut req.auth,
    )
    .await?;

    let storage = state
        .stats_storage
        .as_ref()
        .ok_or_else(|| AppError::Internal("统计存储未初始化".into()))?;
    let salt = crate::config::AppConfig::global()
        .stats
        .user_hash_salt
        .as_deref();
    let (user_hash_opt, _kind) =
        crate::session_auth::derive_user_identity_with_bearer(salt, &req.auth, &bearer_state)?;
    let user_hash =
        user_hash_opt.ok_or_else(|| AppError::Internal("无法识别用户（缺少可用凭证）".into()))?;
    ensure_not_banned(storage, &user_hash).await?;

    // 校验别名
    let alias = req.alias.trim();
    let char_count = alias.chars().count();
    if !(2..=20).contains(&char_count) {
        return Err(AppError::Validation("别名长度需在 2~20 字符之间".into()));
    }
    if !alias
        .chars()
        .all(|c| c.is_alphanumeric() || c == '.' || c == '_' || c == '-' || is_cjk_char(c))
    {
        return Err(AppError::Validation(
            "别名仅允许字母、数字、中日韩文字和 . _ -".into(),
        ));
    }
    let reserved = ["admin", "system", "null", "undefined", "root"];
    if reserved.iter().any(|&w| w.eq_ignore_ascii_case(alias)) {
        return Err(AppError::Validation("别名为保留字".into()));
    }

    let now = chrono::Utc::now().to_rfc3339();
    let cfg = crate::config::AppConfig::global();
    storage
        .upsert_user_alias_with_defaults(
            &user_hash,
            alias,
            crate::stats_contract::UserAliasDefaults {
                is_public: false,
                show_rks_composition: cfg.leaderboard.default_show_rks_composition,
                show_best_top3: cfg.leaderboard.default_show_best_top3,
                show_ap_top3: cfg.leaderboard.default_show_ap_top3,
                now_rfc3339: &now,
            },
        )
        .await?;
    Ok(Json(OkAliasResponse {
        ok: true,
        alias: alias.to_string(),
    }))
}

#[utoipa::path(
    put,
    path = "/leaderboard/profile",
    summary = "更新公开资料开关（文字展示）",
    request_body = ProfileUpdateRequest,
    responses(
        (status = 200, description = "更新成功", body = OkResponse),
        (
            status = 422,
            description = "参数校验失败（例如配置禁止公开）",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        ),
        (
            status = 500,
            description = "统计存储未初始化/更新失败/无法识别用户",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        )
    ),
    tag = "Leaderboard"
)]
pub async fn put_profile(
    State(state): State<AppState>,
    request: axum::extract::Request,
) -> Result<Json<OkResponse>, AppError> {
    let (mut req, bearer_state) =
        crate::session_auth::parse_json_with_bearer_state::<ProfileUpdateRequest>(request).await?;
    crate::session_auth::merge_auth_from_bearer_if_missing(
        state.stats_storage.as_ref(),
        &bearer_state,
        &mut req.auth,
    )
    .await?;

    let storage = state
        .stats_storage
        .as_ref()
        .ok_or_else(|| AppError::Internal("统计存储未初始化".into()))?;
    let salt = crate::config::AppConfig::global()
        .stats
        .user_hash_salt
        .as_deref();
    let (user_hash_opt, _kind) =
        crate::session_auth::derive_user_identity_with_bearer(salt, &req.auth, &bearer_state)?;
    let user_hash =
        user_hash_opt.ok_or_else(|| AppError::Internal("无法识别用户（缺少可用凭证）".into()))?;
    ensure_not_banned(storage, &user_hash).await?;
    let now = chrono::Utc::now().to_rfc3339();

    let mut is_public = None::<i64>;
    let mut show_rc = None::<i64>;
    let mut show_b3 = None::<i64>;
    let mut show_ap3 = None::<i64>;
    if let Some(v) = req.is_public {
        if v && !crate::config::AppConfig::global().leaderboard.allow_public {
            return Err(AppError::Validation("当前配置禁止公开资料".into()));
        }
        is_public = Some(if v { 1 } else { 0 });
    }
    if let Some(v) = req.show_rks_composition {
        show_rc = Some(if v { 1 } else { 0 });
    }
    if let Some(v) = req.show_best_top3 {
        show_b3 = Some(if v { 1 } else { 0 });
    }
    if let Some(v) = req.show_ap_top3 {
        show_ap3 = Some(if v { 1 } else { 0 });
    }

    // Ensure row exists
    if let Err(e) = storage.ensure_user_profile_exists(&user_hash, &now).await {
        tracing::warn!(
            target: "phi_backend::leaderboard",
            user_hash = %user_hash,
            "ensure_user_profile_exists failed (ignored): {e}"
        );
    }

    storage
        .update_user_profile_visibility(&user_hash, &now, is_public, show_rc, show_b3, show_ap3)
        .await
        .map_err(|e| AppError::Internal(format!("更新资料失败: {e}")))?;
    Ok(Json(OkResponse { ok: true }))
}

#[utoipa::path(
    get,
    path = "/public/profile/{alias}",
    summary = "公开玩家资料（纯文字）",
    params(("alias" = String, Path, description = "公开别名")),
    responses(
        (status = 200, description = "公开资料", body = PublicProfileResponse),
        (
            status = 404,
            description = "未找到（别名不存在或未公开）",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        ),
        (
            status = 500,
            description = "统计存储未初始化/查询失败",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        )
    ),
    tag = "Leaderboard"
)]
pub async fn get_public_profile(
    State(state): State<AppState>,
    Path(alias): Path<String>,
) -> Result<Json<PublicProfileResponse>, AppError> {
    let storage = state
        .stats_storage
        .as_ref()
        .ok_or_else(|| AppError::Internal("统计存储未初始化".into()))?;
    let row = storage.query_public_profile_by_alias(&alias).await?;
    let Some(r) = row else {
        return Err(AppError::Search(crate::error::SearchError::NotFound));
    };
    let is_public: i64 = r.try_get("is_public").unwrap_or(0);
    if is_public == 0 {
        return Err(AppError::Search(crate::error::SearchError::NotFound));
    }
    let user_hash: String = r.try_get("user_hash").unwrap_or_default();
    let score: f64 = r.try_get("total_rks").unwrap_or(0.0);
    let updated_at: String = r.try_get("updated_at").unwrap_or_default();
    let show_rc: i64 = r.try_get("show_rks_composition").unwrap_or(0);
    let show_b3: i64 = r.try_get("show_best_top3").unwrap_or(0);
    let show_ap3: i64 = r.try_get("show_ap_top3").unwrap_or(0);

    let mut resp = PublicProfileResponse {
        alias: alias.clone(),
        score,
        updated_at,
        rks_composition: None,
        best_top3: None,
        ap_top3: None,
    };
    if (show_rc != 0 || show_b3 != 0 || show_ap3 != 0)
        && let Some(d) = storage.query_leaderboard_details_row(&user_hash).await?
    {
        if show_rc != 0
            && let Ok(Some(j)) = d.try_get::<String, _>("rks_composition_json").map(Some)
        {
            resp.rks_composition = serde_json::from_str::<RksCompositionText>(&j).ok();
        }
        if show_b3 != 0
            && let Ok(Some(j)) = d.try_get::<String, _>("best_top3_json").map(Some)
        {
            resp.best_top3 = serde_json::from_str::<Vec<ChartTextItem>>(&j).ok();
        }
        if show_ap3 != 0
            && let Ok(Some(j)) = d.try_get::<String, _>("ap_top3_json").map(Some)
        {
            resp.ap_top3 = serde_json::from_str::<Vec<ChartTextItem>>(&j).ok();
        }
    }
    Ok(Json(resp))
}

// ============ Admin endpoints (X-Admin-Token) ============

pub(crate) fn require_admin_with_cfg(
    cfg: &crate::config::AppConfig,
    headers: &HeaderMap,
) -> Result<String, AppError> {
    let provided = headers
        .get("x-admin-token")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .trim();
    if provided.is_empty() {
        return Err(AppError::Auth("缺少管理员令牌".into()));
    }
    let ok = cfg
        .leaderboard
        .admin_tokens
        .iter()
        .any(|t| t.trim() == provided);
    if !ok {
        return Err(AppError::Auth("管理员令牌无效".into()));
    }
    Ok(provided.to_string())
}

pub(crate) fn require_admin(headers: &HeaderMap) -> Result<String, AppError> {
    require_admin_with_cfg(crate::config::AppConfig::global(), headers)
}

#[derive(serde::Serialize, utoipa::ToSchema)]
#[schema(example = json!({
  "user": "ab12****",
  "alias": "Alice",
  "score": 14.73,
  "suspicion": 1.10,
  "updatedAt": "2025-09-20T04:10:44Z"
}))]
#[serde(rename_all = "camelCase")]
pub struct SuspiciousItem {
    user: String,
    alias: Option<String>,
    score: f64,
    suspicion: f64,
    updated_at: String,
}

#[utoipa::path(
    get,
    path = "/admin/leaderboard/suspicious",
    summary = "可疑用户列表",
    description = "需要在 Header 中提供 X-Admin-Token，令牌来源于 config.leaderboard.admin_tokens。",
    params(
        ("X-Admin-Token" = String, Header, description = "管理员令牌（config.leaderboard.admin_tokens）"),
        ("min_score"= Option<f64>, Query, description="最小可疑分，默认0.6"),
        ("limit"=Option<i64>, Query, description="返回数量，默认 100")
    ),
    security(("AdminToken" = [])),
    responses(
        (status = 200, description = "可疑列表", body = [SuspiciousItem]),
        (
            status = 401,
            description = "管理员令牌缺失/无效",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        ),
        (
            status = 500,
            description = "统计存储未初始化/查询失败",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        )
    ),
    tag = "Leaderboard"
)]
pub async fn get_suspicious(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(p): Query<std::collections::HashMap<String, String>>,
) -> Result<Json<Vec<SuspiciousItem>>, AppError> {
    require_admin(&headers)?;
    let storage = state
        .stats_storage
        .as_ref()
        .ok_or_else(|| AppError::Internal("统计存储未初始化".into()))?;
    let min_score = p
        .get("min_score")
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(0.6);
    let limit = p
        .get("limit")
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(100)
        .clamp(1, 500);
    let rows = storage.query_suspicious_rows(min_score, limit).await?;
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        out.push(SuspiciousItem {
            user: mask_user_prefix(&r.get::<String, _>("user_hash")),
            alias: r.try_get("alias").ok(),
            score: r.try_get("total_rks").unwrap_or(0.0),
            suspicion: r.try_get("suspicion_score").unwrap_or(0.0),
            updated_at: r.try_get("updated_at").unwrap_or_default(),
        });
    }
    Ok(Json(out))
}

#[utoipa::path(
    get,
    path = "/admin/leaderboard/users",
    summary = "分页查询排行榜用户（含完整 user_hash）",
    description = "需要在 Header 中提供 X-Admin-Token，返回排行榜用户完整 user_hash，支持按状态与别名筛选。",
    params(
        ("X-Admin-Token" = String, Header, description = "管理员令牌（config.leaderboard.admin_tokens）"),
        ("page" = Option<i64>, Query, description = "页码（从 1 开始，默认 1）"),
        ("pageSize" = Option<i64>, Query, description = "每页条数（1-200，默认 50）"),
        ("status" = Option<String>, Query, description = "状态筛选：active|approved|shadow|banned|rejected"),
        ("alias" = Option<String>, Query, description = "别名模糊筛选")
    ),
    security(("AdminToken" = [])),
    responses(
        (status = 200, description = "分页结果", body = AdminLeaderboardUsersResponse),
        (
            status = 401,
            description = "管理员令牌缺失或无效",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        ),
        (
            status = 422,
            description = "参数校验失败（status 非法等）",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        ),
        (
            status = 500,
            description = "统计存储未初始化/查询失败",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        )
    ),
    tag = "Leaderboard"
)]
pub async fn get_admin_leaderboard_users(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<AdminUsersQuery>,
) -> Result<Json<AdminLeaderboardUsersResponse>, AppError> {
    require_admin(&headers)?;
    let storage = state
        .stats_storage
        .as_ref()
        .ok_or_else(|| AppError::Internal("统计存储未初始化".into()))?;

    let page = q.page.unwrap_or(1).max(1);
    let page_size = q.page_size.unwrap_or(50).clamp(1, 200);
    let offset = (page - 1) * page_size;
    let status_filter = q
        .status
        .as_deref()
        .map(|s| normalize_moderation_status(s).map(|(st, _)| st.to_string()))
        .transpose()?;
    let alias_like = q
        .alias
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(|v| format!("%{v}%"));

    let total = storage
        .query_admin_leaderboard_users_count(status_filter.as_deref(), alias_like.as_deref())
        .await?;
    let rows = storage
        .query_admin_leaderboard_users_rows(
            status_filter.as_deref(),
            alias_like.as_deref(),
            page_size,
            offset,
        )
        .await?;

    let mut items = Vec::with_capacity(rows.len());
    for r in rows {
        let is_hidden_i: i64 = r.try_get("is_hidden").unwrap_or(0);
        items.push(AdminLeaderboardUserItem {
            user_hash: r.try_get("user_hash").unwrap_or_default(),
            alias: r.try_get("alias").ok(),
            score: r.try_get("total_rks").unwrap_or(0.0),
            suspicion: r.try_get("suspicion_score").unwrap_or(0.0),
            is_hidden: is_hidden_i != 0,
            status: r
                .try_get::<String, _>("status")
                .unwrap_or_else(|_| "active".to_string()),
            updated_at: r.try_get("updated_at").unwrap_or_default(),
        });
    }

    Ok(Json(AdminLeaderboardUsersResponse {
        items,
        total,
        page,
        page_size,
    }))
}

#[utoipa::path(
    get,
    path = "/admin/users/status",
    summary = "查询用户全局状态",
    description = "需要在 Header 中提供 X-Admin-Token。",
    params(
        ("X-Admin-Token" = String, Header, description = "管理员令牌（config.leaderboard.admin_tokens）"),
        ("userHash" = String, Query, description = "完整 user_hash")
    ),
    security(("AdminToken" = [])),
    responses(
        (status = 200, description = "查询成功", body = AdminUserStatusResponse),
        (
            status = 401,
            description = "管理员令牌缺失或无效",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        ),
        (
            status = 500,
            description = "统计存储未初始化/查询失败",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        )
    ),
    tag = "Leaderboard"
)]
pub async fn get_admin_user_status(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<AdminUserStatusQuery>,
) -> Result<Json<AdminUserStatusResponse>, AppError> {
    require_admin(&headers)?;
    let storage = state
        .stats_storage
        .as_ref()
        .ok_or_else(|| AppError::Internal("统计存储未初始化".into()))?;
    let row = storage
        .query_user_moderation_state_full_row(&q.user_hash)
        .await?;
    if let Some(r) = row {
        return Ok(Json(AdminUserStatusResponse {
            user_hash: q.user_hash,
            status: r
                .try_get::<String, _>("status")
                .unwrap_or_else(|_| "active".to_string()),
            reason: r.try_get("reason").unwrap_or(None),
            updated_by: r.try_get("updated_by").unwrap_or(None),
            updated_at: r.try_get("updated_at").unwrap_or(None),
        }));
    }
    Ok(Json(AdminUserStatusResponse {
        user_hash: q.user_hash,
        status: "active".to_string(),
        reason: None,
        updated_by: None,
        updated_at: None,
    }))
}

#[utoipa::path(
    post,
    path = "/admin/users/status",
    summary = "设置用户全局状态",
    description = "需要在 Header 中提供 X-Admin-Token。状态支持 active|approved|shadow|banned|rejected。",
    params(("X-Admin-Token" = String, Header, description = "管理员令牌（config.leaderboard.admin_tokens）")),
    security(("AdminToken" = [])),
    request_body = AdminSetUserStatusRequest,
    responses(
        (status = 200, description = "更新成功", body = AdminUserStatusResponse),
        (
            status = 401,
            description = "管理员令牌缺失或无效",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        ),
        (
            status = 422,
            description = "参数校验失败（status 非法等）",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        ),
        (
            status = 500,
            description = "统计存储未初始化/写入失败",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        )
    ),
    tag = "Leaderboard"
)]
pub async fn post_admin_user_status(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<AdminSetUserStatusRequest>,
) -> Result<Json<AdminUserStatusResponse>, AppError> {
    let admin = require_admin(&headers)?;
    let storage = state
        .stats_storage
        .as_ref()
        .ok_or_else(|| AppError::Internal("统计存储未初始化".into()))?;
    let user_hash = req.user_hash.trim();
    if user_hash.is_empty() {
        return Err(AppError::Validation("userHash 不能为空".into()));
    }
    let now = chrono::Utc::now().to_rfc3339();
    let reason_clean = req
        .reason
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty());
    let status =
        apply_user_status(storage, user_hash, &req.status, reason_clean, &admin, &now).await?;
    Ok(Json(AdminUserStatusResponse {
        user_hash: user_hash.to_string(),
        status,
        reason: reason_clean.map(|v| v.to_string()),
        updated_by: Some(admin),
        updated_at: Some(now),
    }))
}

#[derive(serde::Deserialize, utoipa::ToSchema)]
#[schema(example = json!({"userHash":"abcde12345","status":"shadow","reason":"suspicious jump"}))]
#[serde(rename_all = "camelCase")]
pub struct ResolveRequest {
    pub user_hash: String,
    pub status: String,
    pub reason: Option<String>,
}

#[utoipa::path(
    post,
    path = "/admin/leaderboard/resolve",
    summary = "审核可疑用户（approved/shadow/banned/rejected）",
    description = "需要在 Header 中提供 X-Admin-Token，令牌来源于 config.leaderboard.admin_tokens。",
    params(("X-Admin-Token" = String, Header, description = "管理员令牌（config.leaderboard.admin_tokens）")),
    security(("AdminToken" = [])),
    request_body = ResolveRequest,
    responses(
        (status = 200, description = "处理成功", body = OkResponse),
        (
            status = 401,
            description = "管理员令牌缺失/无效",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        ),
        (
            status = 422,
            description = "参数校验失败（status 非法等）",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        ),
        (
            status = 500,
            description = "统计存储未初始化/写入失败",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        )
    ),
    tag = "Leaderboard"
)]
pub async fn post_resolve(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<ResolveRequest>,
) -> Result<Json<OkResponse>, AppError> {
    let admin = require_admin(&headers)?;
    let storage = state
        .stats_storage
        .as_ref()
        .ok_or_else(|| AppError::Internal("统计存储未初始化".into()))?;
    let now = chrono::Utc::now().to_rfc3339();
    let st = req.status.trim().to_lowercase();
    if !matches!(st.as_str(), "approved" | "shadow" | "banned" | "rejected") {
        return Err(AppError::Validation(
            "status 必须为 approved|shadow|banned|rejected".into(),
        ));
    }
    apply_user_status(
        storage,
        &req.user_hash,
        &st,
        req.reason.as_deref(),
        &admin,
        &now,
    )
    .await?;
    Ok(Json(OkResponse { ok: true }))
}

#[derive(serde::Deserialize, utoipa::ToSchema)]
#[schema(example = json!({"userHash":"abcde12345","alias":"Alice"}))]
#[serde(rename_all = "camelCase")]
pub struct ForceAliasRequest {
    pub user_hash: String,
    pub alias: String,
}

#[utoipa::path(
    post,
    path = "/admin/leaderboard/alias/force",
    summary = "管理员强制设置/回收别名（会从原持有人移除）",
    description = "需要在 Header 中提供 X-Admin-Token，令牌来源于 config.leaderboard.admin_tokens。",
    params(("X-Admin-Token" = String, Header, description = "管理员令牌（config.leaderboard.admin_tokens）")),
    security(("AdminToken" = [])),
    request_body = ForceAliasRequest,
    responses(
        (status = 200, description = "设置成功", body = OkAliasResponse),
        (
            status = 401,
            description = "管理员令牌缺失/无效",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        ),
        (
            status = 422,
            description = "参数校验失败（别名非法等）",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        ),
        (
            status = 500,
            description = "统计存储未初始化/写入失败",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        )
    ),
    tag = "Leaderboard"
)]
pub async fn post_alias_force(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<ForceAliasRequest>,
) -> Result<Json<OkAliasResponse>, AppError> {
    require_admin(&headers)?;
    let storage = state
        .stats_storage
        .as_ref()
        .ok_or_else(|| AppError::Internal("统计存储未初始化".into()))?;
    let now = chrono::Utc::now().to_rfc3339();
    let alias = req.alias.trim();
    let char_count = alias.chars().count();
    if !(2..=20).contains(&char_count) {
        return Err(AppError::Validation("别名长度需在 2~20 字符之间".into()));
    }
    if !alias
        .chars()
        .all(|c| c.is_alphanumeric() || c == '.' || c == '_' || c == '-' || is_cjk_char(c))
    {
        return Err(AppError::Validation(
            "别名仅允许字母、数字、中日韩文字和 . _ -".into(),
        ));
    }
    storage
        .force_set_user_alias(&req.user_hash, alias, &now)
        .await?;
    Ok(Json(OkAliasResponse {
        ok: true,
        alias: alias.to_string(),
    }))
}

pub fn create_leaderboard_router() -> Router<AppState> {
    Router::new()
        .route("/leaderboard/rks/top", get(get_top))
        .route("/leaderboard/rks/by-rank", get(get_by_rank))
        .route("/leaderboard/rks/me", post(post_me))
        .route("/leaderboard/alias", put(put_alias))
        .route("/leaderboard/profile", put(put_profile))
        .route("/public/profile/:alias", get(get_public_profile))
        .route("/admin/leaderboard/suspicious", get(get_suspicious))
        .route("/admin/leaderboard/users", get(get_admin_leaderboard_users))
        .route("/admin/leaderboard/resolve", post(post_resolve))
        .route("/admin/users/status", get(get_admin_user_status))
        .route("/admin/users/status", post(post_admin_user_status))
        .route("/admin/leaderboard/alias/force", post(post_alias_force))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mask_user_prefix() {
        assert_eq!(mask_user_prefix("abcd1234"), "abcd****");
        assert_eq!(mask_user_prefix("ab"), "ab****");
        assert_eq!(mask_user_prefix(""), "****");
    }

    #[test]
    fn test_is_cjk_char() {
        // 中文字符
        assert!(is_cjk_char('中'));
        assert!(is_cjk_char('文'));
        assert!(is_cjk_char('测'));
        assert!(is_cjk_char('试'));

        // 日文平假名
        assert!(is_cjk_char('あ'));
        assert!(is_cjk_char('い'));

        // 日文片假名
        assert!(is_cjk_char('ア'));
        assert!(is_cjk_char('イ'));

        // 韩文
        assert!(is_cjk_char('한'));
        assert!(is_cjk_char('글'));

        // 非 CJK 字符
        assert!(!is_cjk_char('a'));
        assert!(!is_cjk_char('Z'));
        assert!(!is_cjk_char('1'));
        assert!(!is_cjk_char('.'));
        assert!(!is_cjk_char('_'));
        assert!(!is_cjk_char('-'));
    }

    #[test]
    fn test_alias_validation_with_chinese() {
        // 测试别名验证逻辑（模拟）
        let valid_aliases = vec![
            "测试用户",
            "Alice测试",
            "用户123",
            "test_用户",
            "玩家.名",
            "日本語テスト",
            "한글테스트",
        ];

        for alias in valid_aliases {
            let char_count = alias.chars().count();
            let is_valid = (2..=20).contains(&char_count)
                && alias.chars().all(|c| {
                    c.is_alphanumeric() || c == '.' || c == '_' || c == '-' || is_cjk_char(c)
                });
            assert!(is_valid, "别名 '{alias}' 应该有效");
        }

        // 无效别名
        let invalid_aliases = vec![
            "a",          // 太短
            "测",         // 太短
            "test@user",  // 包含非法字符 @
            "user#name",  // 包含非法字符 #
            "name space", // 包含空格
        ];

        for alias in invalid_aliases {
            let char_count = alias.chars().count();
            let is_valid = (2..=20).contains(&char_count)
                && alias.chars().all(|c| {
                    c.is_alphanumeric() || c == '.' || c == '_' || c == '-' || is_cjk_char(c)
                });
            assert!(!is_valid, "别名 '{alias}' 应该无效");
        }
    }

    #[test]
    fn test_normalize_moderation_status() {
        assert_eq!(normalize_moderation_status("active").unwrap().0, "active");
        assert_eq!(normalize_moderation_status("approved").unwrap().0, "active");
        assert_eq!(normalize_moderation_status("shadow").unwrap().0, "shadow");
        assert_eq!(normalize_moderation_status("banned").unwrap().0, "banned");
        assert!(normalize_moderation_status("unknown").is_err());
    }

    #[test]
    fn test_require_admin_env() {
        // 避免测试间共享全局配置导致的竞态：直接构造 cfg 注入。
        let mut cfg = crate::config::AppConfig::default();
        cfg.leaderboard.admin_tokens = vec!["t1".into(), "t2".into()];

        let mut headers = HeaderMap::new();
        headers.insert("x-admin-token", axum::http::HeaderValue::from_static("t2"));
        assert!(require_admin_with_cfg(&cfg, &headers).is_ok());
        headers.insert("x-admin-token", axum::http::HeaderValue::from_static("bad"));
        assert!(require_admin_with_cfg(&cfg, &headers).is_err());
    }
}
