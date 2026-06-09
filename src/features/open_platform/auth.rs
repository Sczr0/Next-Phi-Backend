use axum::{
    Router,
    routing::{get, post},
};

use crate::state::AppState;

mod github;
pub(crate) mod handlers;
pub(crate) mod models;
mod service;
mod session;
#[cfg(test)]
mod tests;

pub use self::handlers::{get_github_callback, get_github_login, get_me, post_logout};
pub use self::service::{OpenPlatformAuthService, init_global};
pub use self::session::require_developer;

pub fn create_open_platform_auth_router() -> Router<AppState> {
    Router::<AppState>::new()
        .route("/auth/github/login", get(get_github_login))
        .route("/auth/github/callback", get(get_github_callback))
        .route("/auth/me", get(get_me))
        .route("/auth/logout", post(post_logout))
}
