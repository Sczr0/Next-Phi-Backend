use axum::{
    extract::State,
    response::Response,
};

use crate::extract::{ValidatedPath, ValidatedQuery};
use crate::{error::AppError, state::AppState};

pub(crate) use crate::features::auth::handler::QrCodeQuery;
pub use crate::features::auth::handler::{QrCodeCreateResponse, QrCodeStatusResponse};

pub(crate) async fn post_qrcode(
    state: State<AppState>,
    query: ValidatedQuery<QrCodeQuery>,
) -> Result<Response, AppError> {
    crate::features::auth::handler::post_qrcode(state, query).await
}

pub(crate) async fn get_qrcode_status(
    state: State<AppState>,
    path: ValidatedPath<String>,
) -> Result<Response, AppError> {
    crate::features::auth::handler::get_qrcode_status(state, path).await
}
