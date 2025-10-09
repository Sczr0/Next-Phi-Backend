use base64::{Engine as _, engine::general_purpose};
use serde_json::{Number, Value};
use std::collections::HashMap;

use crate::error::SaveProviderError;

#[derive(Clone, Copy)]
enum NodeType {
    Bool,
    U8,
    U16,
    Float,
    Str,
    VarShort,
    U16Array,
}

struct LeafNode {
    typ: NodeType,
    name: &'static str,
}

const GAMEKEY1: &[LeafNode] = &[LeafNode {
    typ: NodeType::U8,
    name: "lanotaReadKeys",
}];
const GAMEKEY2: &[LeafNode] = &[LeafNode {
    typ: NodeType::Bool,
    name: "camelliaReadKey",
}];

const GAMEPROGRESS1: &[LeafNode] = &[
    LeafNode {
        typ: NodeType::Bool,
        name: "isFirstRun",
    },
    LeafNode {
        typ: NodeType::Bool,
        name: "legacyChapterFinished",
    },
    LeafNode {
        typ: NodeType::Bool,
        name: "alreadyShowCollectionTip",
    },
    LeafNode {
        typ: NodeType::Bool,
        name: "alreadyShowAutoUnlockINTip",
    },
    LeafNode {
        typ: NodeType::Str,
        name: "completed",
    },
    LeafNode {
        typ: NodeType::U8,
        name: "songUpdateInfo",
    },
    LeafNode {
        typ: NodeType::U16,
        name: "challengeModeRank",
    },
    LeafNode {
        typ: NodeType::VarShort,
        name: "money",
    },
    LeafNode {
        typ: NodeType::U8,
        name: "unlockFlagOfSpasmodic",
    },
    LeafNode {
        typ: NodeType::U8,
        name: "unlockFlagOfIgallta",
    },
    LeafNode {
        typ: NodeType::U8,
        name: "unlockFlagOfRrharil",
    },
    LeafNode {
        typ: NodeType::U8,
        name: "flagOfSongRecordKey",
    },
];

const GAMEPROGRESS2: &[LeafNode] = &[LeafNode {
    typ: NodeType::U8,
    name: "randomVersionUnlocked",
}];

const GAMEPROGRESS3: &[LeafNode] = &[
    LeafNode {
        typ: NodeType::Bool,
        name: "chapter8UnlockBegin",
    },
    LeafNode {
        typ: NodeType::Bool,
        name: "chapter8UnlockSecondPhase",
    },
    LeafNode {
        typ: NodeType::Bool,
        name: "chapter8Passed",
    },
    LeafNode {
        typ: NodeType::U8,
        name: "chapter8SongUnlocked",
    },
];

const USER_NODES: &[LeafNode] = &[
    LeafNode {
        typ: NodeType::Bool,
        name: "showPlayerId",
    },
    LeafNode {
        typ: NodeType::Str,
        name: "selfIntro",
    },
    LeafNode {
        typ: NodeType::Str,
        name: "avatar",
    },
    LeafNode {
        typ: NodeType::Str,
        name: "background",
    },
];

const SETTINGS_NODES: &[LeafNode] = &[
    LeafNode {
        typ: NodeType::Bool,
        name: "chordSupport",
    },
    LeafNode {
        typ: NodeType::Bool,
        name: "fcAPIndicator",
    },
    LeafNode {
        typ: NodeType::Bool,
        name: "enableHitSound",
    },
    LeafNode {
        typ: NodeType::Bool,
        name: "lowResolutionMode",
    },
    LeafNode {
        typ: NodeType::Str,
        name: "deviceName",
    },
    LeafNode {
        typ: NodeType::Float,
        name: "bright",
    },
    LeafNode {
        typ: NodeType::Float,
        name: "musicVolume",
    },
    LeafNode {
        typ: NodeType::Float,
        name: "effectVolume",
    },
    LeafNode {
        typ: NodeType::Float,
        name: "hitSoundVolume",
    },
    LeafNode {
        typ: NodeType::Float,
        name: "soundOffset",
    },
    LeafNode {
        typ: NodeType::Float,
        name: "noteScale",
    },
];

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

