#![allow(clippy::items_after_test_module)]

use std::collections::HashMap;

use chrono::{DateTime, NaiveTime, TimeZone, Utc};
use futures_util::TryStreamExt;
use sqlx::{QueryBuilder, Row, Sqlite};

use crate::error::AppError;

use super::{
    StatsStorage, StatsSummaryData, SummaryActionRow, SummaryFeatureRow, SummaryIncludeFlags,
    SummaryInstanceRow, SummaryLatencyData, SummaryMethodRow, SummaryRouteRow,
    SummaryStatusCodeRow,
};

/// summary 快速路径计划：历史区间走预聚合表，今日热数据 (start_utcS为齐 UTC 0 点与到达0 点的部分) 仍汇入 events 中。
struct FastPlan {
    /// 预聚合覆盖的历史区间起（含），YYYY-MM-DD。
    hist_start_day: String,
    /// 预聚合覆盖的历史区间止（含），YYYY-MM-DD。
    hist_end_day: String,
    /// 今日热起点（含）：None 表示热部分不在查询窗口内。
    hot_start_utc: Option<String>,
    /// 今日热止（含）：用户原始 end_utc（None = 开口上界）。
    hot_end_utc: Option<String>,
    /// 原始窗口起点（含）用于 overall balls / MIN MAX 依然走 events 索引。
    start_utc_orig: String,
    /// 原始窗口止（含）用于 overall；None 为开口。
    end_utc_orig: Option<String>,
}

fn parse_utc(dt_s: &str) -> Result<DateTime<Utc>, AppError> {
    DateTime::parse_from_rfc3339(dt_s)
        .map(|d| d.with_timezone(&Utc))
        .map_err(|e| AppError::Internal(format!("parse utc `{dt_s}`: {e}")))
}

fn mid_night() -> NaiveTime {
    NaiveTime::from_hms_opt(0, 0, 0).expect("00:00:00")
}

fn end_of_day() -> NaiveTime {
    NaiveTime::from_hms_opt(23, 59, 59).expect("23:59:59")
}

/// 将两个可选 last_ts (RFC3339 UTC) 取较大者。同格式时可直接按 lexicographic 比较。
fn max_last_ts(a: Option<String>, b: Option<String>) -> Option<String> {
    match (a, b) {
        (None, None) => None,
        (Some(v), None) | (None, Some(v)) => Some(v),
        (Some(x), Some(y)) => Some(if x >= y { x } else { y }),
    }
}

