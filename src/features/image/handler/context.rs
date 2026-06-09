use crate::{config::AppConfig, error::AppError, state::AppState};

pub(super) async fn ensure_image_user_not_banned(
    state: &AppState,
    user_hash: Option<&str>,
) -> Result<(), AppError> {
    if let (Some(storage), Some(user_hash_ref)) = (state.stats_storage.as_ref(), user_hash) {
        storage.ensure_user_not_banned(user_hash_ref).await?;
    }
    Ok(())
}

pub(super) fn image_footer_text() -> Option<String> {
    Some(AppConfig::global().branding.footer_text.clone())
}

pub(super) fn image_cache_enabled() -> bool {
    AppConfig::global().image.cache_enabled
}

pub(super) fn derive_image_user_identity(
    auth: &crate::auth_contract::UnifiedSaveRequest,
    bearer_state: &crate::session_auth::BearerAuthState,
) -> Result<(Option<String>, Option<String>), AppError> {
    let salt = AppConfig::global().stats.user_hash_salt.as_deref();
    crate::session_auth::derive_user_identity_with_bearer(salt, auth, bearer_state)
}
