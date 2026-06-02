use alloc::borrow::ToOwned as _;
use alloc::string::String;
use core::fmt;

/// 二进制编解码错误
#[derive(Debug, Clone)]
pub enum CodecError {
    /// 数据不足
    NotEnoughData,
    /// 无效的 UTF-8 字符串
    InvalidUtf8,
    /// 数据格式不符合预期
    InvalidData,
    /// 编码/解码过程中的其他错误
    Custom(String),
}

impl From<&'static str> for CodecError {
    fn from(s: &'static str) -> Self {
        CodecError::Custom(s.to_owned())
    }
}

impl From<String> for CodecError {
    fn from(s: String) -> Self {
        CodecError::Custom(s)
    }
}

impl fmt::Display for CodecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CodecError::NotEnoughData => write!(f, "not enough data"),
            CodecError::InvalidUtf8 => write!(f, "invalid UTF-8"),
            CodecError::InvalidData => write!(f, "invalid data"),
            CodecError::Custom(msg) => write!(f, "{msg}"),
        }
    }
}

/// 提取 Result 类型别名
pub type Result<T> = core::result::Result<T, CodecError>;
