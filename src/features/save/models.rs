use serde::{Deserialize, Serialize};

use super::client::ExternalApiCredentials;

mod float_serialize {
    use serde::Serializer;

    pub fn serialize_f32_option<S>(value: &Option<f32>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match value {
            Some(v) => {
                // 格式化为1位小数的字符串，然后解析为f64以获得干净的表示
                let formatted = format!("{v:.1}");
                let clean: f64 = formatted.parse().unwrap_or(0.0);
                serializer.serialize_some(&clean)
            }
            None => serializer.serialize_none(),
        }
    }

    pub fn serialize_f64_option_3<S>(value: &Option<f64>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match value {
            Some(v) => {
                // 格式化为3位小数的字符串，然后解析回f64，避免 JSON 输出出现浮点脏小数。
                let formatted = format!("{v:.3}");
                let clean: f64 = formatted.parse().unwrap_or(0.0);
                serializer.serialize_some(&clean)
            }
            None => serializer.serialize_none(),
        }
    }
}

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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
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
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "float_serialize::serialize_f32_option"
    )]
    pub chart_constant: Option<f32>,
    /// 推分ACC（百分比）：用于让玩家显示RKS提升0.01 的目标ACC（千分位精度）。
    /// 仅在 /save?calculate_rks=true 时由服务端回填；默认不计算不返回。
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "float_serialize::serialize_f64_option_3"
    )]
    pub push_acc: Option<f64>,
    /// 推分提示：用于明确区分“不可推分/需Phi/已满ACC”等情况。
    ///
    /// - TargetAcc：需要提升到指定 ACC 才能推分
    /// - PhiOnly：只能推到 100%（Phi）才能推分
    /// - Unreachable：即使 100% 也无法推分
    /// - AlreadyPhi：已满 ACC，无需推分
    #[serde(skip_serializing_if = "Option::is_none")]
    pub push_acc_hint: Option<crate::features::rks::engine::PushAccHint>,
}

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
    pub rks: crate::features::rks::engine::PlayerRksResult,
    /// 按难度统计的 C/FC/P 成绩数量（仅 calculate_rks=true 时返回）
    #[serde(rename = "gradeCounts")]
    pub grade_counts: CfcPCountsByDifficulty,
}
