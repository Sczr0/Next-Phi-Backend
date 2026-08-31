//! 全局 HTTP Client 复用工具（Phase 1 已迁至 phi-http）——shim 保持
//! `crate::http::client_default` 等路径不变（auth/save/open_platform 使用）。
pub use phi_http::http::*;
