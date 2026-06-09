use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

use base64::Engine as _;
use serde::{Deserialize, Serialize};

use crate::error::{CodecError, Result};
use crate::reader::{Reader, get_bit};

/// 单首歌的密钥：解析成功时为结构化 NormalKey，格式未知时保留原始字节
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum Key {
    /// 格式未知或解析出错，保留原始字节
    Raw(Vec<u8>),
    /// 正常解析的密钥结构
    Normal(NormalKey),
}

/// 解析后的密钥字段（每个字段对应 `type_byte` 中的一个 bit）
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NormalKey {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read_collection_piece_num: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unlock_single: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unlock_collection_piece_num: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unlock_illustration: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unlock_avatar: Option<bool>,
}

/// 解析后的游戏密钥信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameKeyParsed {
    pub version: u8,
    /// 各歌曲的密钥（BTreeMap 保证序列化顺序稳定）
    pub keys: BTreeMap<String, Key>,
    pub lanota_read_keys: Option<u8>,
    pub camellia_read_key: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub side_story4_begin_read_key: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub old_score_cleared_v390: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub overflow: Option<String>,
}

/// 解析单首歌的密钥原始字节为 Key enum
///
/// 健壮性设计：如果解析出的格式不符合预期（`type_byte` 高位不为 0、长度不匹配等），
/// 不报错，直接返回 `Key::Raw` 保留原始字节。
fn parse_single_key(data: &[u8]) -> Key {
    // 至少需要 2 字节：length + type_byte
    if data.len() < 2 {
        return Key::Raw(data.to_vec());
    }

    let payload_len = data[0] as usize; // length field
    let type_byte = data[1];

    // 高位 bit (5-7) 必须为 0，否则可能是未知格式
    if (type_byte & 0b1110_0000) != 0 {
        return Key::Raw(data.to_vec());
    }

    let exist_read = get_bit(type_byte, 0);
    let exist_single = get_bit(type_byte, 1);
    let exist_collection = get_bit(type_byte, 2);
    let exist_illust = get_bit(type_byte, 3);
    let exist_avatar = get_bit(type_byte, 4);

    // 预期的字段数 = 存在的 bit 数
    let expected_fields = usize::from(exist_read)
        + usize::from(exist_single)
        + usize::from(exist_collection)
        + usize::from(exist_illust)
        + usize::from(exist_avatar);

    // payload 长度 = length 自身(1) + 字段数据
    let field_data_len = payload_len.saturating_sub(1);
    let data_start = 2; // 跳过 length + type_byte
    if data.len() < data_start + field_data_len || expected_fields != field_data_len {
        return Key::Raw(data.to_vec());
    }

    let field_data = &data[data_start..data_start + field_data_len];
    let mut idx = 0;

    let mut key = NormalKey::default();

    if exist_read {
        key.read_collection_piece_num = Some(field_data[idx]);
        idx += 1;
    }
    if exist_single {
        key.unlock_single = Some(field_data[idx] == 1);
        idx += 1;
    }
    if exist_collection {
        key.unlock_collection_piece_num = Some(field_data[idx]);
        idx += 1;
    }
    if exist_illust {
        key.unlock_illustration = Some(field_data[idx] == 1);
        idx += 1;
    }
    if exist_avatar {
        key.unlock_avatar = Some(field_data[idx] == 1);
    }

    Key::Normal(key)
}

fn parse_game_key_map(reader: &mut Reader) -> Result<BTreeMap<String, Key>> {
    let length = reader.read_varshort()?;
    let mut map = BTreeMap::new();
    for _ in 0..length {
        let key_name = reader.read_owned_string(0)?;
        let first_len = reader.read_u8()? as usize;
        let next = reader.offset() + first_len;
        if next > reader.data.len() {
            return Err(CodecError::NotEnoughData);
        }
        let entry_data = &reader.data[reader.offset()..next];
        let parsed = parse_single_key(entry_data);
        map.insert(key_name, parsed);
        reader.skip(next.saturating_sub(reader.offset()));
    }
    Ok(map)
}

