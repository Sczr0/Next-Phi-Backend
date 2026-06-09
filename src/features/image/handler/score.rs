use std::collections::HashMap;

use crate::{
    error::AppError,
    features::image::RenderRecord,
    save_contract::{Difficulty, DifficultyRecord},
    startup::chart_loader::ChartConstantsMap,
};

pub(super) const ALL_DIFFICULTIES: [Difficulty; 4] = [
    Difficulty::EZ,
    Difficulty::HD,
    Difficulty::IN,
    Difficulty::AT,
];

pub(super) fn u32_from_usize(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn round_non_negative_to_u32(value: f64) -> u32 {
    if !value.is_finite() {
        return 0;
    }
    value.round().clamp(0.0, f64::from(u32::MAX)) as u32
}

fn to_engine_record(r: &RenderRecord) -> Option<crate::rks_contract::engine::RksRecord> {
    let diff = difficulty_from_canonical_label(r.difficulty.as_str())?;
    Some(crate::rks_contract::engine::RksRecord {
        song_id: r.song_id.clone(),
        difficulty: diff,
        score: round_non_negative_to_u32(r.score.unwrap_or(0.0)),
        acc: r.acc,
        rks: r.rks,
        chart_constant: r.difficulty_value,
    })
}

pub(super) fn build_engine_records_from_render_records(
    records: &[RenderRecord],
) -> Vec<crate::rks_contract::engine::RksRecord> {
    let mut engine_records = Vec::with_capacity(records.len());
    engine_records.extend(records.iter().filter_map(to_engine_record));
    engine_records
}

pub(super) fn difficulty_from_canonical_label(label: &str) -> Option<Difficulty> {
    match label {
        "EZ" => Some(Difficulty::EZ),
        "HD" => Some(Difficulty::HD),
        "IN" => Some(Difficulty::IN),
        "AT" => Some(Difficulty::AT),
        _ => None,
    }
}

pub(super) fn parse_user_score_difficulty(input: &str) -> Option<(Difficulty, &'static str)> {
    let value = input.trim();
    ALL_DIFFICULTIES.iter().copied().find_map(|difficulty| {
        let label = difficulty_label(difficulty);
        value
            .eq_ignore_ascii_case(label)
            .then_some((difficulty, label))
    })
}

pub(super) fn difficulty_label(difficulty: Difficulty) -> &'static str {
    match difficulty {
        Difficulty::EZ => "EZ",
        Difficulty::HD => "HD",
        Difficulty::IN => "IN",
        Difficulty::AT => "AT",
    }
}

pub(super) fn difficulty_index(difficulty: Difficulty) -> usize {
    match difficulty {
        Difficulty::EZ => 0,
        Difficulty::HD => 1,
        Difficulty::IN => 2,
        Difficulty::AT => 3,
    }
}

pub(super) fn chart_constant_for_difficulty(
    chart: Option<&crate::startup::chart_loader::ChartConstants>,
    difficulty: Difficulty,
) -> Option<f64> {
    let chart = chart?;
    match difficulty {
        Difficulty::EZ => chart.ez,
        Difficulty::HD => chart.hd,
        Difficulty::IN => chart.in_level,
        Difficulty::AT => chart.at,
    }
    .map(f64::from)
}

pub(super) fn build_engine_records_from_game_record(
    game_record: &HashMap<String, Vec<DifficultyRecord>>,
    chart_constants: &ChartConstantsMap,
) -> Vec<crate::rks_contract::engine::RksRecord> {
    let total_records = game_record.values().map(Vec::len).sum();
    let mut records = Vec::with_capacity(total_records);
    for (song_id, diffs) in game_record {
        let chart = chart_constants.get(song_id);
        for rec in diffs {
            let Some(chart_constant) = chart_constant_for_difficulty(chart, rec.difficulty) else {
                continue;
            };
            let acc_percent = f64::from(rec.accuracy);
            records.push(crate::rks_contract::engine::RksRecord {
                song_id: song_id.clone(),
                difficulty: rec.difficulty,
                score: rec.score,
                acc: acc_percent,
                rks: crate::rks_contract::engine::calculate_chart_rks(acc_percent, chart_constant),
                chart_constant,
            });
        }
    }
    records
}

