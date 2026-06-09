use std::{
    sync::{Arc, OnceLock},
    time::Duration,
};

use moka::future::Cache;

use super::{daily_http::DailyHttpResponse, params::IncludeFlags, summary::StatsSummaryResponse};

const STATS_SUMMARY_CACHE_MAX_ENTRIES: u64 = 256;
const STATS_SUMMARY_CACHE_TTL_SECS: u64 = 60;
const STATS_SUMMARY_CACHE_TTI_SECS: u64 = 30;
const DAILY_HTTP_CACHE_MAX_ENTRIES: u64 = 256;
const DAILY_HTTP_CACHE_TTL_SECS: u64 = 20;
const DAILY_HTTP_CACHE_TTI_SECS: u64 = 10;

pub(super) fn stats_summary_cache() -> &'static Cache<String, Arc<StatsSummaryResponse>> {
    static CACHE: OnceLock<Cache<String, Arc<StatsSummaryResponse>>> = OnceLock::new();
    CACHE.get_or_init(|| {
        Cache::builder()
            .max_capacity(STATS_SUMMARY_CACHE_MAX_ENTRIES)
            .time_to_live(Duration::from_secs(STATS_SUMMARY_CACHE_TTL_SECS))
            .time_to_idle(Duration::from_secs(STATS_SUMMARY_CACHE_TTI_SECS))
            .build()
    })
}

pub(super) fn build_stats_summary_cache_key(
    storage: &Arc<crate::features::stats::storage::StatsStorage>,
    tz_name: &str,
    start_utc: Option<&str>,
    end_utc: Option<&str>,
    feature: Option<&str>,
    include: IncludeFlags,
    top: i64,
) -> String {
    let storage_key = Arc::as_ptr(storage) as usize;
    format!(
        "{storage_key}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}",
        tz_name,
        start_utc.unwrap_or(""),
        end_utc.unwrap_or(""),
        feature.unwrap_or(""),
        top,
        u8::from(include.routes),
        u8::from(include.methods),
        u8::from(include.status_codes),
        u8::from(include.instances),
        u8::from(include.actions),
        u8::from(include.latency),
        u8::from(include.unique_ips),
        u8::from(include.user_kinds),
    )
}

pub(super) fn daily_http_cache() -> &'static Cache<String, Arc<DailyHttpResponse>> {
    static CACHE: OnceLock<Cache<String, Arc<DailyHttpResponse>>> = OnceLock::new();
    CACHE.get_or_init(|| {
        Cache::builder()
            .max_capacity(DAILY_HTTP_CACHE_MAX_ENTRIES)
            .time_to_live(Duration::from_secs(DAILY_HTTP_CACHE_TTL_SECS))
            .time_to_idle(Duration::from_secs(DAILY_HTTP_CACHE_TTI_SECS))
            .build()
    })
}

pub(super) fn build_daily_http_cache_key(
    storage: &Arc<crate::features::stats::storage::StatsStorage>,
    tz_name: &str,
    start: &str,
    end: &str,
    route: Option<&str>,
    method: Option<&str>,
    top: i64,
) -> String {
    let storage_key = Arc::as_ptr(storage) as usize;
    format!(
        "{storage_key}|{tz_name}|{start}|{end}|{}|{}|{top}",
        route.unwrap_or(""),
        method.unwrap_or("")
    )
}
