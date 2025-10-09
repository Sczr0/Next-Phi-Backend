use std::time::{Duration, Instant};

use moka::future::Cache;

use super::models::SessionData;

#[derive(Debug, Clone)]
pub enum QrCodeStatus {
    Pending {
        device_code: String,
        device_id: String,
        interval_secs: u64,
        next_poll_at: Instant,
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

impl QrCodeService {
    pub fn new() -> Self {
        let cache = Cache::builder()
            .max_capacity(10_000)
            .time_to_live(Duration::from_secs(5 * 60))
            .build();
        Self { cache }
    }

    pub async fn set_pending(
        &self,
        qr_id: String,
        device_code: String,
        device_id: String,
        interval_secs: u64,
    ) {
        let next_poll_at = Instant::now();
        self.cache
            .insert(
                qr_id,
                QrCodeStatus::Pending {
                    device_code,
                    device_id,
                    interval_secs,
                    next_poll_at,
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
                },
            )
            .await;
    }
}
