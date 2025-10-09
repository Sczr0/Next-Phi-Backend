use axum::{
    Router,
    extract::{Path, State},
    http::StatusCode,
    response::Json,
    routing::get,
};
use base64::Engine;
use qrcode::{QrCode, render::svg};
use serde::Serialize;
use uuid::Uuid;

use crate::error::AppError;
use crate::state::AppState;

use super::qrcode_service::QrCodeStatus;

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct QrCodeCreateResponse {
    pub qr_id: String,
    pub verification_url: String,
    pub qrcode_base64: String,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct QrCodeStatusResponse {
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retry_after: Option<u64>,
}

#[utoipa::path(
    get,
    path = "/auth/qrcode",
    responses(
        (status = 200, description = "生成二维码成功", body = QrCodeCreateResponse),
        (status = 500, description = "服务器内部错误", body = AppError)
    ),
    tag = "Auth"
)]
pub async fn get_qrcode(
    State(state): State<AppState>,
) -> Result<(StatusCode, Json<QrCodeCreateResponse>), AppError> {
    // 生成 device_id 与 qr_id
    let device_id = Uuid::new_v4().to_string();
    let qr_id = Uuid::new_v4().to_string();

    // 请求 TapTap 设备码
    let device = state.taptap_client.request_device_code(&device_id).await?;

    let device_code = device
        .device_code
        .ok_or_else(|| AppError::Internal("TapTap 未返回 device_code".to_string()))?;
    let verification_url = device
        .verification_url
        .ok_or_else(|| AppError::Internal("TapTap 未返回 verification_url".to_string()))?;

    // 生成二维码（SVG）并 Base64 编码
    let code = QrCode::new(&verification_url)
        .map_err(|e| AppError::Internal(format!("生成二维码失败: {e}")))?;
    let image = code
        .render()
        .min_dimensions(256, 256)
        .dark_color(svg::Color("#000"))
        .light_color(svg::Color("#fff"))
        .build();
    let qrcode_base64 = format!(
        "data:image/svg+xml;base64,{}",
        base64::prelude::BASE64_STANDARD.encode(image)
    );

    // 写入缓存为 Pending 状态
    let interval_secs = device.interval.unwrap_or(5);
    state
        .qrcode_service
        .set_pending(qr_id.clone(), device_code, device_id, interval_secs)
        .await;

    let resp = QrCodeCreateResponse {
        qr_id,
        verification_url,
        qrcode_base64,
    };
    Ok((StatusCode::OK, Json(resp)))
}

#[utoipa::path(
    get,
    path = "/auth/qrcode/{qr_id}/status",
    params(("qr_id" = String, Path, description = "二维码ID")),
    responses(
        (status = 200, description = "状态返回", body = QrCodeStatusResponse),
        (status = 404, description = "二维码不存在或已过期"),
        (status = 500, description = "服务器内部错误", body = AppError)
    ),
    tag = "Auth"
)]
pub async fn get_qrcode_status(
    State(state): State<AppState>,
    Path(qr_id): Path<String>,
) -> Result<(StatusCode, Json<QrCodeStatusResponse>), AppError> {
    let current = match state.qrcode_service.get(&qr_id).await {
        Some(c) => c,
        None => {
            return Ok((
                StatusCode::NOT_FOUND,
                Json(QrCodeStatusResponse {
                    status: "Expired".to_string(),
                    session_token: None,
                    message: Some("二维码不存在或已过期".to_string()),
                    retry_after: None,
                }),
            ));
        }
    };

    match current {
        QrCodeStatus::Confirmed { session_data } => {
            // 命中确认，删除缓存并返回 token
            state.qrcode_service.remove(&qr_id).await;
            Ok((
                StatusCode::OK,
                Json(QrCodeStatusResponse {
                    status: "Confirmed".to_string(),
                    session_token: Some(session_data.session_token),
                    message: None,
                    retry_after: None,
                }),
            ))
        }
        QrCodeStatus::Pending {
            device_code,
            device_id,
            interval_secs,
            next_poll_at,
        } => {
            // 频率限制：遵循 TapTap 建议的 interval
            let now = std::time::Instant::now();
            if now < next_poll_at {
                let retry_secs = (next_poll_at - now).as_secs();
                return Ok((
                    StatusCode::OK,
                    Json(QrCodeStatusResponse {
                        status: "Pending".to_string(),
                        session_token: None,
                        message: None,
                        retry_after: Some(retry_secs),
                    }),
                ));
            }
            // 轮询 TapTap，判断授权状态
            match state
                .taptap_client
                .poll_for_token(&device_code, &device_id)
                .await
            {
                Ok(session) => {
                    // 更新为 Confirmed 并返回
                    state
                        .qrcode_service
                        .set_confirmed(&qr_id, session.clone())
                        .await;
                    state.qrcode_service.remove(&qr_id).await;
                    Ok((
                        StatusCode::OK,
                        Json(QrCodeStatusResponse {
                            status: "Confirmed".to_string(),
                            session_token: Some(session.session_token),
                            message: None,
                            retry_after: None,
                        }),
                    ))
                }
                Err(AppError::AuthPending(_)) => {
                    // 按 interval 延后下一次轮询
                    state
                        .qrcode_service
                        .set_pending_next_poll(&qr_id, device_code, device_id, interval_secs)
                        .await;
                    Ok((
                        StatusCode::OK,
                        Json(QrCodeStatusResponse {
                            status: "Pending".to_string(),
                            session_token: None,
                            message: None,
                            retry_after: Some(interval_secs),
                        }),
                    ))
                }
                Err(e) => Ok((
                    StatusCode::OK,
                    Json(QrCodeStatusResponse {
                        status: "Error".to_string(),
                        session_token: None,
                        message: Some(e.to_string()),
                        retry_after: None,
                    }),
                )),
            }
        }
        QrCodeStatus::Scanned => Ok((
            StatusCode::OK,
            Json(QrCodeStatusResponse {
                status: "Scanned".to_string(),
                session_token: None,
                message: None,
                retry_after: None,
            }),
        )),
    }
}

pub fn create_auth_router() -> Router<AppState> {
    Router::<AppState>::new()
        .route("/qrcode", get(get_qrcode))
        .route("/qrcode/{qr_id}/status", get(get_qrcode_status))
}
