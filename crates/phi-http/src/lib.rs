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

//! phi-http：HTTP/网关层共享件（Phase 1 从根 crate 纯搬迁）。
//!
//! 承载 `AppError`（Problem Details 管线）、`request_id` 中间件与大上下文。
//! 根 crate 的 `src/error.rs` / `src/request_id.rs` 保留为 re-export shim，
//! 全部调用点路径不变。注意：本 crate 是 Phase 1 的临时归属——
//! Phase 2 时随 phi-server 网关层成形，最终并入组合根（Charter §3.7）。

pub mod error;
pub mod http;
pub mod request_id;

pub use error::{AppError, ProblemDetails, SaveProviderError, SearchError};
pub use request_id::{RequestId, request_id_middleware};
