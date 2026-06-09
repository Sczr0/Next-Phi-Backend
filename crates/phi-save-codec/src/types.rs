use serde::{Deserialize, Serialize};

/// 难度枚举
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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

impl TryFrom<u8> for Difficulty {
    type Error = &'static str;

    fn try_from(value: u8) -> core::result::Result<Self, Self::Error> {
        match value {
            0 => Ok(Difficulty::EZ),
            1 => Ok(Difficulty::HD),
            2 => Ok(Difficulty::IN),
            3 => Ok(Difficulty::AT),
            _ => Err("invalid difficulty index"),
        }
    }
}

impl TryFrom<usize> for Difficulty {
    type Error = &'static str;

    fn try_from(value: usize) -> core::result::Result<Self, Self::Error> {
        match value {
            0 => Ok(Difficulty::EZ),
            1 => Ok(Difficulty::HD),
            2 => Ok(Difficulty::IN),
            3 => Ok(Difficulty::AT),
            _ => Err("invalid difficulty index"),
        }
    }
}

/// 单个难度的成绩记录（纯数据，不含推分提示等计算字段）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DifficultyRecord {
    pub difficulty: Difficulty,
    pub score: u32,
    pub accuracy: f32,
    pub is_full_combo: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chart_constant: Option<f32>,
    /// 推分 ACC（百分比），由上层引擎回填
    #[serde(skip_serializing_if = "Option::is_none")]
    pub push_acc: Option<f64>,
}

/// C/FC/P 成绩数量（累计口径）
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
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
