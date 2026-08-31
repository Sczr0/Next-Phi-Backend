//! 谱面定数契约类型（从 startup/chart_loader.rs 下沉，Phase 1 纯搬迁）。
//! 解析逻辑（CSV）留在根 crate；本模块只含纯类型。

use std::collections::HashMap;

/// 单曲各难度定数
#[derive(Debug, Clone, utoipa::ToSchema, serde::Serialize)]
pub struct ChartConstants {
    #[schema(example = 4.5)]
    pub ez: Option<f32>,
    #[schema(example = 7.9)]
    pub hd: Option<f32>,
    #[serde(rename = "in")]
    #[schema(example = 9.6)]
    pub in_level: Option<f32>,
    #[schema(example = 12.3)]
    pub at: Option<f32>,
}

/// 歌曲ID -> 定数映射
pub type ChartConstantsMap = HashMap<String, ChartConstants>;
