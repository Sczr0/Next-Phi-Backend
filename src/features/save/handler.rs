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
use crate::features::rks::engine::{PlayerRksResult, calculate_player_rks};
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
    let salt = crate::config::AppConfig::global().stats.user_hash_salt.as_deref();
    let (user_hash, user_kind) = crate::features::stats::derive_user_identity_from_auth(salt, &payload);

    let source = validate_and_create_source(payload)?;
    let parsed = provider::get_decrypted_save(source).await?;

    // 业务打点：成功获取存档
    if let Some(stats) = state.stats.as_ref() {
        let extra = serde_json::json!({ "user_kind": user_kind });
        stats.track_feature("save", "get_save", user_hash.clone(), Some(extra)).await;
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
