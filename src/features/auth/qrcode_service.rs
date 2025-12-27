use std::time::{Duration, Instant};

use moka::future::Cache;

use super::models::SessionData;

const DEFAULT_QRCODE_EXPIRES_SECS: u64 = 5 * 60;
const DEFAULT_CACHE_TTL_SECS: u64 = 30 * 60;

#[derive(Debug, Clone)]
pub enum QrCodeStatus {
    Pending {
        device_code: String,
        device_id: String,
        interval_secs: u64,
        next_poll_at: Instant,
        expires_at: Instant,
        version: Option<String>, // 添加版本信息以确保轮询时使用正确的API
    },
    Scanned,
    Confirmed {
        session_data: SessionData,
    },
}

#[derive(Clone)]
pub struct QrCodeService {
    pub cache: Cache<String, QrCodeStatus>,
}

impl Default for QrCodeService {
    fn default() -> Self {
        Self::new()
    }
}

impl QrCodeService {
    pub fn new() -> Self {
        let cache = Cache::builder()
            .max_capacity(10_000)
            .time_to_live(Duration::from_secs(DEFAULT_CACHE_TTL_SECS))
            .build();
        Self { cache }
    }

    pub async fn set_pending(
        &self,
        qr_id: String,
        device_code: String,
        device_id: String,
        interval_secs: u64,
        expires_in_secs: Option<u64>,
        version: Option<String>,
    ) {
        let now = Instant::now();
        let next_poll_at = now;
        let expires_secs = expires_in_secs.unwrap_or(DEFAULT_QRCODE_EXPIRES_SECS);
        let expires_at = now + Duration::from_secs(expires_secs);
        self.cache
            .insert(
                qr_id,
                QrCodeStatus::Pending {
                    device_code,
                    device_id,
                    interval_secs,
                    next_poll_at,
                    expires_at,
                    version,
                },
            )
            .await;
    }

    pub async fn set_confirmed(&self, qr_id: &str, session_data: SessionData) {
        self.cache
            .insert(qr_id.to_string(), QrCodeStatus::Confirmed { session_data })
            .await;
    }

    pub async fn get(&self, qr_id: &str) -> Option<QrCodeStatus> {
        self.cache.get(qr_id).await
    }

    pub async fn remove(&self, qr_id: &str) {
        self.cache.invalidate(qr_id).await;
    }

    pub async fn set_pending_next_poll(
        &self,
        qr_id: &str,
        device_code: String,
        device_id: String,
        interval_secs: u64,
        expires_at: Instant,
        version: Option<String>,
    ) {
        let next_poll_at = Instant::now() + Duration::from_secs(interval_secs);
        self.cache
            .insert(
                qr_id.to_string(),
                QrCodeStatus::Pending {
                    device_code,
                    device_id,
                    interval_secs,
                    next_poll_at,
                    expires_at,
                    version,
                },
            )
            .await;
    }
}
