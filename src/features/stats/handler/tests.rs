use super::*;
use crate::auth_services::{QrCodeService, TapTapClient};
use crate::features::stats::models::EventInsert;
use crate::song_contract::SongCatalog;
use crate::startup::chart_loader::ChartConstantsMap;
use axum::body::Bytes;
use moka::future::Cache;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::Semaphore;

use chrono::{TimeZone, Utc};

use super::time::parse_date_bound_utc;

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
async fn stats_summary_user_kinds_dedupes_users_and_ignores_non_string_values() {
    let sqlite_path = tmp_sqlite_path("stats_summary_user_kinds_sql");
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
                feature: Some("bestn".into()),
                action: Some("cache".into()),
                method: None,
                status: None,
                duration_ms: None,
                user_hash: Some("u1".into()),
                client_ip_hash: None,
                instance: Some("inst-a".into()),
                extra_json: Some(serde_json::json!({"user_kind":"official"})),
            },
            EventInsert {
                ts_utc: dt_utc(2025, 12, 24, 0, 1, 2),
                route: None,
                feature: Some("bestn".into()),
                action: Some("render".into()),
                method: None,
                status: None,
                duration_ms: None,
                user_hash: Some("u2".into()),
                client_ip_hash: None,
                instance: Some("inst-b".into()),
                extra_json: Some(serde_json::json!({"user_kind":"official"})),
            },
            EventInsert {
                ts_utc: dt_utc(2025, 12, 24, 0, 1, 3),
                route: None,
                feature: Some("save".into()),
                action: Some("submit".into()),
                method: None,
                status: None,
                duration_ms: None,
                user_hash: Some("u3".into()),
                client_ip_hash: None,
                instance: Some("inst-c".into()),
                extra_json: Some(serde_json::json!({"user_kind":"taptap"})),
            },
            EventInsert {
                ts_utc: dt_utc(2025, 12, 24, 0, 1, 4),
                route: None,
                feature: Some("save".into()),
                action: Some("submit".into()),
                method: None,
                status: None,
                duration_ms: None,
                user_hash: Some("u4".into()),
                client_ip_hash: None,
                instance: Some("inst-d".into()),
                extra_json: Some(serde_json::json!({"user_kind":123})),
            },
            EventInsert {
                ts_utc: dt_utc(2025, 12, 24, 0, 1, 5),
                route: None,
                feature: Some("save".into()),
                action: Some("submit".into()),
                method: None,
                status: None,
                duration_ms: None,
                user_hash: Some("u5".into()),
                client_ip_hash: None,
                instance: Some("inst-e".into()),
                extra_json: Some(serde_json::json!({"user_kind":""})),
            },
        ])
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
    let Json(resp) = get_stats_summary(State(state.clone()), Query(query))
        .await
        .unwrap();

    assert_eq!(resp.unique_users.total, 5);
    assert_eq!(
        resp.unique_users.by_kind,
        vec![("official".to_string(), 2), ("taptap".to_string(), 1)]
    );

    let query = StatsSummaryQuery {
        start: None,
        end: None,
        timezone: Some("Asia/Shanghai".into()),
        feature: Some("bestn".into()),
        include: Some("user_kinds".into()),
        top: Some(10),
    };
    let Json(resp) = get_stats_summary(State(state), Query(query)).await.unwrap();

    assert_eq!(resp.unique_users.total, 2);
    assert_eq!(resp.unique_users.by_kind, vec![("official".to_string(), 2)]);
}

