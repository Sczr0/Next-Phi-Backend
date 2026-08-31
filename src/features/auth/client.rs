//! TapTap/LeanCloud 客户端（Phase 1 已迁至 impl-upstream）——shim 保持
//! `crate::features::auth::client` 路径不变（state.rs / main.rs /
//! auth_services.rs re-export / 测试引用 `TapTapClient`）。
pub use impl_upstream::client::*;
