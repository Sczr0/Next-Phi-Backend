use base64::{Engine as _, engine::general_purpose};

use serde::{Deserialize, Serialize};

use crate::error::SaveProviderError;

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct SummaryParsed {
    pub save_version: u8,
    pub challenge_mode_rank: u16,
    pub ranking_score: f32,
    pub game_version: u8,
    pub avatar: String,
    pub progress: [u16; 12],
}

struct Reader<'a> {
    data: &'a [u8],
    off: usize,
}

impl<'a> Reader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, off: 0 }
    }
    fn remain(&self) -> usize {
        self.data.len().saturating_sub(self.off)
    }
    fn read_u8(&mut self) -> Result<u8, SaveProviderError> {
        if self.remain() < 1 {
            return Err(SaveProviderError::Metadata("EOF while reading u8".into()));
        }
        let b = self.data[self.off];
        self.off += 1;
        Ok(b)
    }
    fn read_u16_le(&mut self) -> Result<u16, SaveProviderError> {
        if self.remain() < 2 {
            return Err(SaveProviderError::Metadata("EOF while reading u16".into()));
        }
        let v = u16::from_le_bytes([self.data[self.off], self.data[self.off + 1]]);
        self.off += 2;
        Ok(v)
    }
    fn read_f32_le(&mut self) -> Result<f32, SaveProviderError> {
        if self.remain() < 4 {
            return Err(SaveProviderError::Metadata("EOF while reading f32".into()));
        }
        let v = f32::from_le_bytes([
            self.data[self.off],
            self.data[self.off + 1],
            self.data[self.off + 2],
            self.data[self.off + 3],
        ]);
        self.off += 4;
        Ok(v)
    }
    fn read_varshort(&mut self) -> Result<usize, SaveProviderError> {
        if self.remain() < 1 {
            return Err(SaveProviderError::Metadata(
                "EOF while reading varshort".into(),
            ));
        }
        let b0 = self.read_u8()?;
        if b0 < 0x80 {
            Ok(b0 as usize)
        } else {
            let b1 = self.read_u8()?;
            // 与 C 版本保持一致： (b0 & 0x7F) ^ (b1 << 7)
            let v = (((b0 as usize) & 0x7F) ^ ((b1 as usize) << 7)) & 0xFFFF;
            Ok(v)
        }
    }
    fn read_string(&mut self) -> Result<String, SaveProviderError> {
        let len = self.read_varshort()?;
        if self.remain() < len {
            return Err(SaveProviderError::Metadata(
                "EOF while reading string".into(),
            ));
        }
        let s = &self.data[self.off..self.off + len];
        self.off += len;
        match String::from_utf8(s.to_vec()) {
            Ok(ok) => Ok(ok),
            Err(_) => Ok(String::from_utf8_lossy(s).into_owned()),
        }
    }
}

/// 解析 LeanCloud `_GameSave.summary`（base64）为结构体
pub fn parse_summary_base64(b64: &str) -> Result<SummaryParsed, SaveProviderError> {
    let bytes = general_purpose::STANDARD
        .decode(b64)
        .map_err(|e| SaveProviderError::Metadata(format!("base64 decode failed: {}", e)))?;
    let mut r = Reader::new(&bytes);

    let save_version = r.read_u8()?;
    let challenge_mode_rank = r.read_u16_le()?;
    let ranking_score = r.read_f32_le()?;
    let game_version = r.read_u8()?;
    let avatar = r.read_string()?;

    let mut progress = [0u16; 12];
    for i in 0..12 {
        progress[i] = r.read_u16_le()?;
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
