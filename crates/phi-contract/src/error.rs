//! 领域存储错误（Charter §3.5 错误规约——核心红线）。
//!
//! 规则回顾：
//! - 本类型是 **领域枚举**（纯 phi-contract 定义），严禁透传 `sqlx::Error` /
//!   `reqwest::Error` 等实现层错误；
//! - **孤儿规则**：`impl From<sqlx::Error> for StorageError` 无法在 impl-storage
//!   实现（`From`、`sqlx::Error`、`StorageError` 三者均非该 crate 本地类型），
//!   且 `thiserror` 的 `#[from]` 写在定义处会把 sqlx 依赖拉进契约层（禁止）。
//!   → 正确形态：转换函数放 impl 侧（`impl_storage::error::map_sqlx`）。
//! - 变体粒度按"业务需要分支"定，新变体等真实分支需求出现再加（原则 5）。

/// 存储层领域错误。HTTP 映射（由业务层完成）：
/// NotFound → 404 / Duplicate → 409 / ConnectionFailed → 503 类 / Internal → 500。
#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum StorageError {
    /// 目标行/记录不存在。
    #[error("not found: {0}")]
    NotFound(String),
    /// 唯一约束冲突（如别名占用、重复注册）。
    #[error("duplicate: {0}")]
    Duplicate(String),
    /// 数据库连接/可用性故障（可重试）。
    #[error("connection failed: {0}")]
    ConnectionFailed(String),
    /// 内部故障（SQL 错误、数据损坏等；原始实现错误须先落日志再归入此变体）。
    #[error("internal: {0}")]
    Internal(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_matches_contract() {
        assert_eq!(
            StorageError::NotFound("user 1".into()).to_string(),
            "not found: user 1"
        );
        assert_eq!(
            StorageError::Duplicate("alias".into()).to_string(),
            "duplicate: alias"
        );
        assert_eq!(
            StorageError::ConnectionFailed("pool exhausted".into()).to_string(),
            "connection failed: pool exhausted"
        );
        assert_eq!(
            StorageError::Internal("boom".into()).to_string(),
            "internal: boom"
        );
    }

    #[test]
    fn variants_are_eq_comparable() {
        // 契约层错误支持相等比较（测试/断言便利），且不依赖任何 sqlx 类型。
        assert_eq!(
            StorageError::NotFound("x".into()),
            StorageError::NotFound("x".into())
        );
        assert_ne!(
            StorageError::Internal("a".into()),
            StorageError::Internal("b".into())
        );
    }
}
