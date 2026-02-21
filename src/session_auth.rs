use std::sync::Arc;

use axum::extract::Request;
use serde::de::DeserializeOwned;

use crate::auth_contract::UnifiedSaveRequest;
use crate::error::AppError;

pub use crate::features::auth::bearer::BearerAuthState;

pub async fn parse_json_with_bearer_state<T>(req: Request) -> Result<(T, BearerAuthState), AppError>
where
    T: DeserializeOwned + Send + 'static,
{
    crate::features::auth::bearer::parse_json_with_bearer_state(req).await
}

pub async fn merge_auth_from_bearer_if_missing(
    stats_storage: Option<&Arc<crate::stats_contract::StatsStorage>>,
    bearer: &BearerAuthState,
    auth: &mut UnifiedSaveRequest,
) -> Result<(), AppError> {
    crate::features::auth::bearer::merge_auth_from_bearer_if_missing(stats_storage, bearer, auth)
        .await
}

pub fn derive_user_identity_with_bearer(
    salt_opt: Option<&str>,
    auth: &UnifiedSaveRequest,
    bearer: &BearerAuthState,
) -> Result<(Option<String>, Option<String>), AppError> {
    crate::features::auth::bearer::derive_user_identity_with_bearer(salt_opt, auth, bearer)
}
