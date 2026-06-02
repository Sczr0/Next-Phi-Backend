use axum::{
    Router,
    extract::{Query, State},
    response::Json,
    routing::{get, post},
};
use chrono::{NaiveDateTime, NaiveTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::error::AppError;
use crate::state::AppState;

use super::{archive::archive_one_day, models::DailyAggRow};

mod cache;
mod params;
mod queries;
mod time;

use cache::{
    build_daily_http_cache_key, build_stats_summary_cache_key, daily_http_cache,
    stats_summary_cache,
};
use params::{
    LatencyAggFilters, normalize_top, normalize_top_per_day, parse_include_flags,
    parse_latency_bucket,
};
use queries::{
    query_daily_agg, query_daily_dau, query_daily_feature_usage, query_daily_http,
    query_latency_agg,
};
use time::{convert_tz, parse_date_bound_utc, parse_ymd, resolve_timezone, validate_date_range};

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

#[derive(Serialize, utoipa::ToSchema, Clone)]
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
    let mut start_utc = q
        .start
        .as_deref()
        .map(|s| parse_date_bound_utc(s, tz, false))
        .transpose()?;
    let end_utc = q
        .end
        .as_deref()
        .map(|s| parse_date_bound_utc(s, tz, true))
        .transpose()?;
    // 两端都未指定 → 默认回退到 retention_hot_days，取整到 UTC 当天 0 点以稳定缓存 key
    if start_utc.is_none() && end_utc.is_none() {
        let today = Utc::now().date_naive();
        let since = NaiveDateTime::new(today, NaiveTime::from_hms_opt(0, 0, 0).unwrap()).and_utc()
            - chrono::Duration::days(i64::from(cfg.stats.retention_hot_days));
        start_utc = Some(since.to_rfc3339());
    }
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
    let want_meta =
        q.start.is_some() || q.end.is_some() || q.feature.is_some() || q.include.is_some();
    let summary = storage
        .query_stats_summary_data(
            start_utc_ref,
            end_utc_ref,
            feature_ref,
            super::storage::SummaryIncludeFlags {
                routes: include.routes,
                methods: include.methods,
                status_codes: include.status_codes,
                instances: include.instances,
                actions: include.actions,
                latency: include.latency,
                unique_ips: include.unique_ips,
                user_kinds: include.user_kinds,
            },
            top,
            want_meta,
        )
        .await?;

    let first_event_at = summary
        .first_event_ts
        .as_deref()
        .and_then(|s| convert_tz(s, tz));
    let last_event_at = summary
        .last_event_ts
        .as_deref()
        .and_then(|s| convert_tz(s, tz));

    let features = summary
        .features
        .into_iter()
        .map(|r| FeatureUsageSummary {
            feature: r.feature,
            count: r.count,
            last_at: r.last_ts.as_deref().and_then(|s| convert_tz(s, tz)),
        })
        .collect::<Vec<_>>();

    let total = summary.unique_users_total;
    let by_kind = summary.by_kind;
    let events_total = summary.events_total;
    let http_total = summary.http_total;
    let http_errors = summary.http_errors;

    let routes = summary.routes.map(|rows| {
        rows.into_iter()
            .map(|r| RouteUsageSummary {
                route: r.route,
                count: r.count,
                err_count: r.err_count,
                last_at: r.last_ts.as_deref().and_then(|s| convert_tz(s, tz)),
            })
            .collect::<Vec<_>>()
    });

    let methods = summary.methods.map(|rows| {
        rows.into_iter()
            .map(|r| MethodUsageSummary {
                method: r.method,
                count: r.count,
            })
            .collect::<Vec<_>>()
    });

    let status_codes = summary.status_codes.map(|rows| {
        rows.into_iter()
            .filter_map(|r| {
                if !(0..=i64::from(u16::MAX)).contains(&r.status) {
                    return None;
                }
                u16::try_from(r.status)
                    .ok()
                    .map(|status| StatusCodeSummary {
                        status,
                        count: r.count,
                    })
            })
            .collect::<Vec<_>>()
    });

    let instances = summary.instances.map(|rows| {
        rows.into_iter()
            .map(|r| InstanceUsageSummary {
                instance: r.instance,
                count: r.count,
                last_at: r.last_ts.as_deref().and_then(|s| convert_tz(s, tz)),
            })
            .collect::<Vec<_>>()
    });

    let actions = summary.actions.map(|rows| {
        rows.into_iter()
            .map(|r| ActionUsageSummary {
                feature: r.feature,
                action: r.action,
                count: r.count,
                last_at: r.last_ts.as_deref().and_then(|s| convert_tz(s, tz)),
            })
            .collect::<Vec<_>>()
    });

    let latency = summary.latency.map(|l| LatencySummary {
        sample_count: l.sample_count,
        avg_ms: l.avg_ms,
        p50_ms: l.p50_ms,
        p95_ms: l.p95_ms,
        max_ms: l.max_ms,
    });
    let unique_ips = summary.unique_ips;

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

#[cfg(test)]
mod tests;
