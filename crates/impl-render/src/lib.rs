//! impl-render：渲染实现（Phase 1 纯搬迁自 features/image/renderer.rs + renderer/，
//! 2026-09）。SVG 模板（minijinja）+ resvg 光栅化 —— 重 CPU 依赖全在此 crate，
//! 是"重依赖隔离"（Charter §3.2）的首次兑现。

// 承托被迁移代码的路径约定（与根 crate 等价的模块路径，零编辑搬迁）：
pub mod error {
    //! 错误 re-export。
    pub use phi_http::error::*;
}

pub mod config {
    //! 配置 re-export。
    pub use phi_common::config::*;
}

pub mod request_id {
    //! request_id 上下文 re-export（signing 的错误响应透传依赖）。
    pub use phi_http::request_id::*;
}

pub mod save_contract {
    //! 存档契约 re-export。
    pub use phi_contract::save::*;
}

pub mod rks_contract {
    //! RKS 引擎 re-export（模块级）。
    pub use impl_rks::engine;
}

pub mod features {
    //! 业务私有类型 re-export。
    pub mod image {
        pub use phi_contract::image::Theme;
    }
}

pub mod renderer;

// 图像管线配套（Phase 1 一并迁入）：曲绘目录定位 + CDN/水印签名与校验（纯密码学）。
// 根侧原 `image/mod.rs` 的对应声明已删除（无根内使用者，仅 renderer 引用）。
pub mod cover_loader;
pub mod signing;
