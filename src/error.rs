use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
};
use thiserror::Error;

use crate::features::song::models::SongInfo;

/// 应用统一错误类型
#[derive(Error, Debug, utoipa::ToSchema)]
pub enum AppError {
    /// 授权未完成（轮询等待用户确认）
    #[error("待授权: {0}")]
    AuthPending(String),
    /// 网络请求错误
    #[error("网络错误: {0}")]
    Network(String),

    /// JSON 解析错误
    #[error("JSON 解析错误: {0}")]
    Json(String),

    /// 认证失败 / 业务错误
    #[error("认证失败: {0}")]
    Auth(String),
    /// 保存处理错误
    #[error("保存处理错误: {0}")]
    SaveHandlerError(String),

    /// 图像渲染错误
    #[error("图像渲染错误: {0}")]
    ImageRendererError(String),

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
    /// 结果不唯一（返回所有候选项，以便提示歧义）
    #[error("查询到多个候选项（需要更精确的关键词）")]
    NotUnique { candidates: Vec<SongInfo> },
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            AppError::AuthPending(_) => (StatusCode::ACCEPTED, self.to_string()),
            AppError::Network(_) => (StatusCode::BAD_GATEWAY, self.to_string()),
            AppError::Json(_) => (StatusCode::BAD_REQUEST, self.to_string()),
            AppError::Auth(_) => (StatusCode::UNAUTHORIZED, self.to_string()),
            AppError::SaveHandlerError(_) => (StatusCode::BAD_REQUEST, self.to_string()),
            AppError::ImageRendererError(_) => (StatusCode::UNPROCESSABLE_ENTITY, self.to_string()),
            AppError::Internal(_) => (StatusCode::INTERNAL_SERVER_ERROR, self.to_string()),
            AppError::SaveProvider(_) => (StatusCode::BAD_REQUEST, self.to_string()),
            AppError::Search(SearchError::NotFound) => (StatusCode::NOT_FOUND, self.to_string()),
            AppError::Search(SearchError::NotUnique { .. }) => {
                (StatusCode::CONFLICT, self.to_string())
            }
        };
        (status, message).into_response()
    }
}

// =============== Error conversions for common external errors ===============

impl From<reqwest::Error> for SaveProviderError {
    fn from(err: reqwest::Error) -> Self {
        SaveProviderError::Network(err.to_string())
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
