//! 存储层错误：领域错误（phi-contract）与错误转换的**唯一实现侧**。
//!
//! - `pub use phi_http::error::*`：承托被迁移代码的 `crate::error::AppError` 路径；
//! - `pub use phi_contract::error::StorageError`：领域存储错误（Charter §3.5）；
//! - `map_sqlx`：`sqlx::Error → StorageError` 的唯一转换点（孤儿规则，
//!   见 phi-contract/src/error.rs 文档）。

pub use phi_contract::error::StorageError;
pub use phi_http::error::*;

/// `sqlx::Error` → `StorageError` 的转换（**唯一** 允许触碰 sqlx 错误类型的位置）。
///
/// 规则（Charter §3.5）：
/// - 原始 `sqlx::Error` 必须先落日志（`Internal(String)` 会丢失来源链）；
/// - 业务层不可见 sqlx——本函数是 impl 侧适配器，不是 `From` 实现
///   （孤儿规则禁止，且 `#[from]` 会把 sqlx 依赖拉进契约层）。
#[must_use]
pub fn map_sqlx(ctx: &str, e: &sqlx::Error) -> StorageError {
    tracing::warn!(ctx, error = %e, "storage error");
    StorageError::Internal(format!("{ctx}: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_sqlx_preserves_context_in_domain_error() {
        // sqlx::Error::RowNotFound：构造一个真实 sqlx 错误（不做 From，走函数）。
        let err = map_sqlx("leaderboard.top", &sqlx::Error::RowNotFound);
        assert_eq!(
            err,
            StorageError::Internal(format!("leaderboard.top: {}", sqlx::Error::RowNotFound))
        );
    }

    #[test]
    fn storage_error_is_not_orphan_violating() {
        // 编译期声明：契约层错误可被本 crate 借调，但不允许为它实现
        // From<sqlx::Error>（孤儿规则）；存在性由 map_sqlx 的使用证明。
        let _: StorageError = StorageError::NotFound("row".into());
    }
}