#[tokio::test]
async fn stats_summary_latency_percentiles_match_existing_index_rule() {
    let sqlite_path = tmp_sqlite_path("stats_summary_latency_percentiles");
    let state = build_test_state(&sqlite_path).await;
    let storage = state.stats_storage.as_ref().unwrap().clone();

    storage
        .insert_events(&[
            EventInsert {
                ts_utc: dt_utc(2025, 12, 24, 0, 0, 1),
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
                ts_utc: dt_utc(2025, 12, 24, 0, 0, 2),
                route: Some("/image/bn".into()),
                feature: None,
                action: None,
                method: Some("GET".into()),
                status: Some(200),
                duration_ms: Some(20),
                user_hash: None,
                client_ip_hash: Some("ip2".into()),
                instance: Some("inst-a".into()),
                extra_json: None,
            },
            EventInsert {
                ts_utc: dt_utc(2025, 12, 24, 0, 0, 3),
                route: Some("/save".into()),
                feature: None,
                action: None,
                method: Some("POST".into()),
                status: Some(500),
                duration_ms: Some(30),
                user_hash: None,
                client_ip_hash: Some("ip3".into()),
                instance: Some("inst-a".into()),
                extra_json: None,
            },
            EventInsert {
                ts_utc: dt_utc(2025, 12, 24, 0, 0, 4),
                route: Some("/song/search".into()),
                feature: None,
                action: None,
                method: Some("GET".into()),
                status: Some(200),
                duration_ms: Some(40),
                user_hash: None,
                client_ip_hash: Some("ip4".into()),
                instance: Some("inst-a".into()),
                extra_json: None,
            },
            EventInsert {
                ts_utc: dt_utc(2025, 12, 24, 0, 0, 5),
                route: None,
                feature: Some("bestn".into()),
                action: Some("render".into()),
                method: None,
                status: None,
                duration_ms: Some(999),
                user_hash: Some("u1".into()),
                client_ip_hash: None,
                instance: Some("inst-a".into()),
                extra_json: None,
            },
        ])
        .await
        .unwrap();

    let query = StatsSummaryQuery {
        start: None,
        end: None,
        timezone: Some("Asia/Shanghai".into()),
        feature: None,
        include: Some("latency".into()),
        top: Some(10),
    };
    let Json(resp) = get_stats_summary(State(state), Query(query)).await.unwrap();

    let latency = resp.latency.unwrap();
    assert_eq!(latency.sample_count, 4);
    assert_eq!(latency.avg_ms, Some(25.0));
    assert_eq!(latency.p50_ms, Some(30));
    assert_eq!(latency.p95_ms, Some(40));
    assert_eq!(latency.max_ms, Some(40));
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
async fn daily_stats_dst_fallback_respects_route_and_method_filters() {
    let sqlite_path = tmp_sqlite_path("stats_daily_dst_route_method_filter");
    let state = build_test_state(&sqlite_path).await;
    let storage = state.stats_storage.as_ref().unwrap().clone();

    storage
        .insert_events(&[
            EventInsert {
                // 纽约时间：2025-03-09 01:00:00-05，夏令时切换前。
                ts_utc: dt_utc(2025, 3, 9, 6, 0, 0),
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
                // 纽约时间：2025-03-09 03:30:00-04，夏令时已切换。
                ts_utc: dt_utc(2025, 3, 9, 7, 30, 0),
                route: Some("/image/bn".into()),
                feature: None,
                action: None,
                method: Some("GET".into()),
                status: Some(500),
                duration_ms: Some(10),
                user_hash: None,
                client_ip_hash: Some("ip2".into()),
                instance: Some("inst-a".into()),
                extra_json: None,
            },
            EventInsert {
                // 同路由不同方法，不应计入 /image/bn GET。
                ts_utc: dt_utc(2025, 3, 9, 8, 0, 0),
                route: Some("/image/bn".into()),
                feature: None,
                action: None,
                method: Some("POST".into()),
                status: Some(404),
                duration_ms: Some(10),
                user_hash: None,
                client_ip_hash: Some("ip3".into()),
                instance: Some("inst-a".into()),
                extra_json: None,
            },
            EventInsert {
                // 同方法不同路由，不应计入 /image/bn GET。
                ts_utc: dt_utc(2025, 3, 9, 8, 1, 0),
                route: Some("/save".into()),
                feature: None,
                action: None,
                method: Some("GET".into()),
                status: Some(500),
                duration_ms: Some(10),
                user_hash: None,
                client_ip_hash: Some("ip4".into()),
                instance: Some("inst-a".into()),
                extra_json: None,
            },
            EventInsert {
                // 纽约时间：2025-03-10 00:30:00-04，验证 fallback 按本地日切片。
                ts_utc: dt_utc(2025, 3, 10, 4, 30, 0),
                route: Some("/image/bn".into()),
                feature: None,
                action: None,
                method: Some("GET".into()),
                status: Some(404),
                duration_ms: Some(10),
                user_hash: None,
                client_ip_hash: Some("ip5".into()),
                instance: Some("inst-a".into()),
                extra_json: None,
            },
        ])
        .await
        .unwrap();

    let q = DailyQuery {
        start: "2025-03-08".into(),
        end: "2025-03-10".into(),
        timezone: Some("America/New_York".into()),
        feature: None,
        route: Some("/image/bn".into()),
        method: Some("GET".into()),
    };
    let Json(rows) = get_daily_stats(State(state), Query(q)).await.unwrap();

    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].date, "2025-03-09");
    assert_eq!(rows[0].route.as_deref(), Some("/image/bn"));
    assert_eq!(rows[0].method.as_deref(), Some("GET"));
    assert_eq!(rows[0].count, 2);
    assert_eq!(rows[0].err_count, 1);
    assert_eq!(rows[1].date, "2025-03-10");
    assert_eq!(rows[1].route.as_deref(), Some("/image/bn"));
    assert_eq!(rows[1].method.as_deref(), Some("GET"));
    assert_eq!(rows[1].count, 1);
    assert_eq!(rows[1].err_count, 1);
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
    let Json(resp) = get_daily_features(State(state.clone()), Query(q))
        .await
        .unwrap();
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

    let q = DailyFeaturesQuery {
        start: "2025-12-24".into(),
        end: "2025-12-25".into(),
        timezone: Some("Asia/Shanghai".into()),
        feature: Some("bestn".into()),
    };
    let Json(resp) = get_daily_features(State(state), Query(q)).await.unwrap();
    assert_eq!(resp.feature_filter.as_deref(), Some("bestn"));
    assert_eq!(resp.rows.len(), 1);
    assert_eq!(resp.rows[0].date, "2025-12-24");
    assert_eq!(resp.rows[0].feature, "bestn");
    assert_eq!(resp.rows[0].count, 2);
    assert_eq!(resp.rows[0].unique_users, 1);
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
async fn daily_http_dst_fallback_pushes_top_down_without_truncating_totals() {
    let sqlite_path = tmp_sqlite_path("stats_daily_http_dst_top");
    let state = build_test_state(&sqlite_path).await;
    let storage = state.stats_storage.as_ref().unwrap().clone();

    storage
        .insert_events(&[
            EventInsert {
                // 纽约时间：2025-03-09 01:00:00-05，夏令时切换前。
                ts_utc: dt_utc(2025, 3, 9, 6, 0, 0),
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
                // 纽约时间：2025-03-09 03:30:00-04，夏令时已切换。
                ts_utc: dt_utc(2025, 3, 9, 7, 30, 0),
                route: Some("/image/bn".into()),
                feature: None,
                action: None,
                method: Some("GET".into()),
                status: Some(500),
                duration_ms: Some(10),
                user_hash: None,
                client_ip_hash: Some("ip2".into()),
                instance: Some("inst-a".into()),
                extra_json: None,
            },
            EventInsert {
                ts_utc: dt_utc(2025, 3, 9, 8, 0, 0),
                route: Some("/save".into()),
                feature: None,
                action: None,
                method: Some("POST".into()),
                status: Some(404),
                duration_ms: Some(10),
                user_hash: None,
                client_ip_hash: Some("ip3".into()),
                instance: Some("inst-a".into()),
                extra_json: None,
            },
            EventInsert {
                ts_utc: dt_utc(2025, 3, 9, 8, 1, 0),
                route: Some("/song/search".into()),
                feature: None,
                action: None,
                method: Some("GET".into()),
                status: Some(200),
                duration_ms: Some(10),
                user_hash: None,
                client_ip_hash: Some("ip4".into()),
                instance: Some("inst-a".into()),
                extra_json: None,
            },
        ])
        .await
        .unwrap();

    let q = DailyHttpQuery {
        start: "2025-03-08".into(),
        end: "2025-03-10".into(),
        timezone: Some("America/New_York".into()),
        route: None,
        method: None,
        top: Some(1),
    };
    let Json(resp) = get_daily_http(State(state), Query(q)).await.unwrap();

    assert_eq!(resp.totals.len(), 3);
    assert_eq!(resp.totals[0].date, "2025-03-08");
    assert_eq!(resp.totals[0].total, 0);
    assert_eq!(resp.totals[1].date, "2025-03-09");
    assert_eq!(resp.totals[1].total, 4);
    assert_eq!(resp.totals[1].errors, 2);
    assert_eq!(resp.totals[1].client_errors, 1);
    assert_eq!(resp.totals[1].server_errors, 1);
    assert_eq!(resp.totals[2].date, "2025-03-10");
    assert_eq!(resp.totals[2].total, 0);

    assert_eq!(resp.routes.len(), 1);
    assert_eq!(resp.routes[0].date, "2025-03-09");
    assert_eq!(resp.routes[0].route, "/image/bn");
    assert_eq!(resp.routes[0].method, "GET");
    assert_eq!(resp.routes[0].total, 2);
    assert_eq!(resp.routes[0].errors, 1);
}

