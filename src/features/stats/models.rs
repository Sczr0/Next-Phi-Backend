use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventInsert {
    pub ts_utc: DateTime<Utc>,
    pub route: Option<String>,
    pub feature: Option<String>,
    pub action: Option<String>,
    pub method: Option<String>,
    pub status: Option<u16>,
    pub duration_ms: Option<i64>,
    pub user_hash: Option<String>,
    pub client_ip_hash: Option<String>,
    pub instance: Option<String>,
    pub extra_json: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct DailyAggRow {
    pub date: String,
    pub feature: Option<String>,
    pub route: Option<String>,
    pub method: Option<String>,
    pub count: i64,
    pub err_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatsFeatureEvent {
    pub feature: String,
    pub action: String,
    pub at: DateTime<Utc>,
}

