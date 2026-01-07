use std::collections::HashMap;

use serde_json::Value;

use super::models::{Difficulty, DifficultyRecord};
use crate::startup::chart_loader::ChartConstantsMap;

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

    let mut result: HashMap<String, Vec<DifficultyRecord>> = HashMap::new();

    for (song_id, scores_value) in obj.iter() {
        let arr = scores_value
            .as_array()
            .ok_or_else(|| format!("scores for '{song_id}' must be a JSON array"))?;

        let mut records: Vec<DifficultyRecord> = Vec::new();

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
            let chart_constant = chart_constants
                .get(song_id)
                .and_then(|consts| match difficulty {
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
