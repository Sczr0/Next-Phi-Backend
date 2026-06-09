use axum::{Json, http::StatusCode};
use serde::Serialize;

use crate::error::AppError;

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UserIdResponse {
    /// 鍘绘晱鍚庣殑绋冲畾鐢ㄦ埛 ID锛?2 浣?hex锛岀瓑浠蜂簬 stats/leaderboard 浣跨敤鐨?user_hash锛?    #[schema(example = "ab12cd34ef56ab12cd34ef56ab12cd34")]
    pub user_id: String,
    /// 鐢ㄤ簬鎺ㄥ user_id 鐨勫嚟璇佺被鍨嬶紙鐢ㄤ簬鎺掓煡鈥滀负浠€涔堝拰浠ュ墠涓嶄竴鑷粹€濓級
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_kind: Option<String>,
}

#[utoipa::path(
    post,
    path = "/auth/user-id",
    summary = "根据凭证生成去敏用户ID",
    description = "使用服务端配置的 stats.user_hash_salt 对凭证做 HMAC-SHA256 去敏，生成稳定用户标识。",
    request_body = crate::auth_contract::UnifiedSaveRequest,
    responses(
        (status = 200, description = "鐢熸垚鎴愬姛", body = UserIdResponse),
        (
            status = 422,
            description = "凭证缺失或无效",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        ),
        (
            status = 500,
            description = "服务端未配置 user_hash_salt",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        )
    ),
    tag = "Auth"
)]
pub async fn post_user_id(
    Json(auth): Json<crate::auth_contract::UnifiedSaveRequest>,
) -> Result<(StatusCode, Json<UserIdResponse>), AppError> {
    // 与 /save 的凭证互斥规则保持一致，避免同一请求在不同接口出现身份不一致。
    if auth.session_token.is_some() && auth.external_credentials.is_some() {
        return Err(AppError::Validation(
            "不能同时提供 sessionToken 和 externalCredentials，请只选择其中一种认证方式".into(),
        ));
    }

    let stable_ok = if let Some(tok) = auth.session_token.as_deref() {
        !tok.is_empty()
    } else if let Some(ext) = auth.external_credentials.as_ref() {
        let has_api_user_id = ext.api_user_id.as_deref().is_some_and(|v| !v.is_empty());
        let has_sessiontoken = ext.sessiontoken.as_deref().is_some_and(|v| !v.is_empty());
        let has_platform_pair = match (&ext.platform, &ext.platform_id) {
            (Some(p), Some(pid)) => !p.is_empty() && !pid.is_empty(),
            _ => false,
        };
        has_api_user_id || has_sessiontoken || has_platform_pair
    } else {
        false
    };
    if !stable_ok {
        return Err(AppError::Validation(
            "无法识别用户：请提供 sessionToken，或 externalCredentials 中的 platform+platformId / sessiontoken / apiUserId（且不能为空）"
                .into(),
        ));
    }

    let salt = crate::config::AppConfig::global()
        .stats
        .user_hash_salt
        .as_deref()
        .ok_or_else(|| {
            AppError::Internal(
                "stats.user_hash_salt 未配置，无法生成稳定 user_id（可通过 APP_STATS_USER_HASH_SALT 设置）"
                    .into(),
            )
        })?;

    let (user_id_opt, user_kind) =
        crate::identity_hash::derive_user_identity_from_auth(Some(salt), &auth);
    let user_id = user_id_opt.ok_or_else(|| AppError::Internal("生成 user_id 失败".into()))?;
    Ok((StatusCode::OK, Json(UserIdResponse { user_id, user_kind })))
}
