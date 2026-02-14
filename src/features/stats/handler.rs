use axum::{
    Router,
    extract::{Query, State},
    response::Json,
    routing::{get, post},
};
use chrono::{Datelike, LocalResult, NaiveDate, NaiveDateTime, NaiveTime, Offset, TimeZone, Utc};
use moka::future::Cache;
use serde::{Deserialize, Serialize};
use sqlx::{QueryBuilder, Row, Sqlite};
use std::{
    sync::{Arc, OnceLock},
    time::Duration,
};

use crate::error::AppError;
use crate::state::AppState;

use super::{archive::archive_one_day, models::DailyAggRow};

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

#[derive(Deserialize)]
pub struct DailyHttpQuery {
    start: String,
    end: String,
    /// 可选时区（IANA 名称，如 Asia/Shanghai），覆盖配置
    timezone: Option<String>,
    /// 可选路由模板过滤（MatchedPath）
    route: Option<String>,
    /// 可选 HTTP 方法过滤（GET/POST 等）
    method: Option<String>,
    /// 每天最多返回的路由明细条数（默认 200）
    top: Option<i64>,
}

#[derive(Serialize, utoipa::ToSchema, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DailyHttpTotalRow {
    date: String,
    total: i64,
    errors: i64,
    /// errors / total（total=0 时为 0）
    error_rate: f64,
    client_errors: i64,
    server_errors: i64,
    client_error_rate: f64,
    server_error_rate: f64,
}

#[derive(Serialize, utoipa::ToSchema, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DailyHttpRouteRow {
    date: String,
    route: String,
    method: String,
    total: i64,
    errors: i64,
    error_rate: f64,
    client_errors: i64,
    server_errors: i64,
    client_error_rate: f64,
    server_error_rate: f64,
}

#[derive(Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DailyHttpResponse {
    timezone: String,
    start: String,
    end: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    route_filter: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    method_filter: Option<String>,
    totals: Vec<DailyHttpTotalRow>,
    routes: Vec<DailyHttpRouteRow>,
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

    Ok(Json(DailyHttpResponse {
        timezone: tz_name,
        start: q.start,
        end: q.end,
        route_filter: q.route,
        method_filter: q.method,
        totals,
        routes,
    }))
}

#[derive(Deserialize)]
pub struct LatencyAggQuery {
    start: String,
    end: String,
    /// 可选时区（IANA 名称，如 Asia/Shanghai），覆盖配置
    timezone: Option<String>,
    /// 聚合粒度：day/week/month（默认 day）
    bucket: Option<String>,
    /// 可选功能名过滤（仅当事件写入了 feature 时生效）
    feature: Option<String>,
    /// 可选路由模板过滤（MatchedPath）
    route: Option<String>,
    /// 可选 HTTP 方法过滤（GET/POST 等）
    method: Option<String>,
}

#[derive(Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LatencyAggRow {
    /// bucket 标签：day=YYYY-MM-DD；week=week_start(YYYY-MM-DD)；month=month_start(YYYY-MM-01)
    bucket: String,
    /// 事件中的 feature（可为空）
    feature: Option<String>,
    /// 事件中的 route（MatchedPath）
    route: Option<String>,
    /// 事件中的 method（GET/POST 等）
    method: Option<String>,
    /// 样本数
    count: i64,
    /// 最小耗时（毫秒）
    min_ms: Option<i64>,
    /// 平均耗时（毫秒）
    avg_ms: Option<f64>,
    /// 最大耗时（毫秒）
    max_ms: Option<i64>,
}

#[derive(Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LatencyAggResponse {
    timezone: String,
    start: String,
    end: String,
    /// day/week/month
    bucket: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    feature_filter: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    route_filter: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    method_filter: Option<String>,
    rows: Vec<LatencyAggRow>,
}

#[utoipa::path(
    get,
    path = "/stats/latency",
    summary = "按天/周/月聚合各端点请求耗时（min/avg/max）",
    description = "基于 stats events 明细（route IS NOT NULL 且 duration_ms IS NOT NULL）按 bucket 聚合输出各端点（route+method，可选 feature）的耗时统计：min/avg/max + 样本数。",
    params(
        ("start" = String, Query, description = "开始日期 YYYY-MM-DD（按 timezone 解释）"),
        ("end" = String, Query, description = "结束日期 YYYY-MM-DD（按 timezone 解释）"),
        ("timezone" = Option<String>, Query, description = "可选时区 IANA 名称（覆盖配置）"),
        ("bucket" = Option<String>, Query, description = "聚合粒度：day/week/month（默认 day）"),
        ("feature" = Option<String>, Query, description = "可选 feature 过滤（仅当事件写入了 feature 时生效）"),
        ("route" = Option<String>, Query, description = "可选路由模板过滤（MatchedPath）"),
        ("method" = Option<String>, Query, description = "可选 HTTP 方法过滤（GET/POST 等）")
    ),
    responses(
        (status = 200, description = "聚合结果", body = LatencyAggResponse),
        (
            status = 422,
            description = "参数校验失败（日期格式、bucket、timezone 等）",
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
pub async fn get_latency_agg(
    State(state): State<AppState>,
    Query(q): Query<LatencyAggQuery>,
) -> Result<Json<LatencyAggResponse>, AppError> {
    let start = parse_ymd(&q.start, "start")?;
    let end = parse_ymd(&q.end, "end")?;
    validate_date_range(start, end)?;

    let bucket = parse_latency_bucket(q.bucket.as_deref())?;

    let cfg = crate::config::AppConfig::global();
    let (tz_name, tz) = resolve_timezone(cfg.stats.timezone.as_str(), q.timezone.as_deref())?;

    let storage = state
        .stats_storage
        .as_ref()
        .ok_or_else(|| AppError::Internal("统计存储未初始化".into()))?;

    let filters = LatencyAggFilters {
        feature: q.feature.as_deref(),
        route: q.route.as_deref(),
        method: q.method.as_deref(),
    };
    let rows = query_latency_agg(storage, tz, bucket, start, end, filters).await?;

    Ok(Json(LatencyAggResponse {
        timezone: tz_name,
        start: q.start,
        end: q.end,
        bucket: bucket.as_str().to_string(),
        feature_filter: q.feature,
        route_filter: q.route,
        method_filter: q.method,
        rows,
    }))
}

#[derive(Deserialize)]
pub struct ArchiveQuery {
    date: Option<String>,
}

#[derive(Serialize, utoipa::ToSchema)]
#[schema(example = json!({"ok": true, "date": "2025-12-23"}))]
#[serde(rename_all = "camelCase")]
pub struct ArchiveNowResponse {
    pub ok: bool,
    pub date: String,
}

#[utoipa::path(
    post,
    path = "/stats/archive/now",
    summary = "手动触发某日归档",
    description = "将指定日期（默认昨天）的明细导出为 Parquet 文件，落地到配置的归档目录",
    params(("date" = Option<String>, Query, description = "归档日期 YYYY-MM-DD，默认为昨天")),
    responses(
        (status = 200, description = "归档已触发", body = ArchiveNowResponse),
        (
            status = 422,
            description = "参数校验失败（日期格式等）",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        ),
        (
            status = 500,
            description = "统计存储未初始化/归档失败",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        )
    ),
    tag = "Stats"
)]
pub async fn trigger_archive_now(
    State(state): State<AppState>,
    Query(q): Query<ArchiveQuery>,
) -> Result<Json<ArchiveNowResponse>, AppError> {
    let day = if let Some(d) = q.date {
        chrono::NaiveDate::parse_from_str(&d, "%Y-%m-%d")
            .map_err(|e| AppError::Validation(format!("date 无效（期望 YYYY-MM-DD）: {e}")))?
    } else {
        (Utc::now() - chrono::Duration::days(1)).date_naive()
    };
    let storage = state
        .stats_storage
        .as_ref()
        .ok_or_else(|| AppError::Internal("统计存储未初始化".into()))?;
    archive_one_day(storage, &crate::config::StatsArchiveConfig::default(), day).await?;
    Ok(Json(ArchiveNowResponse {
        ok: true,
        date: day.to_string(),
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

#[derive(Clone, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct FeatureUsageSummary {
    /// 功能名（可能值：bestn、bestn_user、single_query、save、song_search）。
    /// - bestn：生成 BestN 汇总图
    /// - bestn_user：生成用户自报 BestN 图片
    /// - single_query：生成单曲成绩图
    /// - save：获取并解析玩家存档
    /// - song_search：歌曲检索
    feature: String,
    /// 事件计数
    count: i64,
    /// 最近一次发生时间（本地时区 RFC3339）
    last_at: Option<String>,
}

#[derive(Clone, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UniqueUsersSummary {
    /// 去敏后唯一用户总数
    total: i64,
    /// 按用户来源/凭证类型聚合的唯一用户数，例如 ("official", 123)
    by_kind: Vec<(String, i64)>,
}

#[derive(Deserialize)]
pub struct StatsSummaryQuery {
    /// 可选开始日期（YYYY-MM-DD，按 timezone 解释）
    start: Option<String>,
    /// 可选结束日期（YYYY-MM-DD，按 timezone 解释）
    end: Option<String>,
    /// 可选时区（IANA 名称，如 Asia/Shanghai），覆盖配置
    timezone: Option<String>,
    /// 可选功能名过滤（仅影响 feature/unique_users/actions 等业务维度）
    feature: Option<String>,
    /// 可选额外维度（csv）：routes,status,methods,instances,actions,latency,unique_ips,user_kinds,all
    include: Option<String>,
    /// TopN（默认 20，最大 200）
    top: Option<i64>,
}

#[derive(Clone, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RouteUsageSummary {
    route: String,
    count: i64,
    err_count: i64,
    last_at: Option<String>,
}

#[derive(Clone, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MethodUsageSummary {
    method: String,
    count: i64,
}

#[derive(Clone, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct StatusCodeSummary {
    status: u16,
    count: i64,
}

#[derive(Clone, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct InstanceUsageSummary {
    instance: String,
    count: i64,
    last_at: Option<String>,
}

#[derive(Clone, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ActionUsageSummary {
    feature: String,
    action: String,
    count: i64,
    last_at: Option<String>,
}

#[derive(Clone, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LatencySummary {
    sample_count: i64,
    avg_ms: Option<f64>,
    p50_ms: Option<i64>,
    p95_ms: Option<i64>,
    max_ms: Option<i64>,
}

#[derive(Clone, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct StatsSummaryResponse {
    /// 展示使用的时区（IANA 名称）
    timezone: String,
    /// 配置中设置的统计起始时间（如有）
    config_start_at: Option<String>,
    /// 全量事件中的最早时间（本地时区）
    first_event_at: Option<String>,
    /// 全量事件中的最晚时间（本地时区）
    last_event_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    range_start_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    range_end_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    feature_filter: Option<String>,
    /// 各功能使用概览
    features: Vec<FeatureUsageSummary>,
    /// 唯一用户统计
    unique_users: UniqueUsersSummary,
    #[serde(skip_serializing_if = "Option::is_none")]
    events_total: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    http_total: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    http_errors: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    routes: Option<Vec<RouteUsageSummary>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    methods: Option<Vec<MethodUsageSummary>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    status_codes: Option<Vec<StatusCodeSummary>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    instances: Option<Vec<InstanceUsageSummary>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    actions: Option<Vec<ActionUsageSummary>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    latency: Option<LatencySummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    unique_ips: Option<i64>,
}