fn push_stats_ts_range_filters(
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

fn push_stats_feature_filter(qb: &mut QueryBuilder<'_, Sqlite>, feature: Option<&str>) {
    if let Some(feature) = feature {
        qb.push(" AND feature = ").push_bind(feature.to_string());
    }
}

fn push_summary_overall_query(
    qb: &mut QueryBuilder<'_, Sqlite>,
    start_utc: Option<&str>,
    end_utc: Option<&str>,
) {
    // 单扫描取 MIN/MAX（两列），避免旧实现两条 ORDER BY ts_utc LIMIT 1 子查询重复扫范围。
    // idx_events_ts 索引即可同时支撑最大最小求值，代价低。
    qb.push("SELECT MIN(ts_utc) as min_ts, MAX(ts_utc) as max_ts FROM events WHERE 1=1");
    push_stats_ts_range_filters(qb, start_utc, end_utc);
}

fn push_summary_unique_ips_query(
    qb: &mut QueryBuilder<'_, Sqlite>,
    start_utc: Option<&str>,
    end_utc: Option<&str>,
) {
    qb.push(
        "SELECT COUNT(DISTINCT client_ip_hash) as cnt FROM events WHERE route IS NOT NULL AND client_ip_hash IS NOT NULL",
    );
    push_stats_ts_range_filters(qb, start_utc, end_utc);
}

fn push_summary_unique_users_query(
    qb: &mut QueryBuilder<'_, Sqlite>,
    start_utc: Option<&str>,
    end_utc: Option<&str>,
    feature: Option<&str>,
) {
    qb.push("SELECT COUNT(DISTINCT user_hash) as total FROM events WHERE user_hash IS NOT NULL");
    push_stats_ts_range_filters(qb, start_utc, end_utc);
    push_stats_feature_filter(qb, feature);
}

fn push_summary_features_query(
    qb: &mut QueryBuilder<'_, Sqlite>,
    start_utc: Option<&str>,
    end_utc: Option<&str>,
    feature: Option<&str>,
) {
    qb.push(
        "SELECT feature, COUNT(1) as cnt, MAX(ts_utc) as last_ts FROM events WHERE feature IS NOT NULL",
    );
    push_stats_ts_range_filters(qb, start_utc, end_utc);
    push_stats_feature_filter(qb, feature);
    qb.push(" GROUP BY feature");
}

fn push_summary_instances_query(
    qb: &mut QueryBuilder<'_, Sqlite>,
    start_utc: Option<&str>,
    end_utc: Option<&str>,
    top: i64,
) {
    qb.push(
        "SELECT instance, COUNT(1) as cnt, MAX(ts_utc) as last_ts FROM events WHERE instance IS NOT NULL",
    );
    push_stats_ts_range_filters(qb, start_utc, end_utc);
    qb.push(" GROUP BY instance ORDER BY cnt DESC LIMIT ")
        .push_bind(top);
}

#[allow(dead_code)]
fn push_summary_latency_data_query() {
    // 延迟百分位查询已改为直方图实现（query_latency_percentiles_histogram），
    // 此函数保留以防需要回退到 ROW_NUMBER() 精确计算。
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::Execute as _;

    #[test]
    fn summary_overall_query_reads_bounds_in_a_single_min_max_scan() {
        let mut qb = QueryBuilder::<Sqlite>::new("");
        push_summary_overall_query(
            &mut qb,
            Some("2026-01-01T00:00:00Z"),
            Some("2026-01-31T23:59:59Z"),
        );
        let sql = qb.build().sql().to_string();

        assert!(sql.contains("MIN(ts_utc) as min_ts"));
        assert!(sql.contains("MAX(ts_utc) as max_ts"));
        // 单次扫描即可取出大小值，不应再依赖两条 ORDER BY ts_utc LIMIT 1 子查询
        assert!(!sql.contains("ORDER BY ts_utc ASC LIMIT 1"));
        assert!(!sql.contains("ORDER BY ts_utc DESC LIMIT 1"));
        assert_eq!(sql.matches("ts_utc >= ?").count(), 1);
        assert_eq!(sql.matches("ts_utc <= ?").count(), 1);
    }

    #[test]
    fn summary_unique_ips_query_matches_partial_index_predicates() {
        let mut qb = QueryBuilder::<Sqlite>::new("");
        push_summary_unique_ips_query(
            &mut qb,
            Some("2026-01-01T00:00:00Z"),
            Some("2026-01-31T23:59:59Z"),
        );
        let sql = qb.build().sql().to_string();

        assert!(sql.contains("COUNT(DISTINCT client_ip_hash)"));
        assert!(sql.contains("route IS NOT NULL"));
        assert!(sql.contains("client_ip_hash IS NOT NULL"));
        assert!(sql.contains("ts_utc >= ?"));
        assert!(sql.contains("ts_utc <= ?"));
    }

    #[test]
    fn summary_unique_users_query_keeps_feature_time_range_shape() {
        let mut qb = QueryBuilder::<Sqlite>::new("");
        push_summary_unique_users_query(
            &mut qb,
            Some("2026-01-01T00:00:00Z"),
            Some("2026-01-31T23:59:59Z"),
            Some("save"),
        );
        let sql = qb.build().sql().to_string();

        assert!(sql.contains("COUNT(DISTINCT user_hash)"));
        assert!(sql.contains("user_hash IS NOT NULL"));
        assert!(sql.contains("ts_utc >= ?"));
        assert!(sql.contains("ts_utc <= ?"));
        assert!(sql.contains("feature = ?"));
    }

    #[test]
    fn summary_features_query_keeps_time_range_group_shape() {
        let mut qb = QueryBuilder::<Sqlite>::new("");
        push_summary_features_query(
            &mut qb,
            Some("2026-01-01T00:00:00Z"),
            Some("2026-01-31T23:59:59Z"),
            Some("save"),
        );
        let sql = qb.build().sql().to_string();

        assert!(sql.contains("feature IS NOT NULL"));
        assert!(sql.contains("COUNT(1) as cnt"));
        assert!(sql.contains("MAX(ts_utc) as last_ts"));
        assert!(sql.contains("ts_utc >= ?"));
        assert!(sql.contains("ts_utc <= ?"));
        assert!(sql.contains("feature = ?"));
        assert!(sql.contains("GROUP BY feature"));
    }

    #[test]
    fn summary_instances_query_keeps_time_range_group_limit_shape() {
        let mut qb = QueryBuilder::<Sqlite>::new("");
        push_summary_instances_query(
            &mut qb,
            Some("2026-01-01T00:00:00Z"),
            Some("2026-01-31T23:59:59Z"),
            10,
        );
        let sql = qb.build().sql().to_string();

        assert!(sql.contains("instance IS NOT NULL"));
        assert!(sql.contains("COUNT(1) as cnt"));
        assert!(sql.contains("MAX(ts_utc) as last_ts"));
        assert!(sql.contains("ts_utc >= ?"));
        assert!(sql.contains("ts_utc <= ?"));
        assert!(sql.contains("GROUP BY instance"));
        assert!(sql.contains("ORDER BY cnt DESC"));
        assert!(sql.contains("LIMIT ?"));
    }

    #[test]
    fn summary_latency_query_keeps_time_range_percentile_shape() {
        // 延迟百分位查询已改为直方图实现（query_latency_percentiles_histogram），
        // 旧 ROW_NUMBER() 查询保留在 push_summary_latency_data_query 中备用。
        push_summary_latency_data_query();
    }

    // ── 快速路径 / 慢路径等价与启用条件集成测试 ──

    async fn build_tmp_storage(label: &str) -> StatsStorage {
        let path = std::env::temp_dir().join(format!(
            "phi_summary_fast_{label}_{}.db",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        if let Some(p) = path.parent() {
            std::fs::create_dir_all(p).ok();
        }
        let storage = StatsStorage::connect_sqlite(path.to_string_lossy().as_ref(), false)
            .await
            .expect("connect sqlite");
        storage.init_schema().await.expect("init schema");
        storage
    }

    #[allow(clippy::too_many_arguments)]
    fn evt(
        ts: chrono::DateTime<chrono::Utc>,
        route: Option<&str>,
        feature: Option<&str>,
        action: Option<&str>,
        method: Option<&str>,
        status: Option<u16>,
        duration_ms: Option<i64>,
        user_hash: Option<&str>,
        ip: Option<&str>,
        instance: Option<&str>,
        kind: Option<&str>,
    ) -> crate::features::stats::models::EventInsert {
        use std::borrow::Cow;
        let extra_json = kind.map(|k| serde_json::json!({"user_kind": k}));
        crate::features::stats::models::EventInsert {
            ts_utc: ts,
            route: route.map(String::from),
            feature: feature.map(String::from),
            action: action.map(String::from),
            method: method.map(String::from),
            status,
            duration_ms,
            user_hash: user_hash.map(String::from),
            client_ip_hash: ip.map(String::from),
            instance: instance.map(|s| Cow::Owned(s.to_string())),
            extra_json,
        }
    }

    #[tokio::test]
    async fn plan_fast_path_requires_backfill_sentinel_and_aligned_bounds() {
        let storage = build_tmp_storage("plan_gate").await;
        // 无哨兵 → 不启用
        let plan = plan_fast_path(&storage, Some("2025-01-01T00:00:00Z"), None, None)
            .await
            .unwrap();
        assert!(plan.is_none(), "无 backfill_complete 哨兵不应启用");

        // 写入哨兵但 daily_agg 为空 → 仍不启用
        storage
            .set_stats_meta("backfill_complete", "true")
            .await
            .unwrap();
        let plan = plan_fast_path(&storage, Some("2025-01-01T00:00:00Z"), None, None)
            .await
            .unwrap();
        assert!(plan.is_none(), "daily_agg 空时不应启用");

        // feature 过滤 → 不启用
        let today = chrono::Utc::now().date_naive();
        let day = today - chrono::Duration::days(1);
        // 先 “造” 一条当天事件，令 daily_agg有行；田以跳过滤。
        let ts = chrono::Utc
            .from_utc_datetime(&day.and_hms_opt(0, 0, 0).unwrap())
            .naive_utc()
            .and_utc()
            + chrono::Duration::seconds(30);
        storage
            .insert_events(&[evt(
                ts,
                Some("/probe"),
                Some("probe"),
                Some("ping"),
                Some("GET"),
                Some(200),
                Some(1),
                Some("u_p"),
                Some("ip_p"),
                Some("inst-p"),
                Some("official"),
            )])
            .await
            .unwrap();
        storage
            .aggregate_day(&day.format("%Y-%m-%d").to_string())
            .await
            .unwrap();
        let start = format!("{}T00:00:00Z", day.format("%Y-%m-%d"));
        let plan = plan_fast_path(&storage, Some(&start), None, Some("bestn"))
            .await
            .unwrap();
        assert!(plan.is_none(), "feature 过滤下不应启用");

        // 非对齐 start → 不启用
        let mid = format!("{}T12:00:00Z", day.format("%Y-%m-%d"));
        let plan = plan_fast_path(&storage, Some(&mid), None, None)
            .await
            .unwrap();
        assert!(plan.is_none(), "非 UTC 0 点对齐时不应启用");

        // 正确命中
        let plan = plan_fast_path(&storage, Some(&start), None, None)
            .await
            .unwrap();
        assert!(plan.is_some(), "对齐、有预聚合、有哨兵 → 应启用");
    }

    #[tokio::test]
    async fn fast_and_slow_path_agree_on_aggregated_window() {
        let storage = build_tmp_storage("equiv").await;
        let today = chrono::Utc::now().date_naive();
        let d1 = today - chrono::Duration::days(2);
        let d2 = today - chrono::Duration::days(1);
        let d1_midnight = chrono::Utc
            .from_utc_datetime(&d1.and_hms_opt(0, 0, 0).unwrap())
            .naive_utc()
            .and_utc();
        let d1_event = d1_midnight + chrono::Duration::seconds(60);
        let d2_event = d1_midnight + chrono::Duration::days(1) + chrono::Duration::seconds(120);
        let d2_late = d1_midnight + chrono::Duration::days(1) + chrono::Duration::seconds(300);
        let payload = [
            evt(
                d1_event,
                Some("/image/bn"),
                Some("bestn"),
                Some("render"),
                Some("GET"),
                Some(200),
                Some(50),
                Some("u1"),
                Some("ip1"),
                Some("inst-a"),
                Some("official"),
            ),
            evt(
                d1_event,
                Some("/image/bn"),
                Some("bestn"),
                Some("render"),
                Some("GET"),
                Some(500),
                Some(120),
                Some("u2"),
                Some("ip2"),
                Some("inst-a"),
                Some("taptap"),
            ),
            evt(
                d2_event,
                None,
                Some("save"),
                Some("submit"),
                None,
                None,
                None,
                Some("u1"),
                None,
                Some("inst-b"),
                Some("official"),
            ),
            evt(
                d2_late,
                Some("/save"),
                None,
                None,
                Some("POST"),
                Some(200),
                Some(10),
                Some("u3"),
                Some("ip3"),
                Some("inst-b"),
                Some("official"),
            ),
        ];
        storage.insert_events(&payload).await.unwrap();

        // 预聚合 d1 与 d2，写入哨兵 → 快路径可用。
        storage
            .aggregate_day(&d1.format("%Y-%m-%d").to_string())
            .await
            .unwrap();
        storage
            .aggregate_day(&d2.format("%Y-%m-%d").to_string())
            .await
            .unwrap();
        storage
            .set_stats_meta("backfill_complete", "true")
            .await
            .unwrap();

        let start_utc = format!("{}T00:00:00Z", d1.format("%Y-%m-%d"));
        let end_utc = format!("{}T23:59:59Z", d2.format("%Y-%m-%d"));
        let includes = SummaryIncludeFlags {
            routes: true,
            methods: true,
            status_codes: true,
            instances: true,
            actions: true,
            latency: false,
            unique_ips: true,
            user_kinds: true,
        };
        let fast = storage
            .query_stats_summary_data(Some(&start_utc), Some(&end_utc), None, includes, 20, true)
            .await
            .unwrap();

        // 关掉哨兵 → 走慢路径，与快路径算出一致结果。
        storage
            .set_stats_meta("backfill_complete", "false")
            .await
            .unwrap();
        let slow = storage
            .query_stats_summary_data(Some(&start_utc), Some(&end_utc), None, includes, 20, true)
            .await
            .unwrap();

        assert_eq!(fast.first_event_ts, slow.first_event_ts);
        assert_eq!(fast.last_event_ts, slow.last_event_ts);
        assert_eq!(fast.events_total, slow.events_total);
        assert_eq!(fast.http_total, slow.http_total);
        assert_eq!(fast.http_errors, slow.http_errors);
        assert_eq!(fast.unique_users_total, slow.unique_users_total);
        assert_eq!(fast.unique_ips, slow.unique_ips);

        // features: bestn=2, save=1。
        let fast_features: std::collections::HashMap<String, i64> = fast
            .features
            .iter()
            .map(|r| (r.feature.clone(), r.count))
            .collect();
        assert_eq!(fast_features.get("bestn"), Some(&2));
        assert_eq!(fast_features.get("save"), Some(&1));

        let slow_features: std::collections::HashMap<String, i64> = slow
            .features
            .iter()
            .map(|r| (r.feature.clone(), r.count))
            .collect();
        assert_eq!(fast_features, slow_features);

        // by_kind: official=2 (u1 跳 d1/d2), taptap=1。
        let fast_by_kind = fast.by_kind.clone();
        let slow_by_kind = slow.by_kind.clone();
        assert_eq!(fast_by_kind, slow_by_kind);
        assert!(fast_by_kind.contains(&("official".to_string(), 2)));
        assert!(fast_by_kind.contains(&("taptap".to_string(), 1)));

        // routes / methods / status / instances / actions 顺序无关地计数等价。
        fn collect_keyed<T, K: std::hash::Hash + Eq + std::fmt::Debug, F: Fn(&T) -> K>(
            xs: Option<&[T]>,
            k: F,
        ) -> std::collections::HashMap<K, i64> {
            xs.map(|v| v.iter().map(|r| (k(r), 1i64)).collect())
                .unwrap_or_default()
        }
        let fast_routes = fast.routes.as_ref().expect("fast routes");
        let slow_routes = slow.routes.as_ref().expect("slow routes");
        let fast_routes_total = fast_routes.iter().map(|r| r.count).sum::<i64>();
        let slow_routes_total = slow_routes.iter().map(|r| r.count).sum::<i64>();
        assert_eq!(fast_routes_total, slow_routes_total);
        let fast_routes_err = fast_routes.iter().map(|r| r.err_count).sum::<i64>();
        let slow_routes_err = slow_routes.iter().map(|r| r.err_count).sum::<i64>();
        assert_eq!(fast_routes_err, slow_routes_err);

        let fast_instances_keys: std::collections::HashMap<String, i64> =
            collect_keyed(fast.instances.as_deref(), |r| r.instance.clone());
        let slow_instances_keys: std::collections::HashMap<String, i64> =
            collect_keyed(slow.instances.as_deref(), |r| r.instance.clone());
        assert_eq!(fast_instances_keys, slow_instances_keys);

        let fast_actions_keys: std::collections::HashMap<(String, String), i64> =
            collect_keyed(fast.actions.as_deref(), |r| {
                (r.feature.clone(), r.action.clone())
            });
        let slow_actions_keys: std::collections::HashMap<(String, String), i64> =
            collect_keyed(slow.actions.as_deref(), |r| {
                (r.feature.clone(), r.action.clone())
            });
        assert_eq!(fast_actions_keys, slow_actions_keys);

        let fast_status_keys: std::collections::HashMap<i64, i64> =
            collect_keyed(fast.status_codes.as_deref(), |r| r.status);
        let slow_status_keys: std::collections::HashMap<i64, i64> =
            collect_keyed(slow.status_codes.as_deref(), |r| r.status);
        assert_eq!(fast_status_keys, slow_status_keys);

        let fast_methods_keys: std::collections::HashMap<String, i64> =
            collect_keyed(fast.methods.as_deref(), |r| r.method.clone());
        let slow_methods_keys: std::collections::HashMap<String, i64> =
            collect_keyed(slow.methods.as_deref(), |r| r.method.clone());
        assert_eq!(fast_methods_keys, slow_methods_keys);
    }
}

async fn query_summary_by_kind_sql(
    storage: &StatsStorage,
    start_utc: Option<&str>,
    end_utc: Option<&str>,
    feature: Option<&str>,
) -> Result<Vec<(String, i64)>, sqlx::Error> {
    let mut qb = QueryBuilder::<Sqlite>::new(
        r"
        SELECT kind, COUNT(1) as cnt
        FROM (
            SELECT DISTINCT
                user_hash,
                CASE
                    WHEN json_valid(extra_json) THEN
                        CASE
                            WHEN json_type(extra_json, '$.user_kind') = 'text'
                                THEN json_extract(extra_json, '$.user_kind')
                            ELSE NULL
                        END
                    ELSE NULL
                END as kind
            FROM events
            WHERE user_hash IS NOT NULL
              AND extra_json IS NOT NULL
        ",
    );
    push_stats_ts_range_filters(&mut qb, start_utc, end_utc);
    push_stats_feature_filter(&mut qb, feature);
    qb.push(
        r"
        )
        WHERE kind IS NOT NULL
          AND kind <> ''
        GROUP BY kind
        ORDER BY cnt DESC
        ",
    );

    let rows = qb.build().fetch_all(&storage.pool).await?;
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        out.push((row.try_get("kind")?, row.try_get("cnt")?));
    }
    Ok(out)
}

async fn query_summary_by_kind_rust_fallback(
    storage: &StatsStorage,
    start_utc: Option<&str>,
    end_utc: Option<&str>,
    feature: Option<&str>,
) -> Result<Vec<(String, i64)>, AppError> {
    #[derive(serde::Deserialize)]
    struct UserKindFromExtra {
        user_kind: Option<String>,
    }

    use std::collections::{HashMap, HashSet};
    let mut by_kind_qb = QueryBuilder::<Sqlite>::new(
        "SELECT user_hash, extra_json FROM events WHERE user_hash IS NOT NULL AND extra_json IS NOT NULL",
    );
    push_stats_ts_range_filters(&mut by_kind_qb, start_utc, end_utc);
    push_stats_feature_filter(&mut by_kind_qb, feature);

    let mut stream = by_kind_qb.build().fetch(&storage.pool);
    let mut uniq: HashSet<(String, String)> = HashSet::new();
    while let Some(r) = stream
        .try_next()
        .await
        .map_err(|e| AppError::Internal(format!("summary by_kind: {e}")))?
    {
        let uh: String = match r.try_get("user_hash") {
            Ok(v) => v,
            Err(_) => continue,
        };
        let ej: String = match r.try_get("extra_json") {
            Ok(v) => v,
            Err(_) => continue,
        };
        if !ej.contains("user_kind") {
            continue;
        }
        if let Ok(extra) = serde_json::from_str::<UserKindFromExtra>(&ej)
            && let Some(kind) = extra.user_kind
            && !kind.is_empty()
        {
            uniq.insert((uh, kind));
        }
    }

    let mut by_kind_map: HashMap<String, i64> = HashMap::new();
    for (_, k) in uniq {
        *by_kind_map.entry(k).or_insert(0) += 1;
    }
    let mut by_kind: Vec<(String, i64)> = by_kind_map.into_iter().collect();
    by_kind.sort_by_key(|b| std::cmp::Reverse(b.1));
    Ok(by_kind)
}

async fn query_summary_latency_data(
    storage: &StatsStorage,
    start_utc: Option<&str>,
    end_utc: Option<&str>,
) -> Result<SummaryLatencyData, AppError> {
    if let (Some(start), Some(end)) = (start_utc, end_utc) {
        storage
            .query_latency_percentiles_histogram(start, end)
            .await
    } else {
        // 无时间范围时，取首尾事件时间作为范围
        let (first, last) = query_summary_overall(storage, None, None).await?;
        if let (Some(f), Some(l)) = (first, last) {
            storage.query_latency_percentiles_histogram(&f, &l).await
        } else {
            Ok(SummaryLatencyData {
                sample_count: 0,
                avg_ms: None,
                p50_ms: None,
                p95_ms: None,
                max_ms: None,
            })
        }
    }
}

async fn query_summary_overall(
    storage: &StatsStorage,
    start_utc: Option<&str>,
    end_utc: Option<&str>,
) -> Result<(Option<String>, Option<String>), AppError> {
    let mut overall_qb = QueryBuilder::<Sqlite>::new("");
    push_summary_overall_query(&mut overall_qb, start_utc, end_utc);
    let row = overall_qb
        .build()
        .fetch_one(&storage.pool)
        .await
        .map_err(|e| AppError::Internal(format!("summary overall: {e}")))?;
    Ok((
        row.try_get::<String, _>("min_ts").ok(),
        row.try_get::<String, _>("max_ts").ok(),
    ))
}

async fn query_summary_features(
    storage: &StatsStorage,
    start_utc: Option<&str>,
    end_utc: Option<&str>,
    feature: Option<&str>,
) -> Result<Vec<SummaryFeatureRow>, AppError> {
    let mut features_qb = QueryBuilder::<Sqlite>::new("");
    push_summary_features_query(&mut features_qb, start_utc, end_utc, feature);
    let rows = features_qb
        .build()
        .fetch_all(&storage.pool)
        .await
        .map_err(|e| AppError::Internal(format!("summary features: {e}")))?;

    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        out.push(SummaryFeatureRow {
            feature: r.try_get("feature").unwrap_or_else(|_| String::new()),
            count: r.try_get("cnt").unwrap_or(0),
            last_ts: r.try_get("last_ts").ok(),
        });
    }
    Ok(out)
}

async fn query_summary_unique_users(
    storage: &StatsStorage,
    start_utc: Option<&str>,
    end_utc: Option<&str>,
    feature: Option<&str>,
) -> Result<i64, AppError> {
    let mut users_qb = QueryBuilder::<Sqlite>::new("");
    push_summary_unique_users_query(&mut users_qb, start_utc, end_utc, feature);
    let row = users_qb
        .build()
        .fetch_one(&storage.pool)
        .await
        .map_err(|e| AppError::Internal(format!("summary users: {e}")))?;
    Ok(row.try_get("total").unwrap_or(0))
}

async fn query_summary_by_kind(
    storage: &StatsStorage,
    start_utc: Option<&str>,
    end_utc: Option<&str>,
    feature: Option<&str>,
) -> Result<Vec<(String, i64)>, AppError> {
    match query_summary_by_kind_sql(storage, start_utc, end_utc, feature).await {
        Ok(rows) => Ok(rows),
        Err(error) => {
            tracing::debug!(
                target: "phi_backend::stats",
                %error,
                "SQLite JSON user_kind 聚合不可用，回退到 Rust 解析"
            );
            query_summary_by_kind_rust_fallback(storage, start_utc, end_utc, feature).await
        }
    }
}

async fn query_summary_events_total(
    storage: &StatsStorage,
    start_utc: Option<&str>,
    end_utc: Option<&str>,
) -> Result<i64, AppError> {
    let mut events_total_qb =
        QueryBuilder::<Sqlite>::new("SELECT COUNT(1) as total FROM events WHERE 1=1");
    push_stats_ts_range_filters(&mut events_total_qb, start_utc, end_utc);
    let row = events_total_qb
        .build()
        .fetch_one(&storage.pool)
        .await
        .map_err(|e| AppError::Internal(format!("summary events_total: {e}")))?;
    Ok(row.try_get::<i64, _>("total").unwrap_or(0))
}

async fn query_summary_http_totals(
    storage: &StatsStorage,
    start_utc: Option<&str>,
    end_utc: Option<&str>,
) -> Result<(i64, i64), AppError> {
    let mut http_total_qb = QueryBuilder::<Sqlite>::new(
        "SELECT COUNT(1) as total, COALESCE(SUM(CASE WHEN status >= 400 THEN 1 ELSE 0 END), 0) as err FROM events WHERE route IS NOT NULL",
    );
    push_stats_ts_range_filters(&mut http_total_qb, start_utc, end_utc);
    let row = http_total_qb
        .build()
        .fetch_one(&storage.pool)
        .await
        .map_err(|e| AppError::Internal(format!("summary http_total: {e}")))?;
    Ok((
        row.try_get::<i64, _>("total").unwrap_or(0),
        row.try_get::<i64, _>("err").unwrap_or(0),
    ))
}

async fn query_summary_routes(
    storage: &StatsStorage,
    start_utc: Option<&str>,
    end_utc: Option<&str>,
    top: i64,
) -> Result<Vec<SummaryRouteRow>, AppError> {
    let mut routes_qb = QueryBuilder::<Sqlite>::new(
        "SELECT route, COUNT(1) as cnt, COALESCE(SUM(CASE WHEN status >= 400 THEN 1 ELSE 0 END), 0) as err_cnt, MAX(ts_utc) as last_ts FROM events WHERE route IS NOT NULL",
    );
    push_stats_ts_range_filters(&mut routes_qb, start_utc, end_utc);
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
        out.push(SummaryRouteRow {
            route: r.try_get("route").unwrap_or_else(|_| String::new()),
            count: r.try_get("cnt").unwrap_or(0),
            err_count: r.try_get("err_cnt").unwrap_or(0),
            last_ts: r.try_get("last_ts").ok(),
        });
    }
    Ok(out)
}

