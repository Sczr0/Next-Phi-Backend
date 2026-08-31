//! request_id 中间件与上下文（Phase 1 已迁至 phi-http）。
//!
//! 本 shim 保持 `crate::request_id::*`（router 中间件装配、
//! features 内 `current_request_id()` 调用）路径不变。
pub use phi_http::request_id::*;