#[utoipa::path(
    get,
    path = "/stats/summary",
    summary = "统计总览（唯一用户与功能使用）",
    description = "提供统计模块关键指标：全局首末事件时间、按功能的使用次数与最近时间、唯一用户总量及来源分布。\n\n功能次数统计中的功能名可能值：\n- bestn：生成 BestN 汇总图\n- bestn_user：生成用户自报 BestN 图片\n- single_query：生成单曲成绩图\n- save：获取并解析玩家存档\n- song_search：歌曲检索",
    params(
        ("start" = Option<String>, Query, description = "可选开始日期 YYYY-MM-DD（按 timezone 解释）"),
        ("end" = Option<String>, Query, description = "可选结束日期 YYYY-MM-DD（按 timezone 解释）"),
        ("timezone" = Option<String>, Query, description = "可选时区 IANA 名称（覆盖配置）"),
        ("feature" = Option<String>, Query, description = "可选功能名过滤（仅业务维度）"),
        ("include" = Option<String>, Query, description = "可选额外维度：routes,status,methods,instances,actions,latency,unique_ips,user_kinds,all"),
        ("top" = Option<i64>, Query, description = "TopN（默认 20，最大 200）")
    ),
    responses(
        (status = 200, description = "汇总信息", body = StatsSummaryResponse),
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
pub async fn get_stats_summary(
    State(state): State<AppState>,
    Query(q): Query<StatsSummaryQuery>,
) -> Result<Json<StatsSummaryResponse>, AppError> {
    let storage = state
        .stats_storage
        .as_ref()
        .ok_or_else(|| AppError::Internal("统计存储未初始化".into()))?;
    let cfg = crate::config::AppConfig::global();
    let (tz_name, tz) = resolve_timezone(cfg.stats.timezone.as_str(), q.timezone.as_deref())?;
    let start_utc = q
        .start
        .as_deref()
        .map(|s| parse_date_bound_utc(s, tz, false))
        .transpose()?;
    let end_utc = q
        .end
        .as_deref()
        .map(|s| parse_date_bound_utc(s, tz, true))
        .transpose()?;
    let range_start_at = start_utc.as_deref().and_then(|s| convert_tz(s, tz));
    let range_end_at = end_utc.as_deref().and_then(|s| convert_tz(s, tz));
    let feature_filter = q.feature.clone();
    let include = parse_include_flags(q.include.as_deref());
    let top = normalize_top(q.top)?;

    let cache_key = build_stats_summary_cache_key(
        storage,
        tz_name.as_str(),
        start_utc.as_deref(),
        end_utc.as_deref(),
        q.feature.as_deref(),
        include,
        top,
    );
    if let Some(cached) = stats_summary_cache().get(&cache_key).await {
        return Ok(Json((*cached).clone()));
    }

    let start_utc_ref = start_utc.as_deref();
    let end_utc_ref = end_utc.as_deref();
    let feature_ref = q.feature.as_deref();

    // overall first/last event
    let mut overall_qb = QueryBuilder::<Sqlite>::new(
        "SELECT MIN(ts_utc) as min_ts, MAX(ts_utc) as max_ts FROM events WHERE 1=1",
    );
    push_ts_range_filters(&mut overall_qb, start_utc_ref, end_utc_ref);
    let row = overall_qb
        .build()
        .fetch_one(&storage.pool)
        .await
        .map_err(|e| AppError::Internal(format!("summary overall: {e}")))?;
    let first_event_at = row
        .try_get::<String, _>("min_ts")
        .ok()
        .and_then(|s| convert_tz(&s, tz));
    let last_event_at = row
        .try_get::<String, _>("max_ts")
        .ok()
        .and_then(|s| convert_tz(&s, tz));

    // features usage
    let mut features_qb = QueryBuilder::<Sqlite>::new(
        "SELECT feature, COUNT(1) as cnt, MAX(ts_utc) as last_ts FROM events WHERE feature IS NOT NULL",
    );
    push_ts_range_filters(&mut features_qb, start_utc_ref, end_utc_ref);
    push_feature_filter(&mut features_qb, feature_ref);
    features_qb.push(" GROUP BY feature");
    let feat_rows = features_qb
        .build()
        .fetch_all(&storage.pool)
        .await
        .map_err(|e| AppError::Internal(format!("summary features: {e}")))?;
    let mut features = Vec::with_capacity(feat_rows.len());
    for r in feat_rows {
        let f: String = r.try_get("feature").unwrap_or_else(|_| "".into());
        let c: i64 = r.try_get("cnt").unwrap_or(0);
        let last: Option<String> = r.try_get("last_ts").ok();
        let last = last.and_then(|s| convert_tz(&s, tz));
        features.push(FeatureUsageSummary {
            feature: f,
            count: c,
            last_at: last,
        });
    }

    // unique users (total)
    let mut users_qb = QueryBuilder::<Sqlite>::new(
        "SELECT COUNT(DISTINCT user_hash) as total FROM events WHERE user_hash IS NOT NULL",
    );
    push_ts_range_filters(&mut users_qb, start_utc_ref, end_utc_ref);
    push_feature_filter(&mut users_qb, feature_ref);
    let row = users_qb
        .build()
        .fetch_one(&storage.pool)
        .await
        .map_err(|e| AppError::Internal(format!("summary users: {e}")))?;
    let total: i64 = row.try_get("total").unwrap_or(0);

    // by_kind 改为按需计算：仅在 include 包含 user_kinds/all 时才执行 JSON 解析
    let by_kind = if include.user_kinds {
        use std::collections::{HashMap, HashSet};

        const BATCH: i64 = 5000;
        let mut last_id: i64 = 0;
        let mut uniq: HashSet<(String, String)> = HashSet::new();
        loop {
            let mut by_kind_qb = QueryBuilder::<Sqlite>::new(
                "SELECT id, user_hash, extra_json FROM events WHERE user_hash IS NOT NULL AND extra_json IS NOT NULL",
            );
            by_kind_qb.push(" AND id > ").push_bind(last_id);
            push_ts_range_filters(&mut by_kind_qb, start_utc_ref, end_utc_ref);
            push_feature_filter(&mut by_kind_qb, feature_ref);
            by_kind_qb.push(" ORDER BY id ASC LIMIT ").push_bind(BATCH);
            let rows = by_kind_qb
                .build()
                .fetch_all(&storage.pool)
                .await
                .map_err(|e| AppError::Internal(format!("summary by_kind: {e}")))?;
            if rows.is_empty() {
                break;
            }
            let row_len = rows.len() as i64;
            for r in rows {
                let id: i64 = match r.try_get("id") {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                last_id = id.max(last_id);
                let uh: String = match r.try_get("user_hash") {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                let ej: String = match r.try_get("extra_json") {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(&ej)
                    && let Some(kind) = val.get("user_kind").and_then(|v| v.as_str())
                {
                    uniq.insert((uh, kind.to_string()));
                }
            }
            if row_len < BATCH {
                break;
            }
        }

        let mut by_kind_map: HashMap<String, i64> = HashMap::new();
        for (_, k) in uniq {
            *by_kind_map.entry(k).or_insert(0) += 1;
        }
        let mut by_kind: Vec<(String, i64)> = by_kind_map.into_iter().collect();
        by_kind.sort_by(|a, b| b.1.cmp(&a.1));
        by_kind
    } else {
        Vec::new()
    };

    let want_meta =
        q.start.is_some() || q.end.is_some() || q.feature.is_some() || q.include.is_some();

    let events_total = if want_meta || include.any() {
        let mut events_total_qb =
            QueryBuilder::<Sqlite>::new("SELECT COUNT(1) as total FROM events WHERE 1=1");
        push_ts_range_filters(&mut events_total_qb, start_utc_ref, end_utc_ref);
        let row = events_total_qb
            .build()
            .fetch_one(&storage.pool)
            .await
            .map_err(|e| AppError::Internal(format!("summary events_total: {e}")))?;
        Some(row.try_get::<i64, _>("total").unwrap_or(0))
    } else {
        None
    };

    let (http_total, http_errors) = if include.any_http() {
        let mut http_total_qb = QueryBuilder::<Sqlite>::new(
            "SELECT COUNT(1) as total, COALESCE(SUM(CASE WHEN status >= 400 THEN 1 ELSE 0 END), 0) as err FROM events WHERE route IS NOT NULL",
        );
        push_ts_range_filters(&mut http_total_qb, start_utc_ref, end_utc_ref);
        let row = http_total_qb
            .build()
            .fetch_one(&storage.pool)
            .await
            .map_err(|e| AppError::Internal(format!("summary http_total: {e}")))?;
        (
            Some(row.try_get::<i64, _>("total").unwrap_or(0)),
            Some(row.try_get::<i64, _>("err").unwrap_or(0)),
        )
    } else {
        (None, None)
    };

    let routes = if include.routes {
        let mut routes_qb = QueryBuilder::<Sqlite>::new(
            "SELECT route, COUNT(1) as cnt, COALESCE(SUM(CASE WHEN status >= 400 THEN 1 ELSE 0 END), 0) as err_cnt, MAX(ts_utc) as last_ts FROM events WHERE route IS NOT NULL",
        );
        push_ts_range_filters(&mut routes_qb, start_utc_ref, end_utc_ref);
        routes_qb
            .push(" GROUP BY route ORDER BY cnt DESC LIMIT ")
            .push_bind(top);
        let rows = routes_qb
            .build()
            .fetch_all(&storage.pool)
            .await
            .map_err(|e| AppError::Internal(format!("summary routes: {e}")))?;

        let mut out = Vec::with_capacity(rows.len());
        for r in rows {
            let route: String = r.try_get("route").unwrap_or_else(|_| "".into());
            let count: i64 = r.try_get("cnt").unwrap_or(0);
            let err_count: i64 = r.try_get("err_cnt").unwrap_or(0);
            let last: Option<String> = r.try_get("last_ts").ok();
            let last = last.and_then(|s| convert_tz(&s, tz));
            out.push(RouteUsageSummary {
                route,
                count,
                err_count,
                last_at: last,
            });
        }
        Some(out)
    } else {
        None
    };

    let methods = if include.methods {
        let mut methods_qb = QueryBuilder::<Sqlite>::new(
            "SELECT method, COUNT(1) as cnt FROM events WHERE route IS NOT NULL AND method IS NOT NULL",
        );
        push_ts_range_filters(&mut methods_qb, start_utc_ref, end_utc_ref);
        methods_qb
            .push(" GROUP BY method ORDER BY cnt DESC LIMIT ")
            .push_bind(top);
        let rows = methods_qb
            .build()
            .fetch_all(&storage.pool)
            .await
            .map_err(|e| AppError::Internal(format!("summary methods: {e}")))?;

        let mut out = Vec::with_capacity(rows.len());
        for r in rows {
            let method: String = r.try_get("method").unwrap_or_else(|_| "".into());
            let count: i64 = r.try_get("cnt").unwrap_or(0);
            out.push(MethodUsageSummary { method, count });
        }
        Some(out)
    } else {
        None
    };

    let status_codes = if include.status_codes {
        let mut status_codes_qb = QueryBuilder::<Sqlite>::new(
            "SELECT status, COUNT(1) as cnt FROM events WHERE route IS NOT NULL AND status IS NOT NULL",
        );
        push_ts_range_filters(&mut status_codes_qb, start_utc_ref, end_utc_ref);
        status_codes_qb
            .push(" GROUP BY status ORDER BY cnt DESC LIMIT ")
            .push_bind(top);
        let rows = status_codes_qb
            .build()
            .fetch_all(&storage.pool)
            .await
            .map_err(|e| AppError::Internal(format!("summary status: {e}")))?;

        let mut out = Vec::with_capacity(rows.len());
        for r in rows {
            let status: i64 = r.try_get("status").unwrap_or(0);
            if status < 0 || status > u16::MAX as i64 {
                continue;
            }
            let count: i64 = r.try_get("cnt").unwrap_or(0);
            out.push(StatusCodeSummary {
                status: status as u16,
                count,
            });
        }
        Some(out)
    } else {
        None
    };

    let instances = if include.instances {
        let mut instances_qb = QueryBuilder::<Sqlite>::new(
            "SELECT instance, COUNT(1) as cnt, MAX(ts_utc) as last_ts FROM events WHERE instance IS NOT NULL",
        );
        push_ts_range_filters(&mut instances_qb, start_utc_ref, end_utc_ref);
        instances_qb
            .push(" GROUP BY instance ORDER BY cnt DESC LIMIT ")
            .push_bind(top);
        let rows = instances_qb
            .build()
            .fetch_all(&storage.pool)
            .await
            .map_err(|e| AppError::Internal(format!("summary instances: {e}")))?;

        let mut out = Vec::with_capacity(rows.len());
        for r in rows {
            let instance: String = r.try_get("instance").unwrap_or_else(|_| "".into());
            let count: i64 = r.try_get("cnt").unwrap_or(0);
            let last: Option<String> = r.try_get("last_ts").ok();
            let last = last.and_then(|s| convert_tz(&s, tz));
            out.push(InstanceUsageSummary {
                instance,
                count,
                last_at: last,
            });
        }
        Some(out)
    } else {
        None
    };

    let actions = if include.actions {
        let mut actions_qb = QueryBuilder::<Sqlite>::new(
            "SELECT feature, action, COUNT(1) as cnt, MAX(ts_utc) as last_ts FROM events WHERE feature IS NOT NULL AND action IS NOT NULL",
        );
        push_ts_range_filters(&mut actions_qb, start_utc_ref, end_utc_ref);
        push_feature_filter(&mut actions_qb, feature_ref);
        actions_qb
            .push(" GROUP BY feature, action ORDER BY cnt DESC LIMIT ")
            .push_bind(top);
        let rows = actions_qb
            .build()
            .fetch_all(&storage.pool)
            .await
            .map_err(|e| AppError::Internal(format!("summary actions: {e}")))?;

        let mut out = Vec::with_capacity(rows.len());
        for r in rows {
            let feature: String = r.try_get("feature").unwrap_or_else(|_| "".into());
            let action: String = r.try_get("action").unwrap_or_else(|_| "".into());
            let count: i64 = r.try_get("cnt").unwrap_or(0);
            let last: Option<String> = r.try_get("last_ts").ok();
            let last = last.and_then(|s| convert_tz(&s, tz));
            out.push(ActionUsageSummary {
                feature,
                action,
                count,
                last_at: last,
            });
        }
        Some(out)
    } else {
        None
    };

    let latency = if include.latency {
        let mut latency_base_qb = QueryBuilder::<Sqlite>::new(
            "SELECT COUNT(duration_ms) as n, AVG(duration_ms) as avg, MAX(duration_ms) as max FROM events WHERE route IS NOT NULL AND duration_ms IS NOT NULL",
        );
        push_ts_range_filters(&mut latency_base_qb, start_utc_ref, end_utc_ref);
        let row = latency_base_qb
            .build()
            .fetch_one(&storage.pool)
            .await
            .map_err(|e| AppError::Internal(format!("summary latency base: {e}")))?;

        let n: i64 = row.try_get("n").unwrap_or(0);
        let avg_ms: Option<f64> = row.try_get("avg").ok();
        let max_ms: Option<i64> = row.try_get("max").ok();

        let (p50_ms, p95_ms) = if n > 0 {
            let p50_idx = (((n - 1) as f64) * 0.50).round() as i64;
            let p95_idx = (((n - 1) as f64) * 0.95).round() as i64;

            let pool = &storage.pool;
            let pick = |idx: i64| async move {
                let mut latency_pick_qb = QueryBuilder::<Sqlite>::new(
                    "SELECT duration_ms as v FROM events WHERE route IS NOT NULL AND duration_ms IS NOT NULL",
                );
                push_ts_range_filters(&mut latency_pick_qb, start_utc_ref, end_utc_ref);
                latency_pick_qb
                    .push(" ORDER BY duration_ms ASC LIMIT 1 OFFSET ")
                    .push_bind(idx);
                let r = latency_pick_qb
                    .build()
                    .fetch_optional(pool)
                    .await
                    .map_err(|e| AppError::Internal(format!("summary latency pick: {e}")))?;
                Ok::<Option<i64>, AppError>(r.and_then(|row| row.try_get::<i64, _>("v").ok()))
            };

            let p50 = pick(p50_idx).await?;
            let p95 = pick(p95_idx).await?;
            (p50, p95)
        } else {
            (None, None)
        };

        Some(LatencySummary {
            sample_count: n,
            avg_ms,
            p50_ms,
            p95_ms,
            max_ms,
        })
    } else {
        None
    };

    let unique_ips = if include.unique_ips {
        let mut unique_ips_qb = QueryBuilder::<Sqlite>::new(
            "SELECT COUNT(DISTINCT client_ip_hash) as cnt FROM events WHERE route IS NOT NULL AND client_ip_hash IS NOT NULL",
        );
        push_ts_range_filters(&mut unique_ips_qb, start_utc_ref, end_utc_ref);
        let row = unique_ips_qb
            .build()
            .fetch_one(&storage.pool)
            .await
            .map_err(|e| AppError::Internal(format!("summary unique_ips: {e}")))?;
        Some(row.try_get::<i64, _>("cnt").unwrap_or(0))
    } else {
        None
    };

    let resp = StatsSummaryResponse {
        timezone: tz_name,
        config_start_at: cfg.stats.start_at.clone(),
        first_event_at,
        last_event_at,
        range_start_at,
        range_end_at,
        feature_filter,
        features,
        unique_users: UniqueUsersSummary { total, by_kind },
        events_total,
        http_total,
        http_errors,
        routes,
        methods,
        status_codes,
        instances,
        actions,
        latency,
        unique_ips,
    };
    stats_summary_cache()
        .insert(cache_key, Arc::new(resp.clone()))
        .await;
    Ok(Json(resp))
}

fn convert_tz(ts_rfc3339: &str, tz: chrono_tz::Tz) -> Option<String> {
    let dt = chrono::DateTime::parse_from_rfc3339(ts_rfc3339).ok()?;
    let as_utc = dt.with_timezone(&chrono::Utc);
    Some(as_utc.with_timezone(&tz).to_rfc3339())
}

const STATS_SUMMARY_CACHE_MAX_ENTRIES: u64 = 256;
const STATS_SUMMARY_CACHE_TTL_SECS: u64 = 15;
const STATS_SUMMARY_CACHE_TTI_SECS: u64 = 10;

fn stats_summary_cache() -> &'static Cache<String, Arc<StatsSummaryResponse>> {
    static CACHE: OnceLock<Cache<String, Arc<StatsSummaryResponse>>> = OnceLock::new();
    CACHE.get_or_init(|| {
        Cache::builder()
            .max_capacity(STATS_SUMMARY_CACHE_MAX_ENTRIES)
            .time_to_live(Duration::from_secs(STATS_SUMMARY_CACHE_TTL_SECS))
            .time_to_idle(Duration::from_secs(STATS_SUMMARY_CACHE_TTI_SECS))
            .build()
    })
}

fn build_stats_summary_cache_key(
    storage: &Arc<crate::features::stats::storage::StatsStorage>,
    tz_name: &str,
    start_utc: Option<&str>,
    end_utc: Option<&str>,
    feature: Option<&str>,
    include: IncludeFlags,
    top: i64,
) -> String {
    let storage_key = Arc::as_ptr(storage) as usize;
    format!(
        "{storage_key}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}",
        tz_name,
        start_utc.unwrap_or(""),
        end_utc.unwrap_or(""),
        feature.unwrap_or(""),
        top,
        include.routes as u8,
        include.methods as u8,
        include.status_codes as u8,
        include.instances as u8,
        include.actions as u8,
        include.latency as u8,
        include.unique_ips as u8,
        include.user_kinds as u8,
    )
}

fn parse_ymd(s: &str, field: &str) -> Result<NaiveDate, AppError> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").map_err(|e| {
        AppError::Validation(format!("{field} 日期无效（期望 YYYY-MM-DD）: {s} ({e})"))
    })
}

