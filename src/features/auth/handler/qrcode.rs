use axum::{
    Json,
    extract::{Path, Query, State},
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use base64::Engine;
use qrcode::{QrCode, render::svg};
use serde::{Deserialize, Serialize};
use std::time::Instant;
use uuid::Uuid;

use crate::error::AppError;
use crate::state::AppState;

use crate::features::auth::qrcode_service::QrCodeStatus;

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct QrCodeCreateResponse {
    /// 浜岀淮鐮佹爣璇嗭紝鐢ㄤ簬杞鐘舵€?    #[schema(example = "8b8f2f8a-1a2b-4c3d-9e0f-112233445566")]
    pub qr_id: String,
    /// 鐢ㄦ埛鍦ㄦ祻瑙堝櫒涓闂互纭鎺堟潈鐨?URL
    #[schema(example = "https://www.taptap.com/account/device?code=abcd-efgh")]
    pub verification_url: String,
    /// SVG 浜岀淮鐮佺殑 data URL锛坆ase64 缂栫爜锛?    #[schema(example = "data:image/svg+xml;base64,PHN2ZyB4bWxucz0uLi4=")]
    pub qrcode_base64: String,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct QrCodeStatusResponse {
    /// 褰撳墠鐘舵€侊細Pending/Scanned/Confirmed/Error/Expired
    #[schema(example = "Pending")]
    pub status: QrCodeStatusValue,
    /// 鑻?Confirmed锛岃繑鍥?LeanCloud Session Token
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_token: Option<String>,
    /// 鍙€夛細鏈哄櫒鍙鐨勯敊璇爜锛堜粎鍦?status=Error 鏃跺嚭鐜帮級
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
    /// 鍙€夌殑浜虹被鍙鎻愮ず娑堟伅
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// 鑻ラ渶寤跺悗杞锛岃繑鍥炲缓璁殑绛夊緟绉掓暟
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retry_after: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "PascalCase")]
pub enum QrCodeStatusValue {
    Pending,
    Scanned,
    Confirmed,
    Error,
    Expired,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct QrCodeQuery {
    /// TapTap 鐗堟湰锛歝n锛堝ぇ闄嗙増锛夋垨 global锛堝浗闄呯増锛?    #[serde(default)]
    taptap_version: Option<String>,
}

fn normalize_taptap_version(v: Option<&str>) -> Result<Option<&'static str>, AppError> {
    let Some(v) = v else {
        return Ok(None);
    };
    let v = v.trim();
    if v.is_empty() {
        return Ok(None);
    }
    if v.eq_ignore_ascii_case("cn") {
        return Ok(Some("cn"));
    }
    if v.eq_ignore_ascii_case("global") {
        return Ok(Some("global"));
    }
    Err(AppError::Validation(
        "taptapVersion 蹇呴』涓?cn 鎴?global".to_string(),
    ))
}

fn json_no_store<T: Serialize>(status: StatusCode, body: T) -> Response {
    let mut res = (status, Json(body)).into_response();
    res.headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    res
}

#[utoipa::path(
    post,
    path = "/auth/qrcode",
    summary = "生成登录二维码",
    description = "为设备申请 TapTap 设备码并返回可扫码的 SVG 二维码（base64）与校验 URL。",
    params(
        ("taptapVersion" = Option<String>, Query, description = "TapTap 版本：cn 或 global")
    ),
    responses(
        (status = 200, description = "生成二维码成功", body = QrCodeCreateResponse),
        (
            status = 401,
            description = "认证失败（TapTap 返回认证错误）",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        ),
        (
            status = 422,
            description = "参数校验失败（taptapVersion 非法等）",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        ),
        (
            status = 502,
            description = "上游网络错误",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        ),
        (
            status = 500,
            description = "服务器内部错误",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        )
    ),
    tag = "Auth"
)]
pub(crate) async fn post_qrcode(
    State(state): State<AppState>,
    Query(params): Query<QrCodeQuery>,
) -> Result<Response, AppError> {
    let t_total = Instant::now();

    let device_id = Uuid::new_v4().to_string();
    let qr_id = Uuid::new_v4().to_string();

    let t_version = Instant::now();
    let version = match normalize_taptap_version(params.taptap_version.as_deref()) {
        Ok(v) => {
            tracing::info!(
                target: "phi_backend::auth::performance",
                route = "/auth/qrcode",
                phase = "normalize_version",
                status = "ok",
                dur_ms = t_version.elapsed().as_millis(),
                "auth performance"
            );
            v
        }
        Err(e) => {
            tracing::info!(
                target: "phi_backend::auth::performance",
                route = "/auth/qrcode",
                phase = "normalize_version",
                status = "failed",
                dur_ms = t_version.elapsed().as_millis(),
                "auth performance"
            );
            return Err(e);
        }
    };

    let t_request = Instant::now();
    let device = match state
        .taptap_client
        .request_device_code(&device_id, version)
        .await
    {
        Ok(device) => {
            tracing::info!(
                target: "phi_backend::auth::performance",
                route = "/auth/qrcode",
                phase = "request_device_code",
                status = "ok",
                dur_ms = t_request.elapsed().as_millis(),
                "auth performance"
            );
            device
        }
        Err(e) => {
            tracing::info!(
                target: "phi_backend::auth::performance",
                route = "/auth/qrcode",
                phase = "request_device_code",
                status = "failed",
                dur_ms = t_request.elapsed().as_millis(),
                err = %e,
                "auth performance"
            );
            return Err(e);
        }
    };

    let device_code = device
        .device_code
        .ok_or_else(|| AppError::Internal("TapTap 未返回 device_code".to_string()))?;
    let verification_url = device
        .verification_url
        .ok_or_else(|| AppError::Internal("TapTap 未返回 verification_url".to_string()))?;

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

    let t_qrcode = Instant::now();
    let code = match QrCode::new(&verification_url_for_scan) {
        Ok(code) => code,
        Err(e) => {
            tracing::info!(
                target: "phi_backend::auth::performance",
                route = "/auth/qrcode",
                phase = "generate_qrcode_svg",
                status = "failed",
                dur_ms = t_qrcode.elapsed().as_millis(),
                "auth performance"
            );
            return Err(AppError::Internal(format!("生成二维码失败: {e}")));
        }
    };
    let image = code
        .render()
        .min_dimensions(256, 256)
        .dark_color(svg::Color("#000"))
        .light_color(svg::Color("#fff"))
        .build();
    tracing::info!(
        target: "phi_backend::auth::performance",
        route = "/auth/qrcode",
        phase = "generate_qrcode_svg",
        status = "ok",
        dur_ms = t_qrcode.elapsed().as_millis(),
        "auth performance"
    );
    let qrcode_base64 = format!(
        "data:image/svg+xml;base64,{}",
        base64::prelude::BASE64_STANDARD.encode(image)
    );

    let interval_secs = device.interval.unwrap_or(5);
    let t_cache_set = Instant::now();
    state
        .qrcode_service
        .set_pending(
            qr_id.clone(),
            device_code,
            device_id,
            interval_secs,
            device.expires_in,
            version.map(std::string::ToString::to_string),
        )
        .await;
    tracing::info!(
        target: "phi_backend::auth::performance",
        route = "/auth/qrcode",
        phase = "cache_set_pending",
        status = "ok",
        dur_ms = t_cache_set.elapsed().as_millis(),
        "auth performance"
    );

    let resp = QrCodeCreateResponse {
        qr_id,
        verification_url: verification_url_for_scan,
        qrcode_base64,
    };
    tracing::info!(
        target: "phi_backend::auth::performance",
        route = "/auth/qrcode",
        phase = "total",
        status = "ok",
        dur_ms = t_total.elapsed().as_millis(),
        "auth performance"
    );
    Ok(json_no_store(StatusCode::OK, resp))
}

