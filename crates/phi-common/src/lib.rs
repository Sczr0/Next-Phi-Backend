//! phi-common：共享纯类型底座（Charter §4.0 Phase 0）。
//!
//! 只允许依赖 std + 轻量库（config/serde/once_cell/sha2）；
//! 零 axum / 零 sqlx / 零业务。AppError（axum 层）不在此——
//! 它的归属是 phi-server 网关层（Phase 2，见 docs/ARCHITECTURE.md §3.7）。

pub mod config;
