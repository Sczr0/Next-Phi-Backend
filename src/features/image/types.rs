use serde::{Deserialize, Serialize};

use crate::features::save::models::UnifiedSaveRequest;

/// 渲染主题
#[derive(Debug, Clone, Copy, Serialize, Deserialize, utoipa::ToSchema, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    #[serde(alias = "white", alias = "WHITE")]
    White,
    #[serde(alias = "black", alias = "BLACK")]
    Black,
}

impl Default for Theme {
    fn default() -> Self {
        Theme::Black
    }
}

/// BN 渲染请求体
#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct RenderBnRequest {
    #[serde(flatten)]
    pub auth: UnifiedSaveRequest,
    #[serde(default = "default_n")]
    pub n: u32,
    #[serde(default)]
    pub theme: Theme,
    #[serde(default)]
    pub embed_images: bool,
    /// 可选：用于显示的玩家昵称（若未提供且无法从服务端获取，将使用默认占位）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nickname: Option<String>,
}

/// 单曲渲染请求体
#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct RenderSongRequest {
    #[serde(flatten)]
    pub auth: UnifiedSaveRequest,
    pub song: String,
    #[serde(default)]
    pub embed_images: bool,
    /// 可选：用于显示的玩家昵称
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nickname: Option<String>,
}

fn default_n() -> u32 {
    30
}

/// 用户自定义 BN 渲染请求（未验证成绩）
#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct RenderUserBnRequest {
    /// 主题（默认 black）
    #[serde(default)]
    pub theme: Theme,
    /// 可选昵称（未提供时可从 users/me 获取）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nickname: Option<String>,
    /// 解除水印的口令（匹配配置或动态口令时，显式/隐式水印均关闭）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unlock_password: Option<String>,
    /// 成绩列表
    pub scores: Vec<UserScoreItem>,
}

#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct UserScoreItem {
    /// 歌曲 ID 或名称
    pub song: String,
    /// 难度（EZ/HD/IN/AT）
    pub difficulty: String,
    /// ACC 百分比（示例：98.50）
    pub acc: f64,
    /// 分数（可选）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score: Option<u32>,
}
