use axum::http::HeaderMap;
use axum::{
    Router,
    extract::{Path, Query, State},
    response::Json,
    routing::{get, post, put},
};
use serde::{Deserialize, Serialize};
use sqlx::{QueryBuilder, Row, Sqlite};
use std::collections::HashMap;

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
}

#[derive(Deserialize)]
pub struct RankQuery {
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
#[schema(example = json!({"ok": true}))]
pub struct OkResponse {
    pub ok: bool,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[schema(example = json!({"ok": true, "alias": "Alice"}))]
pub struct OkAliasResponse {
    pub ok: bool,
    pub alias: String,
}

fn mask_user_prefix(hash: &str) -> String {
    let p = hash.chars().take(4).collect::<String>();
    format!("{p}****")
}

/// 批量查询 BestTop3/APTop3 文本详情，避免 N+1 往返。
///
/// 行为保持：详情查询失败时，仍然返回排行榜主数据（详情字段为 None）。
async fn fetch_top3_details_map(
    pool: &sqlx::SqlitePool,
    user_hashes: &[String],
) -> HashMap<String, (Option<String>, Option<String>)> {
    if user_hashes.is_empty() {
        return HashMap::new();
    }

    let mut qb = QueryBuilder::<Sqlite>::new(
        "SELECT user_hash, best_top3_json, ap_top3_json FROM leaderboard_details WHERE user_hash IN (",
    );
    let mut separated = qb.separated(", ");
    for uh in user_hashes {
        separated.push_bind(uh);
    }
    separated.push_unseparated(")");

    let rows = match qb.build().fetch_all(pool).await {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(target: "phi_backend::leaderboard", "batch query leaderboard_details failed (ignored): {e}");
            return HashMap::new();
        }
    };

