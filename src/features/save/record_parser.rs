use std::collections::HashMap;

use serde_json::Value;

use super::models::{Difficulty, DifficultyRecord};
use crate::startup::chart_loader::ChartConstantsMap;

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

    fn read_u8(&mut self) -> Result<u8, String> {
        if self.remain() < 1 {
            return Err("EOF while reading u8".to_string());
        }
        let b = self.data[self.off];
        self.off += 1;
        Ok(b)
    }

    fn read_i32_le(&mut self) -> Result<i32, String> {
        if self.remain() < 4 {
            return Err("EOF while reading i32".to_string());
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

    fn read_f32_le(&mut self) -> Result<f32, String> {
        if self.remain() < 4 {
            return Err("EOF while reading f32".to_string());
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

    fn read_varshort(&mut self) -> Result<usize, String> {
        let b0 = self.read_u8()?;
        if b0 < 0x80 {
            Ok(b0 as usize)
        } else {
            let b1 = self.read_u8()?;
            Ok((((b0 as usize) & 0x7F) ^ ((b1 as usize) << 7)) & 0xFFFF)
        }
    }

    fn read_string_with_end_trim(&mut self, end: usize) -> Result<String, String> {
        let len = self.read_varshort()?;
        if len < end {
            return Err(format!("invalid string length: len={len}, trim={end}"));
        }
        if self.remain() < len {
            return Err("EOF while reading string bytes".to_string());
        }
        let keep_len = len - end;
        let s = &self.data[self.off..self.off + keep_len];
        self.off += len;
        Ok(String::from_utf8_lossy(s).to_string())
    }
}

/// 解析 Phigros 存档中的 gameRecord 部分
/// 输入应为形如 { "song_id": [score, accuracy, fc, score, accuracy, fc, ...], ... } 的 JSON 对象
/// 返回每首歌对应的各难度成绩列表（仅包含有成绩的难度）
pub fn parse_game_record(
    record_value: &Value,
    chart_constants: &ChartConstantsMap,
) -> Result<HashMap<String, Vec<DifficultyRecord>>, String> {
    let obj = record_value
        .as_object()
        .ok_or_else(|| "gameRecord must be a JSON object".to_string())?;

    let mut result: HashMap<String, Vec<DifficultyRecord>> = HashMap::with_capacity(obj.len());

    for (song_id, scores_value) in obj.iter() {
        let arr = scores_value
            .as_array()
            .ok_or_else(|| format!("scores for '{song_id}' must be a JSON array"))?;

        let mut records: Vec<DifficultyRecord> = Vec::with_capacity(4);
        let song_constants = chart_constants.get(song_id);

        for (idx, chunk) in arr.chunks(3).enumerate() {
            if chunk.len() < 3 {
                // 不完整的结尾分组，跳过
                break;
            }

            // score: 整数；当 score <= 0 视为无成绩，跳过
            let score_i64 = chunk[0]
                .as_i64()
                .ok_or_else(|| format!("score at '{song_id}'[{idx}] is not an integer"))?;
            if score_i64 <= 0 {
                continue;
            }
            let score_u32 = u32::try_from(score_i64)
                .map_err(|_| format!("score overflow at '{song_id}'[{idx}]"))?;

            // accuracy: 浮点数
            let accuracy_f32 = if let Some(v) = chunk[1].as_f64() {
                v as f32
            } else if let Some(v) = chunk[1].as_i64() {
                v as f32
            } else {
                return Err(format!("accuracy at '{song_id}'[{idx}] is not a number"));
            };

            // fc: 1 为 true, 0 为 false
            let is_full_combo = match chunk[2].as_i64() {
                Some(1) => true,
                Some(0) => false,
                Some(other) => other != 0,
                None => return Err(format!("fc at '{song_id}'[{idx}] is not an integer")),
            };

            let difficulty = Difficulty::try_from(idx)
                .map_err(|_| format!("invalid difficulty index {idx} for '{song_id}'"))?;

            // 查询定数
            let chart_constant = song_constants.and_then(|consts| match difficulty {
                Difficulty::EZ => consts.ez,
                Difficulty::HD => consts.hd,
                Difficulty::IN => consts.in_level,
                Difficulty::AT => consts.at,
            });

            records.push(DifficultyRecord {
                difficulty,
                score: score_u32,
                accuracy: accuracy_f32,
                is_full_combo,
                chart_constant,
                push_acc: None,
                push_acc_hint: None,
            });
        }

        result.insert(song_id.to_string(), records);
    }

    Ok(result)
}

/// 直接从解密后的 gameRecord 二进制 entry 解析结构化成绩，避免先转 JSON 再二次解析。
///
/// 输入应为完整 entry（包含第 1 字节前缀）。
pub fn parse_game_record_bytes(
    game_record_entry: &[u8],
    chart_constants: &ChartConstantsMap,
) -> Result<HashMap<String, Vec<DifficultyRecord>>, String> {
    if game_record_entry.is_empty() {
        return Err("gameRecord entry is empty".to_string());
    }

    // 与 parser.rs 保持一致：gameRecord 的第 1 字节为 prefix，实际 map 从后续字节开始。
    let mut reader = Reader::new(&game_record_entry[1..]);
    let length = reader.read_varshort()?;
    let mut result: HashMap<String, Vec<DifficultyRecord>> = HashMap::with_capacity(length);

    for _ in 0..length {
        // 与 parser.rs 的 deser_map(end=2) 保持一致：key 尾部裁掉 2 字节。
        let song_id = reader.read_string_with_end_trim(2)?;
        let start = reader.off;
        let first_len = reader.read_u8()? as usize;
        let next = start
            .checked_add(1 + first_len)
            .ok_or_else(|| "gameRecord entry length overflow".to_string())?;
        if next > reader.data.len() {
            return Err("gameRecord entry out of bounds".to_string());
        }

        let mask = reader.read_u8()?;
        let fc_mask = reader.read_u8()?;
        let mut records: Vec<DifficultyRecord> = Vec::with_capacity(4);
        let song_constants = chart_constants.get(&song_id);

        for idx in 0..4usize {
            if ((mask >> idx) & 1) == 0 {
                continue;
            }

            let score_i32 = reader.read_i32_le()?;
            let score_i64 = i64::from(score_i32);
            if score_i64 <= 0 {
                continue;
            }
            let score_u32 = u32::try_from(score_i64)
                .map_err(|_| format!("score overflow for '{song_id}'[{idx}]"))?;

            let mut accuracy = reader.read_f32_le()?;
            if !accuracy.is_finite() {
                accuracy = 0.0;
            }
            let is_full_combo = ((fc_mask >> idx) & 1) != 0;
            let difficulty = Difficulty::try_from(idx)
                .map_err(|_| format!("invalid difficulty index {idx} for '{song_id}'"))?;
            let chart_constant = song_constants.and_then(|consts| match difficulty {
                Difficulty::EZ => consts.ez,
                Difficulty::HD => consts.hd,
                Difficulty::IN => consts.in_level,
                Difficulty::AT => consts.at,
            });

            records.push(DifficultyRecord {
                difficulty,
                score: score_u32,
                accuracy,
                is_full_combo,
                chart_constant,
                push_acc: None,
                push_acc_hint: None,
            });
        }

        reader.off = next;
        result.insert(song_id, records);
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::startup::chart_loader::{ChartConstants, ChartConstantsMap};

    fn push_varshort(buf: &mut Vec<u8>, v: usize) {
        if v < 0x80 {
            buf.push(v as u8);
        } else {
            let b0 = ((v & 0x7F) as u8) | 0x80;
            let b1 = ((v >> 7) & 0xFF) as u8;
            buf.push(b0);
            buf.push(b1);
        }
    }

    #[test]
    fn parse_game_record_bytes_reads_ez_record() {
        let mut entry = Vec::new();
        entry.push(0u8); // prefix
        push_varshort(&mut entry, 1); // one song

        // key: "song" + 2 trim bytes
        let key_full = b"song__";
        push_varshort(&mut entry, key_full.len());
        entry.extend_from_slice(key_full);

        let payload_len = 10u8; // mask + fc + score(i32) + acc(f32)
        entry.push(payload_len);
        entry.push(0b0000_0001); // EZ present
        entry.push(0b0000_0001); // EZ FC
        entry.extend_from_slice(&1_000_000i32.to_le_bytes());
        entry.extend_from_slice(&100.0f32.to_le_bytes());

        let mut chart_constants: ChartConstantsMap = ChartConstantsMap::new();
        chart_constants.insert(
            "song".to_string(),
            ChartConstants {
                ez: Some(9.9),
                hd: None,
                in_level: None,
                at: None,
            },
        );

        let parsed = parse_game_record_bytes(&entry, &chart_constants).expect("parse bytes");
        let recs = parsed.get("song").expect("song exists");
        assert_eq!(recs.len(), 1);
        assert_eq!(recs[0].difficulty, Difficulty::EZ);
        assert_eq!(recs[0].score, 1_000_000);
        assert!(recs[0].is_full_combo);
        assert_eq!(recs[0].chart_constant, Some(9.9));
    }

    #[test]
    fn parse_game_record_bytes_skips_non_positive_score() {
        let mut entry = Vec::new();
        entry.push(0u8); // prefix
        push_varshort(&mut entry, 1); // one song

        let key_full = b"song__";
        push_varshort(&mut entry, key_full.len());
        entry.extend_from_slice(key_full);

        entry.push(10u8);
        entry.push(0b0000_0001); // EZ present
        entry.push(0b0000_0000); // not FC
        entry.extend_from_slice(&0i32.to_le_bytes()); // no score
        entry.extend_from_slice(&98.5f32.to_le_bytes());

        let chart_constants: ChartConstantsMap = ChartConstantsMap::new();
        let parsed = parse_game_record_bytes(&entry, &chart_constants).expect("parse bytes");
        let recs = parsed.get("song").expect("song exists");
        assert!(recs.is_empty());
    }
}
