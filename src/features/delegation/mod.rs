//! 单 client 的简化 OAuth2 委托授权。

pub mod crypto;
pub mod handlers;
pub mod models;

use std::sync::{Arc, OnceLock};

use axum::Router;
use axum::routing::{get, post};
use impl_storage::delegation::DelegationStorage;

use crate::error::AppError;
use crate::state::AppState;

static STORAGE: OnceLock<Arc<DelegationStorage>> = OnceLock::new();
static CONFIG: OnceLock<crate::config::DelegationConfig> = OnceLock::new();

#[allow(clippy::expect_used)]
pub fn config() -> &'static crate::config::DelegationConfig {
    CONFIG
        .get()
        .expect("delegation config 未初始化，请先调用 init")
}

#[allow(clippy::expect_used)]
pub fn storage() -> &'static Arc<DelegationStorage> {
    STORAGE
        .get()
        .expect("delegation storage 未初始化，请先调用 init")
}

pub fn validate(cfg: &crate::config::DelegationConfig) -> Result<(), AppError> {
    if cfg.client_id.trim().is_empty() {
        return Err(AppError::Internal("delegation.client_id 不能为空".into()));
    }
    if cfg.client_secret.trim().is_empty() {
        return Err(AppError::Internal("delegation.client_secret 未配置".into()));
    }
    if cfg.redirect_uri.trim().is_empty() {
        return Err(AppError::Internal("delegation.redirect_uri 未配置".into()));
    }
    if cfg.consent_url.trim().is_empty() {
        return Err(AppError::Internal("delegation.consent_url 未配置".into()));
    }
    if cfg.scopes.is_empty() {
        return Err(AppError::Internal("delegation.scopes 不能为空".into()));
    }
    let key = hex::decode(cfg.credential_key.trim())
        .map_err(|_| AppError::Internal("delegation.credential_key 必须是 hex".into()))?;
    if key.len() != 32 {
        return Err(AppError::Internal(
            "delegation.credential_key 必须为 32 字节".into(),
        ));
    }
    if cfg.credential_key_version == 0 {
        return Err(AppError::Internal(
            "delegation.credential_key_version 必须大于 0".into(),
        ));
    }
    Ok(())
}

pub async fn init(cfg: &crate::config::DelegationConfig) -> Result<(), AppError> {
    validate(cfg)?;
    let storage = DelegationStorage::connect_sqlite(&cfg.sqlite_path, cfg.sqlite_wal).await?;
    storage.init_schema().await?;
    let _ = CONFIG.set(cfg.clone());
    let _ = STORAGE.set(Arc::new(storage));
    Ok(())
}

pub fn router() -> Router<AppState> {
    Router::<AppState>::new()
        .route("/delegations/authorize", get(handlers::authorize))
        .route("/delegations/requests/{id}", get(handlers::request))
        .route(
            "/delegations/requests/{id}/approve",
            post(handlers::approve),
        )
        .route("/delegations/requests/{id}/deny", post(handlers::deny))
        .route("/delegations/token", post(handlers::token))
        .route("/delegations/revoke", post(handlers::revoke))
        .route("/delegations/me", get(handlers::me))
}