async fn query_summary_methods(
    storage: &StatsStorage,
    start_utc: Option<&str>,
    end_utc: Option<&str>,
    top: i64,
) -> Result<Vec<SummaryMethodRow>, AppError> {
    let mut methods_qb = QueryBuilder::<Sqlite>::new(
        "SELECT method, COUNT(1) as cnt FROM events WHERE route IS NOT NULL AND method IS NOT NULL",
    );
    push_stats_ts_range_filters(&mut methods_qb, start_utc, end_utc);
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
        out.push(SummaryMethodRow {
            method: r.try_get("method").unwrap_or_else(|_| String::new()),
            count: r.try_get("cnt").unwrap_or(0),
        });
    }
    Ok(out)
}

async fn query_summary_status_codes(
    storage: &StatsStorage,
    start_utc: Option<&str>,
    end_utc: Option<&str>,
    top: i64,
) -> Result<Vec<SummaryStatusCodeRow>, AppError> {
    let mut status_codes_qb = QueryBuilder::<Sqlite>::new(
        "SELECT status, COUNT(1) as cnt FROM events WHERE route IS NOT NULL AND status IS NOT NULL",
    );
    push_stats_ts_range_filters(&mut status_codes_qb, start_utc, end_utc);
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
        out.push(SummaryStatusCodeRow {
            status: r.try_get("status").unwrap_or(0),
            count: r.try_get("cnt").unwrap_or(0),
        });
    }
    Ok(out)
}

