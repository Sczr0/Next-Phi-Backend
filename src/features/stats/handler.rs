use axum::{
    Router,
    extract::{Query, State},
    response::Json,
    routing::{get, post},
};
use serde::{Deserialize, Serialize};

use crate::error::AppError;
use crate::state::AppState;

use super::models::DailyAggRow;

pub(crate) mod archive_now;
mod cache;
pub use cache::invalidate_all_stats_summary_cache;
pub(crate) mod daily_http;
pub(crate) mod latency;
mod params;
mod queries;
pub(crate) mod summary;
mod time;

pub use self::archive_now::{ArchiveNowResponse, ArchiveQuery, trigger_archive_now};
pub use self::daily_http::{
    DailyHttpQuery, DailyHttpResponse, DailyHttpRouteRow, DailyHttpTotalRow, get_daily_http,
};
pub use self::latency::{LatencyAggQuery, LatencyAggResponse, LatencyAggRow, get_latency_agg};
pub use self::summary::{
    ActionUsageSummary, FeatureUsageSummary, InstanceUsageSummary, LatencySummary,
    MethodUsageSummary, RouteUsageSummary, StatsSummaryQuery, StatsSummaryResponse,
    StatusCodeSummary, UniqueUsersSummary, get_stats_summary,
};
#[cfg(test)]
use params::{normalize_top, parse_include_flags};
use queries::{query_daily_agg, query_daily_dau, query_daily_feature_usage};
use time::{parse_ymd, resolve_timezone, validate_date_range};

#[derive(Deserialize)]
pub struct DailyQuery {
    start: String,
    end: String,
    /// 可选时区（IANA 名称，如 Asia/Shanghai），覆盖配置
    timezone: Option<String>,
    feature: Option<String>,
    route: Option<String>,
    method: Option<String>,
}

#[utoipa::path(
    get,
    path = "/stats/daily",
    summary = "按日聚合的统计数据",
    description = "在 SQLite 明细上进行区间聚合，返回每天每功能/路由的调用与错误次数汇总",
    params(
        ("start" = String, Query, description = "开始日期 YYYY-MM-DD"),
        ("end" = String, Query, description = "结束日期 YYYY-MM-DD"),
        ("timezone" = Option<String>, Query, description = "可选时区 IANA 名称（覆盖配置）"),
        ("feature" = Option<String>, Query, description = "可选功能名"),
        ("route" = Option<String>, Query, description = "可选路由模板（MatchedPath）"),
        ("method" = Option<String>, Query, description = "可选 HTTP 方法（GET/POST 等）")
    ),
    responses(
        (status = 200, description = "聚合结果", body = [DailyAggRow]),
        (
            status = 422,
            description = "参数校验失败（日期格式等）",
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
pub async fn get_daily_stats(
    State(state): State<AppState>,
    Query(q): Query<DailyQuery>,
) -> Result<Json<Vec<DailyAggRow>>, AppError> {
    let start = parse_ymd(&q.start, "start")?;
    let end = parse_ymd(&q.end, "end")?;
    validate_date_range(start, end)?;

    let cfg = crate::config::AppConfig::global();
    let (_, tz) = resolve_timezone(cfg.stats.timezone.as_str(), q.timezone.as_deref())?;
    let storage = state
        .stats_storage
        .as_ref()
        .ok_or_else(|| AppError::Internal("统计存储未初始化".into()))?;

    let rows = query_daily_agg(
        storage,
        tz,
        start,
        end,
        q.feature.as_deref(),
        q.route.as_deref(),
        q.method.as_deref(),
    )
    .await?;
    Ok(Json(rows))
}

#[derive(Deserialize)]
pub struct DailyFeaturesQuery {
    start: String,
    end: String,
    /// 可选时区（IANA 名称，如 Asia/Shanghai），覆盖配置
    timezone: Option<String>,
    /// 可选功能名过滤（bestn/save 等）
    feature: Option<String>,
}

#[derive(Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DailyFeatureUsageRow {
    /// 日期（按 timezone 输出）YYYY-MM-DD
    date: String,
    /// 功能名（bestn/save 等）
    feature: String,
    /// 使用次数
    count: i64,
    /// 当日唯一用户数（基于 user_hash 去敏标识；若事件未记录 user_hash，则不会计入）
    unique_users: i64,
}

#[derive(Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DailyFeaturesResponse {
    /// 展示统计的时区（IANA 名称）
    timezone: String,
    /// 查询开始日期（YYYY-MM-DD，按 timezone 解释）
    start: String,
    /// 查询结束日期（YYYY-MM-DD，按 timezone 解释）
    end: String,
    /// 可选功能过滤
    #[serde(skip_serializing_if = "Option::is_none")]
    feature_filter: Option<String>,
    rows: Vec<DailyFeatureUsageRow>,
}