    let mut map = HashMap::with_capacity(user_hashes.len());
    for r in rows {
        let user_hash: String = r.try_get("user_hash").unwrap_or_default();
        let best: Option<String> = r.try_get("best_top3_json").unwrap_or(None);
        let ap: Option<String> = r.try_get("ap_top3_json").unwrap_or(None);
        map.insert(user_hash, (best, ap));
    }
    map
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
    params(("limit" = Option<i64>, Query, description = "每页数量，默认50，最大200"),("offset" = Option<i64>, Query, description = "偏移量")),
    responses(
        (status = 200, description = "排行榜 TOP", body = LeaderboardTopResponse),
        (
            status = 500,
            description = "统计存储未初始化/查询失败",
            body = String,
            content_type = "text/plain"
        )
    ),
    tag = "Leaderboard"
)]
pub async fn get_top(
    State(state): State<AppState>,
    Query(q): Query<TopQuery>,
) -> Result<Json<LeaderboardTopResponse>, AppError> {
    let storage = state
        .stats_storage
        .as_ref()
        .ok_or_else(|| AppError::Internal("统计存储未初始化".into()))?;
    let limit = q.limit.unwrap_or(50).clamp(1, 200);
    let offset = q.offset.unwrap_or(0).max(0);

    let total_row = sqlx::query("SELECT COUNT(1) AS c FROM leaderboard_rks lr LEFT JOIN user_profile up ON up.user_hash=lr.user_hash WHERE COALESCE(up.is_public,0)=1 AND lr.is_hidden=0")
        .fetch_one(&storage.pool).await.map_err(|e| AppError::Internal(format!("count top: {e}")))?;
    let total: i64 = total_row.try_get("c").unwrap_or(0);

    let rows = if let (Some(sc), Some(upd), Some(usr)) =
        (q.after_score, q.after_updated.clone(), q.after_user.clone())
    {
        // seek 分页
        sqlx::query(
            "SELECT lr.user_hash, lr.total_rks, lr.updated_at, up.alias, COALESCE(up.show_best_top3,0) AS sbt, COALESCE(up.show_ap_top3,0) AS sat
             FROM leaderboard_rks lr LEFT JOIN user_profile up ON up.user_hash=lr.user_hash
             WHERE COALESCE(up.is_public,0)=1 AND lr.is_hidden=0 AND (
               lr.total_rks < ? OR (lr.total_rks = ? AND (lr.updated_at > ? OR (lr.updated_at = ? AND lr.user_hash > ?)))
             )
             ORDER BY lr.total_rks DESC, lr.updated_at ASC, lr.user_hash ASC
             LIMIT ?"
        )
        .bind(sc)
        .bind(sc)
        .bind(&upd)
        .bind(&upd)
        .bind(&usr)
        .bind(limit)
        .fetch_all(&storage.pool)
        .await
        .map_err(|e| AppError::Internal(format!("query top seek: {e}")))?
    } else {
        // offset 分页
        sqlx::query(
            "SELECT lr.user_hash, lr.total_rks, lr.updated_at, up.alias, COALESCE(up.show_best_top3,0) AS sbt, COALESCE(up.show_ap_top3,0) AS sat
             FROM leaderboard_rks lr LEFT JOIN user_profile up ON up.user_hash=lr.user_hash
             WHERE COALESCE(up.is_public,0)=1 AND lr.is_hidden=0
             ORDER BY lr.total_rks DESC, lr.updated_at ASC, lr.user_hash ASC
             LIMIT ? OFFSET ?"
        )
        .bind(limit)
        .bind(offset)
        .fetch_all(&storage.pool)
        .await
        .map_err(|e| AppError::Internal(format!("query top: {e}")))?
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
    let details_map = fetch_top3_details_map(&storage.pool, &detail_users).await;

    for (idx, r) in rows.into_iter().enumerate() {
        let user_hash: String = r.try_get("user_hash").unwrap_or_default();
        let alias: Option<String> = r.try_get("alias").ok();
        let score: f64 = r.try_get("total_rks").unwrap_or(0.0);
        let updated_at: String = r.try_get("updated_at").unwrap_or_default();
        let sbt: i64 = r.try_get("sbt").unwrap_or(0);
        let sat: i64 = r.try_get("sat").unwrap_or(0);

        let mut best_top3: Option<Vec<ChartTextItem>> = None;
        let mut ap_top3: Option<Vec<ChartTextItem>> = None;
        if (sbt != 0 || sat != 0)
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

    Ok(Json(LeaderboardTopResponse {
        items,
        total,
        next_after_score: if has_more { last_score } else { None },
        next_after_updated: if has_more { last_updated } else { None },
        next_after_user: if has_more { last_user_hash } else { None },
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
        ("count" = Option<i64>, Query, description = "返回数量（与 start 组合使用）")
    ),
    responses(
        (status = 200, description = "区间结果", body = LeaderboardTopResponse),
        (
            status = 422,
            description = "参数校验失败（缺少 rank/start 等）",
            body = String,
            content_type = "text/plain"
        ),
        (
            status = 500,
            description = "统计存储未初始化/查询失败",
            body = String,
            content_type = "text/plain"
        )
    ),
    tag = "Leaderboard"
)]
pub async fn get_by_rank(
    State(state): State<AppState>,
    Query(q): Query<RankQuery>,
) -> Result<Json<LeaderboardTopResponse>, AppError> {
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

    let total_row = sqlx::query("SELECT COUNT(1) AS c FROM leaderboard_rks lr LEFT JOIN user_profile up ON up.user_hash=lr.user_hash WHERE COALESCE(up.is_public,0)=1 AND lr.is_hidden=0")
        .fetch_one(&storage.pool).await.map_err(|e| AppError::Internal(format!("count rank: {e}")))?;
    let total: i64 = total_row.try_get("c").unwrap_or(0);

    let rows = sqlx::query(
        "SELECT lr.user_hash, lr.total_rks, lr.updated_at, up.alias, COALESCE(up.show_best_top3,0) AS sbt, COALESCE(up.show_ap_top3,0) AS sat
         FROM leaderboard_rks lr LEFT JOIN user_profile up ON up.user_hash=lr.user_hash
         WHERE COALESCE(up.is_public,0)=1 AND lr.is_hidden=0
         ORDER BY lr.total_rks DESC, lr.updated_at ASC, lr.user_hash ASC
         LIMIT ? OFFSET ?"
    )
        .bind(limit)
        .bind(offset)
        .fetch_all(&storage.pool)
        .await
        .map_err(|e| AppError::Internal(format!("query by rank: {e}")))?;

    let mut items: Vec<LeaderboardTopItem> = Vec::with_capacity(rows.len());
    let has_more = ((start_rank - 1) + rows.len() as i64) < total;
    let (mut last_score, mut last_updated, mut last_user_hash) =
        (None::<f64>, None::<String>, None::<String>);
    if has_more && let Some(r) = rows.last() {
        last_score = r.try_get::<f64, _>("total_rks").ok();
        last_updated = r.try_get::<String, _>("updated_at").ok();
        last_user_hash = r.try_get::<String, _>("user_hash").ok();
    }

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
    let details_map = fetch_top3_details_map(&storage.pool, &detail_users).await;

    for (i, r) in rows.into_iter().enumerate() {
        let user_hash: String = r.try_get("user_hash").unwrap_or_default();
        let alias: Option<String> = r.try_get("alias").ok();
        let score: f64 = r.try_get("total_rks").unwrap_or(0.0);
        let updated_at: String = r.try_get("updated_at").unwrap_or_default();
        let sbt: i64 = r.try_get("sbt").unwrap_or(0);
        let sat: i64 = r.try_get("sat").unwrap_or(0);

        let mut best_top3: Option<Vec<ChartTextItem>> = None;
        let mut ap_top3: Option<Vec<ChartTextItem>> = None;
        if (sbt != 0 || sat != 0)
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

    Ok(Json(LeaderboardTopResponse {
        items,
        total,
        next_after_score: if has_more { last_score } else { None },
        next_after_updated: if has_more { last_updated } else { None },
        next_after_user: if has_more { last_user_hash } else { None },
    }))
}

#[utoipa::path(
    post,
    path = "/leaderboard/rks/me",
    summary = "我的名次（按RKS）",
    description = "通过认证信息推导用户身份，返回名次、分数、总量与百分位（竞争排名）",
    request_body = crate::features::save::models::UnifiedSaveRequest,
    responses(
        (status = 200, description = "查询成功", body = MeResponse),
        (
            status = 500,
            description = "统计存储未初始化/查询失败/无法识别用户",
            body = String,
            content_type = "text/plain"
        )
    ),
    tag = "Leaderboard"
)]
pub async fn post_me(
    State(state): State<AppState>,
    Json(auth): Json<crate::features::save::models::UnifiedSaveRequest>,
) -> Result<Json<MeResponse>, AppError> {
    let storage = state
        .stats_storage
        .as_ref()
        .ok_or_else(|| AppError::Internal("统计存储未初始化".into()))?;
    let salt = crate::config::AppConfig::global()
        .stats
        .user_hash_salt
        .as_deref();
    let (user_hash_opt, _kind) =
        crate::features::stats::derive_user_identity_from_auth(salt, &auth);
    let user_hash =
        user_hash_opt.ok_or_else(|| AppError::Internal("无法识别用户（缺少可用凭证）".into()))?;

    let row_opt =
        sqlx::query("SELECT total_rks, updated_at FROM leaderboard_rks WHERE user_hash=?")
            .bind(&user_hash)
            .fetch_optional(&storage.pool)
            .await
            .map_err(|e| AppError::Internal(format!("me fetch: {e}")))?;
    let (my_score, my_updated) = if let Some(r) = row_opt {
        (
            r.get::<f64, _>("total_rks"),
            r.get::<String, _>("updated_at"),
        )
    } else {
        (0.0, String::from(""))
    };

    let total_row = sqlx::query("SELECT COUNT(1) as c FROM leaderboard_rks lr LEFT JOIN user_profile up ON up.user_hash=lr.user_hash WHERE COALESCE(up.is_public,0)=1 AND lr.is_hidden=0")
        .fetch_one(&storage.pool).await.map_err(|e| AppError::Internal(format!("me total: {e}")))?;
    let total: i64 = total_row.try_get("c").unwrap_or(0);

    if total == 0 || my_score <= 0.0 {
        return Ok(Json(MeResponse {
            rank: 0,
            score: 0.0,
            total,
            percentile: 0.0,
        }));
    }

    // 竞争排名：严格大于 + 稳定并列次序（updated_at 更早在前，user_hash 更小在前）
    let row = sqlx::query(
        "SELECT COUNT(1) as higher FROM leaderboard_rks lr LEFT JOIN user_profile up ON up.user_hash=lr.user_hash
         WHERE COALESCE(up.is_public,0)=1 AND lr.is_hidden=0 AND (
           lr.total_rks > ? OR (lr.total_rks = ? AND (lr.updated_at < ? OR (lr.updated_at = ? AND lr.user_hash < ?)))
         )"
    )
    .bind(my_score).bind(my_score).bind(&my_updated).bind(&my_updated).bind(&user_hash)
    .fetch_one(&storage.pool).await.map_err(|e| AppError::Internal(format!("me rank: {e}")))?;
    let higher: i64 = row.try_get("higher").unwrap_or(0);
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
            body = String,
            content_type = "text/plain"
        ),
        (
            status = 422,
            description = "别名非法",
            body = String,
            content_type = "text/plain"
        ),
        (
            status = 500,
            description = "统计存储未初始化/写入失败/无法识别用户",
            body = String,
            content_type = "text/plain"
        )
    ),
    tag = "Leaderboard"
)]
pub async fn put_alias(
    State(state): State<AppState>,
    Json(req): Json<AliasRequest>,
) -> Result<Json<OkAliasResponse>, AppError> {
    let storage = state
        .stats_storage
        .as_ref()
        .ok_or_else(|| AppError::Internal("统计存储未初始化".into()))?;
    let salt = crate::config::AppConfig::global()
        .stats
        .user_hash_salt
        .as_deref();
    let (user_hash_opt, _kind) =
        crate::features::stats::derive_user_identity_from_auth(salt, &req.auth);
    let user_hash =
        user_hash_opt.ok_or_else(|| AppError::Internal("无法识别用户（缺少可用凭证）".into()))?;

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
    // 默认展示开关读取配置
    let cfg = crate::config::AppConfig::global();
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
    // Upsert profile
    let res = sqlx::query(
        "INSERT INTO user_profile(user_hash,alias,is_public,show_rks_composition,show_best_top3,show_ap_top3,user_kind,created_at,updated_at) VALUES(?,?,?,?,?,?,?,?,?)
         ON CONFLICT(user_hash) DO UPDATE SET alias=excluded.alias, updated_at=excluded.updated_at"
    )
        .bind(&user_hash)
        .bind(alias)
        .bind(0_i64)
        .bind(def_rc)
        .bind(def_b3)
        .bind(def_ap3)
        .bind(Option::<String>::None)
        .bind(&now)
        .bind(&now)
        .execute(&storage.pool)
        .await;
    match res {
        Ok(_) => Ok(Json(OkAliasResponse {
            ok: true,
            alias: alias.to_string(),
        })),
        Err(e) => {
            if format!("{e}").to_lowercase().contains("unique") {
                return Err(AppError::Conflict("别名已被占用".into()));
            }
            Err(AppError::Internal(format!("设置别名失败: {e}")))
        }
    }
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
            body = String,
            content_type = "text/plain"
        ),
        (
            status = 500,
            description = "统计存储未初始化/更新失败/无法识别用户",
            body = String,
            content_type = "text/plain"
        )
    ),
    tag = "Leaderboard"
)]
pub async fn put_profile(
    State(state): State<AppState>,
    Json(req): Json<ProfileUpdateRequest>,
) -> Result<Json<OkResponse>, AppError> {
    let storage = state
        .stats_storage
        .as_ref()
        .ok_or_else(|| AppError::Internal("统计存储未初始化".into()))?;
    let salt = crate::config::AppConfig::global()
        .stats
        .user_hash_salt
        .as_deref();
    let (user_hash_opt, _kind) =
        crate::features::stats::derive_user_identity_from_auth(salt, &req.auth);
    let user_hash =
        user_hash_opt.ok_or_else(|| AppError::Internal("无法识别用户（缺少可用凭证）".into()))?;
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
    sqlx::query("INSERT INTO user_profile(user_hash,created_at,updated_at) VALUES(?,?,?) ON CONFLICT(user_hash) DO NOTHING")
        .bind(&user_hash).bind(&now).bind(&now).execute(&storage.pool).await.ok();

    // Build dynamic update
    let mut sets: Vec<&str> = Vec::new();
    if is_public.is_some() {
        sets.push("is_public=?");
    }
    if show_rc.is_some() {
        sets.push("show_rks_composition=?");
    }
    if show_b3.is_some() {
        sets.push("show_best_top3=?");
    }
    if show_ap3.is_some() {
        sets.push("show_ap_top3=?");
    }
    sets.push("updated_at=?");
    let sql = format!(
        "UPDATE user_profile SET {} WHERE user_hash=?",
        sets.join(",")
    );
    let mut q = sqlx::query(&sql);
    if let Some(v) = is_public {
        q = q.bind(v);
    }
    if let Some(v) = show_rc {
        q = q.bind(v);
    }
    if let Some(v) = show_b3 {
        q = q.bind(v);
    }
    if let Some(v) = show_ap3 {
        q = q.bind(v);
    }
    q = q.bind(&now).bind(&user_hash);
    q.execute(&storage.pool)
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
            body = String,
            content_type = "text/plain"
        ),
        (
            status = 500,
            description = "统计存储未初始化/查询失败",
            body = String,
            content_type = "text/plain"
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
    // join profile + rks
    let row = sqlx::query(
        "SELECT up.user_hash, up.is_public, up.show_rks_composition, up.show_best_top3, up.show_ap_top3, lr.total_rks, lr.updated_at
         FROM user_profile up LEFT JOIN leaderboard_rks lr ON lr.user_hash=up.user_hash WHERE up.alias = ?"
    )
    .bind(&alias)
    .fetch_optional(&storage.pool).await.map_err(|e| AppError::Internal(format!("profile query: {e}")))?;
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
        && let Some(d) = sqlx::query("SELECT rks_composition_json, best_top3_json, ap_top3_json FROM leaderboard_details WHERE user_hash = ?")
            .bind(&user_hash)
            .fetch_optional(&storage.pool).await.map_err(|e| AppError::Internal(format!("details: {e}")))? {
        if show_rc != 0
            && let Ok(Some(j)) = d.try_get::<String,_>("rks_composition_json").map(Some)
        {
            resp.rks_composition = serde_json::from_str::<RksCompositionText>(&j).ok();
        }
        if show_b3 != 0
            && let Ok(Some(j)) = d.try_get::<String,_>("best_top3_json").map(Some)
        {
            resp.best_top3 = serde_json::from_str::<Vec<ChartTextItem>>(&j).ok();
        }
        if show_ap3 != 0
            && let Ok(Some(j)) = d.try_get::<String,_>("ap_top3_json").map(Some)
        {
            resp.ap_top3 = serde_json::from_str::<Vec<ChartTextItem>>(&j).ok();
        }
    }
    Ok(Json(resp))
}

