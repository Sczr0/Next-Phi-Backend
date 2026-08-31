//! impl-upstream：出站上游客户端（Phase 1 纯搬迁，2026-09）。
//! v1 = TapTap OAuth / LeanCloud 登录客户端（原 features/auth/client.rs）。
//! 后续收口：GitHub OAuth、远端 info、曲绘仓库同步。

// 承托被迁移代码的路径约定（与根 crate 等价的模块路径，零编辑搬迁）：
pub mod error {
    //! 错误 re-export。
    pub use phi_http::error::*;
}

pub mod config {
    //! 配置 re-export。
    pub use phi_common::config::*;
}

pub mod models {
    //! 协议 DTO re-export（client 的 super::models 路径）。
    pub use phi_contract::auth::*;
}

pub mod client;
