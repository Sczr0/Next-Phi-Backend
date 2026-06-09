use std::collections::HashMap;

use axum::extract::{MatchedPath, Request};
use once_cell::sync::Lazy;
use tokio::sync::Mutex;

use super::models::{
    OpenApiRateLimitBucketSnapshot, OpenApiRateLimitSnapshot, RateBucketKey, RateWindow,
};

static OPEN_API_RATE_LIMITER: Lazy<Mutex<HashMap<RateBucketKey, RateWindow>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

pub(super) async fn ensure_rate_limit(
    key_id: &str,
    route: &str,
    client_ip: Option<&str>,
    per_minute_limit: u32,
    now_ts: i64,
) -> bool {
    let minute_slot = now_ts / 60;
    let bucket_key = RateBucketKey {
        key_id: key_id.to_string(),
        route: route.to_string(),
        client_ip: client_ip.unwrap_or("-").to_string(),
    };
    let mut guard = OPEN_API_RATE_LIMITER.lock().await;
    let entry = guard.entry(bucket_key).or_insert(RateWindow {
        minute_slot,
        count: 0,
    });
    if entry.minute_slot != minute_slot {
        entry.minute_slot = minute_slot;
        entry.count = 0;
    }

    let max = per_minute_limit.max(1);
    if entry.count >= max {
        return false;
    }
    entry.count += 1;

    if guard.len() > 50_000 {
        guard.retain(|_, v| v.minute_slot >= minute_slot - 1);
    }
    true
}

pub(super) fn resolve_route_bucket(req: &Request) -> String {
    let method = req.method().as_str();
    let path = req
        .extensions()
        .get::<MatchedPath>()
        .map_or(req.uri().path(), MatchedPath::as_str);
    format!("{method} {path}")
}

pub async fn snapshot_rate_limit_by_key(
    key_id: &str,
    include_client_ip: bool,
    limit: usize,
    now_ts: i64,
) -> OpenApiRateLimitSnapshot {
    let minute_slot = now_ts / 60;
    let limit = limit.clamp(1, 500);
    let guard = OPEN_API_RATE_LIMITER.lock().await;

    if include_client_ip {
        let mut buckets: Vec<OpenApiRateLimitBucketSnapshot> = guard
            .iter()
            .filter(|(bucket, window)| bucket.key_id == key_id && window.minute_slot == minute_slot)
            .map(|(bucket, window)| OpenApiRateLimitBucketSnapshot {
                route: bucket.route.clone(),
                client_ip: if bucket.client_ip == "-" {
                    None
                } else {
                    Some(bucket.client_ip.clone())
                },
                request_count: window.count,
            })
            .collect();
        buckets.sort_by(|a, b| {
            b.request_count
                .cmp(&a.request_count)
                .then_with(|| a.route.cmp(&b.route))
                .then_with(|| a.client_ip.cmp(&b.client_ip))
        });
        let total_request_count = buckets.iter().fold(0_u64, |acc, b| {
            acc.saturating_add(u64::from(b.request_count))
        });
        let bucket_count = buckets.len();
        buckets.truncate(limit);
        return OpenApiRateLimitSnapshot {
            minute_slot,
            bucket_count,
            total_request_count,
            buckets,
        };
    }

    let mut per_route: HashMap<String, u32> = HashMap::new();
    let mut total_request_count = 0_u64;
    for (bucket, window) in guard.iter() {
        if bucket.key_id != key_id || window.minute_slot != minute_slot {
            continue;
        }
        total_request_count = total_request_count.saturating_add(u64::from(window.count));
        let route_counter = per_route.entry(bucket.route.clone()).or_insert(0);
        *route_counter = route_counter.saturating_add(window.count);
    }

    let mut buckets: Vec<OpenApiRateLimitBucketSnapshot> = per_route
        .into_iter()
        .map(|(route, request_count)| OpenApiRateLimitBucketSnapshot {
            route,
            client_ip: None,
            request_count,
        })
        .collect();
    buckets.sort_by(|a, b| {
        b.request_count
            .cmp(&a.request_count)
            .then_with(|| a.route.cmp(&b.route))
    });
    let bucket_count = buckets.len();
    buckets.truncate(limit);
    OpenApiRateLimitSnapshot {
        minute_slot,
        bucket_count,
        total_request_count,
        buckets,
    }
}