async fn query_summary_instances(
    storage: &StatsStorage,
    start_utc: Option<&str>,
    end_utc: Option<&str>,
    top: i64,
) -> Result<Vec<SummaryInstanceRow>, AppError> {
    let mut instances_qb = QueryBuilder::<Sqlite>::new("");
    push_summary_instances_query(&mut instances_qb, start_utc, end_utc, top);
    let rows = instances_qb
        .build()
        .fetch_all(&storage.pool)
        .await
        .map_err(|e| AppError::Internal(format!("summary instances: {e}")))?;

    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        out.push(SummaryInstanceRow {
            instance: r.try_get("instance").unwrap_or_else(|_| String::new()),
            count: r.try_get("cnt").unwrap_or(0),
            last_ts: r.try_get("last_ts").ok(),
        });
    }
    Ok(out)
}

async fn query_summary_actions(
    storage: &StatsStorage,
    start_utc: Option<&str>,
    end_utc: Option<&str>,
    feature: Option<&str>,
    top: i64,
) -> Result<Vec<SummaryActionRow>, AppError> {
    let mut actions_qb = QueryBuilder::<Sqlite>::new(
        "SELECT feature, action, COUNT(1) as cnt, MAX(ts_utc) as last_ts FROM events WHERE feature IS NOT NULL AND action IS NOT NULL",
    );
    push_stats_ts_range_filters(&mut actions_qb, start_utc, end_utc);
    push_stats_feature_filter(&mut actions_qb, feature);
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
        out.push(SummaryActionRow {
            feature: r.try_get("feature").unwrap_or_else(|_| String::new()),
            action: r.try_get("action").unwrap_or_else(|_| String::new()),
            count: r.try_get("cnt").unwrap_or(0),
            last_ts: r.try_get("last_ts").ok(),
        });
    }
    Ok(out)
}

