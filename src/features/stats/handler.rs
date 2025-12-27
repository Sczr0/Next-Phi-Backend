use axum::{
    Router,
    extract::{Query, State},
    response::Json,
    routing::{get, post},
};
use chrono::{LocalResult, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use sqlx::Row;

use crate::error::AppError;
use crate::state::AppState;

use super::{archive::archive_one_day, models::DailyAggRow};

#[derive(Deserialize)]
pub struct DailyQuery {
    start: String,
    end: String,
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
    let start = NaiveDate::parse_from_str(&q.start, "%Y-%m-%d")
        .map_err(|e| AppError::Validation(format!("start 日期无效（期望 YYYY-MM-DD）: {e}")))?;
    let end = NaiveDate::parse_from_str(&q.end, "%Y-%m-%d")
        .map_err(|e| AppError::Validation(format!("end 日期无效（期望 YYYY-MM-DD）: {e}")))?;
    let storage = state
        .stats_storage
        .as_ref()
        .ok_or_else(|| AppError::Internal("统计存储未初始化".into()))?;
    let rows = storage
        .query_daily(start, end, q.feature, q.route, q.method)
        .await?;
    Ok(Json(rows))
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
        .route("/stats/archive/now", post(trigger_archive_now))
        .route("/stats/summary", get(get_stats_summary))
}

#[derive(Serialize, utoipa::ToSchema)]
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

#[derive(Serialize, utoipa::ToSchema)]
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
    /// 可选额外维度（csv）：routes,status,methods,instances,actions,latency,unique_ips,all
    include: Option<String>,
    /// TopN（默认 20，最大 200）
    top: Option<i64>,
}

#[derive(Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RouteUsageSummary {
    route: String,
    count: i64,
    err_count: i64,
    last_at: Option<String>,
}

#[derive(Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MethodUsageSummary {
    method: String,
    count: i64,
}

#[derive(Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct StatusCodeSummary {
    status: u16,
    count: i64,
}

#[derive(Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct InstanceUsageSummary {
    instance: String,
    count: i64,
    last_at: Option<String>,
}

#[derive(Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ActionUsageSummary {
    feature: String,
    action: String,
    count: i64,
    last_at: Option<String>,
}

#[derive(Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LatencySummary {
    sample_count: i64,
    avg_ms: Option<f64>,
    p50_ms: Option<i64>,
    p95_ms: Option<i64>,
    max_ms: Option<i64>,
}

