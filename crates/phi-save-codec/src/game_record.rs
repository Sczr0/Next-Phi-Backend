use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use serde::{Deserialize, Serialize};

use crate::error::{CodecError, Result};
use crate::reader::Reader;
use crate::types::{Difficulty, DifficultyRecord};

/// 单首歌曲各难度的成绩记录
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SongLevelRecord {
    pub ez: Option<LevelRecord>,
    pub hd: Option<LevelRecord>,
    #[serde(rename = "in")]
    pub r#in: Option<LevelRecord>,
    pub at: Option<LevelRecord>,
}

/// 单一难度的成绩数据
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct LevelRecord {
    pub score: u32,
    pub acc: f32,
    pub fc: bool,
}

/// 直接从解密后的 gameRecord 二进制 entry 解析结构化成绩
///
/// 健壮性设计：
/// - 单首歌解析失败时静默跳过（不影响其他歌曲的结果）
/// - 使用 payload length 校验检测数据损坏
///
/// # Errors
///
/// 数据不足时返回 `CodecError::NotEnoughData`，数据格式异常时返回 `CodecError::InvalidData`。
///
/// 输入为完整 entry（包含第 1 字节前缀）
pub fn parse_game_record_bytes(
    game_record_entry: &[u8],
    chart_lookup: impl Fn(&str, Difficulty) -> Option<f32>,
) -> Result<BTreeMap<String, Vec<DifficultyRecord>>> {
    if game_record_entry.is_empty() {
        return Err(CodecError::NotEnoughData);
    }

    // gameRecord 的第 1 字节为 prefix，实际 map 从后续字节开始
    let mut reader = Reader::new(&game_record_entry[1..]);
    let length = reader.read_varshort()?;
    let mut result = BTreeMap::new();

    for _ in 0..length {
        let Ok(song_id) = reader.read_owned_string(2) else { continue }; // 跳过损坏的 song_id
        let Ok(first_len) = reader.read_u8() else { continue };
        let first_len = first_len as usize;
        let payload_start = reader.offset(); // 从 payload 开始计数
        let next = payload_start
            .checked_add(first_len)
            .ok_or(CodecError::InvalidData)?;
        if next > reader.data.len() {
            break;
        }

        let Ok(mask) = reader.read_u8() else { continue };
        let Ok(fc_mask) = reader.read_u8() else { continue };
        let mut records = Vec::with_capacity(4);
        let mut parse_ok = true;

        for idx in 0..4usize {
            if ((mask >> idx) & 1) == 0 {
                continue;
            }

            let Ok(score_i32) = reader.read_i32_le() else {
                parse_ok = false;
                break;
            };
            if i64::from(score_i32) <= 0 {
                // score 为 0 表示无成绩，但 acc 字段仍在流中，需要跳过
                let _ = reader.read_f32_le();
                continue;
            }
            let parsed_score = u32::try_from(score_i32).unwrap_or(0);

            let Ok(mut accuracy) = reader.read_f32_le() else {
                parse_ok = false;
                break;
            };
            if !accuracy.is_finite() {
                accuracy = 0.0;
            }
            let is_full_combo = ((fc_mask >> idx) & 1) != 0;
            let Ok(difficulty) = Difficulty::try_from(idx) else {
                parse_ok = false;
                break;
            };
            let chart_constant = chart_lookup(&song_id, difficulty);

            records.push(DifficultyRecord {
                difficulty,
                score: parsed_score,
                accuracy,
                is_full_combo,
                chart_constant,
                push_acc: None,
            });
        }

        if !parse_ok {
            // 跳过这首损坏的歌，不影响其他歌
            reader.skip(next.saturating_sub(reader.offset()));
            continue;
        }

        // 校验 payload 长度：已消耗的字节数不应超过 first_len
        let consumed = reader.offset().saturating_sub(payload_start);
        if consumed > first_len {
            // 读超了，说明格式异常，跳过这首
            reader.skip(next.saturating_sub(reader.offset()));
            continue;
        }
        // 如果读了少于 first_len，可能后面还有未解析的数据就跳过了

        reader.skip(next.saturating_sub(reader.offset()));
        result.insert(song_id, records);
    }

    Ok(result)
}

