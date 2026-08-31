//! impl-storage：存储实现（Charter §3.2——SQLite 唯一所在，收拢 SQL/池/DDL/索引/VACUUM）。
//!
//! Phase 1 从根 crate 纯搬迁（原 `src/features/stats/storage.rs` + `storage/` 整棵子树），
//! 内部引用零修改：`crate::error` / `crate::models` 由本 crate 的 re-export 承托。
//! 存储是"接口抽象（Phase 2）"之前的第一个可独立编译的实现 crate。

/// 错误面：re-export 承托被迁移代码的 `crate::error::AppError` 路径
/// （phi-http），并持有领域存储错误（phi-contract）与 `map_sqlx` 转换点。
pub mod error;

/// 统计/存储领域模型（定义物化于 phi-contract）；被迁移代码内
/// `super::super::models::EventInsert` 由此解析。
pub mod models {
    //! 存储模型 re-export。
    pub use phi_contract::stats::*;
}

pub mod storage;

pub mod open_platform;

pub mod repo;

pub use storage::*;
