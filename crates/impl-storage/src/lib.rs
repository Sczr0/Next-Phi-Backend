// 原根 crate lint 策略随迁移搬入（代码源自根包；与根 lib.rs 的 allow 列表一致）。
#![allow(
    clippy::similar_names,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::too_many_lines,
    clippy::doc_markdown,
    clippy::struct_excessive_bools,
    clippy::items_after_statements,
    clippy::module_name_repetitions
)]

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
///
/// 注：模块内不再使用内联文档（内/外文档属性不可同置，见 rustc 规则）。
pub mod models {
    pub use phi_contract::stats::*;
}

pub mod storage;

pub mod open_platform;

pub mod repo;

pub use storage::*;
