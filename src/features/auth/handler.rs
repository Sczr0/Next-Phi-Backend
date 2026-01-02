use axum::{
    Router,
    extract::{Path, Query, State},
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Json, Response},
    routing::{get, post},
};
use base64::Engine;
use qrcode::{QrCode, render::svg};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::AppError;
use crate::state::AppState;

use super::qrcode_service::QrCodeStatus;

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UserIdResponse {
    /// 去敏后的稳定用户 ID（32 位 hex，等价于 stats/leaderboard 使用的 user_hash）
    #[schema(example = "ab12cd34ef56ab12cd34ef56ab12cd34")]
    pub user_id: String,
    /// 用于推导 user_id 的凭证类型（用于排查“为什么和以前不一致”）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_kind: Option<String>,
}

#[utoipa::path(
    post,
    path = "/auth/user-id",
    summary = "根据凭证生成去敏用户ID",
    description = "使用服务端配置的 stats.user_hash_salt 对凭证做 HMAC-SHA256 去敏（取前 16 字节，32 位 hex），用于同一用户的稳定标识。注意：salt 变更会导致 user_id 整体变化。",
    request_body = crate::features::save::models::UnifiedSaveRequest,
    responses(
        (status = 200, description = "生成成功", body = UserIdResponse),
        (
            status = 422,
            description = "凭证缺失/无效，或无法识别用户",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        ),
        (
            status = 500,
            description = "服务端未配置 user_hash_salt",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        )
    ),
    tag = "Auth"
)]
pub async fn post_user_id(
    Json(auth): Json<crate::features::save::models::UnifiedSaveRequest>,
) -> Result<(StatusCode, Json<UserIdResponse>), AppError> {
    // 与 /save 的凭证互斥语义保持一致，避免“同一个请求不同接口得到不同身份”的困惑。
    if auth.session_token.is_some() && auth.external_credentials.is_some() {
        return Err(AppError::Validation(
            "不能同时提供 sessionToken 和 externalCredentials，请只选择其中一种认证方式".into(),
        ));
    }

    let stable_ok = if let Some(tok) = auth.session_token.as_deref() {
        !tok.is_empty()
    } else if let Some(ext) = auth.external_credentials.as_ref() {
        let has_api_user_id = ext.api_user_id.as_deref().is_some_and(|v| !v.is_empty());
        let has_sessiontoken = ext.sessiontoken.as_deref().is_some_and(|v| !v.is_empty());
        let has_platform_pair = match (&ext.platform, &ext.platform_id) {
            (Some(p), Some(pid)) => !p.is_empty() && !pid.is_empty(),
            _ => false,
        };
        has_api_user_id || has_sessiontoken || has_platform_pair
    } else {
        false
    };
    if !stable_ok {
        return Err(AppError::Validation(
            "无法识别用户：请提供 sessionToken，或 externalCredentials 中的 platform+platformId / sessiontoken / apiUserId（且不能为空）".into(),
        ));
    }

    let salt = crate::config::AppConfig::global()
        .stats
        .user_hash_salt
        .as_deref()
        .ok_or_else(|| {
            AppError::Internal(
                "stats.user_hash_salt 未配置，无法生成稳定 user_id（可通过 APP_STATS_USER_HASH_SALT 设置）"
                    .into(),
            )
        })?;

    let (user_id_opt, user_kind) =
        crate::features::stats::derive_user_identity_from_auth(Some(salt), &auth);
    let user_id = user_id_opt.ok_or_else(|| AppError::Internal("生成 user_id 失败".into()))?;
    Ok((StatusCode::OK, Json(UserIdResponse { user_id, user_kind })))
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
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
    pub status: QrCodeStatusValue,
    /// 若 Confirmed，返回 LeanCloud Session Token
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_token: Option<String>,
    /// 可选：机器可读的错误码（仅在 status=Error 时出现）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
    /// 可选的人类可读提示消息
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// 若需延后轮询，返回建议的等待秒数
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
    /// TapTap 版本：cn（大陆版）或 global（国际版）
    #[serde(default)]
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
    match v.to_ascii_lowercase().as_str() {
        "cn" => Ok(Some("cn")),
        "global" => Ok(Some("global")),
        _ => Err(AppError::Validation(
            "taptapVersion 必须为 cn 或 global".to_string(),
        )),
    }
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
    description = "为设备申请 TapTap 设备码并返回可扫码的 SVG 二维码（base64）与校验 URL。客户端需保存返回的 qrId 以轮询授权状态。",
    params(
        ("taptapVersion" = Option<String>, Query, description = "TapTap 版本：cn（大陆版）或 global（国际版）")
    ),
    responses(
        (status = 200, description = "生成二维码成功", body = QrCodeCreateResponse),
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
    // 生成 device_id 与 qr_id
    let device_id = Uuid::new_v4().to_string();
    let qr_id = Uuid::new_v4().to_string();

    // 获取版本参数
    let version = normalize_taptap_version(params.taptap_version.as_deref())?;

    // 请求 TapTap 设备码
    let device = state
        .taptap_client
        .request_device_code(&device_id, version)
        .await?;

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
        .set_pending(
            qr_id.clone(),
            device_code,
            device_id,
            interval_secs,
            device.expires_in,
            version.map(|v| v.to_string()),
        )
        .await;

    let resp = QrCodeCreateResponse {
        qr_id,
        verification_url: verification_url_for_scan,
        qrcode_base64,
    };
    Ok(json_no_store(StatusCode::OK, resp))
}

#[utoipa::path(
    get,
    path = "/auth/qrcode/{qr_id}/status",
    summary = "轮询二维码授权状态",
    description = "根据 qr_id 查询当前授权进度。若返回 Pending 且包含 retry_after，客户端应按该秒数后再发起轮询。",
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
    let current = match state.qrcode_service.get(&qr_id).await {
        Some(c) => c,
        None => {
            // v2 契约：二维码状态接口始终返回 200 + 状态对象，避免出现“404 但仍返回 JSON body”的特例。
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
        }
    };

    match current {
        QrCodeStatus::Confirmed { session_data } => {
            // 命中确认，删除缓存并返回 token
            state.qrcode_service.remove(&qr_id).await;
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
            // 频率限制：遵循 TapTap 建议的 interval
            let now = std::time::Instant::now();
            // 先判断是否已过期（避免无意义轮询）
            if now >= expires_at {
                state.qrcode_service.remove(&qr_id).await;
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
                    // 按 interval 延后下一次轮询
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
                    let (error_code, message) = match &e {
                        AppError::Auth(_) => ("UNAUTHORIZED", "认证失败"),
                        AppError::Network(_) => ("UPSTREAM_ERROR", "上游网络错误"),
                        AppError::Json(_) => ("UPSTREAM_ERROR", "上游响应解析失败"),
                        AppError::Validation(_) => ("VALIDATION_FAILED", "请求参数错误"),
                        AppError::Conflict(_) => ("CONFLICT", "资源冲突"),
                        AppError::Internal(_) => ("INTERNAL_ERROR", "服务器内部错误"),
                        AppError::SaveProvider(_)
                        | AppError::Search(_)
                        | AppError::SaveHandlerError(_)
                        | AppError::ImageRendererError(_)
                        | AppError::AuthPending(_) => ("INTERNAL_ERROR", "服务器内部错误"),
                    };
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
        QrCodeStatus::Scanned => Ok(json_no_store(
            StatusCode::OK,
            QrCodeStatusResponse {
                status: QrCodeStatusValue::Scanned,
                session_token: None,
                error_code: None,
                message: None,
                retry_after: None,
            },
        )),
    }
}

pub fn create_auth_router() -> Router<AppState> {
    Router::<AppState>::new()
        .route("/qrcode", post(post_qrcode))
        .route("/qrcode/:qr_id/status", get(get_qrcode_status))
        .route("/user-id", post(post_user_id))
}
