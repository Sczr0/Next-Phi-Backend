use crate::rks_contract::engine;
use crate::save_contract::Difficulty;

use super::RenderRecord;
use super::math::round_non_negative_to_u32;

pub(super) fn to_engine_difficulty(code: &str) -> Option<Difficulty> {
    match code {
        "EZ" | "ez" => Some(Difficulty::EZ),
        "HD" | "hd" => Some(Difficulty::HD),
        "IN" | "in" => Some(Difficulty::IN),
        "AT" | "at" => Some(Difficulty::AT),
        _ => None,
    }
}

pub(super) fn to_engine_record(record: &RenderRecord) -> Option<engine::RksRecord> {
    let difficulty = to_engine_difficulty(&record.difficulty)?;
    let score = record
        .score
        .map(round_non_negative_to_u32)
        .unwrap_or_default();
    Some(engine::RksRecord {
        song_id: record.song_id.clone(),
        difficulty,
        score,
        acc: record.acc,
        rks: record.rks,
        chart_constant: record.difficulty_value,
    })
}

pub(super) fn calculate_push_acc(
    target_chart_id: &str,
    difficulty_value: f64,
    engine_records: &[engine::RksRecord],
) -> Option<engine::PushAccHint> {
    if engine_records.is_empty() {
        return None;
    }
    let (song_id, diff_str) = target_chart_id.rsplit_once('-')?;
    let diff = if diff_str.eq_ignore_ascii_case("EZ") {
        Difficulty::EZ
    } else if diff_str.eq_ignore_ascii_case("HD") {
        Difficulty::HD
    } else if diff_str.eq_ignore_ascii_case("IN") {
        Difficulty::IN
    } else if diff_str.eq_ignore_ascii_case("AT") {
        Difficulty::AT
    } else {
        return None;
    };
    // 兜底路径：只在上游未预计算推分时使用，因此这里允许 O(N) 定位目标索引。
    let target_index = engine_records
        .iter()
        .position(|r| r.song_id == song_id && r.difficulty == diff)?;
    let solver = engine::PushAccBatchSolver::new(engine_records);
    solver.solve_for_index(target_index, difficulty_value)
}
