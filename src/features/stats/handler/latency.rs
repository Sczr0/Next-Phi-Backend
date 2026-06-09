use axum::{
    extract::{Query, State},
    response::Json,
};
use serde::{Deserialize, Serialize};

use crate::{error::AppError, state::AppState};

use super::{
    params::{LatencyAggFilters, parse_latency_bucket},
    queries::query_latency_agg,
    time::{parse_ymd, resolve_timezone, validate_date_range},
};

#[derive(Deserialize)]
pub struct LatencyAggQuery {
    pub(super) start: String,
    pub(super) end: String,
    /// 可选时区（IANA 名称，如 Asia/Shanghai），覆盖配置
    pub(super) timezone: Option<String>,
    /// 聚合粒度：day/week/month（默认 day）
    pub(super) bucket: Option<String>,
    /// 可选功能名过滤（仅当事件写入了 feature 时生效）
    pub(super) feature: Option<String>,
    /// 可选路由模板过滤（MatchedPath）
    pub(super) route: Option<String>,
    /// 可选 HTTP 方法过滤（GET/POST 等）
    pub(super) method: Option<String>,
}

#[derive(Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LatencyAggRow {
    /// bucket 标签：day=YYYY-MM-DD；week=week_start(YYYY-MM-DD)；month=month_start(YYYY-MM-01)
    pub(super) bucket: String,
    /// 事件中的 feature（可为空）
    pub(super) feature: Option<String>,
    /// 事件中的 route（MatchedPath）
    pub(super) route: Option<String>,
    /// 事件中的 method（GET/POST 等）
    pub(super) method: Option<String>,
    /// 样本数
    pub(super) count: i64,
    /// 最小耗时（毫秒）
    pub(super) min_ms: Option<i64>,
    /// 平均耗时（毫秒）
    pub(super) avg_ms: Option<f64>,
    /// 最大耗时（毫秒）
    pub(super) max_ms: Option<i64>,
}

#[derive(Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LatencyAggResponse {
    pub(super) timezone: String,
    pub(super) start: String,
    pub(super) end: String,
    /// day/week/month
    pub(super) bucket: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) feature_filter: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) route_filter: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) method_filter: Option<String>,
    pub(super) rows: Vec<LatencyAggRow>,
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