fn deser_object(
    reader: &mut Reader,
    nodes: &[LeafNode],
) -> Result<serde_json::Map<String, Value>, SaveProviderError> {
    let mut obj = serde_json::Map::new();
    let mut bit: u8 = 0;
    let mut bool_byte_pos = reader.off;
    for nd in nodes {
        match nd.typ {
            NodeType::Bool => {
                if bit == 0 {
                    bool_byte_pos = reader.off;
                    if reader.remain() < 1 {
                        return Err(SaveProviderError::Decrypt("EOF bool".into()));
                    }
                }
                let b = reader.data[bool_byte_pos];
                let val = ((b >> bit) & 1) != 0;
                obj.insert(nd.name.to_string(), Value::Bool(val));
                bit = bit.wrapping_add(1);
                if bit == 8 {
                    bit = 0;
                    reader.off = bool_byte_pos + 1;
                }
            }
            _ => {
                if bit != 0 {
                    reader.off = bool_byte_pos + 1;
                    bit = 0;
                }
                match nd.typ {
                    NodeType::U8 => {
                        let v = reader.read_u8()? as i64;
                        obj.insert(nd.name.to_string(), Value::Number(Number::from(v)));
                    }
                    NodeType::U16 => {
                        let v = reader.read_u16_le()? as i64;
                        obj.insert(nd.name.to_string(), Value::Number(Number::from(v)));
                    }
                    NodeType::Float => {
                        let v = reader.read_f32_le()? as f64;
                        obj.insert(
                            nd.name.to_string(),
                            Value::Number(Number::from_f64(v).unwrap_or_else(|| Number::from(0))),
                        );
                    }
                    NodeType::Str => {
                        let s = reader.read_string(0)?;
                        obj.insert(nd.name.to_string(), Value::String(s));
                    }
                    NodeType::VarShort => {
                        let mut arr = Vec::with_capacity(5);
                        for _ in 0..5 {
                            arr.push(Value::Number(Number::from(reader.read_varshort()? as i64)));
                        }
                        obj.insert(nd.name.to_string(), Value::Array(arr));
                    }
                    NodeType::U16Array => {
                        let mut arr = Vec::with_capacity(12);
                        for _ in 0..12 {
                            arr.push(Value::Number(Number::from(reader.read_u16_le()? as i64)));
                        }
                        obj.insert(nd.name.to_string(), Value::Array(arr));
                    }
                    NodeType::Bool => unreachable!(),
                }
            }
        }
    }
    if bit != 0 {
        reader.off = bool_byte_pos + 1;
    }
    Ok(obj)
}

fn deser_nodes_into(
    obj: &mut serde_json::Map<String, Value>,
    reader: &mut Reader,
    groups: &[&[LeafNode]],
) -> Result<(), SaveProviderError> {
    let version = obj.get("version").and_then(|v| v.as_i64()).unwrap_or(0) as usize;
    for i in 0..version.min(groups.len()) {
        let sub = deser_object(reader, groups[i])?;
        for (k, v) in sub {
            obj.insert(k, v);
        }
    }
    Ok(())
}

fn deser_map(reader: &mut Reader, end: u8) -> Result<Value, SaveProviderError> {
    let mut map = serde_json::Map::new();
    let length = reader.read_varshort()?;
    for _ in 0..length {
        let key = reader.read_string(end as usize)?;
        if reader.remain() < 1 {
            return Err(SaveProviderError::Decrypt("EOF map len".into()));
        }
        let first_len = reader.data[reader.off] as usize;
        let next = reader.off + 1 + first_len;
        reader.off += 1;
        let mut arr = Vec::new();
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

pub fn parse_save_to_json(entries: &HashMap<String, Vec<u8>>) -> Result<Value, SaveProviderError> {
    let mut root = serde_json::Map::new();
    if let Some(gr) = entries.get("gameRecord") {
        if gr.is_empty() {
            return Err(SaveProviderError::Decrypt("gameRecord 太短".into()));
        }
        let mut r = Reader::new(&gr[1..]);
        let obj = deser_map(&mut r, 2)?;
        root.insert("gameRecord".to_string(), obj);
    }
    if let Some(gk) = entries.get("gameKey") {
        if gk.is_empty() {
            return Err(SaveProviderError::Decrypt("gameKey 太短".into()));
        }
        let mut r = Reader::new(gk);
        let ver = r.read_u8()?;
        let mut obj = serde_json::Map::new();
        obj.insert(
            "version".to_string(),
            Value::Number(Number::from(ver as i64)),
        );
        obj.insert("map".to_string(), deser_map(&mut r, 0)?);
        deser_nodes_into(&mut obj, &mut r, &[GAMEKEY1, GAMEKEY2])?;
        if r.off < gk.len() {
            obj.insert(
                "overflow".to_string(),
                Value::String(general_purpose::STANDARD.encode(&gk[r.off..])),
            );
        }
        root.insert("gameKey".to_string(), Value::Object(obj));
    }
    if let Some(gp) = entries.get("gameProgress") {
        if gp.is_empty() {
            return Err(SaveProviderError::Decrypt("gameProgress 太短".into()));
        }
        let mut r = Reader::new(gp);
        let ver = r.read_u8()?;
        let mut obj = serde_json::Map::new();
        obj.insert(
            "version".to_string(),
            Value::Number(Number::from(ver as i64)),
        );
        deser_nodes_into(
            &mut obj,
            &mut r,
            &[GAMEPROGRESS1, GAMEPROGRESS2, GAMEPROGRESS3],
        )?;
        if r.off < gp.len() {
            obj.insert(
                "overflow".to_string(),
                Value::String(general_purpose::STANDARD.encode(&gp[r.off..])),
            );
        }
        root.insert("gameProgress".to_string(), Value::Object(obj));
    }
    if let Some(usr) = entries.get("user") {
        if usr.is_empty() {
            return Err(SaveProviderError::Decrypt("user 太短".into()));
        }
        let mut r = Reader::new(&usr[1..]);
        let obj = deser_object(&mut r, USER_NODES)?;
        root.insert("user".to_string(), Value::Object(obj));
    }
    if let Some(st) = entries.get("settings") {
        if st.is_empty() {
            return Err(SaveProviderError::Decrypt("settings 太短".into()));
        }
        let mut r = Reader::new(&st[1..]);
        let obj = deser_object(&mut r, SETTINGS_NODES)?;
        root.insert("settings".to_string(), Value::Object(obj));
    }
    Ok(Value::Object(root))
}