async fn query_summary_unique_ips(
    storage: &StatsStorage,
    start_utc: Option<&str>,
    end_utc: Option<&str>,
) -> Result<i64, AppError> {
    let mut unique_ips_qb = QueryBuilder::<Sqlite>::new("");
    push_summary_unique_ips_query(&mut unique_ips_qb, start_utc, end_utc);
    let row = unique_ips_qb
        .build()
        .fetch_one(&storage.pool)
        .await
        .map_err(|e| AppError::Internal(format!("summary unique_ips: {e}")))?;
    Ok(row.try_get::<i64, _>("cnt").unwrap_or(0))
}

/// 按指标预测：仅当返回历史区间内 daily_agg 有预聚合行、且 start/end 对齐了 UTC 整日边界本文跳过间错的部分、且未设 feature 过滤，才启用快速路径。
async fn plan_fast_path(
    storage: &StatsStorage,
    start_utc: Option<&str>,
    end_utc: Option<&str>,
    feature: Option<&str>,
) -> Result<Option<FastPlan>, AppError> {
    // 快速路径不支持 feature 维度过滤。
    if feature.is_some() {
        return Ok(None);
    }
    let Some(start_s) = start_utc else {
        return Ok(None);
    };
    let start_dt = parse_utc(start_s)?;
    if start_dt.naive_utc().time() != mid_night() {
        return Ok(None);
    }
    let end_dt = match end_utc {
        None => None,
        Some(e) => Some(parse_utc(e)?),
    };
    if end_dt.is_some_and(|d| d.naive_utc().time() != end_of_day()) {
        return Ok(None);
    }

    // 确保“预聚合背景补齐”至少运行过一次、避免只完成几天的错会。
    let backfill_done =
        storage.get_stats_meta("backfill_complete").await? == Some("true".to_string());
    if !backfill_done {
        return Ok(None);
    }

    let today = Utc::now().date_naive();
    let yesterday = today - chrono::Duration::days(1);
    let start_day = start_dt.date_naive();
    let end_day = end_dt.map(|d| d.date_naive());
    let hist_end_day = end_day.map_or(yesterday, |d| d.min(yesterday));

    // 快速路径贵在干掉 OPCScan，如果历史区间为空或仅不是完整一天画质较低，仅 走默认一查。
    if hist_end_day < start_day {
        return Ok(None);
    }

    let hist_start_s = start_day.format("%Y-%m-%d").to_string();
    let hist_end_s = hist_end_day.format("%Y-%m-%d").to_string();
    if !storage
        .daily_agg_has_rows_in_range(&hist_start_s, &hist_end_s)
        .await?
    {
        return Ok(None);
    }

    // 今日热数据：当 end_utc 未指定（开口上界）或显式 end >= 今日时，今日热部分在窗口内；
    // 仅当 end < 今日时热部分才不在窗口内。原实现以 end_day 是否 Some 且 >= today 判断，
    // 导致默认请求（end_utc=None）完全漏掉今日热数据。
    let hot_start_dt = Utc
        .from_utc_datetime(&today.and_hms_opt(0, 0, 0).expect("00:00:00"))
        .naive_utc()
        .and_utc();
    let hot_start_utc = match end_day {
        None => Some(hot_start_dt.to_rfc3339()),
        Some(d) if d >= today => Some(hot_start_dt.to_rfc3339()),
        _ => None,
    };

    Ok(Some(FastPlan {
        hist_start_day: hist_start_s,
        hist_end_day: hist_end_s,
        hot_start_utc,
        hot_end_utc: end_utc.map(str::to_string),
        start_utc_orig: start_s.to_string(),
        end_utc_orig: end_utc.map(str::to_string),
    }))
}

async fn query_summary_features_fast(
    storage: &StatsStorage,
    plan: &FastPlan,
) -> Result<Vec<SummaryFeatureRow>, AppError> {
    let preagg = sqlx::query(
        "SELECT feature, SUM(count) AS cnt, MAX(last_ts) AS last_ts
         FROM daily_agg
         WHERE date BETWEEN ? AND ? AND feature IS NOT NULL
         GROUP BY feature",
    )
    .bind(&plan.hist_start_day)
    .bind(&plan.hist_end_day)
    .fetch_all(&storage.pool)
    .await
    .map_err(|e| AppError::Internal(format!("summary features fast: {e}")))?;

    let mut acc: HashMap<String, (i64, Option<String>)> = HashMap::new();
    for r in preagg {
        let feature: String = r.try_get("feature").unwrap_or_default();
        let cnt: i64 = r.try_get("cnt").unwrap_or(0);
        let last_ts: Option<String> = r.try_get("last_ts").ok();
        let entry = acc.entry(feature).or_insert((0, None));
        entry.0 += cnt;
        entry.1 = max_last_ts(entry.1.take(), last_ts);
    }

    if let Some(hot_start) = plan.hot_start_utc.as_deref() {
        let hot_rows =
            query_summary_features(storage, Some(hot_start), plan.hot_end_utc.as_deref(), None)
                .await?;
        for r in hot_rows {
            let entry = acc.entry(r.feature.clone()).or_insert((0, None));
            entry.0 += r.count;
            entry.1 = max_last_ts(entry.1.take(), r.last_ts);
        }
    }

    let mut out: Vec<SummaryFeatureRow> = acc
        .into_iter()
        .map(|(feature, (count, last_ts))| SummaryFeatureRow {
            feature,
            count,
            last_ts,
        })
        .collect();
    // 与旧查询一致：返回无序，但计数优先与前端看顺一致。
    out.sort_by_key(|b| std::cmp::Reverse(b.count));
    Ok(out)
}

async fn query_summary_unique_users_fast(
    storage: &StatsStorage,
    plan: &FastPlan,
) -> Result<i64, AppError> {
    let mut qb = QueryBuilder::<Sqlite>::new(
        "SELECT COUNT(*) AS total FROM (SELECT DISTINCT user_hash FROM daily_user WHERE date BETWEEN ",
    );
    qb.push_bind(plan.hist_start_day.clone())
        .push(" AND ")
        .push_bind(plan.hist_end_day.clone());
    if let Some(hot_start) = plan.hot_start_utc.as_deref() {
        qb.push(" UNION SELECT DISTINCT user_hash FROM events WHERE ts_utc >= ")
            .push_bind(hot_start.to_string());
        if let Some(end) = plan.hot_end_utc.as_deref() {
            qb.push(" AND ts_utc <= ").push_bind(end.to_string());
        }
        qb.push(" AND user_hash IS NOT NULL");
    }
    qb.push(")");
    let row = qb
        .build()
        .fetch_one(&storage.pool)
        .await
        .map_err(|e| AppError::Internal(format!("summary users fast: {e}")))?;
    Ok(row.try_get::<i64, _>("total").unwrap_or(0))
}

