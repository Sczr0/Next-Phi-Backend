use futures_util::TryStreamExt;
use sqlx::{QueryBuilder, Row, Sqlite};

use crate::error::AppError;

use super::{
    StatsStorage, StatsSummaryData, SummaryActionRow, SummaryFeatureRow, SummaryIncludeFlags,
    SummaryInstanceRow, SummaryLatencyData, SummaryMethodRow, SummaryRouteRow,
    SummaryStatusCodeRow,
};

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

fn push_summary_overall_ts_bound_subquery(
    qb: &mut QueryBuilder<'_, Sqlite>,
    start_utc: Option<&str>,
    end_utc: Option<&str>,
    order_sql: &'static str,
) {
    qb.push("(SELECT ts_utc FROM events WHERE 1=1");
    push_stats_ts_range_filters(qb, start_utc, end_utc);
    qb.push(" ORDER BY ts_utc ")
        .push(order_sql)
        .push(" LIMIT 1)");
}

fn push_summary_overall_query(
    qb: &mut QueryBuilder<'_, Sqlite>,
    start_utc: Option<&str>,
    end_utc: Option<&str>,
) {
    qb.push("SELECT ");
    push_summary_overall_ts_bound_subquery(qb, start_utc, end_utc, "ASC");
    qb.push(" as min_ts, ");
    push_summary_overall_ts_bound_subquery(qb, start_utc, end_utc, "DESC");
    qb.push(" as max_ts");
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

fn push_summary_latency_data_query(
    qb: &mut QueryBuilder<'_, Sqlite>,
    start_utc: Option<&str>,
    end_utc: Option<&str>,
) {
    qb.push(
        r"
        WITH ordered AS (
            SELECT
                duration_ms as v,
                ROW_NUMBER() OVER (ORDER BY duration_ms ASC) - 1 as idx
            FROM events
            WHERE route IS NOT NULL
              AND duration_ms IS NOT NULL
        ",
    );
    push_stats_ts_range_filters(qb, start_utc, end_utc);
    qb.push(
        r"
        ),
        stats AS (
            SELECT
                COUNT(1) as n,
                AVG(v) as avg,
                MAX(v) as max
            FROM ordered
        ),
        targets AS (
            SELECT
                n,
                avg,
                max,
                ((n - 1) * 50 + 50) / 100 as p50_idx,
                ((n - 1) * 95 + 50) / 100 as p95_idx
            FROM stats
        )
        SELECT
            targets.n as n,
            targets.avg as avg,
            targets.max as max,
            MAX(CASE WHEN ordered.idx = targets.p50_idx THEN ordered.v END) as p50,
            MAX(CASE WHEN ordered.idx = targets.p95_idx THEN ordered.v END) as p95
        FROM targets
        LEFT JOIN ordered ON targets.n > 0
        GROUP BY targets.n, targets.avg, targets.max, targets.p50_idx, targets.p95_idx
        ",
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::Execute as _;

    #[test]
    fn summary_overall_query_reads_bounds_from_ts_index_order() {
        let mut qb = QueryBuilder::<Sqlite>::new("");
        push_summary_overall_query(
            &mut qb,
            Some("2026-01-01T00:00:00Z"),
            Some("2026-01-31T23:59:59Z"),
        );
        let sql = qb.build().sql().to_string();

        assert!(sql.contains("ORDER BY ts_utc ASC LIMIT 1"));
        assert!(sql.contains("ORDER BY ts_utc DESC LIMIT 1"));
        assert_eq!(sql.matches("ts_utc >= ?").count(), 2);
        assert_eq!(sql.matches("ts_utc <= ?").count(), 2);
        assert!(!sql.contains("MIN(ts_utc)"));
        assert!(!sql.contains("MAX(ts_utc)"));
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
        let mut qb = QueryBuilder::<Sqlite>::new("");
        push_summary_latency_data_query(
            &mut qb,
            Some("2026-01-01T00:00:00Z"),
            Some("2026-01-31T23:59:59Z"),
        );
        let sql = qb.build().sql().to_string();

        assert!(sql.contains("duration_ms as v"));
        assert!(sql.contains("ROW_NUMBER() OVER (ORDER BY duration_ms ASC)"));
        assert!(sql.contains("route IS NOT NULL"));
        assert!(sql.contains("duration_ms IS NOT NULL"));
        assert!(sql.contains("ts_utc >= ?"));
        assert!(sql.contains("ts_utc <= ?"));
        assert!(sql.contains("p50_idx"));
        assert!(sql.contains("p95_idx"));
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
    let mut qb = QueryBuilder::<Sqlite>::new("");
    push_summary_latency_data_query(&mut qb, start_utc, end_utc);

    let row = qb
        .build()
        .fetch_one(&storage.pool)
        .await
        .map_err(|e| AppError::Internal(format!("summary latency: {e}")))?;

    Ok(SummaryLatencyData {
        sample_count: row.try_get("n").unwrap_or(0),
        avg_ms: row.try_get("avg").ok(),
        p50_ms: row.try_get("p50").ok(),
        p95_ms: row.try_get("p95").ok(),
        max_ms: row.try_get("max").ok(),
    })
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
