use sqlx::SqlitePool;

use super::models::DailyAggRow;

mod connection;
mod events;
mod leaderboard;
mod session;

/// 保存提交入库参数，减少函数参数数量
pub struct SubmissionRecord<'a> {
    pub user_hash: &'a str,
    pub total_rks: f64,
    pub rks_jump: f64,
    pub route: &'a str,
    pub client_ip_hash: Option<&'a str>,
    pub details_json: Option<&'a str>,
    pub suspicion_score: f64,
    pub now_rfc3339: &'a str,
}

#[derive(Debug, Clone, Copy)]
pub struct UserAliasDefaults<'a> {
    pub is_public: bool,
    pub show_rks_composition: bool,
    pub show_best_top3: bool,
    pub show_ap_top3: bool,
    pub now_rfc3339: &'a str,
}

#[derive(Debug, Clone)]
pub struct ArchiveEventRow {
    pub ts_utc: String,
    pub route: Option<String>,
    pub feature: Option<String>,
    pub action: Option<String>,
    pub method: Option<String>,
    pub status: Option<i64>,
    pub duration_ms: Option<i64>,
    pub user_hash: Option<String>,
    pub client_ip_hash: Option<String>,
    pub instance: Option<String>,
    pub extra_json: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DailyAggSliceRow {
    pub feature: Option<String>,
    pub route: Option<String>,
    pub method: Option<String>,
    pub count: i64,
    pub err_count: i64,
}

#[derive(Debug, Clone)]
pub struct DailyFeatureUsageDateRow {
    pub date: String,
    pub feature: String,
    pub count: i64,
    pub unique_users: i64,
}

#[derive(Debug, Clone)]
pub struct DailyFeatureUsageSliceRow {
    pub feature: String,
    pub count: i64,
    pub unique_users: i64,
}

#[derive(Debug, Clone)]
pub struct DailyDauDateRow {
    pub date: String,
    pub active_users: i64,
    pub active_ips: i64,
}

#[derive(Debug, Clone)]
pub struct LatencyAggBucketRow {
    pub bucket: String,
    pub feature: Option<String>,
    pub route: Option<String>,
    pub method: Option<String>,
    pub count: i64,
    pub min_ms: Option<i64>,
    pub avg_ms: Option<f64>,
    pub max_ms: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct LatencyAggSliceRow {
    pub feature: Option<String>,
    pub route: Option<String>,
    pub method: Option<String>,
    pub count: i64,
    pub min_ms: Option<i64>,
    pub avg_ms: Option<f64>,
    pub max_ms: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct DailyHttpRouteMetricRow {
    pub date: String,
    pub route: String,
    pub method: String,
    pub total: i64,
    pub errors: i64,
    pub client_errors: i64,
    pub server_errors: i64,
}

#[derive(Debug, Clone)]
pub struct DailyHttpRouteMetricSliceRow {
    pub route: String,
    pub method: String,
    pub total: i64,
    pub errors: i64,
    pub client_errors: i64,
    pub server_errors: i64,
}

#[derive(Debug, Clone)]
pub struct DailyHttpTotalMetricRow {
    pub date: String,
    pub total: i64,
    pub errors: i64,
    pub client_errors: i64,
    pub server_errors: i64,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SummaryIncludeFlags {
    pub routes: bool,
    pub methods: bool,
    pub status_codes: bool,
    pub instances: bool,
    pub actions: bool,
    pub latency: bool,
    pub unique_ips: bool,
    pub user_kinds: bool,
}

impl SummaryIncludeFlags {
    #[must_use]
    pub fn any(self) -> bool {
        self.routes
            || self.methods
            || self.status_codes
            || self.instances
            || self.actions
            || self.latency
            || self.unique_ips
            || self.user_kinds
    }

    #[must_use]
    pub fn any_http(self) -> bool {
        self.routes || self.methods || self.status_codes || self.latency || self.unique_ips
    }
}

#[derive(Debug, Clone)]
pub struct SummaryFeatureRow {
    pub feature: String,
    pub count: i64,
    pub last_ts: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SummaryRouteRow {
    pub route: String,
    pub count: i64,
    pub err_count: i64,
    pub last_ts: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SummaryMethodRow {
    pub method: String,
    pub count: i64,
}

#[derive(Debug, Clone)]
pub struct SummaryStatusCodeRow {
    pub status: i64,
    pub count: i64,
}

#[derive(Debug, Clone)]
pub struct SummaryInstanceRow {
    pub instance: String,
    pub count: i64,
    pub last_ts: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SummaryActionRow {
    pub feature: String,
    pub action: String,
    pub count: i64,
    pub last_ts: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SummaryLatencyData {
    pub sample_count: i64,
    pub avg_ms: Option<f64>,
    pub p50_ms: Option<i64>,
    pub p95_ms: Option<i64>,
    pub max_ms: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct StatsSummaryData {
    pub first_event_ts: Option<String>,
    pub last_event_ts: Option<String>,
    pub features: Vec<SummaryFeatureRow>,
    pub unique_users_total: i64,
    pub by_kind: Vec<(String, i64)>,
    pub events_total: Option<i64>,
    pub http_total: Option<i64>,
    pub http_errors: Option<i64>,
    pub routes: Option<Vec<SummaryRouteRow>>,
    pub methods: Option<Vec<SummaryMethodRow>>,
    pub status_codes: Option<Vec<SummaryStatusCodeRow>>,
    pub instances: Option<Vec<SummaryInstanceRow>>,
    pub actions: Option<Vec<SummaryActionRow>>,
    pub latency: Option<SummaryLatencyData>,
    pub unique_ips: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct RksHistoryEntry {
    pub rks: f64,
    pub rks_jump: f64,
    pub created_at: String,
}

#[derive(Clone)]
pub struct StatsStorage {
    pub pool: SqlitePool,
}