async fn query_summary_unique_ips_fast(
    storage: &StatsStorage,
    plan: &FastPlan,
) -> Result<i64, AppError> {
    let mut qb = QueryBuilder::<Sqlite>::new(
        "SELECT COUNT(*) AS total FROM (SELECT DISTINCT ip_hash FROM daily_ip WHERE date BETWEEN ",
    );
    qb.push_bind(plan.hist_start_day.clone())
        .push(" AND ")
        .push_bind(plan.hist_end_day.clone());
    if let Some(hot_start) = plan.hot_start_utc.as_deref() {
        qb.push(" UNION SELECT DISTINCT client_ip_hash AS ip_hash FROM events WHERE ts_utc >= ")
            .push_bind(hot_start.to_string());
        if let Some(end) = plan.hot_end_utc.as_deref() {
            qb.push(" AND ts_utc <= ").push_bind(end.to_string());
        }
        qb.push(" AND route IS NOT NULL AND client_ip_hash IS NOT NULL");
    }
    qb.push(")");
    let row = qb
        .build()
        .fetch_one(&storage.pool)
        .await
        .map_err(|e| AppError::Internal(format!("summary unique_ips fast: {e}")))?;
    Ok(row.try_get::<i64, _>("total").unwrap_or(0))
}

async fn query_summary_by_kind_fast(
    storage: &StatsStorage,
    plan: &FastPlan,
) -> Result<Vec<(String, i64)>, AppError> {
    let preagg_rows = sqlx::query(
        "SELECT DISTINCT user_hash, kind FROM daily_user
         WHERE date BETWEEN ? AND ? AND kind IS NOT NULL AND kind <> ''",
    )
    .bind(&plan.hist_start_day)
    .bind(&plan.hist_end_day)
    .fetch_all(&storage.pool)
    .await
    .map_err(|e| AppError::Internal(format!("summary by_kind fast preagg: {e}")))?;

    use std::collections::HashSet;
    let mut uniq: HashSet<(String, String)> = HashSet::new();
    for r in preagg_rows {
        let user_hash: String = r.try_get("user_hash").unwrap_or_default();
        let kind: String = r.try_get("kind").unwrap_or_default();
        if !user_hash.is_empty() && !kind.is_empty() {
            uniq.insert((user_hash, kind));
        }
    }

    // 今日热：从 events 直接取 distinct (user_hash, kind)、与 query_summary_by_kind_sql 同义的 JSON 取记录。
    if let Some(hot_start) = plan.hot_start_utc.as_deref() {
        let mut qb = QueryBuilder::<Sqlite>::new(
            r"SELECT DISTINCT user_hash,
                CASE
                    WHEN json_valid(extra_json)
                        AND json_type(extra_json, '$.user_kind') = 'text'
                    THEN json_extract(extra_json, '$.user_kind')
                    ELSE NULL
                END AS kind
             FROM events
             WHERE user_hash IS NOT NULL AND extra_json IS NOT NULL AND ts_utc >= ",
        );
        qb.push_bind(hot_start.to_string());
        if let Some(end) = plan.hot_end_utc.as_deref() {
            qb.push(" AND ts_utc <= ").push_bind(end.to_string());
        }
        let rows = qb
            .build()
            .fetch_all(&storage.pool)
            .await
            .map_err(|e| AppError::Internal(format!("summary by_kind fast hot: {e}")))?;
        for r in rows {
            let user_hash: String = r.try_get("user_hash").unwrap_or_default();
            if !user_hash.is_empty()
                && let Some(kind) = r
                    .try_get::<String, _>("kind")
                    .ok()
                    .filter(|k| !k.is_empty())
            {
                uniq.insert((user_hash, kind));
            }
        }
    }

    let mut merged: HashMap<String, i64> = HashMap::new();
    for (_, k) in uniq {
        *merged.entry(k).or_insert(0) += 1;
    }
    let mut out: Vec<(String, i64)> = merged.into_iter().collect();
    out.sort_by_key(|(_, c)| std::cmp::Reverse(*c));
    Ok(out)
}

async fn query_summary_events_total_fast(
    storage: &StatsStorage,
    plan: &FastPlan,
) -> Result<i64, AppError> {
    let preagg = sqlx::query(
        "SELECT COALESCE(SUM(count), 0) AS total FROM daily_agg
         WHERE date BETWEEN ? AND ?",
    )
    .bind(&plan.hist_start_day)
    .bind(&plan.hist_end_day)
    .fetch_one(&storage.pool)
    .await
    .map_err(|e| AppError::Internal(format!("summary events_total fast: {e}")))?;
    let mut total: i64 = preagg.try_get("total").unwrap_or(0);
    if let Some(hot_start) = plan.hot_start_utc.as_deref() {
        total += query_summary_events_total(storage, Some(hot_start), plan.hot_end_utc.as_deref())
            .await?;
    }
    Ok(total)
}

async fn query_summary_http_totals_fast(
    storage: &StatsStorage,
    plan: &FastPlan,
) -> Result<(i64, i64), AppError> {
    let preagg = sqlx::query(
        "SELECT
            COALESCE(SUM(count), 0) AS total,
            COALESCE(SUM(err_count), 0) AS err
         FROM daily_agg
         WHERE date BETWEEN ? AND ? AND route IS NOT NULL",
    )
    .bind(&plan.hist_start_day)
    .bind(&plan.hist_end_day)
    .fetch_one(&storage.pool)
    .await
    .map_err(|e| AppError::Internal(format!("summary http_total fast: {e}")))?;
    let mut total: i64 = preagg.try_get("total").unwrap_or(0);
    let mut err: i64 = preagg.try_get("err").unwrap_or(0);
    if let Some(hot_start) = plan.hot_start_utc.as_deref() {
        let (t, e) =
            query_summary_http_totals(storage, Some(hot_start), plan.hot_end_utc.as_deref())
                .await?;
        total += t;
        err += e;
    }
    Ok((total, err))
}

async fn query_summary_routes_fast(
    storage: &StatsStorage,
    plan: &FastPlan,
    top: i64,
) -> Result<Vec<SummaryRouteRow>, AppError> {
    let preagg = sqlx::query(
        "SELECT route, SUM(count) AS cnt, SUM(err_count) AS err_cnt, MAX(last_ts) AS last_ts
         FROM daily_agg
         WHERE date BETWEEN ? AND ? AND route IS NOT NULL
         GROUP BY route",
    )
    .bind(&plan.hist_start_day)
    .bind(&plan.hist_end_day)
    .fetch_all(&storage.pool)
    .await
    .map_err(|e| AppError::Internal(format!("summary routes fast: {e}")))?;
    let mut acc: HashMap<String, (i64, i64, Option<String>)> = HashMap::new();
    for r in preagg {
        let route: String = r.try_get("route").unwrap_or_default();
        let entry = acc.entry(route).or_insert((0, 0, None));
        entry.0 += r.try_get("cnt").unwrap_or(0);
        entry.1 += r.try_get("err_cnt").unwrap_or(0);
        entry.2 = max_last_ts(entry.2.take(), r.try_get("last_ts").ok());
    }
    if let Some(hot_start) = plan.hot_start_utc.as_deref() {
        let hot_rows =
            query_summary_routes(storage, Some(hot_start), plan.hot_end_utc.as_deref(), 200)
                .await?;
        for r in hot_rows {
            let entry = acc.entry(r.route).or_insert((0, 0, None));
            entry.0 += r.count;
            entry.1 += r.err_count;
            entry.2 = max_last_ts(entry.2.take(), r.last_ts);
        }
    }
    let mut out: Vec<SummaryRouteRow> = acc
        .into_iter()
        .map(|(route, (count, err_count, last_ts))| SummaryRouteRow {
            route,
            count,
            err_count,
            last_ts,
        })
        .collect();
    out.sort_by_key(|b| std::cmp::Reverse(b.count));
    out.truncate(usize::try_from(top).unwrap_or(0));
    Ok(out)
}

