use std::collections::HashMap;

use chrono::{NaiveDate, Utc};

use crate::error::AppError;

use super::super::{models::DailyAggRow, storage::StatsStorage};
use super::{
    DailyDauRow, DailyFeatureUsageRow, DailyHttpRouteRow, DailyHttpTotalRow, LatencyAggRow,
    params::{DateBucket, LatencyAggFilters, LatencyBucket},
    time::{
        fixed_offset_minutes_for_range, month_start_day1, next_month_start, parse_date_bound_utc,
        sqlite_minutes_modifier, week_start_monday,
    },
};

fn rate(n: i64, d: i64) -> f64 {
    fn i64_to_f64_lossy(value: i64) -> f64 {
        value.to_string().parse::<f64>().unwrap_or_else(|_| {
            if value.is_negative() {
                f64::MIN
            } else {
                f64::MAX
            }
        })
    }

    if d <= 0 {
        0.0
    } else {
        i64_to_f64_lossy(n) / i64_to_f64_lossy(d)
    }
}

pub(super) async fn query_daily_agg(
    storage: &StatsStorage,
    tz: chrono_tz::Tz,
    start: NaiveDate,
    end: NaiveDate,
    feature: Option<&str>,
    route: Option<&str>,
    method: Option<&str>,
) -> Result<Vec<DailyAggRow>, AppError> {
    // 预聚合表按配置时区的本地日存储；查询时区与配置不一致时无法命中快速路径，
    // 直接按 events 查询（避免错位）。
    let cfg_tz: chrono_tz::Tz = crate::config::AppConfig::global()
        .stats
        .timezone
        .parse()
        .unwrap_or(chrono_tz::Asia::Shanghai);
    let fast_ok = tz == cfg_tz;
    let today = Utc::now().with_timezone(&cfg_tz).date_naive();
    let mut out: Vec<DailyAggRow> = Vec::new();

    // ── 历史日期（< today） ──
    if start < today {
        let agg_end = end.min(today - chrono::Duration::days(1));
        if agg_end >= start {
            if fast_ok {
                // 快速路径 + 覆盖完整性检查：daily_agg 缺失的天从 events 补齐，
                // 避免"部分预聚合"时缺失天被静默跳过（输出缺少该天或计数为 0）。
                let present = storage
                    .daily_agg_dates_in_range(&start.to_string(), &agg_end.to_string())
                    .await?;
                if present.is_empty() {
                    // 整个区间无预聚合：单次 events 查询（保持高效）
                    let hist_utc_start = parse_date_bound_utc(&start.to_string(), tz, false)?;
                    let hist_utc_end = parse_date_bound_utc(&agg_end.to_string(), tz, true)?;
                    if let Some(off_min) = fixed_offset_minutes_for_range(tz, start, agg_end) {
                        let modifier = sqlite_minutes_modifier(off_min);
                        out = storage
                            .query_daily_agg_with_offset(
                                &modifier,
                                &hist_utc_start,
                                &hist_utc_end,
                                feature,
                                route,
                                method,
                            )
                            .await?;
                    } else {
                        let mut cur = start;
                        while cur <= agg_end {
                            let day_start = parse_date_bound_utc(&cur.to_string(), tz, false)?;
                            let day_end = parse_date_bound_utc(&cur.to_string(), tz, true)?;
                            let rows = storage
                                .query_daily_agg_slice(&day_start, &day_end, feature, route, method)
                                .await?;
                            for r in rows {
                                out.push(DailyAggRow {
                                    date: cur.to_string(),
                                    feature: r.feature,
                                    route: r.route,
                                    method: r.method,
                                    count: r.count,
                                    err_count: r.err_count,
                                });
                            }
                            cur += chrono::Duration::days(1);
                        }
                    }
                } else {
                    // 部分覆盖：已聚合天用 fast 行，缺失天逐日从 events 补齐
                    let fast = storage
                        .query_daily_agg_fast(
                            &start.to_string(),
                            &agg_end.to_string(),
                            feature,
                            route,
                            method,
                        )
                        .await?;
                    out.extend(fast);
                    let mut cur = start;
                    while cur <= agg_end {
                        if !present.contains(&cur.to_string()) {
                            let day_start = parse_date_bound_utc(&cur.to_string(), tz, false)?;
                            let day_end = parse_date_bound_utc(&cur.to_string(), tz, true)?;
                            let rows = storage
                                .query_daily_agg_slice(&day_start, &day_end, feature, route, method)
                                .await?;
                            for r in rows {
                                out.push(DailyAggRow {
                                    date: cur.to_string(),
                                    feature: r.feature,
                                    route: r.route,
                                    method: r.method,
                                    count: r.count,
                                    err_count: r.err_count,
                                });
                            }
                        }
                        cur += chrono::Duration::days(1);
                    }
                    out.sort_by(|a, b| a.date.cmp(&b.date));
                }
            } else {
                // 时区与配置不一致：直接按 events 查询（正确性优先）
                let hist_utc_start = parse_date_bound_utc(&start.to_string(), tz, false)?;
                let hist_utc_end = parse_date_bound_utc(&agg_end.to_string(), tz, true)?;
                if let Some(off_min) = fixed_offset_minutes_for_range(tz, start, agg_end) {
                    let modifier = sqlite_minutes_modifier(off_min);
                    out = storage
                        .query_daily_agg_with_offset(
                            &modifier,
                            &hist_utc_start,
                            &hist_utc_end,
                            feature,
                            route,
                            method,
                        )
                        .await?;
                } else {
                    let mut cur = start;
                    while cur <= agg_end {
                        let day_start = parse_date_bound_utc(&cur.to_string(), tz, false)?;
                        let day_end = parse_date_bound_utc(&cur.to_string(), tz, true)?;
                        let rows = storage
                            .query_daily_agg_slice(&day_start, &day_end, feature, route, method)
                            .await?;
                        for r in rows {
                            out.push(DailyAggRow {
                                date: cur.to_string(),
                                feature: r.feature,
                                route: r.route,
                                method: r.method,
                                count: r.count,
                                err_count: r.err_count,
                            });
                        }
                        cur += chrono::Duration::days(1);
                    }
                    out.sort_by(|a, b| a.date.cmp(&b.date));
                }
            }
        }
    }

    // ── 今天及之后 — 逐天从 events 表读取
    let hot_start = today.max(start);
    if hot_start <= end {
        let mut cur = hot_start;
        while cur <= end {
            let day_start = parse_date_bound_utc(&cur.to_string(), tz, false)?;
            let day_end = parse_date_bound_utc(&cur.to_string(), tz, true)?;
            let rows = storage
                .query_daily_agg_slice(&day_start, &day_end, feature, route, method)
                .await?;
            for r in rows {
                out.push(DailyAggRow {
                    date: cur.to_string(),
                    feature: r.feature,
                    route: r.route,
                    method: r.method,
                    count: r.count,
                    err_count: r.err_count,
                });
            }
            cur += chrono::Duration::days(1);
        }

        if fixed_offset_minutes_for_range(tz, start, end).is_none() {
            out.sort_by(|a, b| a.date.cmp(&b.date));
        }
    }

    Ok(out)
}

