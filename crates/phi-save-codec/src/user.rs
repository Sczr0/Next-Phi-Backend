use alloc::string::String;

use serde::{Deserialize, Serialize};

use crate::error::{CodecError, Result};
use crate::reader::Reader;

/// 解析后的用户信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserParsed {
    pub show_player_id: bool,
    pub self_intro: String,
    pub avatar: String,
    pub background: String,
}

/// 解析 user entry
pub fn parse_user_entry(entry: &[u8]) -> Result<UserParsed> {
    if entry.is_empty() {
        return Err(CodecError::NotEnoughData);
    }
    let mut r = Reader::new(&entry[1..]);
    let flags = r.read_u8()?;
    Ok(UserParsed {
        show_player_id: (flags & 0b0001) != 0,
        self_intro: r.read_owned_string(0)?,
        avatar: r.read_owned_string(0)?,
        background: r.read_owned_string(0)?,
    })
}