// ============ Admin endpoints (X-Admin-Token) ============

pub(crate) fn require_admin(headers: &HeaderMap) -> Result<String, AppError> {
    let provided = headers
        .get("x-admin-token")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .trim();
    if provided.is_empty() {
        return Err(AppError::Auth("缺少管理员令牌".into()));
    }
    let cfg = crate::config::AppConfig::global();
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

#[derive(serde::Serialize, utoipa::ToSchema)]
#[schema(example = json!({
  "user": "ab12****",
  "alias": "Alice",
  "score": 14.73,
  "suspicion": 1.10,
  "updated_at": "2025-09-20T04:10:44Z"
}))]
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
            body = String,
            content_type = "text/plain"
        ),
        (
            status = 500,
            description = "统计存储未初始化/查询失败",
            body = String,
            content_type = "text/plain"
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
    let rows = sqlx::query(
        "SELECT lr.user_hash, lr.total_rks, lr.suspicion_score, lr.updated_at, up.alias
         FROM leaderboard_rks lr LEFT JOIN user_profile up ON up.user_hash=lr.user_hash
         WHERE lr.suspicion_score >= ?
         ORDER BY lr.suspicion_score DESC, lr.total_rks DESC
         LIMIT ?",
    )
    .bind(min_score)
    .bind(limit)
    .fetch_all(&storage.pool)
    .await
    .map_err(|e| AppError::Internal(format!("suspicious: {e}")))?;
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

#[derive(serde::Deserialize, utoipa::ToSchema)]
#[schema(example = json!({"user_hash":"abcde12345","status":"shadow","reason":"suspicious jump"}))]
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
            body = String,
            content_type = "text/plain"
        ),
        (
            status = 422,
            description = "参数校验失败（status 非法等）",
            body = String,
            content_type = "text/plain"
        ),
        (
            status = 500,
            description = "统计存储未初始化/写入失败",
            body = String,
            content_type = "text/plain"
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
    let st = req.status.to_lowercase();
    let hide = match st.as_str() {
        "approved" => 0_i64,
        "shadow" | "banned" => 1_i64,
        "rejected" => 1_i64,
        _ => {
            return Err(AppError::Validation(
                "status 必须为 approved|shadow|banned|rejected".into(),
            ));
        }
    };
    sqlx::query("UPDATE leaderboard_rks SET is_hidden=? WHERE user_hash=?")
        .bind(hide)
        .bind(&req.user_hash)
        .execute(&storage.pool)
        .await
        .map_err(|e| AppError::Internal(format!("resolve upd: {e}")))?;
    sqlx::query("INSERT INTO moderation_flags(user_hash,status,reason,severity,created_by,created_at) VALUES(?,?,?,?,?,?)")
        .bind(&req.user_hash).bind(st).bind(req.reason.unwrap_or_default()).bind(0_i64).bind(admin).bind(now)
        .execute(&storage.pool).await.map_err(|e| AppError::Internal(format!("resolve flag: {e}")))?;
    Ok(Json(OkResponse { ok: true }))
}