pub(super) async fn query_latency_agg(
    storage: &StatsStorage,
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

    match bucket {
        LatencyBucket::Day => {
            let start_utc = parse_date_bound_utc(&start.to_string(), tz, false)?;
            let end_utc = parse_date_bound_utc(&end.to_string(), tz, true)?;

            // 预聚合快速路径：daily_latency 按配置时区本地日存储；仅当查询时区与
            // 配置一致且区间被完整覆盖时使用（缺失天从 events 补齐，避免静默为 0）。
            let cfg_tz: chrono_tz::Tz = crate::config::AppConfig::global()
                .stats
                .timezone
                .parse()
                .unwrap_or(chrono_tz::Asia::Shanghai);
            if tz == cfg_tz {
                let start_s = start.to_string();
                let end_s = end.to_string();
                let present = storage
                    .daily_latency_dates_in_range(&start_s, &end_s)
                    .await?;
                if !present.is_empty() {
                    let rows = storage
                        .query_latency_agg_fast(&start_s, &end_s, feature, route, method)
                        .await?;
                    let mut out = Vec::with_capacity(rows.len());
                    for r in rows {
                        out.push(LatencyAggRow {
                            bucket: r.bucket,
                            feature: r.feature,
                            route: r.route,
                            method: r.method,
                            count: r.count,
                            min_ms: r.min_ms,
                            avg_ms: r.avg_ms,
                            max_ms: r.max_ms,
                        });
                    }
                    // 缺失天（未预聚合）从 events 补齐
                    let mut cur = start;
                    while cur <= end {
                        if !present.contains(&cur.to_string()) {
                            let day_start = parse_date_bound_utc(&cur.to_string(), tz, false)?;
                            let day_end = parse_date_bound_utc(&cur.to_string(), tz, true)?;
                            let srows = storage
                                .query_latency_agg_slice(
                                    &day_start, &day_end, feature, route, method,
                                )
                                .await
                                .map_err(|e| {
                                    AppError::Internal(format!(
                                        "latency agg day (preagg 补齐): {e}"
                                    ))
                                })?;
                            for r in srows {
                                out.push(LatencyAggRow {
                                    bucket: cur.to_string(),
                                    feature: r.feature,
                                    route: r.route,
                                    method: r.method,
                                    count: r.count,
                                    min_ms: r.min_ms,
                                    avg_ms: r.avg_ms,
                                    max_ms: r.max_ms,
                                });
                            }
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
                    return Ok(out);
                }
                // present 为空：整体回退到下面的 events 查询
            }

            // fixed-offset（无 DST）优化：单 SQL 按天分组
            if let Some(off_min) = fixed_offset_minutes_for_range(tz, start, end) {
                let modifier = sqlite_minutes_modifier(off_min);
                let rows = storage
                    .query_latency_agg_with_offset(
                        &modifier, &start_utc, &end_utc, feature, route, method,
                    )
                    .await
                    .map_err(|e| AppError::Internal(format!("latency agg day: {e}")))?;

                let mut out = Vec::with_capacity(rows.len());
                for r in rows {
                    out.push(LatencyAggRow {
                        bucket: r.bucket,
                        feature: r.feature,
                        route: r.route,
                        method: r.method,
                        count: r.count,
                        min_ms: r.min_ms,
                        avg_ms: r.avg_ms,
                        max_ms: r.max_ms,
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
                let rows = storage
                    .query_latency_agg_slice(&day_start, &day_end, feature, route, method)
                    .await
                    .map_err(|e| AppError::Internal(format!("latency agg day (fallback): {e}")))?;

                for r in rows {
                    out.push(LatencyAggRow {
                        bucket: cur.to_string(),
                        feature: r.feature,
                        route: r.route,
                        method: r.method,
                        count: r.count,
                        min_ms: r.min_ms,
                        avg_ms: r.avg_ms,
                        max_ms: r.max_ms,
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
                let rows = storage
                    .query_latency_agg_slice(&start_utc, &end_utc, feature, route, method)
                    .await
                    .map_err(|e| AppError::Internal(format!("latency agg week: {e}")))?;

                for r in rows {
                    out.push(LatencyAggRow {
                        bucket: b.label.clone(),
                        feature: r.feature,
                        route: r.route,
                        method: r.method,
                        count: r.count,
                        min_ms: r.min_ms,
                        avg_ms: r.avg_ms,
                        max_ms: r.max_ms,
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
                let rows = storage
                    .query_latency_agg_slice(&start_utc, &end_utc, feature, route, method)
                    .await
                    .map_err(|e| AppError::Internal(format!("latency agg month: {e}")))?;

                for r in rows {
                    out.push(LatencyAggRow {
                        bucket: b.label.clone(),
                        feature: r.feature,
                        route: r.route,
                        method: r.method,
                        count: r.count,
                        min_ms: r.min_ms,
                        avg_ms: r.avg_ms,
                        max_ms: r.max_ms,
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

pub(super) async fn query_daily_feature_usage(
    storage: &StatsStorage,
    tz: chrono_tz::Tz,
    start: NaiveDate,
    end: NaiveDate,
    feature: Option<&str>,
) -> Result<Vec<DailyFeatureUsageRow>, AppError> {
    let start_utc = parse_date_bound_utc(&start.to_string(), tz, false)?;
    let end_utc = parse_date_bound_utc(&end.to_string(), tz, true)?;

    if let Some(off_min) = fixed_offset_minutes_for_range(tz, start, end) {
        let modifier = sqlite_minutes_modifier(off_min);
        let rows = storage
            .query_daily_feature_usage_with_offset(&modifier, &start_utc, &end_utc, feature)
            .await
            .map_err(|e| AppError::Internal(format!("daily features: {e}")))?;

        let mut out = Vec::with_capacity(rows.len());
        for r in rows {
            out.push(DailyFeatureUsageRow {
                date: r.date,
                feature: r.feature,
                count: r.count,
                unique_users: r.unique_users,
            });
        }
        return Ok(out);
    }

    let mut out = Vec::new();
    let mut cur = start;
    while cur <= end {
        let day_start = parse_date_bound_utc(&cur.to_string(), tz, false)?;
        let day_end = parse_date_bound_utc(&cur.to_string(), tz, true)?;
        let rows = storage
            .query_daily_feature_usage_slice(&day_start, &day_end, feature)
            .await
            .map_err(|e| AppError::Internal(format!("daily features (fallback): {e}")))?;

        for r in rows {
            out.push(DailyFeatureUsageRow {
                date: cur.to_string(),
                feature: r.feature,
                count: r.count,
                unique_users: r.unique_users,
            });
        }
        cur += chrono::Duration::days(1);
    }

    out.sort_by(|a, b| a.date.cmp(&b.date));
    Ok(out)
}

pub(super) async fn query_daily_dau(
    storage: &StatsStorage,
    tz: chrono_tz::Tz,
    start: NaiveDate,
    end: NaiveDate,
) -> Result<Vec<DailyDauRow>, AppError> {
    // 预聚合表按配置时区的本地日存储；查询时区与配置不一致时无法命中快速路径，
    // 直接按 events 查询（避免错位）。
    let cfg_tz: chrono_tz::Tz = crate::config::AppConfig::global()
        .stats
        .timezone
        .parse()
        .unwrap_or(chrono_tz::Asia::Shanghai);
    let fast_ok = tz == cfg_tz;
    let today = Utc::now().with_timezone(&cfg_tz).date_naive();
    let mut map: HashMap<String, (i64, i64)> = HashMap::new();

    // ── 历史日期（< today） ──
    if start < today {
        let agg_end = end.min(today - chrono::Duration::days(1));
        if agg_end >= start {
            if fast_ok {
                // 快速路径 + 覆盖完整性检查：daily_dau 缺失的天从 events 补齐，
                // 避免"部分预聚合"时缺失天被错误输出为 0（即使 events 有数据）。
                let present = storage
                    .daily_dau_dates_in_range(&start.to_string(), &agg_end.to_string())
                    .await?;
                if present.is_empty() {
                    // 整个区间无预聚合：单次 events 查询（保持高效）
                    let hist_utc_start = parse_date_bound_utc(&start.to_string(), tz, false)?;
                    let hist_utc_end = parse_date_bound_utc(&agg_end.to_string(), tz, true)?;
                    if let Some(off_min) = fixed_offset_minutes_for_range(tz, start, agg_end) {
                        let modifier = sqlite_minutes_modifier(off_min);
                        let rows = storage
                            .query_daily_dau_with_offset(&modifier, &hist_utc_start, &hist_utc_end)
                            .await?;
                        for r in rows {
                            map.insert(r.date, (r.active_users, r.active_ips));
                        }
                    } else {
                        let mut cur = start;
                        while cur <= agg_end {
                            let day_start = parse_date_bound_utc(&cur.to_string(), tz, false)?;
                            let day_end = parse_date_bound_utc(&cur.to_string(), tz, true)?;
                            let (u, ip) =
                                storage.query_daily_dau_slice(&day_start, &day_end).await?;
                            map.insert(cur.to_string(), (u, ip));
                            cur += chrono::Duration::days(1);
                        }
                    }
                } else {
                    // 部分覆盖：已聚合天用 fast 行，缺失天逐日从 events 补齐
                    let agged = storage
                        .query_daily_dau_fast(&start.to_string(), &agg_end.to_string())
                        .await?;
                    for r in agged {
                        map.insert(r.date, (r.active_users, r.active_ips));
                    }
                    let mut cur = start;
                    while cur <= agg_end {
                        if !present.contains(&cur.to_string()) {
                            let day_start = parse_date_bound_utc(&cur.to_string(), tz, false)?;
                            let day_end = parse_date_bound_utc(&cur.to_string(), tz, true)?;
                            let (u, ip) =
                                storage.query_daily_dau_slice(&day_start, &day_end).await?;
                            map.insert(cur.to_string(), (u, ip));
                        }
                        cur += chrono::Duration::days(1);
                    }
                }
            } else {
                // 时区与配置不一致：直接按 events 查询（正确性优先）
                let hist_utc_start = parse_date_bound_utc(&start.to_string(), tz, false)?;
                let hist_utc_end = parse_date_bound_utc(&agg_end.to_string(), tz, true)?;
                if let Some(off_min) = fixed_offset_minutes_for_range(tz, start, agg_end) {
                    let modifier = sqlite_minutes_modifier(off_min);
                    let rows = storage
                        .query_daily_dau_with_offset(&modifier, &hist_utc_start, &hist_utc_end)
                        .await?;
                    for r in rows {
                        map.insert(r.date, (r.active_users, r.active_ips));
                    }
                } else {
                    let mut cur = start;
                    while cur <= agg_end {
                        let day_start = parse_date_bound_utc(&cur.to_string(), tz, false)?;
                        let day_end = parse_date_bound_utc(&cur.to_string(), tz, true)?;
                        let (u, ip) = storage.query_daily_dau_slice(&day_start, &day_end).await?;
                        map.insert(cur.to_string(), (u, ip));
                        cur += chrono::Duration::days(1);
                    }
                }
            }
        }
    }

    // ── 今天及之后 — 逐天从 events 表读取
    let hot_start = today.max(start);
    if hot_start <= end {
        let mut cur = hot_start;
        while cur <= end {
            let day_start = parse_date_bound_utc(&cur.to_string(), tz, false)?;
            let day_end = parse_date_bound_utc(&cur.to_string(), tz, true)?;
            let (u, ip) = storage.query_daily_dau_slice(&day_start, &day_end).await?;
            map.insert(cur.to_string(), (u, ip));
            cur += chrono::Duration::days(1);
        }
    }

    // 兜底补齐
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

pub(super) async fn query_daily_http(
    storage: &StatsStorage,
    tz: chrono_tz::Tz,
    start: NaiveDate,
    end: NaiveDate,
    route: Option<&str>,
    method: Option<&str>,
    top_per_day: i64,
) -> Result<(Vec<DailyHttpTotalRow>, Vec<DailyHttpRouteRow>), AppError> {
    let start_utc = parse_date_bound_utc(&start.to_string(), tz, false)?;
    let end_utc = parse_date_bound_utc(&end.to_string(), tz, true)?;
    let fixed_offset = fixed_offset_minutes_for_range(tz, start, end);

    let mut route_rows: Vec<DailyHttpRouteRow> = Vec::new();
    let mut totals_map: HashMap<String, (i64, i64, i64, i64)> = HashMap::new();

    if let Some(off_min) = fixed_offset {
        let modifier = sqlite_minutes_modifier(off_min);
        let modifier_ref = modifier.as_str();
        let routes_fut = async {
            storage
                .query_daily_http_routes_with_offset(
                    modifier_ref,
                    &start_utc,
                    &end_utc,
                    route,
                    method,
                    top_per_day,
                )
                .await
                .map_err(|e| AppError::Internal(format!("daily http routes: {e}")))
        };
        let totals_fut = async {
            storage
                .query_daily_http_totals_with_offset(
                    modifier_ref,
                    &start_utc,
                    &end_utc,
                    route,
                    method,
                )
                .await
                .map_err(|e| AppError::Internal(format!("daily http totals: {e}")))
        };
        let (rows, total_rows) = tokio::try_join!(routes_fut, totals_fut)?;

        route_rows.reserve(rows.len());
        for r in rows {
            let date = r.date;
            let route = r.route;
            let method = r.method;
            let total = r.total;
            let errors = r.errors;
            let client_errors = r.client_errors;
            let server_errors = r.server_errors;
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

        for r in total_rows {
            let date = r.date;
            let total = r.total;
            let errors = r.errors;
            let client_errors = r.client_errors;
            let server_errors = r.server_errors;
            totals_map.insert(date, (total, errors, client_errors, server_errors));
        }
    } else {
        // DST fallback: per-day query
        let mut cur = start;
        while cur <= end {
            let day_start = parse_date_bound_utc(&cur.to_string(), tz, false)?;
            let day_end = parse_date_bound_utc(&cur.to_string(), tz, true)?;
            let routes_fut = async {
                if top_per_day > 0 {
                    storage
                        .query_daily_http_route_slice_top(
                            &day_start,
                            &day_end,
                            route,
                            method,
                            top_per_day,
                        )
                        .await
                } else {
                    storage
                        .query_daily_http_route_slice(&day_start, &day_end, route, method)
                        .await
                }
                .map_err(|e| AppError::Internal(format!("daily http routes (fallback): {e}")))
            };
            let totals_fut = async {
                storage
                    .query_daily_http_total_slice(&day_start, &day_end, route, method)
                    .await
                    .map_err(|e| AppError::Internal(format!("daily http totals (fallback): {e}")))
            };
            let (rows, total_row) = tokio::try_join!(routes_fut, totals_fut)?;
            totals_map.insert(cur.to_string(), total_row);

            for r in rows {
                let route = r.route;
                let method = r.method;
                let total = r.total;
                let errors = r.errors;
                let client_errors = r.client_errors;
                let server_errors = r.server_errors;
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

        route_rows.sort_by(|a, b| {
            a.date
                .cmp(&b.date)
                .then(b.errors.cmp(&a.errors))
                .then(b.total.cmp(&a.total))
                .then(a.route.cmp(&b.route))
                .then(a.method.cmp(&b.method))
        });
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

    Ok((totals, route_rows))
}