#[tokio::test]
async fn daily_http_dst_fallback_respects_route_and_method_filters() {
    let sqlite_path = tmp_sqlite_path("stats_daily_http_dst_route_method_filter");
    let state = build_test_state(&sqlite_path).await;
    let storage = state.stats_storage.as_ref().unwrap().clone();

    storage
        .insert_events(&[
            EventInsert {
                // 纽约时间：2025-03-09 01:00:00-05，夏令时切换前。
                ts_utc: dt_utc(2025, 3, 9, 6, 0, 0),
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
                // 纽约时间：2025-03-09 03:30:00-04，夏令时已切换。
                ts_utc: dt_utc(2025, 3, 9, 7, 30, 0),
                route: Some("/image/bn".into()),
                feature: None,
                action: None,
                method: Some("GET".into()),
                status: Some(500),
                duration_ms: Some(10),
                user_hash: None,
                client_ip_hash: Some("ip2".into()),
                instance: Some("inst-a".into()),
                extra_json: None,
            },
            EventInsert {
                // 同路由不同方法，不应计入 /image/bn GET。
                ts_utc: dt_utc(2025, 3, 9, 8, 0, 0),
                route: Some("/image/bn".into()),
                feature: None,
                action: None,
                method: Some("POST".into()),
                status: Some(404),
                duration_ms: Some(10),
                user_hash: None,
                client_ip_hash: Some("ip3".into()),
                instance: Some("inst-a".into()),
                extra_json: None,
            },
            EventInsert {
                // 同方法不同路由，不应计入 /image/bn GET。
                ts_utc: dt_utc(2025, 3, 9, 8, 1, 0),
                route: Some("/save".into()),
                feature: None,
                action: None,
                method: Some("GET".into()),
                status: Some(500),
                duration_ms: Some(10),
                user_hash: None,
                client_ip_hash: Some("ip4".into()),
                instance: Some("inst-a".into()),
                extra_json: None,
            },
            EventInsert {
                // 纽约时间：2025-03-10 00:30:00-04，验证 fallback 按本地日切片。
                ts_utc: dt_utc(2025, 3, 10, 4, 30, 0),
                route: Some("/image/bn".into()),
                feature: None,
                action: None,
                method: Some("GET".into()),
                status: Some(404),
                duration_ms: Some(10),
                user_hash: None,
                client_ip_hash: Some("ip5".into()),
                instance: Some("inst-a".into()),
                extra_json: None,
            },
        ])
        .await
        .unwrap();

    let q = DailyHttpQuery {
        start: "2025-03-08".into(),
        end: "2025-03-10".into(),
        timezone: Some("America/New_York".into()),
        route: Some("/image/bn".into()),
        method: Some("GET".into()),
        top: Some(1),
    };
    let Json(resp) = get_daily_http(State(state), Query(q)).await.unwrap();

    assert_eq!(resp.route_filter.as_deref(), Some("/image/bn"));
    assert_eq!(resp.method_filter.as_deref(), Some("GET"));
    assert_eq!(resp.totals.len(), 3);
    assert_eq!(resp.totals[0].date, "2025-03-08");
    assert_eq!(resp.totals[0].total, 0);
    assert_eq!(resp.totals[1].date, "2025-03-09");
    assert_eq!(resp.totals[1].total, 2);
    assert_eq!(resp.totals[1].errors, 1);
    assert_eq!(resp.totals[1].client_errors, 0);
    assert_eq!(resp.totals[1].server_errors, 1);
    assert_eq!(resp.totals[2].date, "2025-03-10");
    assert_eq!(resp.totals[2].total, 1);
    assert_eq!(resp.totals[2].errors, 1);
    assert_eq!(resp.totals[2].client_errors, 1);
    assert_eq!(resp.totals[2].server_errors, 0);

    assert_eq!(resp.routes.len(), 2);
    assert_eq!(resp.routes[0].date, "2025-03-09");
    assert_eq!(resp.routes[0].route, "/image/bn");
    assert_eq!(resp.routes[0].method, "GET");
    assert_eq!(resp.routes[0].total, 2);
    assert_eq!(resp.routes[0].errors, 1);
    assert_eq!(resp.routes[0].client_errors, 0);
    assert_eq!(resp.routes[0].server_errors, 1);
    assert_eq!(resp.routes[1].date, "2025-03-10");
    assert_eq!(resp.routes[1].route, "/image/bn");
    assert_eq!(resp.routes[1].method, "GET");
    assert_eq!(resp.routes[1].total, 1);
    assert_eq!(resp.routes[1].errors, 1);
    assert_eq!(resp.routes[1].client_errors, 1);
    assert_eq!(resp.routes[1].server_errors, 0);
}