#[derive(serde::Deserialize, utoipa::ToSchema)]
#[schema(example = json!({"user_hash":"abcde12345","alias":"Alice"}))]
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
            body = String,
            content_type = "text/plain"
        ),
        (
            status = 422,
            description = "参数校验失败（别名非法等）",
            body = String,
            content_type = "text/plain"
        ),
        (
            status = 500,
            description = "统计存储未初始化/写入失败",
            body = String,
            content_type = "text/plain"
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
    let mut tx = storage
        .pool
        .begin()
        .await
        .map_err(|e| AppError::Internal(format!("tx begin: {e}")))?;
    // 清理原持有人
    sqlx::query("UPDATE user_profile SET alias=NULL, updated_at=? WHERE alias=?")
        .bind(&now)
        .bind(alias)
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::Internal(format!("clear alias: {e}")))?;
    // 确保目标行存在
    sqlx::query("INSERT INTO user_profile(user_hash,created_at,updated_at) VALUES(?,?,?) ON CONFLICT(user_hash) DO NOTHING")
        .bind(&req.user_hash).bind(&now).bind(&now)
        .execute(&mut *tx).await.map_err(|e| AppError::Internal(format!("ensure profile: {e}")))?;
    // 赋予别名
    sqlx::query("UPDATE user_profile SET alias=?, updated_at=? WHERE user_hash=?")
        .bind(alias)
        .bind(&now)
        .bind(&req.user_hash)
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::Internal(format!("set alias: {e}")))?;
    tx.commit()
        .await
        .map_err(|e| AppError::Internal(format!("tx commit: {e}")))?;
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
        .route("/admin/leaderboard/resolve", post(post_resolve))
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
    fn test_require_admin_env() {
        // 使用环境变量配置管理员令牌，并初始化全局配置，避免未初始化导致的 panic
        unsafe {
            std::env::set_var("APP_LEADERBOARD_ADMIN_TOKENS", "t1,t2");
        }
        let _ = crate::config::AppConfig::init_global();

        let mut headers = HeaderMap::new();
        headers.insert("x-admin-token", axum::http::HeaderValue::from_static("t2"));
        assert!(require_admin(&headers).is_ok());
        headers.insert("x-admin-token", axum::http::HeaderValue::from_static("bad"));
        assert!(require_admin(&headers).is_err());
    }
}
