use axum::{extract::{Query, State}, response::Json, routing::{get, post}, Router};
use chrono::{NaiveDate, Utc};
use serde::Deserialize;
use serde::Serialize;
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
    params(
        ("start" = String, Query, description = "开始日期 YYYY-MM-DD"),
        ("end" = String, Query, description = "结束日期 YYYY-MM-DD"),
        ("feature" = Option<String>, Query, description = "可选功能名")
    ),
    responses((status = 200, body = [DailyAggRow])),
    tag = "Stats"
)]
pub async fn get_daily_stats(State(state): State<AppState>, Query(q): Query<DailyQuery>) -> Result<Json<Vec<DailyAggRow>>, AppError> {
    let start = NaiveDate::parse_from_str(&q.start, "%Y-%m-%d").map_err(|e| AppError::Internal(format!("start 日期无效: {}", e)))?;
    let end = NaiveDate::parse_from_str(&q.end, "%Y-%m-%d").map_err(|e| AppError::Internal(format!("end 日期无效: {}", e)))?;
    let storage = state.stats_storage.as_ref().ok_or_else(|| AppError::Internal("统计存储未初始化".into()))?;
    let rows = storage.query_daily(start, end, q.feature).await?;
    Ok(Json(rows))
}

#[derive(Deserialize)]
pub struct ArchiveQuery { date: Option<String> }

#[utoipa::path(
    post,
    path = "/stats/archive/now",
    params(("date" = Option<String>, Query, description = "归档日期 YYYY-MM-DD，默认为昨天")),
    responses((status = 200, description = "归档已触发")),
    tag = "Stats"
)]
pub async fn trigger_archive_now(State(state): State<AppState>, Query(q): Query<ArchiveQuery>) -> Result<Json<serde_json::Value>, AppError> {
    let day = if let Some(d) = q.date { chrono::NaiveDate::parse_from_str(&d, "%Y-%m-%d").map_err(|e| AppError::Internal(format!("date 无效: {}", e)))? } else { (Utc::now() - chrono::Duration::days(1)).date_naive() };
    let storage = state.stats_storage.as_ref().ok_or_else(|| AppError::Internal("统计存储未初始化".into()))?;
    archive_one_day(storage, &crate::config::StatsArchiveConfig::default(), day).await?;
    Ok(Json(serde_json::json!({"ok": true, "date": day.to_string()})))
}

pub fn create_stats_router() -> Router<AppState> {
    Router::new()
        .route("/stats/daily", get(get_daily_stats))
        .route("/stats/archive/now", post(trigger_archive_now))
        .route("/stats/summary", get(get_stats_summary))
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct FeatureUsageSummary {
    feature: String,
    count: i64,
    last_at: Option<String>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct UniqueUsersSummary {
    total: i64,
    by_kind: Vec<(String, i64)>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct StatsSummaryResponse {
    timezone: String,
    config_start_at: Option<String>,
    first_event_at: Option<String>,
    last_event_at: Option<String>,
    features: Vec<FeatureUsageSummary>,
    unique_users: UniqueUsersSummary,
}

#[utoipa::path(
    get,
    path = "/stats/summary",
    responses((status = 200, body = StatsSummaryResponse)),
    tag = "Stats"
)]
pub async fn get_stats_summary(State(state): State<AppState>) -> Result<Json<StatsSummaryResponse>, AppError> {
    let storage = state.stats_storage.as_ref().ok_or_else(|| AppError::Internal("统计存储未初始化".into()))?;
    let tz_name = crate::config::AppConfig::global().stats.timezone.clone();
    let tz = parse_tz(&tz_name);

    // overall first/last event
    let row = sqlx::query("SELECT MIN(ts_utc) as min_ts, MAX(ts_utc) as max_ts FROM events")
        .fetch_one(&storage.pool).await.map_err(|e| AppError::Internal(format!("summary overall: {}", e)))?;
    let first_event_at = row.try_get::<String, _>("min_ts").ok().and_then(|s| convert_tz(&s, tz.clone()));
    let last_event_at = row.try_get::<String, _>("max_ts").ok().and_then(|s| convert_tz(&s, tz.clone()));

    // features usage
    let feat_rows = sqlx::query("SELECT feature, COUNT(1) as cnt, MAX(ts_utc) as last_ts FROM events WHERE feature IS NOT NULL GROUP BY feature")
        .fetch_all(&storage.pool).await.map_err(|e| AppError::Internal(format!("summary features: {}", e)))?;
    let mut features = Vec::with_capacity(feat_rows.len());
    for r in feat_rows {
        let f: String = r.try_get("feature").unwrap_or_else(|_| "".into());
        let c: i64 = r.try_get("cnt").unwrap_or(0);
        let last: Option<String> = r.try_get("last_ts").ok();
        let last = last.and_then(|s| convert_tz(&s, tz.clone()));
        features.push(FeatureUsageSummary { feature: f, count: c, last_at: last });
    }

    // unique users (total)
    let row = sqlx::query("SELECT COUNT(DISTINCT user_hash) as total FROM events WHERE user_hash IS NOT NULL")
        .fetch_one(&storage.pool).await.map_err(|e| AppError::Internal(format!("summary users: {}", e)))?;
    let total: i64 = row.try_get("total").unwrap_or(0);

    // by_kind via extra_json.user_kind （在应用端聚合，避免对 JSON1 的依赖）
    let rows = sqlx::query("SELECT user_hash, extra_json FROM events WHERE user_hash IS NOT NULL AND extra_json IS NOT NULL")
        .fetch_all(&storage.pool).await.map_err(|e| AppError::Internal(format!("summary by_kind: {}", e)))?;
    use std::collections::{HashMap, HashSet};
    let mut uniq: HashSet<(String, String)> = HashSet::new();
    for r in rows {
        let uh: String = match r.try_get("user_hash") { Ok(v) => v, Err(_) => continue };
        let ej: String = match r.try_get("extra_json") { Ok(v) => v, Err(_) => continue };
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(&ej) {
            if let Some(kind) = val.get("user_kind").and_then(|v| v.as_str()) {
                uniq.insert((uh.clone(), kind.to_string()));
            }
        }
    }
    let mut by_kind_map: HashMap<String, i64> = HashMap::new();
    for (_, k) in uniq.into_iter() { *by_kind_map.entry(k).or_insert(0) += 1; }
    let mut by_kind: Vec<(String, i64)> = by_kind_map.into_iter().collect();
    by_kind.sort_by(|a,b| b.1.cmp(&a.1));

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

fn parse_tz(name: &str) -> chrono_tz::Tz { name.parse::<chrono_tz::Tz>().unwrap_or(chrono_tz::Asia::Shanghai) }

fn convert_tz(ts_rfc3339: &str, tz: chrono_tz::Tz) -> Option<String> {
    let dt = chrono::DateTime::parse_from_rfc3339(ts_rfc3339).ok()?;
    let as_utc = dt.with_timezone(&chrono::Utc);
    Some(as_utc.with_timezone(&tz).to_rfc3339())
}
