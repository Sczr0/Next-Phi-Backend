use axum::{
    extract::{Path, State},
    response::Json,
};
use sqlx::Row;

use crate::{error::AppError, state::AppState};

use super::super::models::{
    AliasRequest, ChartTextItem, ProfileUpdateRequest, PublicProfileResponse, RksCompositionText,
};
use super::{OkAliasResponse, OkResponse, ensure_not_banned, validate_alias_format};

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

    let alias = req.alias.trim();
    validate_alias_format(alias)?;
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
        is_public = Some(i64::from(v));
    }
    if let Some(v) = req.show_rks_composition {
        show_rc = Some(i64::from(v));
    }
    if let Some(v) = req.show_best_top3 {
        show_b3 = Some(i64::from(v));
    }
    if let Some(v) = req.show_ap_top3 {
        show_ap3 = Some(i64::from(v));
    }

    // 保持旧行为：资料行补建失败只记录日志，后续更新仍交给 storage 返回真实错误。
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