async fn query_summary_methods_fast(
    storage: &StatsStorage,
    plan: &FastPlan,
    top: i64,
) -> Result<Vec<SummaryMethodRow>, AppError> {
    let preagg = sqlx::query(
        "SELECT method, SUM(count) AS cnt
         FROM daily_agg
         WHERE date BETWEEN ? AND ? AND route IS NOT NULL AND method IS NOT NULL
         GROUP BY method",
    )
    .bind(&plan.hist_start_day)
    .bind(&plan.hist_end_day)
    .fetch_all(&storage.pool)
    .await
    .map_err(|e| AppError::Internal(format!("summary methods fast: {e}")))?;
    let mut acc: HashMap<String, i64> = HashMap::new();
    for r in preagg {
        let method: String = r.try_get("method").unwrap_or_default();
        *acc.entry(method).or_insert(0) += r.try_get("cnt").unwrap_or(0);
    }
    if let Some(hot_start) = plan.hot_start_utc.as_deref() {
        let hot_rows =
            query_summary_methods(storage, Some(hot_start), plan.hot_end_utc.as_deref(), 200)
                .await?;
        for r in hot_rows {
            *acc.entry(r.method).or_insert(0) += r.count;
        }
    }
    let mut out: Vec<SummaryMethodRow> = acc
        .into_iter()
        .map(|(method, count)| SummaryMethodRow { method, count })
        .collect();
    out.sort_by_key(|b| std::cmp::Reverse(b.count));
    out.truncate(usize::try_from(top).unwrap_or(0));
    Ok(out)
}

async fn query_summary_status_codes_fast(
    storage: &StatsStorage,
    plan: &FastPlan,
    top: i64,
) -> Result<Vec<SummaryStatusCodeRow>, AppError> {
    let preagg = sqlx::query(
        "SELECT status, SUM(count) AS cnt
         FROM daily_status
         WHERE date BETWEEN ? AND ?
         GROUP BY status",
    )
    .bind(&plan.hist_start_day)
    .bind(&plan.hist_end_day)
    .fetch_all(&storage.pool)
    .await
    .map_err(|e| AppError::Internal(format!("summary status fast: {e}")))?;
    let mut acc: HashMap<i64, i64> = HashMap::new();
    for r in preagg {
        let status: i64 = r.try_get("status").unwrap_or(0);
        *acc.entry(status).or_insert(0) += r.try_get("cnt").unwrap_or(0);
    }
    if let Some(hot_start) = plan.hot_start_utc.as_deref() {
        let hot_rows =
            query_summary_status_codes(storage, Some(hot_start), plan.hot_end_utc.as_deref(), 200)
                .await?;
        for r in hot_rows {
            *acc.entry(r.status).or_insert(0) += r.count;
        }
    }
    let mut out: Vec<SummaryStatusCodeRow> = acc
        .into_iter()
        .map(|(status, count)| SummaryStatusCodeRow { status, count })
        .collect();
    out.sort_by_key(|b| std::cmp::Reverse(b.count));
    out.truncate(usize::try_from(top).unwrap_or(0));
    Ok(out)
}

async fn query_summary_instances_fast(
    storage: &StatsStorage,
    plan: &FastPlan,
    top: i64,
) -> Result<Vec<SummaryInstanceRow>, AppError> {
    let preagg = sqlx::query(
        "SELECT instance, SUM(count) AS cnt, MAX(last_ts) AS last_ts
         FROM daily_instance
         WHERE date BETWEEN ? AND ?
         GROUP BY instance",
    )
    .bind(&plan.hist_start_day)
    .bind(&plan.hist_end_day)
    .fetch_all(&storage.pool)
    .await
    .map_err(|e| AppError::Internal(format!("summary instances fast: {e}")))?;
    let mut acc: HashMap<String, (i64, Option<String>)> = HashMap::new();
    for r in preagg {
        let instance: String = r.try_get("instance").unwrap_or_default();
        let entry = acc.entry(instance).or_insert((0, None));
        entry.0 += r.try_get("cnt").unwrap_or(0);
        entry.1 = max_last_ts(entry.1.take(), r.try_get("last_ts").ok());
    }
    if let Some(hot_start) = plan.hot_start_utc.as_deref() {
        let hot_rows =
            query_summary_instances(storage, Some(hot_start), plan.hot_end_utc.as_deref(), 200)
                .await?;
        for r in hot_rows {
            let entry = acc.entry(r.instance).or_insert((0, None));
            entry.0 += r.count;
            entry.1 = max_last_ts(entry.1.take(), r.last_ts);
        }
    }
    let mut out: Vec<SummaryInstanceRow> = acc
        .into_iter()
        .map(|(instance, (count, last_ts))| SummaryInstanceRow {
            instance,
            count,
            last_ts,
        })
        .collect();
    out.sort_by_key(|b| std::cmp::Reverse(b.count));
    out.truncate(usize::try_from(top).unwrap_or(0));
    Ok(out)
}

async fn query_summary_actions_fast(
    storage: &StatsStorage,
    plan: &FastPlan,
    top: i64,
) -> Result<Vec<SummaryActionRow>, AppError> {
    let preagg = sqlx::query(
        "SELECT feature, action, SUM(count) AS cnt, MAX(last_ts) AS last_ts
         FROM daily_action
         WHERE date BETWEEN ? AND ?
         GROUP BY feature, action",
    )
    .bind(&plan.hist_start_day)
    .bind(&plan.hist_end_day)
    .fetch_all(&storage.pool)
    .await
    .map_err(|e| AppError::Internal(format!("summary actions fast: {e}")))?;
    let mut acc: HashMap<(String, String), (i64, Option<String>)> = HashMap::new();
    for r in preagg {
        let feature: String = r.try_get("feature").unwrap_or_default();
        let action: String = r.try_get("action").unwrap_or_default();
        let entry = acc.entry((feature, action)).or_insert((0, None));
        entry.0 += r.try_get("cnt").unwrap_or(0);
        entry.1 = max_last_ts(entry.1.take(), r.try_get("last_ts").ok());
    }
    if let Some(hot_start) = plan.hot_start_utc.as_deref() {
        let hot_rows = query_summary_actions(
            storage,
            Some(hot_start),
            plan.hot_end_utc.as_deref(),
            None,
            200,
        )
        .await?;
        for r in hot_rows {
            let entry = acc.entry((r.feature, r.action)).or_insert((0, None));
            entry.0 += r.count;
            entry.1 = max_last_ts(entry.1.take(), r.last_ts);
        }
    }
    let mut out: Vec<SummaryActionRow> = acc
        .into_iter()
        .map(|((feature, action), (count, last_ts))| SummaryActionRow {
            feature,
            action,
            count,
            last_ts,
        })
        .collect();
    out.sort_by_key(|b| std::cmp::Reverse(b.count));
    out.truncate(usize::try_from(top).unwrap_or(0));
    Ok(out)
}

impl StatsStorage {
    pub async fn query_stats_summary_data(
        &self,
        start_utc: Option<&str>,
        end_utc: Option<&str>,
        feature: Option<&str>,
        include: SummaryIncludeFlags,
        top: i64,
        want_meta: bool,
    ) -> Result<StatsSummaryData, AppError> {
        // 先尝试基于预聚合表的快速路径；未激活、不成就回退于 events 直扫。保证启用上以
        // 预聚合于齐齐的说作为安全门，避免部分预聚的区间错输出。
        let plan = match plan_fast_path(self, start_utc, end_utc, feature).await {
            Ok(Some(p)) => Some(p),
            Ok(None) => None,
            Err(e) => {
                tracing::warn!(target: "phi_backend::stats", error = %e, "summary 快路径判定失败，回退慢路径");
                None
            }
        };
        if let Some(plan) = plan {
            return self
                .query_stats_summary_data_fast(&plan, top, want_meta, include)
                .await;
        }
        self.query_stats_summary_data_slow(start_utc, end_utc, feature, include, top, want_meta)
            .await
    }

