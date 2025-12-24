use axum::{
    Router,
    extract::{Query, State},
    response::Json,
    routing::{get, post},
};
use chrono::{NaiveDate, Utc};
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
}

#[utoipa::path(
    get,
    path = "/stats/daily",
    summary = "按日聚合的统计数据",
    description = "在 SQLite 明细上进行区间聚合，返回每天每功能/路由的调用与错误次数汇总",
    params(
        ("start" = String, Query, description = "开始日期 YYYY-MM-DD"),
        ("end" = String, Query, description = "结束日期 YYYY-MM-DD"),
        ("feature" = Option<String>, Query, description = "可选功能名")
    ),
    responses(
        (status = 200, description = "聚合结果", body = [DailyAggRow]),
        (
            status = 422,
            description = "参数校验失败（日期格式等）",
            body = String,
            content_type = "text/plain"
        ),
        (
            status = 500,
            description = "统计存储未初始化/查询失败",
            body = String,
            content_type = "text/plain"
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
    let rows = storage.query_daily(start, end, q.feature).await?;
    Ok(Json(rows))
}

#[derive(Deserialize)]
pub struct ArchiveQuery {
    date: Option<String>,
}

#[derive(Serialize, utoipa::ToSchema)]
#[schema(example = json!({"ok": true, "date": "2025-12-23"}))]
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
            body = String,
            content_type = "text/plain"
        ),
        (
            status = 500,
            description = "统计存储未初始化/归档失败",
            body = String,
            content_type = "text/plain"
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
pub struct UniqueUsersSummary {
    /// 去敏后唯一用户总数
    total: i64,
    /// 按用户来源/凭证类型聚合的唯一用户数，例如 ("official", 123)
    by_kind: Vec<(String, i64)>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct StatsSummaryResponse {
    /// 展示使用的时区（IANA 名称）
    timezone: String,
    /// 配置中设置的统计起始时间（如有）
    config_start_at: Option<String>,
    /// 全量事件中的最早时间（本地时区）
    first_event_at: Option<String>,
    /// 全量事件中的最晚时间（本地时区）
    last_event_at: Option<String>,
    /// 各功能使用概览
    features: Vec<FeatureUsageSummary>,
    /// 唯一用户统计
    unique_users: UniqueUsersSummary,
}

#[utoipa::path(
    get,
    path = "/stats/summary",
    summary = "统计总览（唯一用户与功能使用）",
    description = "提供统计模块关键指标：全局首末事件时间、按功能的使用次数与最近时间、唯一用户总量及来源分布。\n\n功能次数统计中的功能名可能值：\n- bestn：生成 BestN 汇总图\n- bestn_user：生成用户自报 BestN 图片\n- single_query：生成单曲成绩图\n- save：获取并解析玩家存档\n- song_search：歌曲检索",
    responses(
        (status = 200, description = "汇总信息", body = StatsSummaryResponse),
        (
            status = 500,
            description = "统计存储未初始化/查询失败",
            body = String,
            content_type = "text/plain"
        )
    ),
    tag = "Stats"
)]
pub async fn get_stats_summary(
    State(state): State<AppState>,
) -> Result<Json<StatsSummaryResponse>, AppError> {
    let storage = state
        .stats_storage
        .as_ref()
        .ok_or_else(|| AppError::Internal("统计存储未初始化".into()))?;
    let tz_name = crate::config::AppConfig::global().stats.timezone.clone();
    let tz = parse_tz(&tz_name);

    // overall first/last event
    let row = sqlx::query("SELECT MIN(ts_utc) as min_ts, MAX(ts_utc) as max_ts FROM events")
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
    let feat_rows = sqlx::query("SELECT feature, COUNT(1) as cnt, MAX(ts_utc) as last_ts FROM events WHERE feature IS NOT NULL GROUP BY feature")
        .fetch_all(&storage.pool).await.map_err(|e| AppError::Internal(format!("summary features: {e}")))?;
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
        "SELECT COUNT(DISTINCT user_hash) as total FROM events WHERE user_hash IS NOT NULL",
    )
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
            "SELECT id, user_hash, extra_json FROM events WHERE user_hash IS NOT NULL AND extra_json IS NOT NULL AND id > ? ORDER BY id ASC LIMIT ?",
        )
        .bind(last_id)
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

    let resp = StatsSummaryResponse {
        timezone: tz_name,
        config_start_at: crate::config::AppConfig::global().stats.start_at.clone(),
        first_event_at,
        last_event_at,
        features,
        unique_users: UniqueUsersSummary { total, by_kind },
    };
    Ok(Json(resp))
}

fn parse_tz(name: &str) -> chrono_tz::Tz {
    name.parse::<chrono_tz::Tz>()
        .unwrap_or(chrono_tz::Asia::Shanghai)
}

fn convert_tz(ts_rfc3339: &str, tz: chrono_tz::Tz) -> Option<String> {
    let dt = chrono::DateTime::parse_from_rfc3339(ts_rfc3339).ok()?;
    let as_utc = dt.with_timezone(&chrono::Utc);
    Some(as_utc.with_timezone(&tz).to_rfc3339())
}