#[utoipa::path(
    get,
    path = "/stats/daily/features",
    summary = "按天输出功能使用次数",
    description = "基于 stats 事件明细（feature/action）按天聚合，输出每天各功能的调用次数与当日唯一用户数（user_hash）。",
    params(
        ("start" = String, Query, description = "开始日期 YYYY-MM-DD（按 timezone 解释）"),
        ("end" = String, Query, description = "结束日期 YYYY-MM-DD（按 timezone 解释）"),
        ("timezone" = Option<String>, Query, description = "可选时区 IANA 名称（覆盖配置）"),
        ("feature" = Option<String>, Query, description = "可选功能名过滤（bestn/save 等）")
    ),
    responses(
        (status = 200, description = "按天功能使用次数", body = DailyFeaturesResponse),
        (
            status = 422,
            description = "参数校验失败（日期格式/timezone 等）",
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
pub async fn get_daily_features(
    State(state): State<AppState>,
    Query(q): Query<DailyFeaturesQuery>,
) -> Result<Json<DailyFeaturesResponse>, AppError> {
    let start = parse_ymd(&q.start, "start")?;
    let end = parse_ymd(&q.end, "end")?;
    validate_date_range(start, end)?;

    let cfg = crate::config::AppConfig::global();
    let (tz_name, tz) = resolve_timezone(cfg.stats.timezone.as_str(), q.timezone.as_deref())?;

    let storage = state
        .stats_storage
        .as_ref()
        .ok_or_else(|| AppError::Internal("统计存储未初始化".into()))?;

    let rows = query_daily_feature_usage(storage, tz, start, end, q.feature.as_deref()).await?;

    Ok(Json(DailyFeaturesResponse {
        timezone: tz_name,
        start: q.start,
        end: q.end,
        feature_filter: q.feature,
        rows,
    }))
}

#[derive(Deserialize)]
pub struct DailyDauQuery {
    start: String,
    end: String,
    /// 可选时区（IANA 名称，如 Asia/Shanghai），覆盖配置
    timezone: Option<String>,
}

#[derive(Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DailyDauRow {
    /// 日期（按 timezone 输出）YYYY-MM-DD
    date: String,
    /// 当日活跃用户数（distinct user_hash；仅统计能去敏识别的用户）
    active_users: i64,
    /// 当日活跃 IP 数（distinct client_ip_hash；用于覆盖匿名访问）
    active_ips: i64,
}

#[derive(Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DailyDauResponse {
    timezone: String,
    start: String,
    end: String,
    rows: Vec<DailyDauRow>,
}

#[utoipa::path(
    get,
    path = "/stats/daily/dau",
    summary = "按天输出 DAU（活跃用户数）",
    description = "按天聚合统计活跃用户数：active_users 基于去敏 user_hash；active_ips 基于去敏 client_ip_hash（HTTP 请求采集）。",
    params(
        ("start" = String, Query, description = "开始日期 YYYY-MM-DD（按 timezone 解释）"),
        ("end" = String, Query, description = "结束日期 YYYY-MM-DD（按 timezone 解释）"),
        ("timezone" = Option<String>, Query, description = "可选时区 IANA 名称（覆盖配置）")
    ),
    responses(
        (status = 200, description = "按天 DAU", body = DailyDauResponse),
        (
            status = 422,
            description = "参数校验失败（日期格式/timezone 等）",
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
pub async fn get_daily_dau(
    State(state): State<AppState>,
    Query(q): Query<DailyDauQuery>,
) -> Result<Json<DailyDauResponse>, AppError> {
    let start = parse_ymd(&q.start, "start")?;
    let end = parse_ymd(&q.end, "end")?;
    validate_date_range(start, end)?;

    let cfg = crate::config::AppConfig::global();
    let (tz_name, tz) = resolve_timezone(cfg.stats.timezone.as_str(), q.timezone.as_deref())?;

    let storage = state
        .stats_storage
        .as_ref()
        .ok_or_else(|| AppError::Internal("统计存储未初始化".into()))?;

    let rows = query_daily_dau(storage, tz, start, end).await?;

    Ok(Json(DailyDauResponse {
        timezone: tz_name,
        start: q.start,
        end: q.end,
        rows,
    }))
}

pub fn create_stats_router() -> Router<AppState> {
    Router::new()
        .route("/stats/daily", get(get_daily_stats))
        .route("/stats/daily/features", get(get_daily_features))
        .route("/stats/daily/dau", get(get_daily_dau))
        .route("/stats/daily/http", get(get_daily_http))
        .route("/stats/latency", get(get_latency_agg))
        .route("/stats/archive/now", post(trigger_archive_now))
        .route("/stats/summary", get(get_stats_summary))
}

#[cfg(test)]
mod tests;
