//! impl-rks：RKS 计算引擎实现（Phase 1 纯搬迁自 features/rks/engine.rs，
//! 2026-09）。引擎是纯计算（无 IO/无 HTTP），被渲染层（未来 impl-render）
//! 与业务层（rks/save handler）共享——是"薄缝"的天然候选（Phase 2 定 trait）。

// 承托被迁移代码的路径约定（与根 crate 等价的模块路径，零编辑搬迁）：
pub mod save_contract {
    //! 存档契约 re-export。
    pub use phi_contract::save::*;
}

pub mod startup {
    //! 启动期类型 re-export（chart_loader 纯类型）。
    pub mod chart_loader {
        pub use phi_contract::chart::*;
    }
}

pub mod rks_contract {
    //! RKS 契约 re-export（PushAccHint 等纯类型）。
    pub use phi_contract::rks::*;
}

pub mod engine;
