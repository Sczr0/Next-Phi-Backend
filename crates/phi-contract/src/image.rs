//! 渲染主题契约类型（从 features/image/types.rs 下沉，Phase 1 纯搬迁）。

use serde::{Deserialize, Serialize};

/// 渲染主题
#[derive(Debug, Clone, Copy, Serialize, Deserialize, utoipa::ToSchema, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum Theme {
    #[serde(alias = "white", alias = "WHITE")]
    White,
    #[serde(alias = "black", alias = "BLACK")]
    #[default]
    Black,
}
