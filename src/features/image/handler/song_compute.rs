use std::{collections::HashMap, path::PathBuf, sync::Arc};

use chrono::{DateTime, Utc};

use crate::{
    error::AppError,
    features::image::renderer::SongDifficultyScore,
    save_contract::ParsedSave,
    startup::chart_loader::{ChartConstants, ChartConstantsMap},
};

use super::{
    display::parse_update_time_or_now,
    score::{
        ALL_DIFFICULTIES, build_engine_records_from_game_record, chart_constant_for_difficulty,
        difficulty_index, difficulty_label, find_song_engine_record_indices,
        index_song_difficulty_records, sort_engine_records_by_rks_desc,
    },
};

pub(super) struct SongComputeInput {
    pub(super) parsed: ParsedSave,
    pub(super) chart_constants: Arc<ChartConstantsMap>,
    pub(super) song_id: String,
    pub(super) song_chart_constants: ChartConstants,
}

pub(super) struct SongComputeOutput {
    pub(super) difficulty_scores: HashMap<String, Option<SongDifficultyScore>>,
    pub(super) illustration_path: Option<PathBuf>,
    pub(super) update_time: DateTime<Utc>,
}

#[allow(clippy::unnecessary_wraps)]
pub(super) fn build_song_compute_output(
    input: SongComputeInput,
) -> Result<SongComputeOutput, AppError> {
    let SongComputeInput {
        parsed,
        chart_constants,
        song_id,
        song_chart_constants,
    } = input;

    // 构建所有引擎记录用于推分
    let mut engine_all =
        build_engine_records_from_game_record(&parsed.game_record, &chart_constants);
    sort_engine_records_by_rks_desc(&mut engine_all);
    let song_engine_indices = find_song_engine_record_indices(&engine_all, &song_id);
    let push_solver = crate::rks_contract::engine::PushAccBatchSolver::new(&engine_all);

    // 单曲四难度数据
    let mut difficulty_scores: HashMap<String, Option<SongDifficultyScore>> =
        HashMap::with_capacity(ALL_DIFFICULTIES.len());
    let song_records = parsed
        .game_record
        .get(&song_id)
        .map_or(&[][..], std::vec::Vec::as_slice);
    let song_record_indices = index_song_difficulty_records(song_records);

    for difficulty in ALL_DIFFICULTIES {
        let diff = difficulty_label(difficulty);
        let dv = chart_constant_for_difficulty(Some(&song_chart_constants), difficulty);
        let rec = song_record_indices[difficulty_index(difficulty)];
        let (score, acc, rks, is_fc) = if let Some(r) = rec {
            let ap = f64::from(r.accuracy);
            let rks = dv.map(|v| crate::rks_contract::engine::calculate_chart_rks(ap, v));
            (
                Some(f64::from(r.score)),
                Some(ap),
                rks,
                Some(r.is_full_combo),
            )
        } else {
            (None, None, None, None)
        };

        // 推分 acc：区分“无法推分/只能推到100/需要具体ACC”
        let player_push_acc = if let (Some(dv), Some(a)) = (dv, acc) {
            if a >= 100.0 || dv <= 0.0 {
                None
            } else {
                song_engine_indices[difficulty_index(difficulty)]
                    .and_then(|i| push_solver.solve_for_index(i, dv))
            }
        } else {
            None
        };

        difficulty_scores.insert(
            diff.to_string(),
            Some(SongDifficultyScore {
                score,
                acc,
                rks,
                difficulty_value: dv,
                is_fc,
                is_phi: acc.map(|a| a >= 100.0),
                player_push_acc,
            }),
        );
    }

    // 插画路径
    let ill_png = super::super::cover_loader::covers_dir()
        .join("ill")
        .join(format!("{song_id}.png"));
    let ill_jpg = super::super::cover_loader::covers_dir()
        .join("ill")
        .join(format!("{song_id}.jpg"));
    let illustration_path = if ill_png.exists() {
        Some(ill_png)
    } else if ill_jpg.exists() {
        Some(ill_jpg)
    } else {
        None
    };

    let update_time = parse_update_time_or_now(parsed.updated_at.as_deref());

    Ok(SongComputeOutput {
        difficulty_scores,
        illustration_path,
        update_time,
    })
}
