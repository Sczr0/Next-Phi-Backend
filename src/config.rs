//! 配置模块（Phase 0 下沉至 phi-common）——保留本 shim 以维持
//! `crate::config::*` 全部调用点路径不变（主 crate 与 bin 均不受影响）。
pub use phi_common::config::*;
