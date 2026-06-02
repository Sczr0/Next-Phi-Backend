use alloc::string::String;

use base64::Engine as _;
use serde::{Deserialize, Serialize};

use crate::error::{CodecError, Result};
use crate::reader::Reader;

/// 解析后的游戏进度信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameProgressParsed {
    pub version: u8,
    pub is_first_run: Option<bool>,
    pub legacy_chapter_finished: Option<bool>,
    pub already_show_collection_tip: Option<bool>,
    pub already_show_auto_unlock_in_tip: Option<bool>,
    pub completed: Option<String>,
    pub song_update_info: Option<u8>,
    pub challenge_mode_rank: Option<u16>,
    pub money: Option<[i32; 5]>,
    pub unlock_flag_of_spasmodic: Option<u8>,
    pub unlock_flag_of_igallta: Option<u8>,
    pub unlock_flag_of_rrharil: Option<u8>,
    pub flag_of_song_record_key: Option<u8>,
    pub random_version_unlocked: Option<u8>,
    pub chapter8_unlock_begin: Option<bool>,
    pub chapter8_unlock_second_phase: Option<bool>,
    pub chapter8_passed: Option<bool>,
    pub chapter8_song_unlocked: Option<u8>,
    /// gameProgress v4: 歌曲记录密钥 Takumi 标志位 (3 bits)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flag_of_song_record_key_takumi: Option<[bool; 3]>,
    pub overflow: Option<String>,
}

fn read_bool_array<const N: usize>(reader: &mut Reader) -> Result<[bool; N]> {
    let b = reader.read_u8()?;
    let mut arr = [false; N];
    for i in 0..N {
        arr[i] = ((b >> i) & 1) != 0;
    }
    Ok(arr)
}

/// 解析 gameProgress entry
pub fn parse_game_progress_entry(entry: &[u8]) -> Result<GameProgressParsed> {
    if entry.is_empty() {
        return Err(CodecError::NotEnoughData);
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
        flag_of_song_record_key_takumi: None,
        overflow: None,
    };

    if version >= 1 {
        let flags = r.read_u8()?;
        out.is_first_run = Some((flags & 0b0001) != 0);
        out.legacy_chapter_finished = Some((flags & 0b0010) != 0);
        out.already_show_collection_tip = Some((flags & 0b0100) != 0);
        out.already_show_auto_unlock_in_tip = Some((flags & 0b1000) != 0);
        out.completed = Some(r.read_owned_string(0)?);
        out.song_update_info = Some(r.read_u8()?);
        out.challenge_mode_rank = Some(r.read_u16_le()?);
        let mut money = [0_i32; 5];
        for slot in &mut money {
            *slot = r.read_varshort()? as i32;
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
    if version >= 4 {
        // v4: flag_of_song_record_key_takumi (3 bits)
        out.flag_of_song_record_key_takumi = Some(read_bool_array::<3>(&mut r)?);
    }
    if r.offset() < entry.len() {
        out.overflow = Some(base64::engine::general_purpose::STANDARD.encode(&entry[r.offset()..]));
    }
    Ok(out)
}
