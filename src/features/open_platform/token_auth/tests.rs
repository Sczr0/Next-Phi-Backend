use axum::http::{HeaderMap, HeaderValue};

use super::{
    crypto::client_ip_from_headers,
    rate_limit::{ensure_rate_limit, snapshot_rate_limit_by_key},
};

#[test]
fn client_ip_prefers_x_forwarded_for() {
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-forwarded-for",
        HeaderValue::from_static("1.2.3.4, 9.9.9.9"),
    );
    headers.insert("x-real-ip", HeaderValue::from_static("8.8.8.8"));
    assert_eq!(
        client_ip_from_headers(&headers),
        Some("1.2.3.4".to_string())
    );
}

#[tokio::test]
async fn rate_limit_blocks_when_exceeded() {
    let key = format!("key_test_{}", uuid::Uuid::new_v4().simple());
    let route = "POST /open/save";
    let ip = Some("127.0.0.1");
    let now = 1_700_000_000_i64;

    assert!(ensure_rate_limit(&key, route, ip, 2, now).await);
    assert!(ensure_rate_limit(&key, route, ip, 2, now + 1).await);
    assert!(!ensure_rate_limit(&key, route, ip, 2, now + 2).await);
}

#[tokio::test]
async fn rate_limit_isolated_by_route() {
    let key = format!("key_route_{}", uuid::Uuid::new_v4().simple());
    let ip = Some("127.0.0.1");
    let now = 1_710_000_000_i64;

    assert!(ensure_rate_limit(&key, "GET /open/songs/search", ip, 1, now).await);
    assert!(ensure_rate_limit(&key, "POST /open/save", ip, 1, now).await);
    assert!(!ensure_rate_limit(&key, "GET /open/songs/search", ip, 1, now + 1).await);
    assert!(!ensure_rate_limit(&key, "POST /open/save", ip, 1, now + 1).await);
}

#[tokio::test]
async fn snapshot_aggregates_by_route() {
    let key = format!("key_snapshot_{}", uuid::Uuid::new_v4().simple());
    let now = 1_720_000_000_i64;

    assert!(ensure_rate_limit(&key, "GET /open/songs/search", Some("10.0.0.1"), 10, now).await);
    assert!(
        ensure_rate_limit(
            &key,
            "GET /open/songs/search",
            Some("10.0.0.2"),
            10,
            now + 1,
        )
        .await
    );
    assert!(ensure_rate_limit(&key, "POST /open/save", None, 10, now + 2).await);

    let aggregated = snapshot_rate_limit_by_key(&key, false, 100, now + 2).await;
    assert_eq!(aggregated.total_request_count, 3);
    assert_eq!(aggregated.bucket_count, 2);
    assert_eq!(aggregated.buckets.len(), 2);
    assert!(
        aggregated
            .buckets
            .iter()
            .any(|b| b.route == "GET /open/songs/search" && b.request_count == 2)
    );

    let detailed = snapshot_rate_limit_by_key(&key, true, 100, now + 2).await;
    assert_eq!(detailed.total_request_count, 3);
    assert_eq!(detailed.bucket_count, 3);
    assert_eq!(detailed.buckets.len(), 3);
}