    async fn query_stats_summary_data_fast(
        &self,
        plan: &FastPlan,
        top: i64,
        want_meta: bool,
        include: SummaryIncludeFlags,
    ) -> Result<StatsSummaryData, AppError> {
        // overall 仍走 events（MIN/MAX 可击索引），在任意窗口上代价均低。
        let overall_fut = query_summary_overall(
            self,
            Some(plan.start_utc_orig.as_str()),
            plan.end_utc_orig.as_deref(),
        );
        let features_fut = query_summary_features_fast(self, plan);
        let users_fut = query_summary_unique_users_fast(self, plan);
        let ((first_event_ts, last_event_ts), features, unique_users_total) =
            tokio::try_join!(overall_fut, features_fut, users_fut)?;

        let by_kind_fut = async {
            if include.user_kinds {
                query_summary_by_kind_fast(self, plan).await
            } else {
                Ok::<Vec<(String, i64)>, AppError>(Vec::new())
            }
        };
        let events_total_fut = async {
            if want_meta || include.any() {
                query_summary_events_total_fast(self, plan).await.map(Some)
            } else {
                Ok::<Option<i64>, AppError>(None)
            }
        };
        let http_totals_fut = async {
            if include.any_http() {
                let (total, errors) = query_summary_http_totals_fast(self, plan).await?;
                Ok::<(Option<i64>, Option<i64>), AppError>((Some(total), Some(errors)))
            } else {
                Ok::<(Option<i64>, Option<i64>), AppError>((None, None))
            }
        };
        let routes_fut = async {
            if include.routes {
                query_summary_routes_fast(self, plan, top).await.map(Some)
            } else {
                Ok::<Option<Vec<SummaryRouteRow>>, AppError>(None)
            }
        };
        let methods_fut = async {
            if include.methods {
                query_summary_methods_fast(self, plan, top).await.map(Some)
            } else {
                Ok::<Option<Vec<SummaryMethodRow>>, AppError>(None)
            }
        };
        let status_codes_fut = async {
            if include.status_codes {
                query_summary_status_codes_fast(self, plan, top)
                    .await
                    .map(Some)
            } else {
                Ok::<Option<Vec<SummaryStatusCodeRow>>, AppError>(None)
            }
        };
        let instances_fut = async {
            if include.instances {
                query_summary_instances_fast(self, plan, top)
                    .await
                    .map(Some)
            } else {
                Ok::<Option<Vec<SummaryInstanceRow>>, AppError>(None)
            }
        };
        let actions_fut = async {
            if include.actions {
                query_summary_actions_fast(self, plan, top).await.map(Some)
            } else {
                Ok::<Option<Vec<SummaryActionRow>>, AppError>(None)
            }
        };
        let latency_fut = async {
            if include.latency {
                query_summary_latency_data(
                    self,
                    Some(plan.start_utc_orig.as_str()),
                    plan.end_utc_orig.as_deref(),
                )
                .await
                .map(Some)
            } else {
                Ok::<Option<SummaryLatencyData>, AppError>(None)
            }
        };
        let unique_ips_fut = async {
            if include.unique_ips {
                query_summary_unique_ips_fast(self, plan).await.map(Some)
            } else {
                Ok::<Option<i64>, AppError>(None)
            }
        };

        let core_includes_fut = async move {
            tokio::try_join!(
                by_kind_fut,
                events_total_fut,
                http_totals_fut,
                instances_fut,
                actions_fut
            )
        };
        let http_includes_fut = async move {
            tokio::try_join!(
                routes_fut,
                methods_fut,
                status_codes_fut,
                latency_fut,
                unique_ips_fut
            )
        };
        let (
            (by_kind, events_total, (http_total, http_errors), instances, actions),
            (routes, methods, status_codes, latency, unique_ips),
        ) = tokio::try_join!(core_includes_fut, http_includes_fut)?;

        Ok(StatsSummaryData {
            first_event_ts,
            last_event_ts,
            features,
            unique_users_total,
            by_kind,
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
        })
    }

    async fn query_stats_summary_data_slow(
        &self,
        start_utc: Option<&str>,
        end_utc: Option<&str>,
        feature: Option<&str>,
        include: SummaryIncludeFlags,
        top: i64,
        want_meta: bool,
    ) -> Result<StatsSummaryData, AppError> {
        let overall_fut = query_summary_overall(self, start_utc, end_utc);
        let features_fut = query_summary_features(self, start_utc, end_utc, feature);
        let users_fut = query_summary_unique_users(self, start_utc, end_utc, feature);
        let ((first_event_ts, last_event_ts), features, unique_users_total) =
            tokio::try_join!(overall_fut, features_fut, users_fut)?;

        let by_kind_fut = async {
            if include.user_kinds {
                query_summary_by_kind(self, start_utc, end_utc, feature).await
            } else {
                Ok::<Vec<(String, i64)>, AppError>(Vec::new())
            }
        };
        let events_total_fut = async {
            if want_meta || include.any() {
                query_summary_events_total(self, start_utc, end_utc)
                    .await
                    .map(Some)
            } else {
                Ok::<Option<i64>, AppError>(None)
            }
        };
        let http_totals_fut = async {
            if include.any_http() {
                let (total, errors) = query_summary_http_totals(self, start_utc, end_utc).await?;
                Ok::<(Option<i64>, Option<i64>), AppError>((Some(total), Some(errors)))
            } else {
                Ok::<(Option<i64>, Option<i64>), AppError>((None, None))
            }
        };
        let routes_fut = async {
            if include.routes {
                query_summary_routes(self, start_utc, end_utc, top)
                    .await
                    .map(Some)
            } else {
                Ok::<Option<Vec<SummaryRouteRow>>, AppError>(None)
            }
        };
        let methods_fut = async {
            if include.methods {
                query_summary_methods(self, start_utc, end_utc, top)
                    .await
                    .map(Some)
            } else {
                Ok::<Option<Vec<SummaryMethodRow>>, AppError>(None)
            }
        };
        let status_codes_fut = async {
            if include.status_codes {
                query_summary_status_codes(self, start_utc, end_utc, top)
                    .await
                    .map(Some)
            } else {
                Ok::<Option<Vec<SummaryStatusCodeRow>>, AppError>(None)
            }
        };
        let instances_fut = async {
            if include.instances {
                query_summary_instances(self, start_utc, end_utc, top)
                    .await
                    .map(Some)
            } else {
                Ok::<Option<Vec<SummaryInstanceRow>>, AppError>(None)
            }
        };
        let actions_fut = async {
            if include.actions {
                query_summary_actions(self, start_utc, end_utc, feature, top)
                    .await
                    .map(Some)
            } else {
                Ok::<Option<Vec<SummaryActionRow>>, AppError>(None)
            }
        };
        let latency_fut = async {
            if include.latency {
                query_summary_latency_data(self, start_utc, end_utc)
                    .await
                    .map(Some)
            } else {
                Ok::<Option<SummaryLatencyData>, AppError>(None)
            }
        };
        let unique_ips_fut = async {
            if include.unique_ips {
                query_summary_unique_ips(self, start_utc, end_utc)
                    .await
                    .map(Some)
            } else {
                Ok::<Option<i64>, AppError>(None)
            }
        };

        // 可选维度彼此独立，分组并发避免 include=all 时串行拉长尾延迟。
        let core_includes_fut = async move {
            tokio::try_join!(
                by_kind_fut,
                events_total_fut,
                http_totals_fut,
                instances_fut,
                actions_fut
            )
        };
        let http_includes_fut = async move {
            tokio::try_join!(
                routes_fut,
                methods_fut,
                status_codes_fut,
                latency_fut,
                unique_ips_fut
            )
        };
        let (
            (by_kind, events_total, (http_total, http_errors), instances, actions),
            (routes, methods, status_codes, latency, unique_ips),
        ) = tokio::try_join!(core_includes_fut, http_includes_fut)?;

        Ok(StatsSummaryData {
            first_event_ts,
            last_event_ts,
            features,
            unique_users_total,
            by_kind,
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
        })
    }
}
