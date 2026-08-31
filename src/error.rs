//! 统一错误处理模块（Phase 1 已迁至 phi-http）。
//!
//! 本 shim 保持 `crate::error::*` 全部调用点路径不变
//! （app 层错误管线、features 的 `crate::error::sanitize_reqwest_error` 等）。
pub use phi_http::error::*;