/// 从 `serde_json` Value 解析 gameRecord JSON 格式
///
/// # Errors
///
/// 当 JSON 结构不符合预期（类型错误、字段缺失等）时返回描述错误的 `String`。
pub fn parse_game_record_json(
    record_value: &serde_json::Value,
    chart_lookup: impl Fn(&str, Difficulty) -> Option<f32>,
) -> core::result::Result<BTreeMap<String, Vec<DifficultyRecord>>, String> {
    let obj = record_value
        .as_object()
        .ok_or_else(|| "gameRecord must be a JSON object".to_string())?;

    let mut result = BTreeMap::new();

    for (song_id, scores_value) in obj {
        let arr = scores_value
            .as_array()
            .ok_or_else(|| format!("scores for '{song_id}' must be a JSON array"))?;

        let mut records: Vec<DifficultyRecord> = Vec::with_capacity(4);

        for (idx, chunk) in arr.chunks(3).enumerate() {
            if chunk.len() < 3 {
                break;
            }

            let score_i64 = chunk[0]
                .as_i64()
                .ok_or_else(|| format!("score at '{song_id}'[{idx}] is not an integer"))?;
            if score_i64 <= 0 {
                continue;
            }
            let parsed_score = u32::try_from(score_i64)
                .map_err(|_| format!("score overflow at '{song_id}'[{idx}]"))?;

            let accuracy_f64 = chunk[1]
                .as_f64()
                .or_else(|| chunk[1].as_i64().map(|i| i as f64))
                .ok_or_else(|| format!("accuracy at '{song_id}'[{idx}] is not a number"))?;
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_precision_loss
            )]
            let accuracy_f32 = accuracy_f64 as f32;

            let is_full_combo = match chunk[2].as_i64() {
                Some(1) => true,
                Some(0) => false,
                Some(other) => other != 0,
                None => return Err(format!("fc at '{song_id}'[{idx}] is not an integer")),
            };

            let difficulty = Difficulty::try_from(idx)
                .map_err(|_| format!("invalid difficulty index {idx} for '{song_id}'"))?;
            let chart_constant = chart_lookup(song_id, difficulty);

            records.push(DifficultyRecord {
                difficulty,
                score: parsed_score,
                accuracy: accuracy_f32,
                is_full_combo,
                chart_constant,
                push_acc: None,
            });
        }

        result.insert(song_id.clone(), records);
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::format;
    use alloc::string::ToString;
    use alloc::vec;

    /// 辅助函数：构造一条歌曲在 gameRecord 中的完整二进制数据块
    fn build_song_chunk(song_id: &str, mask: u8, fc_mask: u8, scores: &[(u32, f32)]) -> Vec<u8> {
        let mut chunk = Vec::new();
        // song_id: varshort(len) + id + 2 trim bytes
        let key_full = format!("{}__", song_id);
        push_varshort(&mut chunk, key_full.len());
        chunk.extend_from_slice(key_full.as_bytes());

        // payload length = scores*8 + 2(mask+fc)
        let payload_len = (scores.len() * 8 + 2) as u8;
        chunk.push(payload_len);

        chunk.push(mask);
        chunk.push(fc_mask);

        for &(score, acc) in scores {
            chunk.extend_from_slice(&(score as i32).to_le_bytes());
            chunk.extend_from_slice(&acc.to_le_bytes());
        }

        chunk
    }

    fn build_entry(songs: Vec<Vec<u8>>) -> Vec<u8> {
        let mut entry = Vec::new();
        entry.push(0u8); // prefix
        push_varshort(&mut entry, songs.len());
        for song in songs {
            entry.extend_from_slice(&song);
        }
        entry
    }

    fn push_varshort(buf: &mut Vec<u8>, v: usize) {
        if v < 0x80 {
            buf.push(v as u8);
        } else {
            buf.push(((v & 0x7F) | 0x80) as u8);
            buf.push(((v >> 7) & 0xFF) as u8);
        }
    }

    fn noop_chart_lookup(_: &str, _: Difficulty) -> Option<f32> {
        None
    }

    #[test]
    fn empty_entry_returns_err() {
        assert!(parse_game_record_bytes(&[], noop_chart_lookup).is_err());
    }

    #[test]
    fn single_song_parsed() {
        let chunk = build_song_chunk("song1", 0b0001, 0b0001, &[(1_000_000, 100.0)]);
        let entry = build_entry(vec![chunk]);
        let result = parse_game_record_bytes(&entry, noop_chart_lookup).expect("should parse");
        let recs = result.get("song1").expect("song1 exists");
        assert_eq!(recs.len(), 1);
        assert_eq!(recs[0].difficulty, Difficulty::EZ);
        assert_eq!(recs[0].score, 1_000_000);
        assert!(recs[0].is_full_combo);
    }

    #[test]
    fn corrupted_song_skipped_gracefully() {
        // 构造：第一首歌正常，第二首歌数据截断
        let chunk1 = build_song_chunk("song1", 0b0001, 0b0001, &[(1_000_000, 100.0)]);

        // 第二首：截断（payload_len 说 10 字节但只给了 1 字节）
        let mut chunk2 = Vec::new();
        let key2 = "song2__";
        push_varshort(&mut chunk2, key2.len());
        chunk2.extend_from_slice(key2.as_bytes());
        chunk2.push(10u8); // fake payload_len
        chunk2.push(0b0001);
        // 故意截断：不给 fc_mask + score + acc

        let entry = build_entry(vec![chunk1, chunk2]);

        let result = parse_game_record_bytes(&entry, noop_chart_lookup).expect("should not fail");
        // 第一首应该在，第二首被跳过
        assert!(result.contains_key("song1"), "song1 should be present");
        // 第二首可能被完全跳过或在读取时出错被跳过
        assert!(!result.contains_key("song2"), "song2 should be skipped");
    }

    #[test]
    fn all_corrupted_returns_empty_not_error() {
        // 只有 1 字节 prefix + 0 首歌
        let entry = vec![0u8, 0u8];
        let result = parse_game_record_bytes(&entry, noop_chart_lookup).expect("should parse");
        assert!(result.is_empty());
    }

    #[test]
    fn two_songs_both_nonzero_score() {
        // 两首歌：EZ(score 900k), HD(score 950k) 都有成绩
        let chunk = build_song_chunk("s", 0b0011, 0b0000, &[(900_000, 90.0), (950_000, 95.0)]);
        let entry = build_entry(vec![chunk]);
        let result = parse_game_record_bytes(&entry, noop_chart_lookup).expect("should parse");
        let recs = result.get("s").expect("s exists");
        assert_eq!(recs.len(), 2);
        assert_eq!(recs[0].difficulty, Difficulty::EZ);
        assert_eq!(recs[0].score, 900_000);
        assert_eq!(recs[1].difficulty, Difficulty::HD);
        assert_eq!(recs[1].score, 950_000);
    }

    #[test]
    fn zero_score_skips_that_difficulty() {
        let chunk = build_song_chunk("s", 0b0011, 0b0000, &[(0, 0.0), (950_000, 95.0)]);
        let entry = build_entry(vec![chunk]);
        let result = parse_game_record_bytes(&entry, noop_chart_lookup).expect("should parse");
        let recs = result.get("s").expect("s exists");
        // EZ (index 0) 的 score=0 被跳过，只有 HD (index 1) 留下
        assert_eq!(recs.len(), 1);
        assert_eq!(recs[0].difficulty, Difficulty::HD);
        assert_eq!(recs[0].score, 950_000);
    }
}
