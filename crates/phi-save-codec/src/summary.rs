use alloc::format;
use alloc::string::String;

use base64::Engine as _;
use serde::{Deserialize, Serialize};

use crate::error::{CodecError, Result};
use crate::reader::Reader;

/// 解析后的存档摘要
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SummaryParsed {
    pub save_version: u8,
    pub challenge_mode_rank: u16,
    pub ranking_score: f32,
    pub game_version: u8,
    pub avatar: String,
    pub progress: [u16; 12],
}

/// 从 base64 编码的 summary 解析
pub fn parse_summary_base64(b64: &str) -> Result<SummaryParsed> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .map_err(|e| CodecError::from(format!("base64 decode failed: {e}")))?;
    let mut r = Reader::new(&bytes);

    let save_version = r.read_u8()?;
    let challenge_mode_rank = r.read_u16_le()?;
    let ranking_score = r.read_f32_le()?;
    let game_version = r.read_u8()?;
    let avatar = r.read_owned_string(0)?;

    let mut progress = [0u16; 12];
    for slot in &mut progress {
        *slot = r.read_u16_le()?;
    }

    Ok(SummaryParsed {
        save_version,
        challenge_mode_rank,
        ranking_score,
        game_version,
        avatar,
        progress,
    })
}
