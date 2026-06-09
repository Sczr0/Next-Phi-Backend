//! RKS 历史查询 API 处理模块

use axum::{Router, extract::State, response::Json, routing::post};
use serde::{Deserialize, Serialize};
use std::time::Instant;

use crate::{
    error::AppError,
    features::stats::storage::{RksHistoryCursor, RksHistoryEntry},
    state::AppState,
};

fn parse_rks_history_cursor(raw: Option<&str>) -> Result<Option<RksHistoryCursor>, AppError> {
    let Some(raw) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    let Some((created_at, id_raw)) = raw.rsplit_once('|') else {
        return Err(AppError::Validation(
            "cursor 无效（期望格式：createdAt|id）".into(),
        ));
    };
    let id = id_raw
        .parse::<i64>()
        .map_err(|e| AppError::Validation(format!("cursor id 无效: {e}")))?;
    if created_at.trim().is_empty() || id <= 0 {
        return Err(AppError::Validation(
            "cursor 无效（createdAt 不能为空且 id 必须为正数）".into(),
        ));
    }
    Ok(Some(RksHistoryCursor {
        created_at: created_at.to_string(),
        id,
    }))
}

fn encode_rks_history_cursor(entry: Option<&RksHistoryEntry>) -> Option<String> {
    entry.map(|entry| format!("{}|{}", entry.created_at, entry.id))
}

/// RKS 历史查询请求
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(example = json!({
    "auth": {"sessionToken": "r:abcdefg.hijklmn"},
    "limit": 50,
    "offset": 0,
    "cursor": "2025-11-28T10:30:00Z|12345"
}))]
pub struct RksHistoryRequest {
    /// 认证信息
    pub auth: crate::auth_contract::UnifiedSaveRequest,
    /// 返回数量（默认 50，最大 200）
    #[serde(default)]
    pub limit: Option<i64>,
    /// 分页偏移（默认 0）
    #[serde(default)]
    pub offset: Option<i64>,
    /// 游标分页位置。存在时优先使用 cursor，并忽略 offset。
    #[serde(default)]
    pub cursor: Option<String>,
}

/// 单条 RKS 历史记录
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(example = json!({
    "rks": 14.73,
    "rksJump": 0.05,
    "createdAt": "2025-11-28T10:30:00Z"
}))]
pub struct RksHistoryItem {
    /// RKS 值
    pub rks: f64,
    /// 相比上次的变化量
    pub rks_jump: f64,
    /// 记录时间（UTC RFC3339）
    pub created_at: String,
}

