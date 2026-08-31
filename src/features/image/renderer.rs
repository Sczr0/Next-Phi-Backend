//! 渲染模块（Phase 1 已迁至 impl-render）——shim 保持
//! `crate::features::image::renderer` 与 image/mod.rs 的
//! `pub use renderer::{...}` 路径不变。
pub use impl_render::renderer::*;
