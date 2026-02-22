use axum::{
    extract::{Path, Query, State},
    response::Response,
};

use crate::{error::AppError, state::AppState};

pub(crate) use crate::features::auth::handler::QrCodeQuery;
pub use crate::features::auth::handler::{QrCodeCreateResponse, QrCodeStatusResponse};

pub(crate) async fn post_qrcode(
    state: State<AppState>,
    query: Query<QrCodeQuery>,
) -> Result<Response, AppError> {
    crate::features::auth::handler::post_qrcode(state, query).await
}

pub(crate) async fn get_qrcode_status(
    state: State<AppState>,
    path: Path<String>,
) -> Result<Response, AppError> {
    crate::features::auth::handler::get_qrcode_status(state, path).await
}
