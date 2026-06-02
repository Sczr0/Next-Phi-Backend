//! gameRecord 二进制格式解析 — 委托给 phi-save-codec crate

use std::collections::HashMap;

use crate::features::save::models::{Difficulty, DifficultyRecord};
use crate::startup::chart_loader::ChartConstantsMap;

fn convert_records(
    records: HashMap<String, Vec<phi_save_codec::DifficultyRecord>>,
) -> HashMap<String, Vec<DifficultyRecord>> {
    records
        .into_iter()
        .map(|(song_id, recs)| {
            (
                song_id,
                recs.into_iter()
                    .map(|r| DifficultyRecord {
                        difficulty: Difficulty::from(r.difficulty),
                        score: r.score,
                        accuracy: r.accuracy,
                        is_full_combo: r.is_full_combo,
                        chart_constant: r.chart_constant,
                        push_acc: r.push_acc,
                        push_acc_hint: None,
                    })
                    .collect(),
            )
        })
        .collect()
}

/// 直接从解密后的 gameRecord 二进制 entry 解析结构化成绩
///
/// 注意：此函数需要 ChartConstantsMap 来查找定数，
/// 是主项目对 codec 库的适配层。
pub fn parse_game_record_bytes(
    game_record_entry: &[u8],
    chart_constants: &ChartConstantsMap,
) -> Result<HashMap<String, Vec<DifficultyRecord>>, String> {
    let result: HashMap<_, _> =
        phi_save_codec::parse_game_record_bytes(game_record_entry, |song_id, diff| {
            chart_constants.get(song_id).and_then(|c| match diff {
                phi_save_codec::Difficulty::EZ => c.ez,
                phi_save_codec::Difficulty::HD => c.hd,
                phi_save_codec::Difficulty::IN => c.in_level,
                phi_save_codec::Difficulty::AT => c.at,
            })
        })
        .map_err(|e| e.to_string())?
        .into_iter()
        .collect();

    Ok(convert_records(result))
}

/// 从 serde_json Value 解析 gameRecord（旧 JSON 格式）
pub fn parse_game_record(
    record_value: &serde_json::Value,
    chart_constants: &ChartConstantsMap,
) -> Result<HashMap<String, Vec<DifficultyRecord>>, String> {
    let result: HashMap<_, _> =
        phi_save_codec::parse_game_record_json(record_value, |song_id, diff| {
            chart_constants.get(song_id).and_then(|c| match diff {
                phi_save_codec::Difficulty::EZ => c.ez,
                phi_save_codec::Difficulty::HD => c.hd,
                phi_save_codec::Difficulty::IN => c.in_level,
                phi_save_codec::Difficulty::AT => c.at,
            })
        })
        .map_err(|e| e.to_string())?
        .into_iter()
        .collect();

    Ok(convert_records(result))
}
