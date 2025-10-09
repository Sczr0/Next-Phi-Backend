use serde::{Deserialize, Serialize};

use super::client::ExternalApiCredentials;

/// 统一的存档请求结构
#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UnifiedSaveRequest {
    /// 官方 LeanCloud 会话令牌
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_token: Option<String>,

    /// 外部 API 凭证
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_credentials: Option<ExternalApiCredentials>,
}

/// 存档响应结构
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct SaveResponse {
    /// 存档数据
    pub data: serde_json::Value,
}

// 示例保留：若后续需要可用于生成示例 JSON
#[allow(dead_code)]
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

/// 难度枚举
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub enum Difficulty {
    EZ,
    HD,
    IN,
    AT,
}

impl core::fmt::Display for Difficulty {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let s = match self {
            Difficulty::EZ => "EZ",
            Difficulty::HD => "HD",
            Difficulty::IN => "IN",
            Difficulty::AT => "AT",
        };
        f.write_str(s)
    }
}

impl core::convert::TryFrom<u8> for Difficulty {
    type Error = &'static str;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Difficulty::EZ),
            1 => Ok(Difficulty::HD),
            2 => Ok(Difficulty::IN),
            3 => Ok(Difficulty::AT),
            _ => Err("invalid difficulty index"),
        }
    }
}

impl core::convert::TryFrom<usize> for Difficulty {
    type Error = &'static str;

    fn try_from(value: usize) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Difficulty::EZ),
            1 => Ok(Difficulty::HD),
            2 => Ok(Difficulty::IN),
            3 => Ok(Difficulty::AT),
            _ => Err("invalid difficulty index"),
        }
    }
}

/// 单个难度的成绩记录
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DifficultyRecord {
    pub difficulty: Difficulty,
    pub score: u32,
    pub accuracy: f32,
    pub is_full_combo: bool,
}

// 仅用于 OpenAPI 文档展示的响应模型（包含 updatedAt 字段）
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct ParsedSaveDoc {
    #[serde(rename = "updatedAt")]
    #[schema(example = "2025-09-20T04:10:44.188Z")]
    pub updated_at: Option<String>,
    #[serde(rename = "summaryParsed")]
    pub summary_parsed: Option<serde_json::Value>,
    pub game_record: serde_json::Value,
    pub game_progress: serde_json::Value,
    pub user: serde_json::Value,
    pub settings: serde_json::Value,
    pub game_key: serde_json::Value,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct SaveResponseDoc {
    pub data: ParsedSaveDoc,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct SaveAndRksResponseDoc {
    pub save: ParsedSaveDoc,
    pub rks: crate::features::rks::engine::PlayerRksResult,
}