#[tokio::test]
async fn daily_http_cache_returns_stale_within_ttl() {
    let sqlite_path = tmp_sqlite_path("stats_daily_http_cache_hit");
    let state = build_test_state(&sqlite_path).await;
    let storage = state.stats_storage.as_ref().unwrap().clone();

    storage
        .insert_events(&[EventInsert {
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
        }])
        .await
        .unwrap();

    let q = DailyHttpQuery {
        start: "2025-12-24".into(),
        end: "2025-12-24".into(),
        timezone: Some("Asia/Shanghai".into()),
        route: None,
        method: None,
        top: Some(200),
    };
    let Json(first) = get_daily_http(State(state.clone()), Query(q))
        .await
        .unwrap();
    assert_eq!(first.totals.len(), 1);
    assert_eq!(first.totals[0].total, 1);

    storage
        .insert_events(&[EventInsert {
            ts_utc: dt_utc(2025, 12, 24, 1, 0, 1),
            route: Some("/image/bn".into()),
            feature: None,
            action: None,
            method: Some("GET".into()),
            status: Some(500),
            duration_ms: Some(10),
            user_hash: None,
            client_ip_hash: Some("ip2".into()),
            instance: Some("inst-a".into()),
            extra_json: None,
        }])
        .await
        .unwrap();

    let q = DailyHttpQuery {
        start: "2025-12-24".into(),
        end: "2025-12-24".into(),
        timezone: Some("Asia/Shanghai".into()),
        route: None,
        method: None,
        top: Some(200),
    };
    let Json(second) = get_daily_http(State(state), Query(q)).await.unwrap();
    assert_eq!(second.totals.len(), 1);
    assert_eq!(second.totals[0].total, 1);
    assert_eq!(second.totals[0].errors, 0);
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
async fn latency_agg_respects_feature_filter() {
    let sqlite_path = tmp_sqlite_path("stats_latency_feature_filter");
    let state = build_test_state(&sqlite_path).await;
    let storage = state.stats_storage.as_ref().unwrap().clone();

    storage
        .insert_events(&[
            EventInsert {
                ts_utc: dt_utc(2025, 12, 24, 1, 0, 0),
                route: Some("/image/bn".into()),
                feature: Some("bestn".into()),
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
                ts_utc: dt_utc(2025, 12, 24, 1, 0, 1),
                route: Some("/image/bn".into()),
                feature: Some("bestn".into()),
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
                ts_utc: dt_utc(2025, 12, 24, 1, 0, 2),
                route: Some("/save".into()),
                feature: Some("save".into()),
                action: None,
                method: Some("POST".into()),
                status: Some(200),
                duration_ms: Some(20),
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
        feature: Some("bestn".into()),
        route: None,
        method: None,
    };
    let Json(resp) = get_latency_agg(State(state), Query(q)).await.unwrap();

    assert_eq!(resp.rows.len(), 1);
    assert_eq!(resp.rows[0].feature.as_deref(), Some("bestn"));
    assert_eq!(resp.rows[0].route.as_deref(), Some("/image/bn"));
    assert_eq!(resp.rows[0].method.as_deref(), Some("GET"));
    assert_eq!(resp.rows[0].count, 2);
    assert_eq!(resp.rows[0].min_ms, Some(100));
    assert_eq!(resp.rows[0].avg_ms, Some(200.0));
    assert_eq!(resp.rows[0].max_ms, Some(300));
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