fn validate_date_range(start: NaiveDate, end: NaiveDate) -> Result<(), AppError> {
    if end < start {
        return Err(AppError::Validation("end 不能早于 start".into()));
    }
    const MAX_DAYS: i64 = 366;
    let days = (end - start).num_days() + 1;
    if days > MAX_DAYS {
        return Err(AppError::Validation(format!(
            "日期范围过大：{days} 天（上限 {MAX_DAYS} 天）"
        )));
    }
    Ok(())
}

fn sqlite_minutes_modifier(offset_minutes: i32) -> String {
    format!("{offset_minutes:+} minutes")
}

fn fixed_offset_minutes_for_range(
    tz: chrono_tz::Tz,
    start: NaiveDate,
    end: NaiveDate,
) -> Option<i32> {
    let mut cur = start;
    let mut offset: Option<i32> = None;
    while cur <= end {
        // noon 一般不会处于 DST 的 ambiguous/none 区间，作为稳定采样点
        let local_noon = NaiveDateTime::new(cur, NaiveTime::from_hms_opt(12, 0, 0).unwrap());
        let dt = match tz.from_local_datetime(&local_noon) {
            LocalResult::Single(v) => v,
            LocalResult::Ambiguous(a, _) => a,
            LocalResult::None => tz.from_utc_datetime(&local_noon),
        };
        let off_secs = dt.offset().fix().local_minus_utc();
        let off_min = off_secs / 60;
        match offset {
            None => offset = Some(off_min),
            Some(prev) if prev == off_min => {}
            Some(_) => return None,
        }
        cur += chrono::Duration::days(1);
    }
    offset
}

fn rate(n: i64, d: i64) -> f64 {
    if d <= 0 { 0.0 } else { (n as f64) / (d as f64) }
}

