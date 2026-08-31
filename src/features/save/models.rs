use serde::{Deserialize, Serialize};

use super::client::ExternalApiCredentials;

// Phase 1 纯搬迁：Difficulty/DifficultyRecord 定义已物化到 phi-contract
// （save.rs）；此 re-export 保持 crate::save_contract::Difficulty 等路径不变。
pub use phi_contract::save::{Difficulty, DifficultyRecord};

/// 统一的存档请求结构
#[derive(Debug, Deserialize, Serialize, Clone, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UnifiedSaveRequest {
    /// 官方 LeanCloud 会话令牌
    #[schema(example = "r:abcdefg.hijklmn-opqrstuvwxyz")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_token: Option<String>,

    /// 外部 API 凭证
    /// 三选一：platform+platformId / sessiontoken / apiUserId
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_credentials: Option<ExternalApiCredentials>,

    /// TapTap 版本选择：cn（大陆版，默认）或 global（国际版）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub taptap_version: Option<String>,
}

/// 存档响应结构
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct SaveResponse {
    /// 存档数据
    #[schema(value_type = Object)]
    pub data: serde_json::Value,
}

// 示例保留：若后续需要可用于生成示例 JSON
#[allow(dead_code)]
#[must_use]
pub fn save_response_example() -> serde_json::Value {
    serde_json::json!({
        "data": {
            "updatedAt": "2025-09-20T04:10:44.188Z",
            "gameRecord": {},
            "gameProgress": {},
            "user": {},
            "settings": {},
            "gameKey": {}
        }
    })
}

// Phase 1 纯搬迁：(以下 Difficulty/DifficultyRecord 定义块已物化到
// phi-contract/src/save.rs，本文件顶部 re-export)
/// C/FC/P 成绩数量（累计口径）
///
/// 说明：按需求定义 C<FC<P，且 FC 的成绩同时计入 C，P 的成绩同时计入 FC 与 C。
#[derive(Debug, Clone, Copy, Default, Serialize, utoipa::ToSchema)]
pub struct CfcPCounts {
    /// Clear 数量（包含 FC 与 P）
    #[serde(rename = "C")]
    pub c: u32,
    /// Full Combo 数量（包含 P）
    #[serde(rename = "FC")]
    pub fc: u32,
    /// Perfect 数量
    #[serde(rename = "P")]
    pub p: u32,
}

/// 按难度统计的 C/FC/P 成绩数量
///
/// JSON 结构使用大写键名（EZ/HD/IN/AT），保证“各个难度”恒存在（即使为 0）。
#[derive(Debug, Clone, Copy, Default, Serialize, utoipa::ToSchema)]
pub struct CfcPCountsByDifficulty {
    #[serde(rename = "EZ")]
    pub ez: CfcPCounts,
    #[serde(rename = "HD")]
    pub hd: CfcPCounts,
    #[serde(rename = "IN")]
    pub in_: CfcPCounts,
    #[serde(rename = "AT")]
    pub at: CfcPCounts,
}

// 仅用于 OpenAPI 文档展示的响应模型（字段命名以实际返回为准）
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct ParsedSaveDoc {
    /// 与实际返回保持一致：字段名为 updatedAt
    #[serde(rename = "updatedAt", skip_serializing_if = "Option::is_none")]
    #[schema(example = "2025-09-20T04:10:44.188Z")]
    pub updated_at: Option<String>,
    /// 解析自 summary 的关键摘要（如段位、RKS 等）
    /// 与实际返回保持一致：字段名为 summaryParsed
    #[serde(rename = "summaryParsed", skip_serializing_if = "Option::is_none")]
    pub summary_parsed: Option<serde_json::Value>,
    /// 结构化成绩（歌曲ID -> [四难度成绩]）
    pub game_record: serde_json::Value,
    /// 进度信息（如金钱、拓展信息）
    pub game_progress: serde_json::Value,
    /// 用户基本信息
    pub user: serde_json::Value,
    /// 客户端设置
    pub settings: serde_json::Value,
    /// 游戏密钥块
    pub game_key: serde_json::Value,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct SaveResponseDoc {
    /// 解析后的存档对象
    pub data: ParsedSaveDoc,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct SaveAndRksResponseDoc {
    /// 解析后的存档对象
    pub save: ParsedSaveDoc,
    /// 玩家 RKS 概览
    pub rks: crate::rks_contract::engine::PlayerRksResult,
    /// 按难度统计的 C/FC/P 成绩数量（仅 calculate_rks=true 时返回）
    #[serde(rename = "gradeCounts")]
    pub grade_counts: CfcPCountsByDifficulty,
}
