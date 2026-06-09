use axum::{
    Router,
    routing::{get, post},
};

use crate::state::AppState;

pub(crate) mod handlers;
mod helpers;
pub(crate) mod models;
#[cfg(test)]
mod tests;

pub use self::handlers::{
    get_api_key_events, get_api_key_rate_limit, get_api_keys, post_create_api_key,
    post_delete_api_key, post_revoke_api_key, post_rotate_api_key,
};
pub use self::models::{
    ApiKeyEventItem, ApiKeyEventsResponse, ApiKeyIssueResponse, ApiKeyListItem, ApiKeyListQuery,
    ApiKeyListResponse, ApiKeyRateLimitBucketItem, ApiKeyRateLimitQuery, ApiKeyRateLimitResponse,
    CreateApiKeyRequest, DeleteApiKeyRequest, EventsQuery, OkResponse, RevokeApiKeyRequest,
    RotateApiKeyRequest,
};

pub fn create_open_platform_keys_router() -> Router<AppState> {
    Router::<AppState>::new()
        .route("/developer/api-keys", post(post_create_api_key))
        .route("/developer/api-keys", get(get_api_keys))
        .route(
            "/developer/api-keys/:key_id/rotate",
            post(post_rotate_api_key),
        )
        .route(
            "/developer/api-keys/:key_id/revoke",
            post(post_revoke_api_key),
        )
        .route(
            "/developer/api-keys/:key_id/delete",
            post(post_delete_api_key),
        )
        .route(
            "/developer/api-keys/:key_id/events",
            get(get_api_key_events),
        )
        .route(
            "/developer/api-keys/:key_id/rate-limit",
            get(get_api_key_rate_limit),
        )
}