fn normalize_top_per_day(top: Option<i64>) -> Result<i64, AppError> {
    const DEFAULT_TOP: i64 = 200;
    const MAX_TOP: i64 = 200;
    match top {
        None => Ok(DEFAULT_TOP),
        Some(v) if v <= 0 => Err(AppError::Validation("top 必须为正整数".into())),
        Some(v) => Ok(v.min(MAX_TOP)),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LatencyBucket {
    Day,
    Week,
    Month,
}

impl LatencyBucket {
    fn as_str(&self) -> &'static str {
        match self {
            LatencyBucket::Day => "day",
            LatencyBucket::Week => "week",
            LatencyBucket::Month => "month",
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct LatencyAggFilters<'a> {
    feature: Option<&'a str>,
    route: Option<&'a str>,
    method: Option<&'a str>,
}

fn parse_latency_bucket(s: Option<&str>) -> Result<LatencyBucket, AppError> {
    match s.unwrap_or("day") {
        "day" => Ok(LatencyBucket::Day),
        "week" => Ok(LatencyBucket::Week),
        "month" => Ok(LatencyBucket::Month),
        other => Err(AppError::Validation(format!(
            "bucket 无效（可选：day/week/month）：{other}"
        ))),
    }
}

#[derive(Debug, Clone)]
struct DateBucket {
    label: String,
    start: NaiveDate,
    end: NaiveDate,
}

fn week_start_monday(d: NaiveDate) -> NaiveDate {
    let delta = d.weekday().num_days_from_monday() as i64;
    d - chrono::Duration::days(delta)
}

fn month_start_day1(d: NaiveDate) -> NaiveDate {
    NaiveDate::from_ymd_opt(d.year(), d.month(), 1).expect("valid ymd")
}

fn next_month_start(d: NaiveDate) -> NaiveDate {
    let (y, m) = if d.month() == 12 {
        (d.year() + 1, 1)
    } else {
        (d.year(), d.month() + 1)
    };
    NaiveDate::from_ymd_opt(y, m, 1).expect("valid ymd")
}

async fn query_daily_agg(
    storage: &super::storage::StatsStorage,
    tz: chrono_tz::Tz,
    start: NaiveDate,
    end: NaiveDate,
    feature: Option<&str>,
    route: Option<&str>,
    method: Option<&str>,
) -> Result<Vec<DailyAggRow>, AppError> {
    let start_utc = parse_date_bound_utc(&start.to_string(), tz, false)?;
    let end_utc = parse_date_bound_utc(&end.to_string(), tz, true)?;

    if let Some(off_min) = fixed_offset_minutes_for_range(tz, start, end) {
        let modifier = sqlite_minutes_modifier(off_min);
        let rows = sqlx::query(
            r#"
            SELECT date(ts_utc, ?) as date,
                   feature,
                   route,
                   method,
                   COUNT(1) as count,
                   COALESCE(SUM(CASE WHEN status >= 400 THEN 1 ELSE 0 END), 0) as err_count
            FROM events
            WHERE ts_utc BETWEEN ? AND ?
              AND (? IS NULL OR feature = ?)
              AND (? IS NULL OR route = ?)
              AND (? IS NULL OR method = ?)
            GROUP BY date, feature, route, method
            ORDER BY date ASC
        "#,
        )
        .bind(modifier)
        .bind(&start_utc)
        .bind(&end_utc)
        .bind(feature)
        .bind(feature)
        .bind(route)
        .bind(route)
        .bind(method)
        .bind(method)
        .fetch_all(&storage.pool)
        .await
        .map_err(|e| AppError::Internal(format!("query daily: {e}")))?;

        let mut out = Vec::with_capacity(rows.len());
        for r in rows {
            out.push(DailyAggRow {
                date: r.get::<String, _>("date"),
                feature: r.try_get::<String, _>("feature").ok(),
                route: r.try_get::<String, _>("route").ok(),
                method: r.try_get::<String, _>("method").ok(),
                count: r.get::<i64, _>("count"),
                err_count: r.get::<i64, _>("err_count"),
            });
        }
        return Ok(out);
    }

    // DST（或 offset 不固定）fallback：按天窗口聚合，确保本地日期口径正确
    let mut out: Vec<DailyAggRow> = Vec::new();
    let mut cur = start;
    while cur <= end {
        let day_start = parse_date_bound_utc(&cur.to_string(), tz, false)?;
        let day_end = parse_date_bound_utc(&cur.to_string(), tz, true)?;
        let rows = sqlx::query(
            r#"
            SELECT feature,
                   route,
                   method,
                   COUNT(1) as count,
                   COALESCE(SUM(CASE WHEN status >= 400 THEN 1 ELSE 0 END), 0) as err_count
            FROM events
            WHERE ts_utc BETWEEN ? AND ?
              AND (? IS NULL OR feature = ?)
              AND (? IS NULL OR route = ?)
              AND (? IS NULL OR method = ?)
            GROUP BY feature, route, method
        "#,
        )
        .bind(&day_start)
        .bind(&day_end)
        .bind(feature)
        .bind(feature)
        .bind(route)
        .bind(route)
        .bind(method)
        .bind(method)
        .fetch_all(&storage.pool)
        .await
        .map_err(|e| AppError::Internal(format!("query daily (fallback): {e}")))?;

        for r in rows {
            out.push(DailyAggRow {
                date: cur.to_string(),
                feature: r.try_get::<String, _>("feature").ok(),
                route: r.try_get::<String, _>("route").ok(),
                method: r.try_get::<String, _>("method").ok(),
                count: r.get::<i64, _>("count"),
                err_count: r.get::<i64, _>("err_count"),
            });
        }
        cur += chrono::Duration::days(1);
    }

    // 保持与 fixed-offset 路径一致的输出顺序：按 date ASC
    out.sort_by(|a, b| a.date.cmp(&b.date));
    Ok(out)
}

async fn query_latency_agg(
    storage: &super::storage::StatsStorage,
    tz: chrono_tz::Tz,
    bucket: LatencyBucket,
    start: NaiveDate,
    end: NaiveDate,
    filters: LatencyAggFilters<'_>,
) -> Result<Vec<LatencyAggRow>, AppError> {
    let LatencyAggFilters {
        feature,
        route,
        method,
    } = filters;

    // 统计口径：只统计“请求返回耗时”事件
    // - route IS NOT NULL：来自 HTTP stats_middleware 的 MatchedPath
    // - duration_ms IS NOT NULL：有耗时样本
    const SQL_BUCKET: &str = r#"
        SELECT feature,
               route,
               method,
               COUNT(1) as count,
               MIN(duration_ms) as min_ms,
               AVG(duration_ms) as avg_ms,
               MAX(duration_ms) as max_ms
        FROM events
        WHERE route IS NOT NULL
          AND duration_ms IS NOT NULL
          AND ts_utc BETWEEN ? AND ?
          AND (? IS NULL OR feature = ?)
          AND (? IS NULL OR route = ?)
          AND (? IS NULL OR method = ?)
        GROUP BY feature, route, method
        ORDER BY route ASC, method ASC
    "#;

    match bucket {
        LatencyBucket::Day => {
            let start_utc = parse_date_bound_utc(&start.to_string(), tz, false)?;
            let end_utc = parse_date_bound_utc(&end.to_string(), tz, true)?;

            // fixed-offset（无 DST）优化：单 SQL 按天分组
            if let Some(off_min) = fixed_offset_minutes_for_range(tz, start, end) {
                let modifier = sqlite_minutes_modifier(off_min);
                let rows = sqlx::query(
                    r#"
                    SELECT date(ts_utc, ?) as bucket,
                           feature,
                           route,
                           method,
                           COUNT(1) as count,
                           MIN(duration_ms) as min_ms,
                           AVG(duration_ms) as avg_ms,
                           MAX(duration_ms) as max_ms
                    FROM events
                    WHERE route IS NOT NULL
                      AND duration_ms IS NOT NULL
                      AND ts_utc BETWEEN ? AND ?
                      AND (? IS NULL OR feature = ?)
                      AND (? IS NULL OR route = ?)
                      AND (? IS NULL OR method = ?)
                    GROUP BY bucket, feature, route, method
                    ORDER BY bucket ASC, route ASC, method ASC
                "#,
                )
                .bind(modifier)
                .bind(&start_utc)
                .bind(&end_utc)
                .bind(feature)
                .bind(feature)
                .bind(route)
                .bind(route)
                .bind(method)
                .bind(method)
                .fetch_all(&storage.pool)
                .await
                .map_err(|e| AppError::Internal(format!("latency agg day: {e}")))?;

                let mut out = Vec::with_capacity(rows.len());
                for r in rows {
                    out.push(LatencyAggRow {
                        bucket: r.get::<String, _>("bucket"),
                        feature: r.try_get::<String, _>("feature").ok(),
                        route: r.try_get::<String, _>("route").ok(),
                        method: r.try_get::<String, _>("method").ok(),
                        count: r.get::<i64, _>("count"),
                        min_ms: r.try_get::<i64, _>("min_ms").ok(),
                        avg_ms: r.try_get::<f64, _>("avg_ms").ok(),
                        max_ms: r.try_get::<i64, _>("max_ms").ok(),
                    });
                }
                return Ok(out);
            }

            // DST（或 offset 不固定）fallback：按天窗口查询
            let mut out: Vec<LatencyAggRow> = Vec::new();
            let mut cur = start;
            while cur <= end {
                let day_start = parse_date_bound_utc(&cur.to_string(), tz, false)?;
                let day_end = parse_date_bound_utc(&cur.to_string(), tz, true)?;
                let rows = sqlx::query(SQL_BUCKET)
                    .bind(&day_start)
                    .bind(&day_end)
                    .bind(feature)
                    .bind(feature)
                    .bind(route)
                    .bind(route)
                    .bind(method)
                    .bind(method)
                    .fetch_all(&storage.pool)
                    .await
                    .map_err(|e| AppError::Internal(format!("latency agg day (fallback): {e}")))?;

                for r in rows {
                    out.push(LatencyAggRow {
                        bucket: cur.to_string(),
                        feature: r.try_get::<String, _>("feature").ok(),
                        route: r.try_get::<String, _>("route").ok(),
                        method: r.try_get::<String, _>("method").ok(),
                        count: r.get::<i64, _>("count"),
                        min_ms: r.try_get::<i64, _>("min_ms").ok(),
                        avg_ms: r.try_get::<f64, _>("avg_ms").ok(),
                        max_ms: r.try_get::<i64, _>("max_ms").ok(),
                    });
                }
                cur += chrono::Duration::days(1);
            }

            out.sort_by(|a, b| {
                a.bucket
                    .cmp(&b.bucket)
                    .then_with(|| a.route.cmp(&b.route))
                    .then_with(|| a.method.cmp(&b.method))
                    .then_with(|| a.feature.cmp(&b.feature))
            });
            Ok(out)
        }
        LatencyBucket::Week => {
            let mut buckets: Vec<DateBucket> = Vec::new();
            let mut cur = week_start_monday(start);
            while cur <= end {
                let week_end = cur + chrono::Duration::days(6);
                let eff_start = cur.max(start);
                let eff_end = week_end.min(end);
                buckets.push(DateBucket {
                    label: cur.to_string(),
                    start: eff_start,
                    end: eff_end,
                });
                cur += chrono::Duration::days(7);
            }

            let mut out: Vec<LatencyAggRow> = Vec::new();
            for b in buckets {
                let start_utc = parse_date_bound_utc(&b.start.to_string(), tz, false)?;
                let end_utc = parse_date_bound_utc(&b.end.to_string(), tz, true)?;
                let rows = sqlx::query(SQL_BUCKET)
                    .bind(&start_utc)
                    .bind(&end_utc)
                    .bind(feature)
                    .bind(feature)
                    .bind(route)
                    .bind(route)
                    .bind(method)
                    .bind(method)
                    .fetch_all(&storage.pool)
                    .await
                    .map_err(|e| AppError::Internal(format!("latency agg week: {e}")))?;

                for r in rows {
                    out.push(LatencyAggRow {
                        bucket: b.label.clone(),
                        feature: r.try_get::<String, _>("feature").ok(),
                        route: r.try_get::<String, _>("route").ok(),
                        method: r.try_get::<String, _>("method").ok(),
                        count: r.get::<i64, _>("count"),
                        min_ms: r.try_get::<i64, _>("min_ms").ok(),
                        avg_ms: r.try_get::<f64, _>("avg_ms").ok(),
                        max_ms: r.try_get::<i64, _>("max_ms").ok(),
                    });
                }
            }

            out.sort_by(|a, b| {
                a.bucket
                    .cmp(&b.bucket)
                    .then_with(|| a.route.cmp(&b.route))
                    .then_with(|| a.method.cmp(&b.method))
                    .then_with(|| a.feature.cmp(&b.feature))
            });
            Ok(out)
        }
        LatencyBucket::Month => {
            let mut buckets: Vec<DateBucket> = Vec::new();
            let mut cur = month_start_day1(start);
            while cur <= end {
                let next = next_month_start(cur);
                let month_end = next - chrono::Duration::days(1);
                let eff_start = cur.max(start);
                let eff_end = month_end.min(end);
                buckets.push(DateBucket {
                    label: cur.to_string(),
                    start: eff_start,
                    end: eff_end,
                });
                cur = next;
            }

            let mut out: Vec<LatencyAggRow> = Vec::new();
            for b in buckets {
                let start_utc = parse_date_bound_utc(&b.start.to_string(), tz, false)?;
                let end_utc = parse_date_bound_utc(&b.end.to_string(), tz, true)?;
                let rows = sqlx::query(SQL_BUCKET)
                    .bind(&start_utc)
                    .bind(&end_utc)
                    .bind(feature)
                    .bind(feature)
                    .bind(route)
                    .bind(route)
                    .bind(method)
                    .bind(method)
                    .fetch_all(&storage.pool)
                    .await
                    .map_err(|e| AppError::Internal(format!("latency agg month: {e}")))?;

                for r in rows {
                    out.push(LatencyAggRow {
                        bucket: b.label.clone(),
                        feature: r.try_get::<String, _>("feature").ok(),
                        route: r.try_get::<String, _>("route").ok(),
                        method: r.try_get::<String, _>("method").ok(),
                        count: r.get::<i64, _>("count"),
                        min_ms: r.try_get::<i64, _>("min_ms").ok(),
                        avg_ms: r.try_get::<f64, _>("avg_ms").ok(),
                        max_ms: r.try_get::<i64, _>("max_ms").ok(),
                    });
                }
            }

            out.sort_by(|a, b| {
                a.bucket
                    .cmp(&b.bucket)
                    .then_with(|| a.route.cmp(&b.route))
                    .then_with(|| a.method.cmp(&b.method))
                    .then_with(|| a.feature.cmp(&b.feature))
            });
            Ok(out)
        }
    }
}

async fn query_daily_feature_usage(
    storage: &super::storage::StatsStorage,
    tz: chrono_tz::Tz,
    start: NaiveDate,
    end: NaiveDate,
    feature: Option<&str>,
) -> Result<Vec<DailyFeatureUsageRow>, AppError> {
    let start_utc = parse_date_bound_utc(&start.to_string(), tz, false)?;
    let end_utc = parse_date_bound_utc(&end.to_string(), tz, true)?;

    if let Some(off_min) = fixed_offset_minutes_for_range(tz, start, end) {
        let modifier = sqlite_minutes_modifier(off_min);
        let rows = sqlx::query(
            r#"
            SELECT date(ts_utc, ?) as date,
                   feature,
                   COUNT(1) as count,
                   COUNT(DISTINCT user_hash) as unique_users
            FROM events
            WHERE feature IS NOT NULL
              AND ts_utc BETWEEN ? AND ?
              AND (? IS NULL OR feature = ?)
            GROUP BY date, feature
            ORDER BY date ASC
        "#,
        )
        .bind(modifier)
        .bind(&start_utc)
        .bind(&end_utc)
        .bind(feature)
        .bind(feature)
        .fetch_all(&storage.pool)
        .await
        .map_err(|e| AppError::Internal(format!("daily features: {e}")))?;

        let mut out = Vec::with_capacity(rows.len());
        for r in rows {
            out.push(DailyFeatureUsageRow {
                date: r.get::<String, _>("date"),
                feature: r.get::<String, _>("feature"),
                count: r.get::<i64, _>("count"),
                unique_users: r.get::<i64, _>("unique_users"),
            });
        }
        return Ok(out);
    }

    let mut out = Vec::new();
    let mut cur = start;
    while cur <= end {
        let day_start = parse_date_bound_utc(&cur.to_string(), tz, false)?;
        let day_end = parse_date_bound_utc(&cur.to_string(), tz, true)?;
        let rows = sqlx::query(
            r#"
            SELECT feature,
                   COUNT(1) as count,
                   COUNT(DISTINCT user_hash) as unique_users
            FROM events
            WHERE feature IS NOT NULL
              AND ts_utc BETWEEN ? AND ?
              AND (? IS NULL OR feature = ?)
            GROUP BY feature
        "#,
        )
        .bind(&day_start)
        .bind(&day_end)
        .bind(feature)
        .bind(feature)
        .fetch_all(&storage.pool)
        .await
        .map_err(|e| AppError::Internal(format!("daily features (fallback): {e}")))?;

        for r in rows {
            out.push(DailyFeatureUsageRow {
                date: cur.to_string(),
                feature: r.get::<String, _>("feature"),
                count: r.get::<i64, _>("count"),
                unique_users: r.get::<i64, _>("unique_users"),
            });
        }
        cur += chrono::Duration::days(1);
    }

    out.sort_by(|a, b| a.date.cmp(&b.date));
    Ok(out)
}

async fn query_daily_dau(
    storage: &super::storage::StatsStorage,
    tz: chrono_tz::Tz,
    start: NaiveDate,
    end: NaiveDate,
) -> Result<Vec<DailyDauRow>, AppError> {
    use std::collections::HashMap;

    let start_utc = parse_date_bound_utc(&start.to_string(), tz, false)?;
    let end_utc = parse_date_bound_utc(&end.to_string(), tz, true)?;

    let mut map: HashMap<String, (i64, i64)> = HashMap::new();

    if let Some(off_min) = fixed_offset_minutes_for_range(tz, start, end) {
        let modifier = sqlite_minutes_modifier(off_min);
        let mut qb = QueryBuilder::<Sqlite>::new("SELECT date(ts_utc, ");
        qb.push_bind(modifier)
            .push(
                ") as date, COUNT(DISTINCT user_hash) as active_users, COUNT(DISTINCT client_ip_hash) as active_ips FROM events WHERE ts_utc BETWEEN ",
            )
            .push_bind(start_utc.clone())
            .push(" AND ")
            .push_bind(end_utc.clone())
            .push(" GROUP BY date ORDER BY date ASC");

        let rows = qb
            .build()
            .fetch_all(&storage.pool)
            .await
            .map_err(|e| AppError::Internal(format!("daily dau: {e}")))?;

        for r in rows {
            let date = r.get::<String, _>("date");
            let u = r.get::<i64, _>("active_users");
            let ip = r.get::<i64, _>("active_ips");
            map.insert(date, (u, ip));
        }
    } else {
        // DST fallback: per-day query
        let mut cur = start;
        while cur <= end {
            let day_start = parse_date_bound_utc(&cur.to_string(), tz, false)?;
            let day_end = parse_date_bound_utc(&cur.to_string(), tz, true)?;
            let r = sqlx::query(
                r#"
                SELECT COUNT(DISTINCT user_hash) as active_users,
                       COUNT(DISTINCT client_ip_hash) as active_ips
                FROM events
                WHERE ts_utc BETWEEN ? AND ?
            "#,
            )
            .bind(&day_start)
            .bind(&day_end)
            .fetch_one(&storage.pool)
            .await
            .map_err(|e| AppError::Internal(format!("daily dau (fallback): {e}")))?;
            let u = r.get::<i64, _>("active_users");
            let ip = r.get::<i64, _>("active_ips");
            map.insert(cur.to_string(), (u, ip));
            cur += chrono::Duration::days(1);
        }
    }

    // 兜底补齐：确保 start..end 每天都有一条记录（无数据则为 0）
    let mut out = Vec::new();
    let mut cur = start;
    while cur <= end {
        let key = cur.to_string();
        let (u, ip) = map.get(&key).copied().unwrap_or((0, 0));
        out.push(DailyDauRow {
            date: key,
            active_users: u,
            active_ips: ip,
        });
        cur += chrono::Duration::days(1);
    }
    Ok(out)
}

async fn query_daily_http(
    storage: &super::storage::StatsStorage,
    tz: chrono_tz::Tz,
    start: NaiveDate,
    end: NaiveDate,
    route: Option<&str>,
    method: Option<&str>,
    top_per_day: i64,
) -> Result<(Vec<DailyHttpTotalRow>, Vec<DailyHttpRouteRow>), AppError> {
    use std::collections::HashMap;

    let start_utc = parse_date_bound_utc(&start.to_string(), tz, false)?;
    let end_utc = parse_date_bound_utc(&end.to_string(), tz, true)?;
    let fixed_offset = fixed_offset_minutes_for_range(tz, start, end);

    let mut route_rows: Vec<DailyHttpRouteRow> = Vec::new();
    let mut totals_map: HashMap<String, (i64, i64, i64, i64)> = HashMap::new();

    if let Some(off_min) = fixed_offset {
        let modifier = sqlite_minutes_modifier(off_min);
        let modifier_ref = modifier.as_str();
        let rows = if top_per_day > 0 {
            let mut qb = QueryBuilder::<Sqlite>::new(
                r#"
                SELECT date, route, method, total, errors, client_errors, server_errors
                FROM (
                    SELECT date(ts_utc, 
                "#,
            );
            qb.push_bind(modifier_ref)
                .push(
                    r#") as date,
                           route,
                           method,
                           COUNT(1) as total,
                           COALESCE(SUM(CASE WHEN status >= 400 THEN 1 ELSE 0 END), 0) as errors,
                           COALESCE(SUM(CASE WHEN status BETWEEN 400 AND 499 THEN 1 ELSE 0 END), 0) as client_errors,
                           COALESCE(SUM(CASE WHEN status >= 500 THEN 1 ELSE 0 END), 0) as server_errors,
                           ROW_NUMBER() OVER (
                               PARTITION BY date(ts_utc, "#,
                )
                .push_bind(modifier_ref)
                .push(
                    r#")
                               ORDER BY
                                   COALESCE(SUM(CASE WHEN status >= 400 THEN 1 ELSE 0 END), 0) DESC,
                                   COUNT(1) DESC,
                                   route ASC,
                                   method ASC
                           ) as rn
                    FROM events
                    WHERE route IS NOT NULL
                      AND status IS NOT NULL
                      AND ts_utc BETWEEN
                "#,
                )
                .push_bind(start_utc.clone())
                .push(" AND ")
                .push_bind(end_utc.clone());

            if let Some(route) = route {
                qb.push(" AND route = ").push_bind(route.to_string());
            }
            if let Some(method) = method {
                qb.push(" AND method = ").push_bind(method.to_string());
            }

            qb.push(
                r#"
                    GROUP BY date(ts_utc, "#,
            )
            .push_bind(modifier_ref)
            .push(
                r#"), route, method
                ) ranked
                WHERE rn <=
                "#,
            )
            .push_bind(top_per_day)
            .push(" ORDER BY date ASC, errors DESC, total DESC, route ASC, method ASC");
            qb.build()
                .fetch_all(&storage.pool)
                .await
                .map_err(|e| AppError::Internal(format!("daily http: {e}")))?
        } else {
            let mut qb = QueryBuilder::<Sqlite>::new(
                r#"
                SELECT date(ts_utc, 
                "#,
            );
            qb.push_bind(modifier_ref)
                .push(
                    r#") as date,
                       route,
                       method,
                       COUNT(1) as total,
                       COALESCE(SUM(CASE WHEN status >= 400 THEN 1 ELSE 0 END), 0) as errors,
                       COALESCE(SUM(CASE WHEN status BETWEEN 400 AND 499 THEN 1 ELSE 0 END), 0) as client_errors,
                       COALESCE(SUM(CASE WHEN status >= 500 THEN 1 ELSE 0 END), 0) as server_errors
                FROM events
                WHERE route IS NOT NULL
                  AND status IS NOT NULL
                  AND ts_utc BETWEEN
                "#,
                )
                .push_bind(start_utc.clone())
                .push(" AND ")
                .push_bind(end_utc.clone());

            if let Some(route) = route {
                qb.push(" AND route = ").push_bind(route.to_string());
            }
            if let Some(method) = method {
                qb.push(" AND method = ").push_bind(method.to_string());
            }

            qb.push(
                r#"
                GROUP BY date(ts_utc, "#,
            )
            .push_bind(modifier_ref)
            .push(
                r#"), route, method
                ORDER BY date ASC
            "#,
            );

            qb.build()
                .fetch_all(&storage.pool)
                .await
                .map_err(|e| AppError::Internal(format!("daily http: {e}")))?
        };

        route_rows.reserve(rows.len());
        for r in rows {
            let date = r.get::<String, _>("date");
            let route = r.get::<String, _>("route");
            let method = r.get::<String, _>("method");
            let total = r.get::<i64, _>("total");
            let errors = r.get::<i64, _>("errors");
            let client_errors = r.get::<i64, _>("client_errors");
            let server_errors = r.get::<i64, _>("server_errors");
            route_rows.push(DailyHttpRouteRow {
                date,
                route,
                method,
                total,
                errors,
                error_rate: rate(errors, total),
                client_errors,
                server_errors,
                client_error_rate: rate(client_errors, total),
                server_error_rate: rate(server_errors, total),
            });
        }

        let mut totals_qb = QueryBuilder::<Sqlite>::new("SELECT date(ts_utc, ");
        totals_qb
            .push_bind(modifier_ref)
            .push(
                ") as date, COUNT(1) as total, COALESCE(SUM(CASE WHEN status >= 400 THEN 1 ELSE 0 END), 0) as errors, COALESCE(SUM(CASE WHEN status BETWEEN 400 AND 499 THEN 1 ELSE 0 END), 0) as client_errors, COALESCE(SUM(CASE WHEN status >= 500 THEN 1 ELSE 0 END), 0) as server_errors FROM events WHERE route IS NOT NULL AND status IS NOT NULL AND ts_utc BETWEEN ",
            )
            .push_bind(start_utc.clone())
            .push(" AND ")
            .push_bind(end_utc.clone());

        if let Some(route) = route {
            totals_qb.push(" AND route = ").push_bind(route.to_string());
        }
        if let Some(method) = method {
            totals_qb
                .push(" AND method = ")
                .push_bind(method.to_string());
        }

        totals_qb
            .push(" GROUP BY date(ts_utc, ")
            .push_bind(modifier_ref)
            .push(") ORDER BY date ASC");

        let total_rows = totals_qb
            .build()
            .fetch_all(&storage.pool)
            .await
            .map_err(|e| AppError::Internal(format!("daily http totals: {e}")))?;

        for r in total_rows {
            let date = r.get::<String, _>("date");
            let total = r.get::<i64, _>("total");
            let errors = r.get::<i64, _>("errors");
            let client_errors = r.get::<i64, _>("client_errors");
            let server_errors = r.get::<i64, _>("server_errors");
            totals_map.insert(date, (total, errors, client_errors, server_errors));
        }
    } else {
        // DST fallback: per-day query
        let mut cur = start;
        while cur <= end {
            let day_start = parse_date_bound_utc(&cur.to_string(), tz, false)?;
            let day_end = parse_date_bound_utc(&cur.to_string(), tz, true)?;
            let rows = sqlx::query(
                r#"
                SELECT route,
                       method,
                       COUNT(1) as total,
                       COALESCE(SUM(CASE WHEN status >= 400 THEN 1 ELSE 0 END), 0) as errors,
                       COALESCE(SUM(CASE WHEN status BETWEEN 400 AND 499 THEN 1 ELSE 0 END), 0) as client_errors,
                       COALESCE(SUM(CASE WHEN status >= 500 THEN 1 ELSE 0 END), 0) as server_errors
                FROM events
                WHERE route IS NOT NULL
                  AND status IS NOT NULL
                  AND ts_utc BETWEEN ? AND ?
                  AND (? IS NULL OR route = ?)
                  AND (? IS NULL OR method = ?)
                GROUP BY route, method
            "#,
            )
            .bind(&day_start)
            .bind(&day_end)
            .bind(route)
            .bind(route)
            .bind(method)
            .bind(method)
            .fetch_all(&storage.pool)
            .await
            .map_err(|e| AppError::Internal(format!("daily http (fallback): {e}")))?;

            for r in rows {
                let route = r.get::<String, _>("route");
                let method = r.get::<String, _>("method");
                let total = r.get::<i64, _>("total");
                let errors = r.get::<i64, _>("errors");
                let client_errors = r.get::<i64, _>("client_errors");
                let server_errors = r.get::<i64, _>("server_errors");
                route_rows.push(DailyHttpRouteRow {
                    date: cur.to_string(),
                    route,
                    method,
                    total,
                    errors,
                    error_rate: rate(errors, total),
                    client_errors,
                    server_errors,
                    client_error_rate: rate(client_errors, total),
                    server_error_rate: rate(server_errors, total),
                });
            }

            cur += chrono::Duration::days(1);
        }

        route_rows.sort_by(|a, b| a.date.cmp(&b.date));
    }

    // totals: sum(route rows) per day, and fill missing days with 0
    if fixed_offset.is_none() {
        for r in route_rows.iter() {
            let e = totals_map.entry(r.date.clone()).or_insert((0, 0, 0, 0));
            e.0 += r.total;
            e.1 += r.errors;
            e.2 += r.client_errors;
            e.3 += r.server_errors;
        }
    }

    let mut totals: Vec<DailyHttpTotalRow> = Vec::new();
    let mut cur = start;
    while cur <= end {
        let key = cur.to_string();
        let (total, errors, client_errors, server_errors) =
            totals_map.get(&key).copied().unwrap_or((0, 0, 0, 0));
        totals.push(DailyHttpTotalRow {
            date: key,
            total,
            errors,
            error_rate: rate(errors, total),
            client_errors,
            server_errors,
            client_error_rate: rate(client_errors, total),
            server_error_rate: rate(server_errors, total),
        });
        cur += chrono::Duration::days(1);
    }

    // DST fallback 的 per-day top limit 在内存应用（fixed-offset 路径已在 SQL 下推）
    if top_per_day > 0 && fixed_offset.is_none() {
        let mut grouped: HashMap<String, Vec<DailyHttpRouteRow>> = HashMap::new();
        for r in route_rows.into_iter() {
            grouped.entry(r.date.clone()).or_default().push(r);
        }

        let mut dates: Vec<String> = grouped.keys().cloned().collect();
        dates.sort();

        let mut out_routes: Vec<DailyHttpRouteRow> = Vec::new();
        for d in dates {
            if let Some(mut v) = grouped.remove(&d) {
                v.sort_by(|a, b| {
                    b.errors
                        .cmp(&a.errors)
                        .then(b.total.cmp(&a.total))
                        .then(a.route.cmp(&b.route))
                        .then(a.method.cmp(&b.method))
                });
                v.truncate(top_per_day as usize);
                // 保持输出按 date ASC，再按排序 key
                out_routes.extend(v);
            }
        }
        return Ok((totals, out_routes));
    }

    Ok((totals, route_rows))
}

#[derive(Default, Clone, Copy)]
struct IncludeFlags {
    routes: bool,
    methods: bool,
    status_codes: bool,
    instances: bool,
    actions: bool,
    latency: bool,
    unique_ips: bool,
    user_kinds: bool,
}

impl IncludeFlags {
    fn any(self) -> bool {
        self.routes
            || self.methods
            || self.status_codes
            || self.instances
            || self.actions
            || self.latency
            || self.unique_ips
            || self.user_kinds
    }

    fn any_http(self) -> bool {
        self.routes || self.methods || self.status_codes || self.latency || self.unique_ips
    }
}

fn push_ts_range_filters(
    qb: &mut QueryBuilder<'_, Sqlite>,
    start_utc: Option<&str>,
    end_utc: Option<&str>,
) {
    if let Some(start) = start_utc {
        qb.push(" AND ts_utc >= ").push_bind(start.to_string());
    }
    if let Some(end) = end_utc {
        qb.push(" AND ts_utc <= ").push_bind(end.to_string());
    }
}

fn push_feature_filter(qb: &mut QueryBuilder<'_, Sqlite>, feature: Option<&str>) {
    if let Some(feature) = feature {
        qb.push(" AND feature = ").push_bind(feature.to_string());
    }
}

fn parse_include_flags(include: Option<&str>) -> IncludeFlags {
    let Some(s) = include else {
        return IncludeFlags::default();
    };

    let mut flags = IncludeFlags::default();
    for raw in s.split([',', ';', ' ', '\t', '\n']) {
        let t = raw.trim().to_ascii_lowercase();
        if t.is_empty() {
            continue;
        }
        if t == "all" {
            return IncludeFlags {
                routes: true,
                methods: true,
                status_codes: true,
                instances: true,
                actions: true,
                latency: true,
                unique_ips: true,
                user_kinds: true,
            };
        }
        match t.as_str() {
            "routes" | "route" => flags.routes = true,
            "methods" | "method" => flags.methods = true,
            "status" | "statuses" | "status_codes" | "statuscodes" => flags.status_codes = true,
            "instances" | "instance" => flags.instances = true,
            "actions" | "action" => flags.actions = true,
            "latency" => flags.latency = true,
            "unique_ips" | "uniqueip" | "uniqueips" | "ips" => flags.unique_ips = true,
            "user_kinds" | "userkind" | "userkinds" | "kinds" | "by_kind" => {
                flags.user_kinds = true
            }
            _ => {}
        }
    }
    flags
}

fn normalize_top(top: Option<i64>) -> Result<i64, AppError> {
    const DEFAULT_TOP: i64 = 20;
    const MAX_TOP: i64 = 200;
    match top {
        None => Ok(DEFAULT_TOP),
        Some(v) if v <= 0 => Err(AppError::Validation("top 必须为正整数".into())),
        Some(v) => Ok(v.min(MAX_TOP)),
    }
}

fn resolve_timezone(
    config_tz: &str,
    query_tz: Option<&str>,
) -> Result<(String, chrono_tz::Tz), AppError> {
    if let Some(name) = query_tz {
        let tz = name
            .parse::<chrono_tz::Tz>()
            .map_err(|_| AppError::Validation(format!("timezone 无效: {name}")))?;
        return Ok((name.to_string(), tz));
    }
    match config_tz.parse::<chrono_tz::Tz>() {
        Ok(tz) => Ok((config_tz.to_string(), tz)),
        Err(_) => Ok(("Asia/Shanghai".to_string(), chrono_tz::Asia::Shanghai)),
    }
}

fn parse_date_bound_utc(
    date_ymd: &str,
    tz: chrono_tz::Tz,
    is_end: bool,
) -> Result<String, AppError> {
    let date = NaiveDate::parse_from_str(date_ymd, "%Y-%m-%d").map_err(|e| {
        AppError::Validation(format!("日期无效（期望 YYYY-MM-DD）: {date_ymd} ({e})"))
    })?;
    let time = if is_end {
        NaiveTime::from_hms_opt(23, 59, 59).unwrap()
    } else {
        NaiveTime::from_hms_opt(0, 0, 0).unwrap()
    };
    let ndt = NaiveDateTime::new(date, time);

    let local = match tz.from_local_datetime(&ndt) {
        LocalResult::Single(v) => v,
        LocalResult::Ambiguous(a, b) => {
            if is_end {
                b
            } else {
                a
            }
        }
        LocalResult::None => chrono::Utc.from_utc_datetime(&ndt).with_timezone(&tz),
    };
    Ok(local.with_timezone(&chrono::Utc).to_rfc3339())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::auth::client::TapTapClient;
    use crate::features::auth::qrcode_service::QrCodeService;
    use crate::features::song::models::SongCatalog;
    use crate::features::stats::models::EventInsert;
    use crate::startup::chart_loader::ChartConstantsMap;
    use axum::body::Bytes;
    use moka::future::Cache;
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::time::{SystemTime, UNIX_EPOCH};
    use tokio::sync::Semaphore;

    fn init_config_for_test() {
        let _ = crate::config::AppConfig::init_global();
    }

    fn tmp_sqlite_path(prefix: &str) -> String {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        std::env::temp_dir()
            .join(format!("phi_backend_{prefix}_{ts}.db"))
            .to_string_lossy()
            .to_string()
    }

    async fn build_test_state(sqlite_path: &str) -> AppState {
        init_config_for_test();

        let storage = Arc::new(
            super::super::storage::StatsStorage::connect_sqlite(sqlite_path, false)
                .await
                .unwrap(),
        );
        storage.init_schema().await.unwrap();

        let chart_constants: ChartConstantsMap = HashMap::new();
        let song_catalog = SongCatalog::default();
        let taptap_client =
            Arc::new(TapTapClient::new(&crate::config::AppConfig::global().taptap).unwrap());
        let qrcode_service = Arc::new(QrCodeService::default());

        let bn_image_cache: Cache<String, Bytes> = Cache::builder().max_capacity(10).build();
        let song_image_cache: Cache<String, Bytes> = Cache::builder().max_capacity(10).build();

        AppState {
            chart_constants: Arc::new(chart_constants),
            song_catalog: Arc::new(song_catalog),
            taptap_client,
            qrcode_service,
            stats: None,
            stats_storage: Some(storage),
            render_semaphore: Arc::new(Semaphore::new(1)),
            bn_image_cache,
            song_image_cache,
        }
    }

    fn dt_utc(y: i32, m: u32, d: u32, hh: u32, mm: u32, ss: u32) -> chrono::DateTime<Utc> {
        Utc.with_ymd_and_hms(y, m, d, hh, mm, ss).single().unwrap()
    }

    #[test]
    fn include_flags_parse_all_and_partials() {
        let a = parse_include_flags(Some("all"));
        assert!(a.any());
        assert!(a.any_http());
        assert!(a.user_kinds);

        let b = parse_include_flags(Some("routes, latency,unique_ips"));
        assert!(b.routes);
        assert!(b.latency);
        assert!(b.unique_ips);
        assert!(!b.user_kinds);
        assert!(!b.actions);
        assert!(b.any_http());

        let c = parse_include_flags(Some("user_kinds"));
        assert!(c.user_kinds);
        assert!(!c.any_http());
    }

    #[test]
    fn normalize_top_limits_and_rejects_invalid() {
        assert_eq!(normalize_top(None).unwrap(), 20);
        assert_eq!(normalize_top(Some(200)).unwrap(), 200);
        assert_eq!(normalize_top(Some(999)).unwrap(), 200);
        assert!(normalize_top(Some(0)).is_err());
        assert!(normalize_top(Some(-1)).is_err());
    }

    #[test]
    fn parse_date_bound_utc_uses_timezone() {
        let tz: chrono_tz::Tz = "Asia/Shanghai".parse().unwrap();
        assert_eq!(
            parse_date_bound_utc("2025-12-25", tz, false).unwrap(),
            "2025-12-24T16:00:00+00:00"
        );
        assert_eq!(
            parse_date_bound_utc("2025-12-25", tz, true).unwrap(),
            "2025-12-25T15:59:59+00:00"
        );
    }

    #[tokio::test]
    async fn stats_summary_supports_include_and_filters() {
        let sqlite_path = tmp_sqlite_path("stats_summary");
        let state = build_test_state(&sqlite_path).await;
        let storage = state.stats_storage.as_ref().unwrap().clone();

        // 3 个 HTTP 请求事件（2 个路由，带状态与耗时）
        storage
            .insert_events(&[
                EventInsert {
                    ts_utc: dt_utc(2025, 12, 24, 0, 0, 1),
                    route: Some("/image/bn".into()),
                    feature: None,
                    action: None,
                    method: Some("GET".into()),
                    status: Some(200),
                    duration_ms: Some(50),
                    user_hash: None,
                    client_ip_hash: Some("ip1".into()),
                    instance: Some("inst-a".into()),
                    extra_json: None,
                },
                EventInsert {
                    ts_utc: dt_utc(2025, 12, 24, 0, 0, 2),
                    route: Some("/image/bn".into()),
                    feature: None,
                    action: None,
                    method: Some("GET".into()),
                    status: Some(500),
                    duration_ms: Some(100),
                    user_hash: None,
                    client_ip_hash: Some("ip1".into()),
                    instance: Some("inst-a".into()),
                    extra_json: None,
                },
                EventInsert {
                    ts_utc: dt_utc(2025, 12, 24, 0, 0, 3),
                    route: Some("/song/search".into()),
                    feature: None,
                    action: None,
                    method: Some("GET".into()),
                    status: Some(200),
                    duration_ms: Some(200),
                    user_hash: None,
                    client_ip_hash: Some("ip2".into()),
                    instance: Some("inst-b".into()),
                    extra_json: None,
                },
                // 2 个业务打点事件（2 个用户）
                EventInsert {
                    ts_utc: dt_utc(2025, 12, 24, 0, 1, 0),
                    route: None,
                    feature: Some("bestn".into()),
                    action: Some("render".into()),
                    method: None,
                    status: None,
                    duration_ms: None,
                    user_hash: Some("u1".into()),
                    client_ip_hash: None,
                    instance: Some("inst-a".into()),
                    extra_json: Some(serde_json::json!({"user_kind":"official"})),
                },
                EventInsert {
                    ts_utc: dt_utc(2025, 12, 24, 0, 1, 1),
                    route: None,
                    feature: Some("save".into()),
                    action: Some("submit".into()),
                    method: None,
                    status: None,
                    duration_ms: None,
                    user_hash: Some("u2".into()),
                    client_ip_hash: None,
                    instance: Some("inst-b".into()),
                    extra_json: Some(serde_json::json!({"user_kind":"taptap"})),
                },
            ])
            .await
            .unwrap();

        let query = StatsSummaryQuery {
            start: None,
            end: None,
            timezone: Some("Asia/Shanghai".into()),
            feature: None,
            include: Some("all".into()),
            top: Some(50),
        };
        let Json(resp) = get_stats_summary(State(state.clone()), Query(query))
            .await
            .unwrap();

        assert_eq!(resp.timezone, "Asia/Shanghai");
        assert_eq!(resp.events_total, Some(5));
        assert_eq!(resp.http_total, Some(3));
        assert_eq!(resp.http_errors, Some(1));

        assert_eq!(resp.unique_users.total, 2);
        assert_eq!(resp.unique_users.by_kind.len(), 2);

        assert!(resp.routes.as_ref().is_some_and(|v| !v.is_empty()));
        assert!(resp.methods.as_ref().is_some_and(|v| !v.is_empty()));
        assert!(resp.status_codes.as_ref().is_some_and(|v| !v.is_empty()));
        assert!(resp.instances.as_ref().is_some_and(|v| !v.is_empty()));
        assert!(resp.actions.as_ref().is_some_and(|v| !v.is_empty()));

        let latency = resp.latency.as_ref().unwrap();
        assert_eq!(latency.sample_count, 3);
        assert_eq!(latency.p50_ms, Some(100));
        assert_eq!(latency.p95_ms, Some(200));
        assert_eq!(resp.unique_ips, Some(2));

        // feature 过滤：只保留 bestn 的业务维度（且 top/include 可按需选择）
        let query = StatsSummaryQuery {
            start: None,
            end: None,
            timezone: Some("Asia/Shanghai".into()),
            feature: Some("bestn".into()),
            include: Some("actions".into()),
            top: Some(10),
        };
        let Json(resp) = get_stats_summary(State(state), Query(query)).await.unwrap();
        assert_eq!(resp.feature_filter.as_deref(), Some("bestn"));
        assert_eq!(resp.unique_users.total, 1);
        assert_eq!(resp.features.len(), 1);
        assert_eq!(resp.features[0].feature, "bestn");
        assert!(
            resp.actions
                .as_ref()
                .is_some_and(|v| v.iter().all(|a| a.feature == "bestn"))
        );
        assert!(resp.http_total.is_none());
    }

    #[tokio::test]
    async fn stats_summary_skips_user_kinds_when_not_requested() {
        let sqlite_path = tmp_sqlite_path("stats_summary_no_user_kinds");
        let state = build_test_state(&sqlite_path).await;
        let storage = state.stats_storage.as_ref().unwrap().clone();

        storage
            .insert_events(&[
                EventInsert {
                    ts_utc: dt_utc(2025, 12, 24, 0, 1, 0),
                    route: None,
                    feature: Some("bestn".into()),
                    action: Some("render".into()),
                    method: None,
                    status: None,
                    duration_ms: None,
                    user_hash: Some("u1".into()),
                    client_ip_hash: None,
                    instance: Some("inst-a".into()),
                    extra_json: Some(serde_json::json!({"user_kind":"official"})),
                },
                EventInsert {
                    ts_utc: dt_utc(2025, 12, 24, 0, 1, 1),
                    route: None,
                    feature: Some("save".into()),
                    action: Some("submit".into()),
                    method: None,
                    status: None,
                    duration_ms: None,
                    user_hash: Some("u2".into()),
                    client_ip_hash: None,
                    instance: Some("inst-b".into()),
                    extra_json: Some(serde_json::json!({"user_kind":"taptap"})),
                },
            ])
            .await
            .unwrap();

        let query = StatsSummaryQuery {
            start: None,
            end: None,
            timezone: Some("Asia/Shanghai".into()),
            feature: None,
            include: Some("actions".into()),
            top: Some(10),
        };
        let Json(resp) = get_stats_summary(State(state), Query(query)).await.unwrap();
        assert_eq!(resp.unique_users.total, 2);
        assert!(resp.unique_users.by_kind.is_empty());
    }

    #[tokio::test]
    async fn stats_summary_cache_returns_stale_within_ttl() {
        let sqlite_path = tmp_sqlite_path("stats_summary_cache_hit");
        let state = build_test_state(&sqlite_path).await;
        let storage = state.stats_storage.as_ref().unwrap().clone();

        storage
            .insert_events(&[EventInsert {
                ts_utc: dt_utc(2025, 12, 24, 0, 1, 0),
                route: None,
                feature: Some("bestn".into()),
                action: Some("render".into()),
                method: None,
                status: None,
                duration_ms: None,
                user_hash: Some("u1".into()),
                client_ip_hash: None,
                instance: Some("inst-a".into()),
                extra_json: Some(serde_json::json!({"user_kind":"official"})),
            }])
            .await
            .unwrap();

        let query = StatsSummaryQuery {
            start: None,
            end: None,
            timezone: Some("Asia/Shanghai".into()),
            feature: None,
            include: Some("user_kinds".into()),
            top: Some(10),
        };
        let Json(first) = get_stats_summary(State(state.clone()), Query(query))
            .await
            .unwrap();
        assert_eq!(first.unique_users.total, 1);
        assert_eq!(first.unique_users.by_kind.len(), 1);

        storage
            .insert_events(&[EventInsert {
                ts_utc: dt_utc(2025, 12, 24, 0, 2, 0),
                route: None,
                feature: Some("save".into()),
                action: Some("submit".into()),
                method: None,
                status: None,
                duration_ms: None,
                user_hash: Some("u2".into()),
                client_ip_hash: None,
                instance: Some("inst-b".into()),
                extra_json: Some(serde_json::json!({"user_kind":"taptap"})),
            }])
            .await
            .unwrap();

        let query = StatsSummaryQuery {
            start: None,
            end: None,
            timezone: Some("Asia/Shanghai".into()),
            feature: None,
            include: Some("user_kinds".into()),
            top: Some(10),
        };
        let Json(second) = get_stats_summary(State(state), Query(query)).await.unwrap();

        assert_eq!(second.unique_users.total, 1);
        assert_eq!(second.unique_users.by_kind.len(), 1);
    }

    #[tokio::test]
    async fn daily_stats_supports_route_and_method_filters() {
        let sqlite_path = tmp_sqlite_path("stats_daily");
        let state = build_test_state(&sqlite_path).await;
        let storage = state.stats_storage.as_ref().unwrap().clone();

        storage
            .insert_events(&[
                EventInsert {
                    ts_utc: dt_utc(2025, 12, 24, 1, 0, 0),
                    route: Some("/image/bn".into()),
                    feature: None,
                    action: None,
                    method: Some("GET".into()),
                    status: Some(200),
                    duration_ms: Some(10),
                    user_hash: None,
                    client_ip_hash: None,
                    instance: Some("inst-a".into()),
                    extra_json: None,
                },
                EventInsert {
                    ts_utc: dt_utc(2025, 12, 24, 1, 0, 1),
                    route: Some("/image/bn".into()),
                    feature: None,
                    action: None,
                    method: Some("POST".into()),
                    status: Some(200),
                    duration_ms: Some(10),
                    user_hash: None,
                    client_ip_hash: None,
                    instance: Some("inst-a".into()),
                    extra_json: None,
                },
                EventInsert {
                    ts_utc: dt_utc(2025, 12, 24, 1, 0, 2),
                    route: Some("/song/search".into()),
                    feature: None,
                    action: None,
                    method: Some("GET".into()),
                    status: Some(200),
                    duration_ms: Some(10),
                    user_hash: None,
                    client_ip_hash: None,
                    instance: Some("inst-a".into()),
                    extra_json: None,
                },
            ])
            .await
            .unwrap();

        let q = DailyQuery {
            start: "2025-12-24".into(),
            end: "2025-12-24".into(),
            timezone: Some("Asia/Shanghai".into()),
            feature: None,
            route: Some("/image/bn".into()),
            method: Some("GET".into()),
        };
        let Json(rows) = get_daily_stats(State(state), Query(q)).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].route.as_deref(), Some("/image/bn"));
        assert_eq!(rows[0].method.as_deref(), Some("GET"));
        assert_eq!(rows[0].count, 1);
    }

    #[tokio::test]
    async fn daily_stats_respects_timezone_day_boundary() {
        let sqlite_path = tmp_sqlite_path("stats_daily_tz");
        let state = build_test_state(&sqlite_path).await;
        let storage = state.stats_storage.as_ref().unwrap().clone();

        // Asia/Shanghai: 2025-12-24 16:00:00Z == 2025-12-25 00:00:00+08
        storage
            .insert_events(&[
                EventInsert {
                    ts_utc: dt_utc(2025, 12, 24, 15, 59, 59),
                    route: Some("/song/search".into()),
                    feature: None,
                    action: None,
                    method: Some("GET".into()),
                    status: Some(200),
                    duration_ms: Some(10),
                    user_hash: None,
                    client_ip_hash: Some("ip1".into()),
                    instance: Some("inst-a".into()),
                    extra_json: None,
                },
                EventInsert {
                    ts_utc: dt_utc(2025, 12, 24, 16, 0, 0),
                    route: Some("/song/search".into()),
                    feature: None,
                    action: None,
                    method: Some("GET".into()),
                    status: Some(200),
                    duration_ms: Some(10),
                    user_hash: None,
                    client_ip_hash: Some("ip2".into()),
                    instance: Some("inst-a".into()),
                    extra_json: None,
                },
            ])
            .await
            .unwrap();

        // local day: 2025-12-24 should only include the first event
        let q = DailyQuery {
            start: "2025-12-24".into(),
            end: "2025-12-24".into(),
            timezone: Some("Asia/Shanghai".into()),
            feature: None,
            route: Some("/song/search".into()),
            method: Some("GET".into()),
        };
        let Json(rows) = get_daily_stats(State(state.clone()), Query(q))
            .await
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].date, "2025-12-24");
        assert_eq!(rows[0].count, 1);

        // local day: 2025-12-25 should only include the second event
        let q = DailyQuery {
            start: "2025-12-25".into(),
            end: "2025-12-25".into(),
            timezone: Some("Asia/Shanghai".into()),
            feature: None,
            route: Some("/song/search".into()),
            method: Some("GET".into()),
        };
        let Json(rows) = get_daily_stats(State(state), Query(q)).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].date, "2025-12-25");
        assert_eq!(rows[0].count, 1);
    }

    #[tokio::test]
    async fn daily_features_outputs_counts_and_unique_users() {
        let sqlite_path = tmp_sqlite_path("stats_daily_features");
        let state = build_test_state(&sqlite_path).await;
        let storage = state.stats_storage.as_ref().unwrap().clone();

        storage
            .insert_events(&[
                // local 2025-12-24 (+8) -> utc 2025-12-23 16:00..
                EventInsert {
                    ts_utc: dt_utc(2025, 12, 24, 1, 0, 0),
                    route: None,
                    feature: Some("bestn".into()),
                    action: Some("generate_image".into()),
                    method: None,
                    status: None,
                    duration_ms: None,
                    user_hash: Some("u1".into()),
                    client_ip_hash: None,
                    instance: Some("inst-a".into()),
                    extra_json: None,
                },
                EventInsert {
                    ts_utc: dt_utc(2025, 12, 24, 2, 0, 0),
                    route: None,
                    feature: Some("bestn".into()),
                    action: Some("generate_image".into()),
                    method: None,
                    status: None,
                    duration_ms: None,
                    user_hash: Some("u1".into()),
                    client_ip_hash: None,
                    instance: Some("inst-a".into()),
                    extra_json: None,
                },
                // another day
                EventInsert {
                    ts_utc: dt_utc(2025, 12, 25, 1, 0, 0),
                    route: None,
                    feature: Some("save".into()),
                    action: Some("get_save".into()),
                    method: None,
                    status: None,
                    duration_ms: None,
                    user_hash: Some("u2".into()),
                    client_ip_hash: None,
                    instance: Some("inst-a".into()),
                    extra_json: None,
                },
            ])
            .await
            .unwrap();

        let q = DailyFeaturesQuery {
            start: "2025-12-24".into(),
            end: "2025-12-25".into(),
            timezone: Some("Asia/Shanghai".into()),
            feature: None,
        };
        let Json(resp) = get_daily_features(State(state), Query(q)).await.unwrap();
        assert_eq!(resp.timezone, "Asia/Shanghai");
        assert_eq!(resp.rows.len(), 2);

        let r0 = &resp.rows[0];
        assert_eq!(r0.date, "2025-12-24");
        assert_eq!(r0.feature, "bestn");
        assert_eq!(r0.count, 2);
        assert_eq!(r0.unique_users, 1);

        let r1 = &resp.rows[1];
        assert_eq!(r1.date, "2025-12-25");
        assert_eq!(r1.feature, "save");
        assert_eq!(r1.count, 1);
        assert_eq!(r1.unique_users, 1);
    }

    #[tokio::test]
    async fn daily_dau_fills_missing_days_with_zero() {
        let sqlite_path = tmp_sqlite_path("stats_daily_dau");
        let state = build_test_state(&sqlite_path).await;
        let storage = state.stats_storage.as_ref().unwrap().clone();

        storage
            .insert_events(&[EventInsert {
                ts_utc: dt_utc(2025, 12, 24, 1, 0, 0),
                route: Some("/song/search".into()),
                feature: None,
                action: None,
                method: Some("GET".into()),
                status: Some(200),
                duration_ms: Some(10),
                user_hash: Some("u1".into()),
                client_ip_hash: Some("ip1".into()),
                instance: Some("inst-a".into()),
                extra_json: None,
            }])
            .await
            .unwrap();

        let q = DailyDauQuery {
            start: "2025-12-24".into(),
            end: "2025-12-25".into(),
            timezone: Some("Asia/Shanghai".into()),
        };
        let Json(resp) = get_daily_dau(State(state), Query(q)).await.unwrap();
        assert_eq!(resp.rows.len(), 2);
        assert_eq!(resp.rows[0].date, "2025-12-24");
        assert_eq!(resp.rows[0].active_users, 1);
        assert_eq!(resp.rows[0].active_ips, 1);
        assert_eq!(resp.rows[1].date, "2025-12-25");
        assert_eq!(resp.rows[1].active_users, 0);
        assert_eq!(resp.rows[1].active_ips, 0);
    }

    #[tokio::test]
    async fn daily_http_computes_error_rates_and_respects_top_per_day() {
        let sqlite_path = tmp_sqlite_path("stats_daily_http");
        let state = build_test_state(&sqlite_path).await;
        let storage = state.stats_storage.as_ref().unwrap().clone();

        storage
            .insert_events(&[
                EventInsert {
                    ts_utc: dt_utc(2025, 12, 24, 1, 0, 0),
                    route: Some("/image/bn".into()),
                    feature: None,
                    action: None,
                    method: Some("GET".into()),
                    status: Some(200),
                    duration_ms: Some(10),
                    user_hash: None,
                    client_ip_hash: Some("ip1".into()),
                    instance: Some("inst-a".into()),
                    extra_json: None,
                },
                EventInsert {
                    ts_utc: dt_utc(2025, 12, 24, 1, 0, 1),
                    route: Some("/image/bn".into()),
                    feature: None,
                    action: None,
                    method: Some("GET".into()),
                    status: Some(404),
                    duration_ms: Some(10),
                    user_hash: None,
                    client_ip_hash: Some("ip2".into()),
                    instance: Some("inst-a".into()),
                    extra_json: None,
                },
                EventInsert {
                    ts_utc: dt_utc(2025, 12, 24, 1, 0, 2),
                    route: Some("/save".into()),
                    feature: None,
                    action: None,
                    method: Some("POST".into()),
                    status: Some(500),
                    duration_ms: Some(10),
                    user_hash: None,
                    client_ip_hash: Some("ip3".into()),
                    instance: Some("inst-a".into()),
                    extra_json: None,
                },
            ])
            .await
            .unwrap();

        let q = DailyHttpQuery {
            start: "2025-12-24".into(),
            end: "2025-12-24".into(),
            timezone: Some("Asia/Shanghai".into()),
            route: None,
            method: None,
            top: Some(1),
        };
        let Json(resp) = get_daily_http(State(state), Query(q)).await.unwrap();
        assert_eq!(resp.totals.len(), 1);
        assert_eq!(resp.totals[0].date, "2025-12-24");
        assert_eq!(resp.totals[0].total, 3);
        assert_eq!(resp.totals[0].errors, 2);
        assert_eq!(resp.totals[0].client_errors, 1);
        assert_eq!(resp.totals[0].server_errors, 1);
        assert!((resp.totals[0].error_rate - (2.0 / 3.0)).abs() < 1e-9);

        // top=1: only one route row should be returned, but totals must reflect all routes
        assert_eq!(resp.routes.len(), 1);
        assert_eq!(resp.routes[0].date, "2025-12-24");
        assert_eq!(resp.routes[0].errors, 1);
    }

    #[tokio::test]
    async fn daily_http_top_per_day_prefers_higher_total_on_equal_errors() {
        let sqlite_path = tmp_sqlite_path("stats_daily_http_tie_break");
        let state = build_test_state(&sqlite_path).await;
        let storage = state.stats_storage.as_ref().unwrap().clone();

        storage
            .insert_events(&[
                EventInsert {
                    ts_utc: dt_utc(2025, 12, 24, 1, 0, 0),
                    route: Some("/image/bn".into()),
                    feature: None,
                    action: None,
                    method: Some("GET".into()),
                    status: Some(200),
                    duration_ms: Some(10),
                    user_hash: None,
                    client_ip_hash: Some("ip1".into()),
                    instance: Some("inst-a".into()),
                    extra_json: None,
                },
                EventInsert {
                    ts_utc: dt_utc(2025, 12, 24, 1, 0, 1),
                    route: Some("/image/bn".into()),
                    feature: None,
                    action: None,
                    method: Some("GET".into()),
                    status: Some(404),
                    duration_ms: Some(10),
                    user_hash: None,
                    client_ip_hash: Some("ip2".into()),
                    instance: Some("inst-a".into()),
                    extra_json: None,
                },
                EventInsert {
                    ts_utc: dt_utc(2025, 12, 24, 1, 0, 2),
                    route: Some("/save".into()),
                    feature: None,
                    action: None,
                    method: Some("POST".into()),
                    status: Some(500),
                    duration_ms: Some(10),
                    user_hash: None,
                    client_ip_hash: Some("ip3".into()),
                    instance: Some("inst-a".into()),
                    extra_json: None,
                },
            ])
            .await
            .unwrap();

        let q = DailyHttpQuery {
            start: "2025-12-24".into(),
            end: "2025-12-24".into(),
            timezone: Some("Asia/Shanghai".into()),
            route: None,
            method: None,
            top: Some(1),
        };
        let Json(resp) = get_daily_http(State(state), Query(q)).await.unwrap();
        assert_eq!(resp.routes.len(), 1);
        assert_eq!(resp.routes[0].route, "/image/bn");
        assert_eq!(resp.routes[0].method, "GET");
        assert_eq!(resp.routes[0].errors, 1);
        assert_eq!(resp.routes[0].total, 2);
    }

    #[tokio::test]
    async fn latency_agg_supports_day_week_month_and_filters() {
        let sqlite_path = tmp_sqlite_path("stats_latency_agg");
        let state = build_test_state(&sqlite_path).await;
        let storage = state.stats_storage.as_ref().unwrap().clone();

        storage
            .insert_events(&[
                // week_start=2025-12-22, month_start=2025-12-01
                EventInsert {
                    ts_utc: dt_utc(2025, 12, 24, 1, 0, 0),
                    route: Some("/song/search".into()),
                    feature: None,
                    action: None,
                    method: Some("GET".into()),
                    status: Some(200),
                    duration_ms: Some(100),
                    user_hash: None,
                    client_ip_hash: None,
                    instance: Some("inst-a".into()),
                    extra_json: None,
                },
                // week_start=2025-12-29, month_start=2025-12-01
                EventInsert {
                    ts_utc: dt_utc(2025, 12, 30, 1, 0, 0),
                    route: Some("/song/search".into()),
                    feature: None,
                    action: None,
                    method: Some("GET".into()),
                    status: Some(200),
                    duration_ms: Some(300),
                    user_hash: None,
                    client_ip_hash: None,
                    instance: Some("inst-a".into()),
                    extra_json: None,
                },
                EventInsert {
                    ts_utc: dt_utc(2025, 12, 30, 2, 0, 0),
                    route: Some("/song/search".into()),
                    feature: None,
                    action: None,
                    method: Some("GET".into()),
                    status: Some(200),
                    duration_ms: Some(100),
                    user_hash: None,
                    client_ip_hash: None,
                    instance: Some("inst-a".into()),
                    extra_json: None,
                },
                EventInsert {
                    ts_utc: dt_utc(2025, 12, 30, 3, 0, 0),
                    route: Some("/image/bn".into()),
                    feature: None,
                    action: None,
                    method: Some("POST".into()),
                    status: Some(200),
                    duration_ms: Some(10),
                    user_hash: None,
                    client_ip_hash: None,
                    instance: Some("inst-a".into()),
                    extra_json: None,
                },
                // week_start=2026-01-05, month_start=2026-01-01
                EventInsert {
                    ts_utc: dt_utc(2026, 1, 5, 1, 0, 0),
                    route: Some("/song/search".into()),
                    feature: None,
                    action: None,
                    method: Some("GET".into()),
                    status: Some(200),
                    duration_ms: Some(50),
                    user_hash: None,
                    client_ip_hash: None,
                    instance: Some("inst-a".into()),
                    extra_json: None,
                },
            ])
            .await
            .unwrap();

        // day
        let q = LatencyAggQuery {
            start: "2025-12-24".into(),
            end: "2026-01-05".into(),
            timezone: Some("Asia/Shanghai".into()),
            bucket: Some("day".into()),
            feature: None,
            route: None,
            method: None,
        };
        let Json(resp) = get_latency_agg(State(state.clone()), Query(q))
            .await
            .unwrap();
        assert_eq!(resp.bucket, "day");

        let r = resp
            .rows
            .iter()
            .find(|r| r.bucket == "2025-12-24" && r.route.as_deref() == Some("/song/search"))
            .unwrap();
        assert_eq!(r.count, 1);
        assert_eq!(r.min_ms, Some(100));
        assert_eq!(r.max_ms, Some(100));
        assert_eq!(r.avg_ms, Some(100.0));

        let r = resp
            .rows
            .iter()
            .find(|r| r.bucket == "2025-12-30" && r.route.as_deref() == Some("/song/search"))
            .unwrap();
        assert_eq!(r.count, 2);
        assert_eq!(r.min_ms, Some(100));
        assert_eq!(r.max_ms, Some(300));
        assert_eq!(r.avg_ms, Some(200.0));

        let r = resp
            .rows
            .iter()
            .find(|r| r.bucket == "2025-12-30" && r.route.as_deref() == Some("/image/bn"))
            .unwrap();
        assert_eq!(r.count, 1);
        assert_eq!(r.min_ms, Some(10));
        assert_eq!(r.max_ms, Some(10));
        assert_eq!(r.avg_ms, Some(10.0));

        let r = resp
            .rows
            .iter()
            .find(|r| r.bucket == "2026-01-05" && r.route.as_deref() == Some("/song/search"))
            .unwrap();
        assert_eq!(r.count, 1);
        assert_eq!(r.min_ms, Some(50));
        assert_eq!(r.max_ms, Some(50));
        assert_eq!(r.avg_ms, Some(50.0));

        // week（bucket 标签为 week_start）
        let q = LatencyAggQuery {
            start: "2025-12-24".into(),
            end: "2026-01-05".into(),
            timezone: Some("Asia/Shanghai".into()),
            bucket: Some("week".into()),
            feature: None,
            route: None,
            method: None,
        };
        let Json(resp) = get_latency_agg(State(state.clone()), Query(q))
            .await
            .unwrap();
        assert_eq!(resp.bucket, "week");
        assert!(
            resp.rows
                .iter()
                .any(|r| r.bucket == "2025-12-22" && r.route.as_deref() == Some("/song/search"))
        );
        assert!(
            resp.rows
                .iter()
                .any(|r| r.bucket == "2025-12-29" && r.route.as_deref() == Some("/image/bn"))
        );
        assert!(
            resp.rows
                .iter()
                .any(|r| r.bucket == "2026-01-05" && r.route.as_deref() == Some("/song/search"))
        );

        // month（bucket 标签为 month_start）
        let q = LatencyAggQuery {
            start: "2025-12-24".into(),
            end: "2026-01-05".into(),
            timezone: Some("Asia/Shanghai".into()),
            bucket: Some("month".into()),
            feature: None,
            route: None,
            method: None,
        };
        let Json(resp) = get_latency_agg(State(state.clone()), Query(q))
            .await
            .unwrap();
        assert_eq!(resp.bucket, "month");

        let dec = resp
            .rows
            .iter()
            .find(|r| r.bucket == "2025-12-01" && r.route.as_deref() == Some("/song/search"))
            .unwrap();
        assert_eq!(dec.count, 3);
        assert_eq!(dec.min_ms, Some(100));
        assert_eq!(dec.max_ms, Some(300));
        assert!((dec.avg_ms.unwrap() - (500.0 / 3.0)).abs() < 1e-9);

        let jan = resp
            .rows
            .iter()
            .find(|r| r.bucket == "2026-01-01" && r.route.as_deref() == Some("/song/search"))
            .unwrap();
        assert_eq!(jan.count, 1);
        assert_eq!(jan.min_ms, Some(50));
        assert_eq!(jan.max_ms, Some(50));
        assert_eq!(jan.avg_ms, Some(50.0));

        // filters（只看 /song/search GET）
        let q = LatencyAggQuery {
            start: "2025-12-24".into(),
            end: "2026-01-05".into(),
            timezone: Some("Asia/Shanghai".into()),
            bucket: Some("day".into()),
            feature: None,
            route: Some("/song/search".into()),
            method: Some("GET".into()),
        };
        let Json(resp) = get_latency_agg(State(state), Query(q)).await.unwrap();
        assert!(
            resp.rows
                .iter()
                .all(|r| r.route.as_deref() == Some("/song/search"))
        );
        assert!(resp.rows.iter().all(|r| r.method.as_deref() == Some("GET")));
    }

    #[tokio::test]
    async fn latency_agg_respects_timezone_day_boundary() {
        let sqlite_path = tmp_sqlite_path("stats_latency_agg_tz");
        let state = build_test_state(&sqlite_path).await;
        let storage = state.stats_storage.as_ref().unwrap().clone();

        // Asia/Shanghai: 2025-12-24 16:00:00Z == 2025-12-25 00:00:00+08
        storage
            .insert_events(&[
                EventInsert {
                    ts_utc: dt_utc(2025, 12, 24, 15, 59, 59),
                    route: Some("/song/search".into()),
                    feature: None,
                    action: None,
                    method: Some("GET".into()),
                    status: Some(200),
                    duration_ms: Some(10),
                    user_hash: None,
                    client_ip_hash: None,
                    instance: Some("inst-a".into()),
                    extra_json: None,
                },
                EventInsert {
                    ts_utc: dt_utc(2025, 12, 24, 16, 0, 0),
                    route: Some("/song/search".into()),
                    feature: None,
                    action: None,
                    method: Some("GET".into()),
                    status: Some(200),
                    duration_ms: Some(10),
                    user_hash: None,
                    client_ip_hash: None,
                    instance: Some("inst-a".into()),
                    extra_json: None,
                },
            ])
            .await
            .unwrap();

        let q = LatencyAggQuery {
            start: "2025-12-24".into(),
            end: "2025-12-24".into(),
            timezone: Some("Asia/Shanghai".into()),
            bucket: Some("day".into()),
            feature: None,
            route: Some("/song/search".into()),
            method: Some("GET".into()),
        };
        let Json(resp) = get_latency_agg(State(state.clone()), Query(q))
            .await
            .unwrap();
        assert_eq!(resp.rows.len(), 1);
        assert_eq!(resp.rows[0].bucket, "2025-12-24");
        assert_eq!(resp.rows[0].count, 1);

        let q = LatencyAggQuery {
            start: "2025-12-25".into(),
            end: "2025-12-25".into(),
            timezone: Some("Asia/Shanghai".into()),
            bucket: Some("day".into()),
            feature: None,
            route: Some("/song/search".into()),
            method: Some("GET".into()),
        };
        let Json(resp) = get_latency_agg(State(state), Query(q)).await.unwrap();
        assert_eq!(resp.rows.len(), 1);
        assert_eq!(resp.rows[0].bucket, "2025-12-25");
        assert_eq!(resp.rows[0].count, 1);
    }
}
