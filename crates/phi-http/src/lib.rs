//! phi-http：HTTP/网关层共享件（Phase 1 从根 crate 纯搬迁）。
//!
//! 承载 `AppError`（Problem Details 管线）、`request_id` 中间件与大上下文。
//! 根 crate 的 `src/error.rs` / `src/request_id.rs` 保留为 re-export shim，
//! 全部调用点路径不变。注意：本 crate 是 Phase 1 的临时归属——
//! Phase 2 时随 phi-server 网关层成形，最终并入组合根（Charter §3.7）。

pub mod error;
pub mod request_id;

pub use error::{AppError, ProblemDetails, SaveProviderError, SearchError};
pub use request_id::{RequestId, request_id_middleware};
