//! RKS 历史查询 API 处理模块

use axum::{Router, extract::State, response::Json, routing::post};
use serde::{Deserialize, Serialize};
use std::time::Instant;

use crate::{error::AppError, state::AppState};

/// RKS 历史查询请求
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(example = json!({
    "auth": {"sessionToken": "r:abcdefg.hijklmn"},
    "limit": 50,
    "offset": 0
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
    "peakRks": 14.73
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

    // 分页参数
    let limit = req.limit.unwrap_or(50).clamp(1, 200);
    let offset = req.offset.unwrap_or(0).max(0);

    // 查询历史记录
    let t_query = Instant::now();
    let (entries, total) = storage.query_rks_history(&user_hash, limit, offset).await?;
    let items: Vec<RksHistoryItem> = entries
        .into_iter()
        .map(|entry| RksHistoryItem {
            rks: entry.rks,
            rks_jump: entry.rks_jump,
            created_at: entry.created_at,
        })
        .collect();

    // 获取当前 RKS
    let current_rks = storage
        .get_prev_rks(&user_hash)
        .await?
        .map(|(rks, _)| rks)
        .unwrap_or(0.0);

    // 获取历史最高 RKS
    let peak_rks = storage.get_peak_rks(&user_hash).await?;
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
}
