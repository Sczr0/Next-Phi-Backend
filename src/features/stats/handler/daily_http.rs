use axum::{
    extract::{Query, State},
    response::Json,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::{error::AppError, state::AppState};

use super::{
    cache::{build_daily_http_cache_key, daily_http_cache},
    params::normalize_top_per_day,
    queries::query_daily_http,
    time::{parse_ymd, resolve_timezone, validate_date_range},
};

#[derive(Deserialize)]
pub struct DailyHttpQuery {
    pub(super) start: String,
    pub(super) end: String,
    /// 可选时区（IANA 名称，如 Asia/Shanghai），覆盖配置
    pub(super) timezone: Option<String>,
    /// 可选路由模板过滤（MatchedPath）
    pub(super) route: Option<String>,
    /// 可选 HTTP 方法过滤（GET/POST 等）
    pub(super) method: Option<String>,
    /// 每天最多返回的路由明细条数（默认 200）
    pub(super) top: Option<i64>,
}

#[derive(Serialize, utoipa::ToSchema, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DailyHttpTotalRow {
    pub(super) date: String,
    pub(super) total: i64,
    pub(super) errors: i64,
    /// errors / total（total=0 时为 0）
    pub(super) error_rate: f64,
    pub(super) client_errors: i64,
    pub(super) server_errors: i64,
    pub(super) client_error_rate: f64,
    pub(super) server_error_rate: f64,
}

#[derive(Serialize, utoipa::ToSchema, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DailyHttpRouteRow {
    pub(super) date: String,
    pub(super) route: String,
    pub(super) method: String,
    pub(super) total: i64,
    pub(super) errors: i64,
    pub(super) error_rate: f64,
    pub(super) client_errors: i64,
    pub(super) server_errors: i64,
    pub(super) client_error_rate: f64,
    pub(super) server_error_rate: f64,
}

#[derive(Serialize, utoipa::ToSchema, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DailyHttpResponse {
    pub(super) timezone: String,
    pub(super) start: String,
    pub(super) end: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) route_filter: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) method_filter: Option<String>,
    pub(super) totals: Vec<DailyHttpTotalRow>,
    pub(super) routes: Vec<DailyHttpRouteRow>,
}

#[utoipa::path(
    get,
    path = "/stats/daily/http",
    summary = "按天输出 HTTP 错误率（含总错误率）",
    description = "按天聚合所有 HTTP 请求（route+method）并计算错误率：overall(>=400)、4xx、5xx；同时给出每天总错误率与路由明细。",
    params(
        ("start" = String, Query, description = "开始日期 YYYY-MM-DD（按 timezone 解释）"),
        ("end" = String, Query, description = "结束日期 YYYY-MM-DD（按 timezone 解释）"),
        ("timezone" = Option<String>, Query, description = "可选时区 IANA 名称（覆盖配置）"),
        ("route" = Option<String>, Query, description = "可选路由模板过滤（MatchedPath）"),
        ("method" = Option<String>, Query, description = "可选 HTTP 方法过滤（GET/POST 等）"),
        ("top" = Option<i64>, Query, description = "每天最多返回的路由明细条数（默认 200，最多 200）")
    ),
    responses(
        (status = 200, description = "按天 HTTP 错误率", body = DailyHttpResponse),
        (
            status = 422,
            description = "参数校验失败（日期格式/timezone/top 等）",
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
    tag = "Stats"
)]
pub async fn get_daily_http(
    State(state): State<AppState>,
    Query(q): Query<DailyHttpQuery>,
) -> Result<Json<DailyHttpResponse>, AppError> {
    let start = parse_ymd(&q.start, "start")?;
    let end = parse_ymd(&q.end, "end")?;
    validate_date_range(start, end)?;

    let top = normalize_top_per_day(q.top)?;

    let cfg = crate::config::AppConfig::global();
    let (tz_name, tz) = resolve_timezone(cfg.stats.timezone.as_str(), q.timezone.as_deref())?;

    let storage = state
        .stats_storage
        .as_ref()
        .ok_or_else(|| AppError::Internal("统计存储未初始化".into()))?;

    let cache_key = build_daily_http_cache_key(
        storage,
        tz_name.as_str(),
        &q.start,
        &q.end,
        q.route.as_deref(),
        q.method.as_deref(),
        top,
    );
    if let Some(cached) = daily_http_cache().get(&cache_key).await {
        return Ok(Json((*cached).clone()));
    }

    let (totals, routes) = query_daily_http(
        storage,
        tz,
        start,
        end,
        q.route.as_deref(),
        q.method.as_deref(),
        top,
    )
    .await?;

    let resp = DailyHttpResponse {
        timezone: tz_name,
        start: q.start,
        end: q.end,
        route_filter: q.route,
        method_filter: q.method,
        totals,
        routes,
    };
    daily_http_cache()
        .insert(cache_key, Arc::new(resp.clone()))
        .await;
    Ok(Json(resp))
}