pub(super) fn find_song_engine_record_indices(
    engine_all: &[crate::rks_contract::engine::RksRecord],
    song_id: &str,
) -> [Option<usize>; 4] {
    let mut indices = [None; 4];
    for (index, record) in engine_all.iter().enumerate() {
        if record.song_id != song_id {
            continue;
        }
        let slot = difficulty_index(record.difficulty);
        if indices[slot].is_none() {
            indices[slot] = Some(index);
        }
    }
    indices
}

pub(super) fn index_song_difficulty_records(
    records: &[DifficultyRecord],
) -> [Option<&DifficultyRecord>; 4] {
    let mut indices = [None; 4];
    for record in records {
        let slot = difficulty_index(record.difficulty);
        if indices[slot].is_none() {
            indices[slot] = Some(record);
        }
    }
    indices
}

pub(super) fn user_score_difficulty_error(
    index: usize,
    song_name: &str,
    difficulty: &str,
) -> AppError {
    AppError::ImageRendererError(format!(
        "第{}条成绩难度无效或无定数: {} {}",
        index + 1,
        song_name,
        difficulty
    ))
}

pub(super) fn is_user_score_full_combo(score: Option<u32>, acc: f64) -> bool {
    score == Some(1_000_000) || acc >= 100.0
}

pub(super) fn sort_render_records_by_rks_desc(records: &mut [RenderRecord]) {
    records.sort_by(|a, b| {
        b.rks
            .partial_cmp(&a.rks)
            .unwrap_or(core::cmp::Ordering::Equal)
    });
}

pub(super) fn sort_engine_records_by_rks_desc(
    records: &mut [crate::rks_contract::engine::RksRecord],
) {
    records.sort_by(|a, b| {
        b.rks
            .partial_cmp(&a.rks)
            .unwrap_or(core::cmp::Ordering::Equal)
    });
}

pub(super) fn calculate_ap_top_3_avg(records: &[RenderRecord]) -> Option<f64> {
    let mut ap_count = 0usize;
    let mut ap_sum = 0.0;
    for record in records.iter().filter(|record| record.acc >= 100.0).take(3) {
        ap_count += 1;
        ap_sum += record.rks;
    }

    (ap_count == 3).then_some(ap_sum / 3.0)
}

pub(super) fn calculate_best_27_avg(records: &[RenderRecord]) -> Option<f64> {
    if records.is_empty() {
        return None;
    }

    let count = records.len().min(27);
    let sum = records
        .iter()
        .take(27)
        .map(|record| record.rks)
        .sum::<f64>();
    Some(sum / f64::from(u32_from_usize(count)))
}

pub(super) fn collect_ap_top_3_scores(records: &[RenderRecord]) -> Vec<RenderRecord> {
    records
        .iter()
        .filter(|record| record.acc >= 100.0)
        .take(3)
        .cloned()
        .collect()
}

pub(super) fn calculate_push_acc_map(
    records: &[RenderRecord],
    engine_all: &[crate::rks_contract::engine::RksRecord],
    limit: usize,
) -> HashMap<String, crate::rks_contract::engine::PushAccHint> {
    let solver = crate::rks_contract::engine::PushAccBatchSolver::new(engine_all);
    let mut push_acc_map = HashMap::with_capacity(records.len().min(limit));
    for (idx, record) in records.iter().take(limit).enumerate() {
        if record.acc >= 100.0 || record.difficulty_value <= 0.0 {
            continue;
        }
        let key = format!("{}-{}", record.song_id, record.difficulty);
        if let Some(hint) = solver.solve_for_index(idx, record.difficulty_value) {
            push_acc_map.insert(key, hint);
        }
    }
    push_acc_map
}