#[derive(Serialize, utoipa::ToSchema)]
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
        ("include" = Option<String>, Query, description = "可选额外维度：routes,status,methods,instances,actions,latency,unique_ips,all"),
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

    // overall first/last event
    let row = sqlx::query(
        "SELECT MIN(ts_utc) as min_ts, MAX(ts_utc) as max_ts FROM events WHERE (? IS NULL OR ts_utc >= ?) AND (? IS NULL OR ts_utc <= ?)",
    )
        .bind(start_utc.as_deref())
        .bind(start_utc.as_deref())
        .bind(end_utc.as_deref())
        .bind(end_utc.as_deref())
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
    let feat_rows = sqlx::query(
        "SELECT feature, COUNT(1) as cnt, MAX(ts_utc) as last_ts FROM events WHERE feature IS NOT NULL AND (? IS NULL OR ts_utc >= ?) AND (? IS NULL OR ts_utc <= ?) AND (? IS NULL OR feature = ?) GROUP BY feature",
    )
    .bind(start_utc.as_deref())
    .bind(start_utc.as_deref())
    .bind(end_utc.as_deref())
    .bind(end_utc.as_deref())
    .bind(q.feature.as_deref())
    .bind(q.feature.as_deref())
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
    let row = sqlx::query(
        "SELECT COUNT(DISTINCT user_hash) as total FROM events WHERE user_hash IS NOT NULL AND (? IS NULL OR ts_utc >= ?) AND (? IS NULL OR ts_utc <= ?) AND (? IS NULL OR feature = ?)",
    )
    .bind(start_utc.as_deref())
    .bind(start_utc.as_deref())
    .bind(end_utc.as_deref())
    .bind(end_utc.as_deref())
    .bind(q.feature.as_deref())
    .bind(q.feature.as_deref())
    .fetch_one(&storage.pool)
    .await
    .map_err(|e| AppError::Internal(format!("summary users: {e}")))?;
    let total: i64 = row.try_get("total").unwrap_or(0);

    // by_kind via extra_json.user_kind（在应用端聚合，避免对 SQLite JSON1 的依赖）
    //
    // 性能优化：避免一次性 `fetch_all` 造成内存峰值，按 events.id 分批扫描。
    use std::collections::{HashMap, HashSet};
    const BATCH: i64 = 5000;
    let mut last_id: i64 = 0;
    let mut uniq: HashSet<(String, String)> = HashSet::new();
    loop {
        let rows = sqlx::query(
            "SELECT id, user_hash, extra_json FROM events WHERE user_hash IS NOT NULL AND extra_json IS NOT NULL AND id > ? AND (? IS NULL OR ts_utc >= ?) AND (? IS NULL OR ts_utc <= ?) AND (? IS NULL OR feature = ?) ORDER BY id ASC LIMIT ?",
        )
        .bind(last_id)
        .bind(start_utc.as_deref())
        .bind(start_utc.as_deref())
        .bind(end_utc.as_deref())
        .bind(end_utc.as_deref())
        .bind(q.feature.as_deref())
        .bind(q.feature.as_deref())
        .bind(BATCH)
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
    for (_, k) in uniq.into_iter() {
        *by_kind_map.entry(k).or_insert(0) += 1;
    }
    let mut by_kind: Vec<(String, i64)> = by_kind_map.into_iter().collect();
    by_kind.sort_by(|a, b| b.1.cmp(&a.1));

    let want_meta =
        q.start.is_some() || q.end.is_some() || q.feature.is_some() || q.include.is_some();

    let events_total = if want_meta || include.any() {
        let row = sqlx::query(
            "SELECT COUNT(1) as total FROM events WHERE (? IS NULL OR ts_utc >= ?) AND (? IS NULL OR ts_utc <= ?)",
        )
        .bind(start_utc.as_deref())
        .bind(start_utc.as_deref())
        .bind(end_utc.as_deref())
        .bind(end_utc.as_deref())
        .fetch_one(&storage.pool)
        .await
        .map_err(|e| AppError::Internal(format!("summary events_total: {e}")))?;
        Some(row.try_get::<i64, _>("total").unwrap_or(0))
    } else {
        None
    };

    let (http_total, http_errors) = if include.any_http() {
        let row = sqlx::query(
            "SELECT COUNT(1) as total, COALESCE(SUM(CASE WHEN status >= 400 THEN 1 ELSE 0 END), 0) as err FROM events WHERE route IS NOT NULL AND (? IS NULL OR ts_utc >= ?) AND (? IS NULL OR ts_utc <= ?)",
        )
        .bind(start_utc.as_deref())
        .bind(start_utc.as_deref())
        .bind(end_utc.as_deref())
        .bind(end_utc.as_deref())
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
        let rows = sqlx::query(
            "SELECT route, COUNT(1) as cnt, COALESCE(SUM(CASE WHEN status >= 400 THEN 1 ELSE 0 END), 0) as err_cnt, MAX(ts_utc) as last_ts FROM events WHERE route IS NOT NULL AND (? IS NULL OR ts_utc >= ?) AND (? IS NULL OR ts_utc <= ?) GROUP BY route ORDER BY cnt DESC LIMIT ?",
        )
        .bind(start_utc.as_deref())
        .bind(start_utc.as_deref())
        .bind(end_utc.as_deref())
        .bind(end_utc.as_deref())
        .bind(top)
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
        let rows = sqlx::query(
            "SELECT method, COUNT(1) as cnt FROM events WHERE route IS NOT NULL AND method IS NOT NULL AND (? IS NULL OR ts_utc >= ?) AND (? IS NULL OR ts_utc <= ?) GROUP BY method ORDER BY cnt DESC LIMIT ?",
        )
        .bind(start_utc.as_deref())
        .bind(start_utc.as_deref())
        .bind(end_utc.as_deref())
        .bind(end_utc.as_deref())
        .bind(top)
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
        let rows = sqlx::query(
            "SELECT status, COUNT(1) as cnt FROM events WHERE route IS NOT NULL AND status IS NOT NULL AND (? IS NULL OR ts_utc >= ?) AND (? IS NULL OR ts_utc <= ?) GROUP BY status ORDER BY cnt DESC LIMIT ?",
        )
        .bind(start_utc.as_deref())
        .bind(start_utc.as_deref())
        .bind(end_utc.as_deref())
        .bind(end_utc.as_deref())
        .bind(top)
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
        let rows = sqlx::query(
            "SELECT instance, COUNT(1) as cnt, MAX(ts_utc) as last_ts FROM events WHERE instance IS NOT NULL AND (? IS NULL OR ts_utc >= ?) AND (? IS NULL OR ts_utc <= ?) GROUP BY instance ORDER BY cnt DESC LIMIT ?",
        )
        .bind(start_utc.as_deref())
        .bind(start_utc.as_deref())
        .bind(end_utc.as_deref())
        .bind(end_utc.as_deref())
        .bind(top)
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
        let rows = sqlx::query(
            "SELECT feature, action, COUNT(1) as cnt, MAX(ts_utc) as last_ts FROM events WHERE feature IS NOT NULL AND action IS NOT NULL AND (? IS NULL OR ts_utc >= ?) AND (? IS NULL OR ts_utc <= ?) AND (? IS NULL OR feature = ?) GROUP BY feature, action ORDER BY cnt DESC LIMIT ?",
        )
        .bind(start_utc.as_deref())
        .bind(start_utc.as_deref())
        .bind(end_utc.as_deref())
        .bind(end_utc.as_deref())
        .bind(q.feature.as_deref())
        .bind(q.feature.as_deref())
        .bind(top)
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
        let row = sqlx::query(
            "SELECT COUNT(duration_ms) as n, AVG(duration_ms) as avg, MAX(duration_ms) as max FROM events WHERE route IS NOT NULL AND duration_ms IS NOT NULL AND (? IS NULL OR ts_utc >= ?) AND (? IS NULL OR ts_utc <= ?)",
        )
        .bind(start_utc.as_deref())
        .bind(start_utc.as_deref())
        .bind(end_utc.as_deref())
        .bind(end_utc.as_deref())
        .fetch_one(&storage.pool)
        .await
        .map_err(|e| AppError::Internal(format!("summary latency base: {e}")))?;

        let n: i64 = row.try_get("n").unwrap_or(0);
        let avg_ms: Option<f64> = row.try_get("avg").ok();
        let max_ms: Option<i64> = row.try_get("max").ok();

        let (p50_ms, p95_ms) = if n > 0 {
            let p50_idx = (((n - 1) as f64) * 0.50).round() as i64;
            let p95_idx = (((n - 1) as f64) * 0.95).round() as i64;

            let start = start_utc.as_deref();
            let end = end_utc.as_deref();
            let pool = &storage.pool;
            let pick = |idx: i64| async move {
                let r = sqlx::query(
                    "SELECT duration_ms as v FROM events WHERE route IS NOT NULL AND duration_ms IS NOT NULL AND (? IS NULL OR ts_utc >= ?) AND (? IS NULL OR ts_utc <= ?) ORDER BY duration_ms ASC LIMIT 1 OFFSET ?",
                )
                .bind(start)
                .bind(start)
                .bind(end)
                .bind(end)
                .bind(idx)
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
        let row = sqlx::query(
            "SELECT COUNT(DISTINCT client_ip_hash) as cnt FROM events WHERE route IS NOT NULL AND client_ip_hash IS NOT NULL AND (? IS NULL OR ts_utc >= ?) AND (? IS NULL OR ts_utc <= ?)",
        )
        .bind(start_utc.as_deref())
        .bind(start_utc.as_deref())
        .bind(end_utc.as_deref())
        .bind(end_utc.as_deref())
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
    Ok(Json(resp))
}

fn convert_tz(ts_rfc3339: &str, tz: chrono_tz::Tz) -> Option<String> {
    let dt = chrono::DateTime::parse_from_rfc3339(ts_rfc3339).ok()?;
    let as_utc = dt.with_timezone(&chrono::Utc);
    Some(as_utc.with_timezone(&tz).to_rfc3339())
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
    }

    fn any_http(self) -> bool {
        self.routes || self.methods || self.status_codes || self.latency || self.unique_ips
    }
}

fn parse_include_flags(include: Option<&str>) -> IncludeFlags {
    let Some(s) = include else {
        return IncludeFlags::default();
    };

    let mut flags = IncludeFlags::default();
    for raw in s.split(|c| c == ',' || c == ';' || c == ' ' || c == '\t' || c == '\n') {
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

        let b = parse_include_flags(Some("routes, latency,unique_ips"));
        assert!(b.routes);
        assert!(b.latency);
        assert!(b.unique_ips);
        assert!(!b.actions);
        assert!(b.any_http());
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
}
