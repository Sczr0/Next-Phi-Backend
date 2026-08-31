//! 登录协议 DTO（Phase 1 定义已物化至 phi-contract/auth.rs）——shim 保持
//! `crate::features::auth::models::*` 路径不变（qrcode_service 等引用
//! `super::models::SessionData`）。
pub use phi_contract::auth::*;
