use alloc::string::String;

use serde::{Deserialize, Serialize};

use crate::error::{CodecError, Result};
use crate::reader::Reader;

/// 解析后的客户端设置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettingsParsed {
    pub chord_support: bool,
    pub fc_ap_indicator: bool,
    pub enable_hit_sound: bool,
    pub low_resolution_mode: bool,
    pub device_name: String,
    pub bright: f64,
    pub music_volume: f64,
    pub effect_volume: f64,
    pub hit_sound_volume: f64,
    pub sound_offset: f64,
    pub note_scale: f64,
}

/// 解析 settings entry
pub fn parse_settings_entry(entry: &[u8]) -> Result<SettingsParsed> {
    if entry.is_empty() {
        return Err(CodecError::NotEnoughData);
    }
    let mut r = Reader::new(&entry[1..]);
    let flags = r.read_u8()?;
    Ok(SettingsParsed {
        chord_support: (flags & 0b0001) != 0,
        fc_ap_indicator: (flags & 0b0010) != 0,
        enable_hit_sound: (flags & 0b0100) != 0,
        low_resolution_mode: (flags & 0b1000) != 0,
        device_name: r.read_owned_string(0)?,
        bright: f64::from(r.read_f32_le()?),
        music_volume: f64::from(r.read_f32_le()?),
        effect_volume: f64::from(r.read_f32_le()?),
        hit_sound_volume: f64::from(r.read_f32_le()?),
        sound_offset: f64::from(r.read_f32_le()?),
        note_scale: f64::from(r.read_f32_le()?),
    })
}
