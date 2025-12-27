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
#[serde(rename_all = "camelCase")]
pub struct DailyAggRow {
    /// 日期（本地时区）YYYY-MM-DD
    pub date: String,
    /// 业务功能名（bestn/single_query/save 等）
    pub feature: Option<String>,
    /// 路由模板（例如 /image/bn）
    pub route: Option<String>,
    /// HTTP 方法（GET/POST 等）
    pub method: Option<String>,
    /// 调用次数
    pub count: i64,
    /// 错误次数（status >= 400）
    pub err_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatsFeatureEvent {
    pub feature: String,
    pub action: String,
    pub at: DateTime<Utc>,
}
