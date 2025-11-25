use axum::{
    Router,
    extract::{Path, Query, State},
    http::StatusCode,
    response::Json,
    routing::get,
};
use std::collections::HashMap;
use base64::Engine;
use qrcode::{QrCode, render::svg};
use serde::Serialize;
use uuid::Uuid;

use crate::error::AppError;
use crate::state::AppState;

use super::qrcode_service::QrCodeStatus;

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct QrCodeCreateResponse {
    /// 二维码标识，用于轮询状态
    #[schema(example = "8b8f2f8a-1a2b-4c3d-9e0f-112233445566")]
    pub qr_id: String,
    /// 用户在浏览器中访问以确认授权的 URL
    #[schema(example = "https://www.taptap.com/account/device?code=abcd-efgh")]
    pub verification_url: String,
    /// SVG 二维码的 data URL（base64 编码）
    #[schema(example = "data:image/svg+xml;base64,PHN2ZyB4bWxucz0uLi4=")]
    pub qrcode_base64: String,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct QrCodeStatusResponse {
    /// 当前状态：Pending/Scanned/Confirmed/Error/Expired
    #[schema(example = "Pending")]
    pub status: String,
    /// 若 Confirmed，返回 LeanCloud Session Token
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_token: Option<String>,
    /// 可选的人类可读提示消息
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// 若需延后轮询，返回建议的等待秒数
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retry_after: Option<u64>,
}

#[utoipa::path(
    get,
    path = "/auth/qrcode",
    summary = "生成登录二维码",
    description = "为设备申请 TapTap 设备码并返回可扫码的 SVG 二维码（base64）与校验 URL。客户端需保存返回的 qr_id 以轮询授权状态。",
    params(
        ("version" = Option<String>, Query, description = "TapTap 版本：cn（大陆版，默认）或 global（国际版）")
    ),
    responses(
        (status = 200, description = "生成二维码成功", body = QrCodeCreateResponse),
        (status = 500, description = "服务器内部错误", body = AppError)
    ),
    tag = "Auth"
)]
pub async fn get_qrcode(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<(StatusCode, Json<QrCodeCreateResponse>), AppError> {
    // 生成 device_id 与 qr_id
    let device_id = Uuid::new_v4().to_string();
    let qr_id = Uuid::new_v4().to_string();
    
    // 获取版本参数
    let version = params.get("version").map(|v| v.as_str());

    // 请求 TapTap 设备码
    let device = state.taptap_client.request_device_code(&device_id, version).await?;

    let device_code = device
        .device_code
        .ok_or_else(|| AppError::Internal("TapTap 未返回 device_code".to_string()))?;
    let verification_url = device
        .verification_url
        .ok_or_else(|| AppError::Internal("TapTap 未返回 verification_url".to_string()))?;

    // 组合用于扫码/跳转的最终链接：优先使用服务端提供的 qrcode_url；
    // 否则在 verification_url 基础上拼接 user_code 参数。
    let verification_url_for_scan = if let Some(qr) = device.qrcode_url.clone() {
        qr
    } else if let Some(code) = device.user_code.clone() {
        if verification_url.contains('?') {
            format!("{verification_url}&qrcode=1&user_code={code}")
        } else {
            format!("{verification_url}?qrcode=1&user_code={code}")
        }
    } else {
        verification_url.clone()
    };

    // 生成二维码（SVG）并 Base64 编码
    let code = QrCode::new(&verification_url_for_scan)
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
        .set_pending(qr_id.clone(), device_code, device_id, interval_secs, version.map(|v| v.to_string()))
        .await;

    let resp = QrCodeCreateResponse {
        qr_id,
        verification_url: verification_url_for_scan,
        qrcode_base64,
    };
    Ok((StatusCode::OK, Json(resp)))
}

#[utoipa::path(
    get,
    path = "/auth/qrcode/{qr_id}/status",
    summary = "轮询二维码授权状态",
    description = "根据 qr_id 查询当前授权进度。若返回 Pending 且包含 retry_after，客户端应按该秒数后再发起轮询。",
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
            version,
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
            // 使用生成二维码时保存的版本信息
            match state
                .taptap_client
                .poll_for_token(&device_code, &device_id, version.as_deref())
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
                        .set_pending_next_poll(&qr_id, device_code, device_id, interval_secs, version)
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
        .route("/qrcode/:qr_id/status", get(get_qrcode_status))
}
