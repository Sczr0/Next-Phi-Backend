//! 开放平台存储模块（Phase 1 已迁至 impl-storage/open_platform）——shim 保持
//! `crate::features::open_platform::storage` 路径不变
//! （keys/handlers、token_auth/middleware、AppState 等引用
//! `OpenPlatformStorage` / const 常量）。
pub use impl_storage::open_platform::*;
