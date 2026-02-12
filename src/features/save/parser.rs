use base64::{Engine as _, engine::general_purpose};
use serde_json::{Number, Value};
use std::collections::HashMap;

use crate::error::SaveProviderError;

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
            return Err(SaveProviderError::Decrypt("EOF".into()));
        }
        let b = self.data[self.off];
        self.off += 1;
        Ok(b)
    }
    fn read_u16_le(&mut self) -> Result<u16, SaveProviderError> {
        if self.remain() < 2 {
            return Err(SaveProviderError::Decrypt("EOF".into()));
        }
        let v = u16::from_le_bytes([self.data[self.off], self.data[self.off + 1]]);
        self.off += 2;
        Ok(v)
    }
    fn read_i32_le(&mut self) -> Result<i32, SaveProviderError> {
        if self.remain() < 4 {
            return Err(SaveProviderError::Decrypt("EOF".into()));
        }
        let v = i32::from_le_bytes([
            self.data[self.off],
            self.data[self.off + 1],
            self.data[self.off + 2],
            self.data[self.off + 3],
        ]);
        self.off += 4;
        Ok(v)
    }
    fn read_f32_le(&mut self) -> Result<f32, SaveProviderError> {
        if self.remain() < 4 {
            return Err(SaveProviderError::Decrypt("EOF".into()));
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
    fn read_varshort(&mut self) -> Result<i32, SaveProviderError> {
        let b0 = self.read_u8()?;
        if b0 < 0x80 {
            Ok(b0 as i32)
        } else {
            let b1 = self.read_u8()?;
            Ok(((b0 as i32 & 0x7F) ^ ((b1 as i32) << 7)) & 0xFFFF)
        }
    }
    fn read_string(&mut self, end: usize) -> Result<String, SaveProviderError> {
        let len = self.read_varshort()? as usize;
        if self.remain() < len {
            return Err(SaveProviderError::Decrypt("EOF string".into()));
        }
        let s = &self.data[self.off..self.off + len - end];
        self.off += len;
        Ok(String::from_utf8_lossy(s).to_string())
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameKeyParsed {
    pub version: u8,
    pub map: HashMap<String, [u8; 5]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lanota_read_keys: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub camellia_read_key: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub overflow: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameProgressParsed {
    pub version: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_first_run: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub legacy_chapter_finished: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub already_show_collection_tip: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub already_show_auto_unlock_in_tip: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub song_update_info: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub challenge_mode_rank: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub money: Option<[i32; 5]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unlock_flag_of_spasmodic: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unlock_flag_of_igallta: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unlock_flag_of_rrharil: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flag_of_song_record_key: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub random_version_unlocked: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chapter8_unlock_begin: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chapter8_unlock_second_phase: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chapter8_passed: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chapter8_song_unlocked: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub overflow: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserParsed {
    pub show_player_id: bool,
    pub self_intro: String,
    pub avatar: String,
    pub background: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
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

fn parse_game_key_map(reader: &mut Reader) -> Result<HashMap<String, [u8; 5]>, SaveProviderError> {
    let length = reader.read_varshort()?;
    let mut map = HashMap::with_capacity(usize::try_from(length).unwrap_or(0));
    for _ in 0..length {
        let key = reader.read_string(0)?;
        if reader.remain() < 1 {
            return Err(SaveProviderError::Decrypt("EOF map len".into()));
        }
        let first_len = reader.data[reader.off] as usize;
        let next = reader.off + 1 + first_len;
        if next > reader.data.len() {
            return Err(SaveProviderError::Decrypt("EOF map payload".into()));
        }
        reader.off += 1;
        let len = reader.read_u8()?;
        let mut arr = [0u8; 5];
        for (idx, slot) in arr.iter_mut().enumerate() {
            if ((len >> idx) & 1) != 0 {
                *slot = reader.read_u8()?;
            }
        }
        map.insert(key, arr);
        reader.off = next;
    }
    Ok(map)
}

pub fn parse_game_key_entry(entry: &[u8]) -> Result<GameKeyParsed, SaveProviderError> {
    if entry.is_empty() {
        return Err(SaveProviderError::Decrypt("gameKey 澶煭".into()));
    }
    let mut r = Reader::new(entry);
    let version = r.read_u8()?;
    let map = parse_game_key_map(&mut r)?;
    let lanota_read_keys = if version >= 1 {
        Some(r.read_u8()?)
    } else {
        None
    };
    let camellia_read_key = if version >= 2 {
        Some((r.read_u8()? & 1) != 0)
    } else {
        None
    };
    let overflow = if r.off < entry.len() {
        Some(general_purpose::STANDARD.encode(&entry[r.off..]))
    } else {
        None
    };
    Ok(GameKeyParsed {
        version,
        map,
        lanota_read_keys,
        camellia_read_key,
        overflow,
    })
}

pub fn parse_game_progress_entry(entry: &[u8]) -> Result<GameProgressParsed, SaveProviderError> {
    if entry.is_empty() {
        return Err(SaveProviderError::Decrypt("gameProgress 澶煭".into()));
    }
    let mut r = Reader::new(entry);
    let version = r.read_u8()?;
    let mut out = GameProgressParsed {
        version,
        is_first_run: None,
        legacy_chapter_finished: None,
        already_show_collection_tip: None,
        already_show_auto_unlock_in_tip: None,
        completed: None,
        song_update_info: None,
        challenge_mode_rank: None,
        money: None,
        unlock_flag_of_spasmodic: None,
        unlock_flag_of_igallta: None,
        unlock_flag_of_rrharil: None,
        flag_of_song_record_key: None,
        random_version_unlocked: None,
        chapter8_unlock_begin: None,
        chapter8_unlock_second_phase: None,
        chapter8_passed: None,
        chapter8_song_unlocked: None,
        overflow: None,
    };

    if version >= 1 {
        let flags = r.read_u8()?;
        out.is_first_run = Some((flags & 0b0001) != 0);
        out.legacy_chapter_finished = Some((flags & 0b0010) != 0);
        out.already_show_collection_tip = Some((flags & 0b0100) != 0);
        out.already_show_auto_unlock_in_tip = Some((flags & 0b1000) != 0);
        out.completed = Some(r.read_string(0)?);
        out.song_update_info = Some(r.read_u8()?);
        out.challenge_mode_rank = Some(r.read_u16_le()?);
        let mut money = [0_i32; 5];
        for slot in &mut money {
            *slot = r.read_varshort()?;
        }
        out.money = Some(money);
        out.unlock_flag_of_spasmodic = Some(r.read_u8()?);
        out.unlock_flag_of_igallta = Some(r.read_u8()?);
        out.unlock_flag_of_rrharil = Some(r.read_u8()?);
        out.flag_of_song_record_key = Some(r.read_u8()?);
    }
    if version >= 2 {
        out.random_version_unlocked = Some(r.read_u8()?);
    }
    if version >= 3 {
        let flags = r.read_u8()?;
        out.chapter8_unlock_begin = Some((flags & 0b0001) != 0);
        out.chapter8_unlock_second_phase = Some((flags & 0b0010) != 0);
        out.chapter8_passed = Some((flags & 0b0100) != 0);
        out.chapter8_song_unlocked = Some(r.read_u8()?);
    }
    if r.off < entry.len() {
        out.overflow = Some(general_purpose::STANDARD.encode(&entry[r.off..]));
    }
    Ok(out)
}

pub fn parse_user_entry(entry: &[u8]) -> Result<UserParsed, SaveProviderError> {
    if entry.is_empty() {
        return Err(SaveProviderError::Decrypt("user 澶煭".into()));
    }
    let mut r = Reader::new(&entry[1..]);
    let flags = r.read_u8()?;
    Ok(UserParsed {
        show_player_id: (flags & 0b0001) != 0,
        self_intro: r.read_string(0)?,
        avatar: r.read_string(0)?,
        background: r.read_string(0)?,
    })
}

pub fn parse_settings_entry(entry: &[u8]) -> Result<SettingsParsed, SaveProviderError> {
    if entry.is_empty() {
        return Err(SaveProviderError::Decrypt("settings 澶煭".into()));
    }
    let mut r = Reader::new(&entry[1..]);
    let flags = r.read_u8()?;
    Ok(SettingsParsed {
        chord_support: (flags & 0b0001) != 0,
        fc_ap_indicator: (flags & 0b0010) != 0,
        enable_hit_sound: (flags & 0b0100) != 0,
        low_resolution_mode: (flags & 0b1000) != 0,
        device_name: r.read_string(0)?,
        bright: r.read_f32_le()? as f64,
        music_volume: r.read_f32_le()? as f64,
        effect_volume: r.read_f32_le()? as f64,
        hit_sound_volume: r.read_f32_le()? as f64,
        sound_offset: r.read_f32_le()? as f64,
        note_scale: r.read_f32_le()? as f64,
    })
}

fn deser_map(reader: &mut Reader, end: u8) -> Result<Value, SaveProviderError> {
    let length = reader.read_varshort()?;
    let mut map = serde_json::Map::with_capacity(usize::try_from(length).unwrap_or(0));
    for _ in 0..length {
        let key = reader.read_string(end as usize)?;
        if reader.remain() < 1 {
            return Err(SaveProviderError::Decrypt("EOF map len".into()));
        }
        let first_len = reader.data[reader.off] as usize;
        let next = reader.off + 1 + first_len;
        reader.off += 1;
        let mut arr = Vec::with_capacity(if end != 0 { 12 } else { 5 });
        let len = reader.read_u8()?;
        if end != 0 {
            let fc = reader.read_u8()?;
            for level in 0..4 {
                if ((len >> level) & 1) != 0 {
                    let score = reader.read_i32_le()? as i64;
                    let acc = reader.read_f32_le()? as f64;
                    let fc_bit = ((fc >> level) & 1) as i64;
                    arr.push(Value::Number(Number::from(score)));
                    arr.push(Value::Number(
                        Number::from_f64(acc).unwrap_or_else(|| Number::from(0)),
                    ));
                    arr.push(Value::Number(Number::from(fc_bit)));
                } else {
                    arr.push(Value::Number(Number::from(0)));
                    arr.push(Value::Number(Number::from(0)));
                    arr.push(Value::Number(Number::from(0)));
                }
            }
        } else {
            for ii in 0..5 {
                if ((len >> ii) & 1) != 0 {
                    arr.push(Value::Number(Number::from(reader.read_u8()? as i64)));
                } else {
                    arr.push(Value::Number(Number::from(0)));
                }
            }
        }
        map.insert(key, Value::Array(arr));
        reader.off = next;
    }
    Ok(Value::Object(map))
}

pub fn parse_single_save_entry_to_json(
    name: &str,
    entry: &[u8],
) -> Result<Value, SaveProviderError> {
    match name {
        "gameRecord" => {
            if entry.is_empty() {
                return Err(SaveProviderError::Decrypt("gameRecord 太短".into()));
            }
            let mut r = Reader::new(&entry[1..]);
            deser_map(&mut r, 2)
        }
        "gameKey" => serde_json::to_value(parse_game_key_entry(entry)?).map_err(|e| {
            SaveProviderError::Json(format!("serialize parsed save entry failed: {e}"))
        }),
        "gameProgress" => serde_json::to_value(parse_game_progress_entry(entry)?).map_err(|e| {
            SaveProviderError::Json(format!("serialize parsed save entry failed: {e}"))
        }),
        "user" => serde_json::to_value(parse_user_entry(entry)?).map_err(|e| {
            SaveProviderError::Json(format!("serialize parsed save entry failed: {e}"))
        }),
        "settings" => serde_json::to_value(parse_settings_entry(entry)?).map_err(|e| {
            SaveProviderError::Json(format!("serialize parsed save entry failed: {e}"))
        }),
        _ => Err(SaveProviderError::Json(format!(
            "unsupported save entry name: {name}"
        ))),
    }
}

pub fn parse_save_to_json(entries: &HashMap<String, Vec<u8>>) -> Result<Value, SaveProviderError> {
    let mut root = serde_json::Map::with_capacity(entries.len().min(5));
    for name in ["gameRecord", "gameKey", "gameProgress", "user", "settings"] {
        if let Some(entry) = entries.get(name) {
            let parsed = parse_single_save_entry_to_json(name, entry)?;
            root.insert(name.to_string(), parsed);
        }
    }
    Ok(Value::Object(root))
}
