use serde::{Deserialize, Serialize};

use crate::features::save::models::UnifiedSaveRequest;

/// 输出图片格式
#[derive(Debug, Clone, Copy, Serialize, Deserialize, utoipa::ToSchema, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum ImageFormat {
    /// PNG（默认，保真但体积较大）
    #[default]
    Png,
    /// JPEG（有损压缩，体积显著更小，适合照片/插画类背景）
    Jpeg,
    /// WebP（新一代图片格式，相比JPEG/PNG可减少25-35%的文件大小，同时支持有损和无损压缩）
    Webp,
}

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


/// BN 渲染请求体
#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct RenderBnRequest {
    /// 认证方式（二选一）：sessionToken 或 externalCredentials
    #[serde(flatten)]
    pub auth: UnifiedSaveRequest,
    /// 取前 N 条 RKS 最高的成绩（默认 30）
    #[schema(example = 30)]
    #[serde(default = "default_n")]
    pub n: u32,
    /// 渲染主题：white/black（默认 black）
    #[serde(default)]
    pub theme: Theme,
    /// 是否将封面等资源内嵌到 PNG（默认为 false）
    #[serde(default)]
    pub embed_images: bool,
    /// 可选：用于显示的玩家昵称（若未提供且无法从服务端获取，将使用默认占位）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nickname: Option<String>,
    /// 输出图片格式（png/jpeg，默认 png）
    #[serde(default)]
    pub format: ImageFormat,
    /// 目标宽度像素（可选；不填使用默认 1200）。用于下采样以减小体积。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
}

/// 单曲渲染请求体
#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct RenderSongRequest {
    /// 认证方式（二选一）：sessionToken 或 externalCredentials
    #[serde(flatten)]
    pub auth: UnifiedSaveRequest,
    /// 歌曲 ID 或名称
    #[schema(example = "Arcahv")] 
    pub song: String,
    /// 是否将封面等资源内嵌到 PNG（默认为 false）
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
