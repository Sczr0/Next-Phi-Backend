use axum::{
    Json,
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::Serialize;
use thiserror::Error;

use crate::features::song::models::SongCandidatePreview;

/// 应用统一错误类型
#[derive(Error, Debug, utoipa::ToSchema)]
pub enum AppError {
    /// 授权未完成（轮询等待用户确认）
    #[error("待授权: {0}")]
    AuthPending(String),
    /// 网络请求错误
    #[error("网络错误: {0}")]
    Network(String),
    /// 上游请求超时（包含 connect/read 等阶段）
    #[error("请求超时: {0}")]
    Timeout(String),

    /// JSON 解析错误
    #[error("JSON 解析错误: {0}")]
    Json(String),

    /// 认证失败 / 业务错误
    #[error("认证失败: {0}")]
    Auth(String),
    /// 禁止访问
    #[error("禁止访问: {0}")]
    Forbidden(String),
    /// 保存处理错误
    #[error("保存处理错误: {0}")]
    SaveHandlerError(String),

    /// 图像渲染错误
    #[error("图像渲染错误: {0}")]
    ImageRendererError(String),

    /// 参数校验错误
    #[error("参数校验错误: {0}")]
    Validation(String),

    /// 资源冲突（如别名占用）
    #[error("资源冲突: {0}")]
    Conflict(String),

    /// 内部服务器错误
    #[error("内部错误: {0}")]
    Internal(String),

    /// 存档提供器错误
    #[error("存档提供器错误: {0}")]
    SaveProvider(#[from] SaveProviderError),

    /// 搜索错误
    #[error("搜索错误: {0}")]
    Search(#[from] SearchError),
}

/// 存档提供器错误类型
#[derive(Error, Debug, utoipa::ToSchema)]
pub enum SaveProviderError {
    /// 网络请求错误
    #[error("网络错误: {0}")]
    Network(String),

    /// 认证失败
    #[error("认证失败: {0}")]
    Auth(String),

    /// 元数据解析错误
    #[error("元数据解析错误: {0}")]
    Metadata(String),

    /// 缺少必需字段
    #[error("缺少必需字段: {0}")]
    MissingField(String),

    /// 解密失败
    #[error("解密失败: {0}")]
    Decrypt(String),

    /// 完整性检查失败
    #[error("完整性检查失败: {0}")]
    Integrity(String),

    /// 无效的填充
    #[error("无效的填充")]
    InvalidPadding,

    /// ZIP 解析错误
    #[error("ZIP 解析错误: {0}")]
    ZipError(String),

    /// I/O 错误
    #[error("I/O 错误: {0}")]
    Io(String),

    /// JSON 解析错误
    #[error("JSON 解析错误: {0}")]
    Json(String),

    /// 不支持的功能
    #[error("不支持的功能: {0}")]
    Unsupported(String),

    /// 无效的响应
    #[error("无效的响应: {0}")]
    InvalidResponse(String),

    /// 超时
    #[error("超时")]
    Timeout,

    /// 无效的头部格式
    #[error("无效的头部格式")]
    InvalidHeader,

    /// 标签验证失败
    #[error("标签验证失败")]
    TagVerification,

    /// 无效的凭据
    #[error("无效的凭据: {0}")]
    InvalidCredentials(String),
}

/// 搜索错误类型
#[derive(Error, Debug, utoipa::ToSchema)]
pub enum SearchError {
    /// 未找到匹配项
    #[error("未找到匹配项")]
    NotFound,
    /// 结果不唯一（返回候选预览，以便提示歧义）
    #[error("查询到多个候选项（需要更精确的关键词）")]
    NotUnique {
        /// 总命中数（用于提示调用方候选已截断）
        total: u32,
        /// 候选预览（受控数量，避免返回过大 payload）
        candidates: Vec<SongCandidatePreview>,
    },
}

/// RFC7807 风格的错误响应（Problem Details）。
///
/// 设计目标：
/// - 让所有 API 错误返回结构化 JSON，便于 SDK/调用方稳定处理
/// - 与 OpenAPI 一致（content-type = application/problem+json）
/// - 允许在不破坏主结构的前提下扩展字段（如 requestId、字段级校验错误）
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProblemDetails {
    /// 问题类型（URI）。若无更细分的类型，可使用 about:blank。
    #[serde(rename = "type")]
    #[schema(example = "about:blank")]
    pub type_url: String,

    /// 简短标题，用于概括错误。
    #[schema(example = "Validation Failed")]
    pub title: String,

    /// HTTP 状态码（与响应 status 一致）。
    #[schema(example = 422)]
    pub status: u16,

    /// 人类可读的详细信息（尽量稳定，不建议依赖解析）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,

    /// 稳定的错误码，用于程序化处理。
    #[schema(example = "VALIDATION_FAILED")]
    pub code: String,

    /// 可选：请求追踪 ID（如果后续加入 request-id middleware 可回填）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,

    /// 可选：字段级校验错误（如表单/参数校验）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub errors: Option<Vec<ProblemFieldError>>,

    /// 可选：搜索候选预览（通常用于 SEARCH_NOT_UNIQUE）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub candidates: Option<Vec<SongCandidatePreview>>,

    /// 可选：候选总数（用于提示 candidates 已截断）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub candidates_total: Option<u32>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProblemFieldError {
    /// 字段名（camelCase）。
    pub field: String,
    /// 字段错误信息。
    pub message: String,
}

impl AppError {
    fn status_code(&self) -> StatusCode {
        match self {
            AppError::AuthPending(_) => StatusCode::ACCEPTED,
            AppError::Network(_) => StatusCode::BAD_GATEWAY,
            AppError::Timeout(_) => StatusCode::GATEWAY_TIMEOUT,
            AppError::Json(_) => StatusCode::BAD_REQUEST,
            AppError::Auth(_) => StatusCode::UNAUTHORIZED,
            AppError::Forbidden(_) => StatusCode::FORBIDDEN,
            AppError::SaveHandlerError(_) => StatusCode::BAD_REQUEST,
            AppError::ImageRendererError(_) => StatusCode::UNPROCESSABLE_ENTITY,
            AppError::Validation(_) => StatusCode::UNPROCESSABLE_ENTITY,
            AppError::Conflict(_) => StatusCode::CONFLICT,
            AppError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
            AppError::SaveProvider(e) => match e {
                SaveProviderError::Auth(_) | SaveProviderError::InvalidCredentials(_) => {
                    StatusCode::UNAUTHORIZED
                }
                SaveProviderError::Timeout => StatusCode::GATEWAY_TIMEOUT,
                SaveProviderError::Network(_) => StatusCode::BAD_GATEWAY,
                // 其余情况更偏向“请求可理解但无法处理”（如解密/完整性/格式问题）
                _ => StatusCode::UNPROCESSABLE_ENTITY,
            },
            AppError::Search(SearchError::NotFound) => StatusCode::NOT_FOUND,
            AppError::Search(SearchError::NotUnique { .. }) => StatusCode::CONFLICT,
        }
    }

    fn stable_code(&self) -> &'static str {
        match self {
            AppError::AuthPending(_) => "AUTH_PENDING",
            AppError::Network(_) => "UPSTREAM_ERROR",
            AppError::Timeout(_) => "UPSTREAM_TIMEOUT",
            AppError::Json(_) => "BAD_REQUEST",
            AppError::Auth(_) => "UNAUTHORIZED",
            AppError::Forbidden(_) => "FORBIDDEN",
            AppError::SaveHandlerError(_) => "SAVE_BAD_REQUEST",
            AppError::ImageRendererError(_) => "IMAGE_RENDER_FAILED",
            AppError::Validation(_) => "VALIDATION_FAILED",
            AppError::Conflict(_) => "CONFLICT",
            AppError::Internal(_) => "INTERNAL_ERROR",
            AppError::SaveProvider(e) => match e {
                SaveProviderError::Auth(_) | SaveProviderError::InvalidCredentials(_) => {
                    "SAVE_AUTH_FAILED"
                }
                SaveProviderError::Timeout => "UPSTREAM_TIMEOUT",
                SaveProviderError::Network(_) => "UPSTREAM_ERROR",
                SaveProviderError::Decrypt(_)
                | SaveProviderError::Integrity(_)
                | SaveProviderError::InvalidPadding
                | SaveProviderError::TagVerification => "SAVE_DECRYPT_FAILED",
                SaveProviderError::ZipError(_)
                | SaveProviderError::Io(_)
                | SaveProviderError::Json(_)
                | SaveProviderError::Metadata(_)
                | SaveProviderError::MissingField(_)
                | SaveProviderError::Unsupported(_)
                | SaveProviderError::InvalidResponse(_)
                | SaveProviderError::InvalidHeader => "SAVE_INVALID_DATA",
            },
            AppError::Search(SearchError::NotFound) => "NOT_FOUND",
            AppError::Search(SearchError::NotUnique { .. }) => "SEARCH_NOT_UNIQUE",
        }
    }

    fn title(&self) -> &'static str {
        match self.status_code() {
            StatusCode::BAD_REQUEST => "Bad Request",
            StatusCode::UNAUTHORIZED => "Unauthorized",
            StatusCode::FORBIDDEN => "Forbidden",
            StatusCode::NOT_FOUND => "Not Found",
            StatusCode::CONFLICT => "Conflict",
            StatusCode::UNPROCESSABLE_ENTITY => "Validation Failed",
            StatusCode::BAD_GATEWAY => "Bad Gateway",
            StatusCode::GATEWAY_TIMEOUT => "Gateway Timeout",
            StatusCode::INTERNAL_SERVER_ERROR => "Internal Server Error",
            StatusCode::ACCEPTED => "Accepted",
            _ => "Error",
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = self.status_code();
        let title = self.title().to_string();
        let code = self.stable_code().to_string();
        let detail = Some(self.to_string());

        // 默认不返回候选；仅在“搜索结果不唯一”时附带受控数量的预览，提升 UX 且避免无收益开销。
        let (candidates, candidates_total) = match self {
            AppError::Search(SearchError::NotUnique { total, candidates }) => {
                (Some(candidates), Some(total))
            }
            _ => (None, None),
        };

        let problem = ProblemDetails {
            type_url: "about:blank".to_string(),
            title,
            status: status.as_u16(),
            detail,
            code,
            request_id: crate::request_id::current_request_id(),
            errors: None,
            candidates,
            candidates_total,
        };

        let mut res = Json(problem).into_response();
        *res.status_mut() = status;
        res.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/problem+json"),
        );
        res
    }
}

// =============== Error conversions for common external errors ===============

impl From<reqwest::Error> for SaveProviderError {
    fn from(err: reqwest::Error) -> Self {
        if err.is_timeout() {
            SaveProviderError::Timeout
        } else {
            SaveProviderError::Network(err.to_string())
        }
    }
}

impl From<zip::result::ZipError> for SaveProviderError {
    fn from(err: zip::result::ZipError) -> Self {
        SaveProviderError::ZipError(err.to_string())
    }
}

impl From<std::io::Error> for SaveProviderError {
    fn from(err: std::io::Error) -> Self {
        SaveProviderError::Io(err.to_string())
    }
}

impl From<serde_json::Error> for SaveProviderError {
    fn from(err: serde_json::Error) -> Self {
        SaveProviderError::Json(err.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::SaveProviderError;
    use std::time::Duration;

    async fn start_hanging_http_server() -> std::net::SocketAddr {
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind tcp listener");
        let addr = listener.local_addr().expect("local addr");

        tokio::spawn(async move {
            loop {
                let (socket, _) = match listener.accept().await {
                    Ok(v) => v,
                    Err(_) => break,
                };
                tokio::spawn(async move {
                    // 不返回任何 HTTP 响应，触发客户端 read timeout。
                    tokio::time::sleep(Duration::from_secs(3)).await;
                    drop(socket);
                });
            }
        });

        addr
    }

    #[tokio::test]
    async fn save_provider_error_from_reqwest_timeout_is_timeout() {
        let addr = start_hanging_http_server().await;
        let client = reqwest::Client::builder()
            .timeout(Duration::from_millis(100))
            .build()
            .expect("build reqwest client");

        let err = client
            .get(format!("http://{addr}/"))
            .send()
            .await
            .expect_err("expected timeout");
        assert!(err.is_timeout(), "expected reqwest timeout, got: {err}");

        let sp: SaveProviderError = err.into();
        assert!(
            matches!(sp, SaveProviderError::Timeout),
            "expected SaveProviderError::Timeout, got: {sp:?}"
        );
    }
}