#[utoipa::path(
    get,
    path = "/auth/qrcode/{qr_id}/status",
    summary = "轮询二维码授权状态",
    description = "根据 qr_id 查询当前授权进度。若返回 Pending 且包含 retry_after，客户端应按该秒数后再轮询。",
    params(("qr_id" = String, Path, description = "二维码ID")),
    responses(
        (status = 200, description = "状态返回", body = QrCodeStatusResponse)
    ),
    tag = "Auth"
)]
pub async fn get_qrcode_status(
    State(state): State<AppState>,
    Path(qr_id): Path<String>,
) -> Result<Response, AppError> {
    let t_total = Instant::now();
    let log_total = |result_status: &'static str| {
        tracing::info!(
            target: "phi_backend::auth::performance",
            route = "/auth/qrcode/:qr_id/status",
            phase = "total",
            status = "ok",
            result_status,
            dur_ms = t_total.elapsed().as_millis(),
            "auth performance"
        );
    };

    let t_cache_get = Instant::now();
    let current = if let Some(c) = state.qrcode_service.get(&qr_id).await {
        tracing::info!(
            target: "phi_backend::auth::performance",
            route = "/auth/qrcode/:qr_id/status",
            phase = "cache_get",
            status = "hit",
            dur_ms = t_cache_get.elapsed().as_millis(),
            "auth performance"
        );
        c
    } else {
        tracing::info!(
            target: "phi_backend::auth::performance",
            route = "/auth/qrcode/:qr_id/status",
            phase = "cache_get",
            status = "miss",
            dur_ms = t_cache_get.elapsed().as_millis(),
            "auth performance"
        );
        log_total("expired_not_found");
        return Ok(json_no_store(
            StatusCode::OK,
            QrCodeStatusResponse {
                status: QrCodeStatusValue::Expired,
                session_token: None,
                error_code: None,
                message: Some("二维码不存在或已过期".to_string()),
                retry_after: None,
            },
        ));
    };

    match current {
        QrCodeStatus::Confirmed { session_data } => {
            let t_cache_remove = Instant::now();
            state.qrcode_service.remove(&qr_id).await;
            tracing::info!(
                target: "phi_backend::auth::performance",
                route = "/auth/qrcode/:qr_id/status",
                phase = "cache_remove",
                status = "ok",
                dur_ms = t_cache_remove.elapsed().as_millis(),
                "auth performance"
            );
            log_total("confirmed");
            Ok(json_no_store(
                StatusCode::OK,
                QrCodeStatusResponse {
                    status: QrCodeStatusValue::Confirmed,
                    session_token: Some(session_data.session_token),
                    error_code: None,
                    message: None,
                    retry_after: None,
                },
            ))
        }
        QrCodeStatus::Pending {
            device_code,
            device_id,
            interval_secs,
            next_poll_at,
            expires_at,
            version,
        } => {
            let now = std::time::Instant::now();

            if now >= expires_at {
                let t_cache_remove = Instant::now();
                state.qrcode_service.remove(&qr_id).await;
                tracing::info!(
                    target: "phi_backend::auth::performance",
                    route = "/auth/qrcode/:qr_id/status",
                    phase = "cache_remove",
                    status = "expired",
                    dur_ms = t_cache_remove.elapsed().as_millis(),
                    "auth performance"
                );
                log_total("expired");
                return Ok(json_no_store(
                    StatusCode::OK,
                    QrCodeStatusResponse {
                        status: QrCodeStatusValue::Expired,
                        session_token: None,
                        error_code: None,
                        message: Some("二维码已过期".to_string()),
                        retry_after: None,
                    },
                ));
            }

            if now < next_poll_at {
                let retry_secs = (next_poll_at - now).as_secs();
                tracing::info!(
                    target: "phi_backend::auth::performance",
                    route = "/auth/qrcode/:qr_id/status",
                    phase = "poll_gate",
                    status = "deferred",
                    retry_after = retry_secs,
                    dur_ms = 0_u64,
                    "auth performance"
                );
                log_total("pending_wait");
                return Ok(json_no_store(
                    StatusCode::OK,
                    QrCodeStatusResponse {
                        status: QrCodeStatusValue::Pending,
                        session_token: None,
                        error_code: None,
                        message: None,
                        retry_after: Some(retry_secs),
                    },
                ));
            }

            let t_poll = Instant::now();
            match state
                .taptap_client
                .poll_for_token(&device_code, &device_id, version.as_deref())
                .await
            {
                Ok(session) => {
                    tracing::info!(
                        target: "phi_backend::auth::performance",
                        route = "/auth/qrcode/:qr_id/status",
                        phase = "poll_for_token",
                        status = "ok",
                        dur_ms = t_poll.elapsed().as_millis(),
                        "auth performance"
                    );
                    let t_cache_update = Instant::now();
                    state
                        .qrcode_service
                        .set_confirmed(&qr_id, session.clone())
                        .await;
                    state.qrcode_service.remove(&qr_id).await;
                    tracing::info!(
                        target: "phi_backend::auth::performance",
                        route = "/auth/qrcode/:qr_id/status",
                        phase = "cache_update",
                        status = "confirmed",
                        dur_ms = t_cache_update.elapsed().as_millis(),
                        "auth performance"
                    );
                    log_total("confirmed");
                    Ok(json_no_store(
                        StatusCode::OK,
                        QrCodeStatusResponse {
                            status: QrCodeStatusValue::Confirmed,
                            session_token: Some(session.session_token),
                            error_code: None,
                            message: None,
                            retry_after: None,
                        },
                    ))
                }
                Err(AppError::AuthPending(_)) => {
                    tracing::info!(
                        target: "phi_backend::auth::performance",
                        route = "/auth/qrcode/:qr_id/status",
                        phase = "poll_for_token",
                        status = "pending",
                        dur_ms = t_poll.elapsed().as_millis(),
                        "auth performance"
                    );
                    let t_cache_update = Instant::now();
                    state
                        .qrcode_service
                        .set_pending_next_poll(
                            &qr_id,
                            device_code,
                            device_id,
                            interval_secs,
                            expires_at,
                            version,
                        )
                        .await;
                    tracing::info!(
                        target: "phi_backend::auth::performance",
                        route = "/auth/qrcode/:qr_id/status",
                        phase = "cache_update",
                        status = "pending",
                        dur_ms = t_cache_update.elapsed().as_millis(),
                        "auth performance"
                    );
                    log_total("pending");
                    Ok(json_no_store(
                        StatusCode::OK,
                        QrCodeStatusResponse {
                            status: QrCodeStatusValue::Pending,
                            session_token: None,
                            error_code: None,
                            message: None,
                            retry_after: Some(interval_secs),
                        },
                    ))
                }
                Err(e) => {
                    tracing::warn!(err = %e, "qrcode poll failed");
                    tracing::info!(
                        target: "phi_backend::auth::performance",
                        route = "/auth/qrcode/:qr_id/status",
                        phase = "poll_for_token",
                        status = "failed",
                        dur_ms = t_poll.elapsed().as_millis(),
                        err = %e,
                        "auth performance"
                    );
                    let (error_code, message) = match &e {
                        AppError::Auth(_) => ("UNAUTHORIZED", "认证失败"),
                        AppError::Forbidden(_) => ("FORBIDDEN", "访问被禁止"),
                        AppError::Network(_) => ("UPSTREAM_ERROR", "上游网络错误"),
                        AppError::Timeout(_) => ("UPSTREAM_TIMEOUT", "上游超时"),
                        AppError::Json(_) => ("UPSTREAM_ERROR", "上游响应解析失败"),
                        AppError::Validation(_) => ("VALIDATION_FAILED", "请求参数错误"),
                        AppError::Conflict(_) => ("CONFLICT", "资源冲突"),
                        AppError::Internal(_)
                        | AppError::SaveProvider(_)
                        | AppError::Search(_)
                        | AppError::SaveHandlerError(_)
                        | AppError::ImageRendererError(_)
                        | AppError::AuthPending(_) => ("INTERNAL_ERROR", "服务器内部错误"),
                    };
                    log_total("error");
                    Ok(json_no_store(
                        StatusCode::OK,
                        QrCodeStatusResponse {
                            status: QrCodeStatusValue::Error,
                            session_token: None,
                            error_code: Some(error_code.to_string()),
                            message: Some(message.to_string()),
                            retry_after: None,
                        },
                    ))
                }
            }
        }
        QrCodeStatus::Scanned => {
            log_total("scanned");
            Ok(json_no_store(
                StatusCode::OK,
                QrCodeStatusResponse {
                    status: QrCodeStatusValue::Scanned,
                    session_token: None,
                    error_code: None,
                    message: None,
                    retry_after: None,
                },
            ))
        }
    }
}
