use serde::Serialize;

#[derive(Debug, Clone)]
pub struct OpenApiAuthContext {
    pub developer_id: String,
    pub key_id: String,
    pub scopes: Vec<String>,
    pub client_ip: Option<String>,
}

#[derive(Debug, Clone)]
pub struct OpenApiRoutePolicy {
    pub required_scopes: &'static [&'static str],
}

impl OpenApiRoutePolicy {
    #[must_use]
    pub const fn new(required_scopes: &'static [&'static str]) -> Self {
        Self { required_scopes }
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct RateWindow {
    pub minute_slot: i64,
    pub count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) struct RateBucketKey {
    pub key_id: String,
    pub route: String,
    pub client_ip: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenApiRateLimitBucketSnapshot {
    pub route: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_ip: Option<String>,
    pub request_count: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenApiRateLimitSnapshot {
    pub minute_slot: i64,
    pub bucket_count: usize,
    pub total_request_count: u64,
    pub buckets: Vec<OpenApiRateLimitBucketSnapshot>,
}
