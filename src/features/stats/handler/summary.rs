use axum::{extract::Query, extract::State, response::Json};
use chrono::{NaiveDateTime, NaiveTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::{error::AppError, state::AppState};

use super::{
    cache::{build_stats_summary_cache_key, stats_summary_cache},
    params::{normalize_top, parse_include_flags},
    time::{convert_tz, parse_date_bound_utc, resolve_timezone},
};

#[derive(Clone, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct FeatureUsageSummary {
    /// 功能名（可能值：bestn、bestn_user、single_query、save、song_search）。
    /// - bestn：生成 BestN 汇总图
    /// - bestn_user：生成用户自报 BestN 图片
    /// - single_query：生成单曲成绩图
    /// - save：获取并解析玩家存档
    /// - song_search：歌曲检索
    pub(super) feature: String,
    /// 事件计数
    pub(super) count: i64,
    /// 最近一次发生时间（本地时区 RFC3339）
    pub(super) last_at: Option<String>,
}

#[derive(Clone, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UniqueUsersSummary {
    /// 去敏后唯一用户总数
    pub(super) total: i64,
    /// 按用户来源/凭证类型聚合的唯一用户数，例如 ("official", 123)
    pub(super) by_kind: Vec<(String, i64)>,
}

#[derive(Deserialize)]
pub struct StatsSummaryQuery {
    /// 可选开始日期（YYYY-MM-DD，按 timezone 解释）
    pub(super) start: Option<String>,
    /// 可选结束日期（YYYY-MM-DD，按 timezone 解释）
    pub(super) end: Option<String>,
    /// 可选时区（IANA 名称，如 Asia/Shanghai），覆盖配置
    pub(super) timezone: Option<String>,
    /// 可选功能名过滤（仅影响 feature/unique_users/actions 等业务维度）
    pub(super) feature: Option<String>,
    /// 可选额外维度（csv）：routes,status,methods,instances,actions,latency,unique_ips,user_kinds,all
    pub(super) include: Option<String>,
    /// TopN（默认 20，最大 200）
    pub(super) top: Option<i64>,
}

#[derive(Clone, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RouteUsageSummary {
    pub(super) route: String,
    pub(super) count: i64,
    pub(super) err_count: i64,
    pub(super) last_at: Option<String>,
}

#[derive(Clone, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MethodUsageSummary {
    pub(super) method: String,
    pub(super) count: i64,
}

#[derive(Clone, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct StatusCodeSummary {
    pub(super) status: u16,
    pub(super) count: i64,
}

#[derive(Clone, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct InstanceUsageSummary {
    pub(super) instance: String,
    pub(super) count: i64,
    pub(super) last_at: Option<String>,
}

#[derive(Clone, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ActionUsageSummary {
    pub(super) feature: String,
    pub(super) action: String,
    pub(super) count: i64,
    pub(super) last_at: Option<String>,
}

#[derive(Clone, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LatencySummary {
    pub(super) sample_count: i64,
    pub(super) avg_ms: Option<f64>,
    pub(super) p50_ms: Option<i64>,
    pub(super) p95_ms: Option<i64>,
    pub(super) max_ms: Option<i64>,
}

#[derive(Clone, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct StatsSummaryResponse {
    /// 展示使用的时区（IANA 名称）
    pub(super) timezone: String,
    /// 配置中设置的统计起始时间（如有）
    pub(super) config_start_at: Option<String>,
    /// 全量事件中的最早时间（本地时区）
    pub(super) first_event_at: Option<String>,
    /// 全量事件中的最晚时间（本地时区）
    pub(super) last_event_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) range_start_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) range_end_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) feature_filter: Option<String>,
    /// 各功能使用概览
    pub(super) features: Vec<FeatureUsageSummary>,
    /// 唯一用户统计
    pub(super) unique_users: UniqueUsersSummary,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) events_total: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) http_total: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) http_errors: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) routes: Option<Vec<RouteUsageSummary>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) methods: Option<Vec<MethodUsageSummary>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) status_codes: Option<Vec<StatusCodeSummary>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) instances: Option<Vec<InstanceUsageSummary>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) actions: Option<Vec<ActionUsageSummary>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) latency: Option<LatencySummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) unique_ips: Option<i64>,
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
    // 两端都未指定时回退到热数据保留窗口，并取整到 UTC 当天 0 点以稳定缓存 key。
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
            super::super::storage::SummaryIncludeFlags {
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