/// RKS 历史查询响应
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(example = json!({
    "items": [
        {"rks": 14.73, "rksJump": 0.05, "createdAt": "2025-11-28T10:30:00Z"},
        {"rks": 14.68, "rksJump": 0.12, "createdAt": "2025-11-27T15:20:00Z"}
    ],
    "total": 42,
    "currentRks": 14.73,
    "peakRks": 14.73,
    "hasMore": true,
    "nextCursor": "2025-11-27T15:20:00Z|12344"
}))]
pub struct RksHistoryResponse {
    /// 历史记录列表（按时间倒序）
    pub items: Vec<RksHistoryItem>,
    /// 总记录数
    pub total: i64,
    /// 当前 RKS
    pub current_rks: f64,
    /// 历史最高 RKS
    pub peak_rks: f64,
    /// 是否还有下一页
    pub has_more: bool,
    /// 下一页游标；为空表示已到末尾
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

/// 查询用户 RKS 历史变化
#[utoipa::path(
    post,
    path = "/rks/history",
    summary = "查询 RKS 历史变化",
    description = "通过认证信息查询用户的 RKS 历史变化记录，包括每次提交的 RKS 值和变化量。",
    request_body = RksHistoryRequest,
    responses(
        (status = 200, body = RksHistoryResponse, description = "成功返回 RKS 历史"),
        (
            status = 401,
            description = "认证失败/无法识别用户",
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
    tag = "RKS"
)]
pub async fn post_rks_history(
    State(state): State<AppState>,
    request: axum::extract::Request,
) -> Result<Json<RksHistoryResponse>, AppError> {
    let t_total = Instant::now();
    let (mut req, bearer_state) =
        crate::session_auth::parse_json_with_bearer_state::<RksHistoryRequest>(request).await?;
    crate::session_auth::merge_auth_from_bearer_if_missing(
        state.stats_storage.as_ref(),
        &bearer_state,
        &mut req.auth,
    )
    .await?;
    tracing::debug!(
        target: "phi_backend::rks",
        has_session_token = req.auth.session_token.is_some(),
        has_external_credentials = req.auth.external_credentials.is_some(),
        "rks auth after bearer merge"
    );

    let storage = state
        .stats_storage
        .as_ref()
        .ok_or_else(|| AppError::Internal("统计存储未初始化".into()))?;

    // 解析用户身份
    let salt = crate::config::AppConfig::global()
        .stats
        .user_hash_salt
        .as_deref();
    let (user_hash_opt, _kind) =
        crate::session_auth::derive_user_identity_with_bearer(salt, &req.auth, &bearer_state)?;
    let user_hash =
        user_hash_opt.ok_or_else(|| AppError::Auth("无法识别用户（缺少可用凭证）".into()))?;
    storage.ensure_user_not_banned(&user_hash).await?;

    // 分页参数。cursor 存在时优先走 seek 分页，避免深页 OFFSET 扫描。
    let limit = req.limit.unwrap_or(50).clamp(1, 200);
    let offset = req.offset.unwrap_or(0).max(0);
    let cursor = parse_rks_history_cursor(req.cursor.as_deref())?;

    let t_query = Instant::now();
    let history_fut = storage.query_rks_history_page(&user_hash, limit, offset, cursor.as_ref());
    let current_fut = async {
        storage
            .get_prev_rks(&user_hash)
            .await
            .map(|value| value.map_or(0.0, |(rks, _)| rks))
    };
    let peak_fut = storage.get_peak_rks(&user_hash);
    let (page, current_rks, peak_rks) = tokio::try_join!(history_fut, current_fut, peak_fut)?;
    let next_cursor = if page.has_more {
        encode_rks_history_cursor(page.entries.last())
    } else {
        None
    };
    let total = page.total;
    let has_more = page.has_more;
    let items: Vec<RksHistoryItem> = page
        .entries
        .into_iter()
        .map(|entry| RksHistoryItem {
            rks: entry.rks,
            rks_jump: entry.rks_jump,
            created_at: entry.created_at,
        })
        .collect();
    let query_ms = t_query.elapsed().as_millis();

    tracing::info!(
        target: "phi_backend::rks::performance",
        route = "/rks/history",
        phase = "total",
        status = "ok",
        items = items.len(),
        total,
        query_ms,
        total_dur_ms = t_total.elapsed().as_millis(),
        "rks performance"
    );

    Ok(Json(RksHistoryResponse {
        items,
        total,
        current_rks,
        peak_rks,
        has_more,
        next_cursor,
    }))
}

/// 创建 RKS 路由
pub fn create_rks_router() -> Router<AppState> {
    Router::new().route("/rks/history", post(post_rks_history))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rks_history_item_serialize() {
        let item = RksHistoryItem {
            rks: 14.73,
            rks_jump: 0.05,
            created_at: "2025-11-28T10:30:00Z".to_string(),
        };
        let json = serde_json::to_string(&item).unwrap();
        assert!(json.contains("14.73"));
        assert!(json.contains("0.05"));
    }

    #[test]
    fn rks_history_cursor_roundtrips_created_at_and_id() {
        let cursor = parse_rks_history_cursor(Some("2025-11-28T10:30:00Z|12345"))
            .unwrap()
            .unwrap();
        assert_eq!(cursor.created_at, "2025-11-28T10:30:00Z");
        assert_eq!(cursor.id, 12345);
    }

    #[test]
    fn rks_history_cursor_rejects_invalid_input() {
        assert!(parse_rks_history_cursor(Some("2025-11-28T10:30:00Z")).is_err());
        assert!(parse_rks_history_cursor(Some("2025-11-28T10:30:00Z|0")).is_err());
        assert!(parse_rks_history_cursor(Some("2025-11-28T10:30:00Z|abc")).is_err());
    }
}
