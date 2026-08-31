//! 存档客户端（Phase 1 已迁至 impl-save）——shim 保持
//! `crate::features::save::client` 路径不变（model/identity_hash 等引用
//! `super::client::ExternalApiCredentials`）。
pub use impl_save::client::*;
