// 文档注释以中文为主 + 标识符/表名/路径密集，`doc_markdown` 会产生大量误报
// （与根 crate 同决策：根 lib.rs 的 allow 列表一致，见其注释）。
#![allow(clippy::doc_markdown)]

//! phi-contract：对外契约类型（Charter §3.3）。
//!
//! 只允许 std + 轻量库（serde/thiserror/utoipa 元数据派生）；
//! 零 tokio / 零 sqlx / 零 reqwest / 零业务逻辑。
//! 定义从此处物化；根 crate 的 `contracts/*` 与 `features/*/models` 以
//! re-export shim 保持原调用路径不变（Phase 1 纯搬迁，无行为变化）。

pub mod auth;
pub mod chart;
pub mod error;
pub mod image;
pub mod repo;
pub mod rks;
pub mod save;
pub mod song;
pub mod stats;
pub mod storage;
