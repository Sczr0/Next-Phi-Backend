use axum::{
    Router,
    routing::{get, post},
};

use crate::state::AppState;

pub(crate) mod qrcode;
pub(crate) mod session;
pub(crate) mod user_id;

pub use self::qrcode::{
    QrCodeCreateResponse, QrCodeStatusResponse, QrCodeStatusValue, get_qrcode_status,
};
pub(crate) use self::qrcode::{QrCodeQuery, post_qrcode};
pub use self::session::{
    SessionExchangeRequest, SessionExchangeResponse, SessionLogoutRequest, SessionLogoutResponse,
    SessionLogoutScope, post_session_exchange, post_session_logout, post_session_refresh,
};
pub use self::user_id::{UserIdResponse, post_user_id};

pub fn create_auth_router() -> Router<AppState> {
    Router::<AppState>::new()
        .route("/qrcode", post(post_qrcode))
        .route("/qrcode/:qr_id/status", get(get_qrcode_status))
        .route("/user-id", post(post_user_id))
        .route("/session/exchange", post(post_session_exchange))
        .route("/session/refresh", post(post_session_refresh))
        .route("/session/logout", post(post_session_logout))
}
