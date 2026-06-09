use axum::http::HeaderMap;
use axum::{
    extract::{Query, State},
    response::Json,
};
use serde::{Deserialize, Serialize};
use sqlx::Row;
use std::collections::BTreeMap;

use crate::{error::AppError, state::AppState};

use super::{
    OkAliasResponse, OkResponse, apply_user_status, mask_user_prefix, normalize_moderation_status,
    validate_alias_format,
};

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

#[derive(Serialize, utoipa::ToSchema)]
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
    Query(p): Query<BTreeMap<String, String>>,
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

    let total_fut = storage
        .query_admin_leaderboard_users_count(status_filter.as_deref(), alias_like.as_deref());
    let rows_fut = storage.query_admin_leaderboard_users_rows(
        status_filter.as_deref(),
        alias_like.as_deref(),
        page_size,
        offset,
    );
    let (total, rows) = tokio::try_join!(total_fut, rows_fut)?;

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
        reason: reason_clean.map(std::string::ToString::to_string),
        updated_by: Some(admin),
        updated_at: Some(now),
    }))
}

#[derive(Deserialize, utoipa::ToSchema)]
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

#[derive(Deserialize, utoipa::ToSchema)]
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
    validate_alias_format(alias)?;
    storage
        .force_set_user_alias(&req.user_hash, alias, &now)
        .await?;
    Ok(Json(OkAliasResponse {
        ok: true,
        alias: alias.to_string(),
    }))
}
