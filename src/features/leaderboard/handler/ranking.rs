use axum::{
    extract::{Query, State},
    response::Json,
};
use serde::Deserialize;
use sqlx::{Row, sqlite::SqliteRow};
use std::collections::HashMap;
use std::time::Instant;

use crate::{error::AppError, state::AppState};

use super::super::models::{ChartTextItem, LeaderboardTopItem, LeaderboardTopResponse, MeResponse};
use super::cursor::{
    LeaderboardCursor, normalize_leaderboard_seek, parse_leaderboard_cursor,
    seal_leaderboard_cursor,
};
use super::{ensure_not_banned, mask_user_prefix};

#[derive(Deserialize)]
pub struct TopQuery {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
    pub after_score: Option<f64>,
    pub after_updated: Option<String>,
    pub after_user: Option<String>,
    /// 加密游标。存在时优先使用 cursor，并忽略 offset 与 after_*。
    pub cursor: Option<String>,
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

fn usize_to_i64_saturating(value: usize) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

fn i64_to_usize_saturating(value: i64) -> usize {
    usize::try_from(value).unwrap_or(usize::MAX)
}

fn i64_to_f64_lossy(value: i64) -> f64 {
    value.to_string().parse::<f64>().unwrap_or_else(|_| {
        if value.is_negative() {
            f64::MIN
        } else {
            f64::MAX
        }
    })
}

fn truncate_overfetched_rows<T>(rows: &mut Vec<T>, limit: i64) -> bool {
    let limit = i64_to_usize_saturating(limit.max(0));
    let has_more = rows.len() > limit;
    if has_more {
        rows.truncate(limit);
    }
    has_more
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

async fn build_leaderboard_items(
    storage: &crate::stats_contract::StatsStorage,
    rows: Vec<SqliteRow>,
    rank_base: i64,
    lite: bool,
) -> Vec<LeaderboardTopItem> {
    let details_map = if lite {
        HashMap::new()
    } else {
        let mut detail_users: Vec<String> = Vec::new();
        for r in &rows {
            let sbt: i64 = r.try_get("sbt").unwrap_or(0);
            let sat: i64 = r.try_get("sat").unwrap_or(0);
            if (sbt != 0 || sat != 0)
                && let Ok(uh) = r.try_get::<String, _>("user_hash")
            {
                detail_users.push(uh);
            }
        }
        fetch_top3_details_map(storage, &detail_users).await
    };

    let mut items = Vec::with_capacity(rows.len());
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
            rank: rank_base + usize_to_i64_saturating(idx),
            alias,
            user: mask_user_prefix(&user_hash),
            score,
            updated_at,
            best_top3,
            ap_top3,
        });
    }
    items
}

#[utoipa::path(
    get,
    path = "/leaderboard/rks/top",
    summary = "排行榜TOP（按RKS）",
    description = "返回公开玩家的RKS排行榜。若玩家开启展示，将在条目中附带BestTop3/APTop3文字数据。",
    params(
        ("limit" = Option<i64>, Query, description = "每页数量，默认50；普通模式最大200，lite=true时最大1000"),
        ("offset" = Option<i64>, Query, description = "偏移量"),
        ("cursor" = Option<String>, Query, description = "加密游标；存在时优先使用 cursor，并忽略 offset 与 after_*"),
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
    let cursor = parse_leaderboard_cursor(q.cursor.as_deref())?;
    let seek = normalize_leaderboard_seek(
        cursor,
        q.after_score,
        q.after_updated.clone(),
        q.after_user.clone(),
    )?;
    let page_rank_base = seek
        .as_ref()
        .and_then(|cursor| cursor.rank_base)
        .unwrap_or(offset + 1);
    let fetch_limit = limit.saturating_add(1);

    let total_fut = storage.count_public_leaderboard_total();
    let rows_fut = async {
        if let Some(cursor) = seek.as_ref() {
            storage
                .query_leaderboard_top_seek(
                    cursor.score,
                    &cursor.updated_at,
                    &cursor.user_hash,
                    fetch_limit,
                )
                .await
        } else {
            storage
                .query_leaderboard_top_offset(fetch_limit, offset)
                .await
        }
    };
    let (total, mut rows) = tokio::try_join!(total_fut, rows_fut)?;

    let has_more = truncate_overfetched_rows(&mut rows, limit);
    let (mut last_score, mut last_updated, mut last_user_hash) =
        (None::<f64>, None::<String>, None::<String>);
    if has_more && let Some(r) = rows.last() {
        last_score = r.try_get::<f64, _>("total_rks").ok();
        last_updated = r.try_get::<String, _>("updated_at").ok();
        last_user_hash = r.try_get::<String, _>("user_hash").ok();
    }

    let items = build_leaderboard_items(storage, rows, page_rank_base, lite).await;

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
    let next_cursor = if has_more {
        match (
            last_score,
            last_updated.as_deref(),
            last_user_hash.as_deref(),
        ) {
            (Some(score), Some(updated_at), Some(user_hash)) => {
                seal_leaderboard_cursor(&LeaderboardCursor {
                    score,
                    updated_at: updated_at.to_string(),
                    user_hash: user_hash.to_string(),
                    rank_base: Some(page_rank_base + usize_to_i64_saturating(items.len())),
                })
            }
            _ => None,
        }
    } else {
        None
    };
    Ok(Json(LeaderboardTopResponse {
        items,
        total,
        next_after_score: if has_more { last_score } else { None },
        next_after_updated: if has_more { last_updated } else { None },
        next_cursor,
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

    let has_more = ((start_rank - 1) + usize_to_i64_saturating(rows.len())) < total;
    let (mut last_score, mut last_updated, mut last_user_hash) =
        (None::<f64>, None::<String>, None::<String>);
    if has_more && let Some(r) = rows.last() {
        last_score = r.try_get::<f64, _>("total_rks").ok();
        last_updated = r.try_get::<String, _>("updated_at").ok();
        last_user_hash = r.try_get::<String, _>("user_hash").ok();
    }

    let items = build_leaderboard_items(storage, rows, start_rank, lite).await;

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
        next_cursor: None,
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
            (0.0, String::new())
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
    let percentile = 100.0 * (1.0 - (i64_to_f64_lossy(rank - 1) / i64_to_f64_lossy(total)));
    Ok(Json(MeResponse {
        rank,
        score: my_score,
        total,
        percentile,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_overfetched_rows_detects_has_more() {
        let mut rows = vec![1, 2, 3];
        assert!(truncate_overfetched_rows(&mut rows, 2));
        assert_eq!(rows, vec![1, 2]);

        let mut rows = vec![1, 2];
        assert!(!truncate_overfetched_rows(&mut rows, 2));
        assert_eq!(rows, vec![1, 2]);
    }
}
