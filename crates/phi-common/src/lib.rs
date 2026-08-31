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

//! phi-common：共享纯类型底座（Charter §4.0 Phase 0）。
//!
//! 只允许依赖 std + 轻量库（config/serde/once_cell/sha2）；
//! 零 axum / 零 sqlx / 零业务。AppError（axum 层）不在此——
//! 它的归属是 phi-server 网关层（Phase 2，见 docs/ARCHITECTURE.md §3.7）。

pub mod config;
