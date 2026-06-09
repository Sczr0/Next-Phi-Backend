use std::sync::Arc;

use moka::future::Cache;
use once_cell::sync::OnceCell;
use tokio::time::Duration;
use uuid::Uuid;

use crate::{config::OpenPlatformConfig, error::AppError};

static OPEN_PLATFORM_AUTH_SERVICE: OnceCell<Arc<OpenPlatformAuthService>> = OnceCell::new();

#[derive(Clone)]
pub struct OpenPlatformAuthService {
    oauth_state_cache: Cache<String, bool>,
}

pub fn init_global(cfg: &OpenPlatformConfig) -> Result<(), AppError> {
    let service = OpenPlatformAuthService::new(cfg);
    OPEN_PLATFORM_AUTH_SERVICE
        .set(Arc::new(service))
        .map_err(|_| AppError::Internal("开放平台鉴权服务已初始化".into()))
}

pub(super) fn global() -> Result<&'static Arc<OpenPlatformAuthService>, AppError> {
    OPEN_PLATFORM_AUTH_SERVICE
        .get()
        .ok_or_else(|| AppError::Internal("开放平台鉴权服务未初始化".into()))
}

impl OpenPlatformAuthService {
    fn new(cfg: &OpenPlatformConfig) -> Self {
        let ttl_secs = cfg.github.state_ttl_secs.max(60);
        let cache = Cache::builder()
            .max_capacity(10_000)
            .time_to_live(Duration::from_secs(ttl_secs))
            .build();
        Self {
            oauth_state_cache: cache,
        }
    }

    pub(super) async fn issue_oauth_state(&self) -> String {
        let state = format!("ghs_{}", Uuid::new_v4().simple());
        self.oauth_state_cache.insert(state.clone(), true).await;
        state
    }

    pub(super) async fn consume_oauth_state(&self, state: &str) -> bool {
        let exists = self.oauth_state_cache.get(state).await.unwrap_or(false);
        if exists {
            self.oauth_state_cache.invalidate(state).await;
        }
        exists
    }
}
