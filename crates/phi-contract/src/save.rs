//! 存档难度契约类型（从 features/save/models.rs 下沉，Phase 1 纯搬迁）。

use serde::{Deserialize, Serialize};

/// 浮点序列化辅助（原 features/save/models.rs 内 mod float_serialize）。
/// DifficultyRecord 的 serde 属性依赖；纯 serde 辅助，无逻辑。
mod float_serialize {
    use serde::Serializer;

    #[allow(clippy::ref_option, clippy::trivially_copy_pass_by_ref)]
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

    #[allow(clippy::ref_option, clippy::trivially_copy_pass_by_ref)]
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

/// 难度枚举
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub enum Difficulty {
    EZ,
    HD,
    IN,
    AT,
}

impl core::convert::From<phi_save_codec::Difficulty> for Difficulty {
    fn from(d: phi_save_codec::Difficulty) -> Self {
        match d {
            phi_save_codec::Difficulty::EZ => Difficulty::EZ,
            phi_save_codec::Difficulty::HD => Difficulty::HD,
            phi_save_codec::Difficulty::IN => Difficulty::IN,
            phi_save_codec::Difficulty::AT => Difficulty::AT,
        }
    }
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

/// 单条难度记录（保存/推分输出）
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
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
    /// 推分提示：用于明确区分"不可推分/需Phi/已满ACC"等情况。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub push_acc_hint: Option<crate::rks::PushAccHint>,
}
