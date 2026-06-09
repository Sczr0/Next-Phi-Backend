use std::{collections::HashMap, sync::Arc};

use crate::{
    error::AppError,
    features::image::{renderer::RenderRecord, types::UserScoreItem},
    song_contract::SongCatalog,
};

use super::score::{
    build_engine_records_from_render_records, calculate_ap_top_3_avg, calculate_best_27_avg,
    calculate_push_acc_map, chart_constant_for_difficulty, collect_ap_top_3_scores,
    is_user_score_full_combo, parse_user_score_difficulty, sort_render_records_by_rks_desc,
    user_score_difficulty_error,
};

pub(super) struct UserBnComputeOutput {
    pub(super) records: Vec<RenderRecord>,
    pub(super) push_acc_map: HashMap<String, crate::rks_contract::engine::PushAccHint>,
    pub(super) exact_rks: f64,
    pub(super) ap_top_3_avg: Option<f64>,
    pub(super) best_27_avg: Option<f64>,
    pub(super) ap_top_3_scores: Vec<RenderRecord>,
}

#[allow(clippy::needless_pass_by_value)]
pub(super) fn build_user_bn_compute_output(
    scores: Vec<UserScoreItem>,
    song_catalog: Arc<SongCatalog>,
) -> Result<UserBnComputeOutput, AppError> {
    // 解析成绩并计算 RKS
    let mut records: Vec<RenderRecord> = Vec::with_capacity(scores.len());
    let mut song_lookup_cache = HashMap::<String, Arc<_>>::with_capacity(scores.len());
    for (idx, item) in scores.iter().enumerate() {
        // 同一次自报请求内可能多次引用同一首歌，避免重复执行模糊/别名搜索。
        let song_lookup_key = item.song.trim();
        let info = if let Some(info) = song_lookup_cache.get(song_lookup_key) {
            Arc::clone(info)
        } else {
            let info = song_catalog
                .search_unique(song_lookup_key)
                .map_err(AppError::Search)?;
            song_lookup_cache.insert(song_lookup_key.to_string(), Arc::clone(&info));
            info
        };
        let Some((difficulty, difficulty_label)) = parse_user_score_difficulty(&item.difficulty)
        else {
            return Err(user_score_difficulty_error(
                idx,
                &info.name,
                &item.difficulty,
            ));
        };
        // 定数
        let Some(dv) = chart_constant_for_difficulty(Some(&info.chart_constants), difficulty)
        else {
            return Err(user_score_difficulty_error(
                idx,
                &info.name,
                &item.difficulty,
            ));
        };
        // ACC 统一百分比
        let acc = item.acc;
        // RKS
        let rks = crate::rks_contract::engine::calculate_chart_rks(acc, dv);
        records.push(RenderRecord {
            song_id: info.id.clone(),
            song_name: info.name.clone(),
            difficulty: difficulty_label.to_string(),
            score: item.score.map(f64::from),
            acc,
            rks,
            difficulty_value: dv,
            is_fc: is_user_score_full_combo(item.score, acc),
        });
    }

    // 排序、截取 N（按传入成绩数量）
    sort_render_records_by_rks_desc(&mut records);

    // 推分 ACC
    let engine_all = build_engine_records_from_render_records(&records);
    let push_acc_map = calculate_push_acc_map(&records, &engine_all, records.len());

    // 统计项
    let (exact_rks, _rounded) =
        crate::rks_contract::engine::calculate_player_rks_details(&engine_all);
    let ap_top_3_avg = calculate_ap_top_3_avg(&records);
    let best_27_avg = calculate_best_27_avg(&records);
    let ap_top_3_scores = collect_ap_top_3_scores(&records);

    Ok(UserBnComputeOutput {
        records,
        push_acc_map,
        exact_rks,
        ap_top_3_avg,
        best_27_avg,
        ap_top_3_scores,
    })
}