/// 解析 gameKey entry
///
/// # Errors
///
/// 当数据不足或解析过程中出现格式错误时返回 `CodecError`。
pub fn parse_game_key_entry(entry: &[u8]) -> Result<GameKeyParsed> {
    if entry.is_empty() {
        return Err(CodecError::NotEnoughData);
    }
    let mut r = Reader::new(entry);
    let version = r.read_u8()?;
    let keys = parse_game_key_map(&mut r)?;

    let lanota_read_keys = if version >= 1 {
        Some(r.read_u8()?)
    } else {
        None
    };

    let camellia_read_key = if version >= 2 {
        if r.remain() > 0 {
            Some((r.read_u8()? & 1) != 0)
        } else {
            None
        }
    } else {
        None
    };

    let side_story4_begin_read_key = if version >= 3 && r.offset() < entry.len() {
        Some((r.read_u8()? & 1) != 0)
    } else {
        None
    };

    let old_score_cleared_v390 = if version >= 3 && r.offset() < entry.len() {
        Some((r.read_u8()? & 1) != 0)
    } else {
        None
    };

    let overflow = if r.offset() < entry.len() {
        Some(base64::engine::general_purpose::STANDARD.encode(&entry[r.offset()..]))
    } else {
        None
    };

    Ok(GameKeyParsed {
        version,
        keys,
        lanota_read_keys,
        camellia_read_key,
        side_story4_begin_read_key,
        old_score_cleared_v390,
        overflow,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    #[test]
    fn parse_single_key_malformed_returns_raw() {
        // 只有 1 字节，不足 2 字节 -> Raw
        let data = vec![0x01];
        let result = parse_single_key(&data);
        assert!(matches!(result, Key::Raw(_)));
    }

    #[test]
    fn parse_single_key_high_bits_set_returns_raw() {
        // type_byte 高位 (5-7) 不为 0 -> 未知格式 -> Raw
        let data = vec![0x02, 0b0010_0000];
        let result = parse_single_key(&data);
        assert!(matches!(result, Key::Raw(_)));
    }

    #[test]
    fn parse_single_key_length_mismatch_returns_raw() {
        // 声明了 2 字节 payload 但 type_byte 只有 1 个 bit -> 不匹配 -> Raw
        let data = vec![0x03, 0b0000_0001, 0x01, 0x02];
        let result = parse_single_key(&data);
        assert!(matches!(result, Key::Raw(_)));
    }

    #[test]
    fn parse_single_key_normal_read_collection() {
        // length=2, type=bit0(read), data=[42]
        let data = vec![0x02, 0b0000_0001, 42];
        let result = parse_single_key(&data);
        match result {
            Key::Normal(nk) => {
                assert_eq!(nk.read_collection_piece_num, Some(42));
                assert!(nk.unlock_single.is_none());
            }
            _ => panic!("expected Normal"),
        }
    }

    #[test]
    fn parse_single_key_normal_unlock_single() {
        // length=2, type=bit1(single), data=[1=true]
        let data = vec![0x02, 0b0000_0010, 1];
        let result = parse_single_key(&data);
        match result {
            Key::Normal(nk) => {
                assert_eq!(nk.unlock_single, Some(true));
                assert!(nk.read_collection_piece_num.is_none());
            }
            _ => panic!("expected Normal"),
        }
    }

    #[test]
    fn parse_single_key_normal_unlock_single_false() {
        let data = vec![0x02, 0b0000_0010, 0];
        let result = parse_single_key(&data);
        match result {
            Key::Normal(nk) => {
                assert_eq!(nk.unlock_single, Some(false));
            }
            _ => panic!("expected Normal"),
        }
    }

    #[test]
    fn parse_single_key_normal_all_fields() {
        // bit0..4 全部设置, data = 5 bytes
        let data = vec![
            0x06,        // length (1 type + 5 data)
            0b0001_1111, // all 5 bits
            10,
            1,
            20,
            0,
            1, // read=10, single=true, collection=20, illust=false, avatar=true
        ];
        let result = parse_single_key(&data);
        match result {
            Key::Normal(nk) => {
                assert_eq!(nk.read_collection_piece_num, Some(10));
                assert_eq!(nk.unlock_single, Some(true));
                assert_eq!(nk.unlock_collection_piece_num, Some(20));
                assert_eq!(nk.unlock_illustration, Some(false));
                assert_eq!(nk.unlock_avatar, Some(true));
            }
            _ => panic!("expected Normal"),
        }
    }

    #[test]
    fn parse_game_key_entry_minimal() {
        // version=0, song_count=0, no trailing bytes
        let data = vec![0x00, 0x00];
        let parsed = parse_game_key_entry(&data).expect("should parse");
        assert_eq!(parsed.version, 0);
        assert!(parsed.keys.is_empty());
        assert!(parsed.lanota_read_keys.is_none());
        assert!(parsed.overflow.is_none());
    }

    #[test]
    fn parse_game_key_entry_v1_with_lanota() {
        // version=1, keys=[], lanota_read_keys=0b00111111, overflow=none
        let data = vec![0x01, 0x00, 0b0011_1111];
        // 刚好 3 字节，没有 overflow
        let parsed = parse_game_key_entry(&data).expect("should parse");
        assert_eq!(parsed.version, 1);
        assert_eq!(parsed.lanota_read_keys, Some(0b0011_1111));
        assert!(parsed.overflow.is_none());
    }
}
