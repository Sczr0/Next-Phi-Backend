use std::{collections::HashMap, sync::Arc, time::Instant};

use chrono::{DateTime, Utc};

use crate::{
    features::image::renderer::RenderRecord, save_contract::ParsedSave, song_contract::SongCatalog,
    startup::chart_loader::ChartConstantsMap,
};

use super::{
    display::{format_data_string, parse_challenge_rank, parse_update_time_or_now},
    runtime::duration_ms_i64,
    score::{
        build_engine_records_from_render_records, calculate_ap_top_3_avg, calculate_best_27_avg,
        calculate_push_acc_map, chart_constant_for_difficulty, collect_ap_top_3_scores,
        difficulty_label, sort_render_records_by_rks_desc,
    },
    usize_from_u32,
};

pub(super) struct BnComputeOutput {
    pub(super) top: Vec<RenderRecord>,
    pub(super) push_acc_map: HashMap<String, crate::rks_contract::engine::PushAccHint>,
    pub(super) exact_rks: f64,
    pub(super) ap_top_3_avg: Option<f64>,
    pub(super) best_27_avg: Option<f64>,
    pub(super) ap_top_3_scores: Vec<RenderRecord>,
    pub(super) challenge_rank: Option<(String, String)>,
    pub(super) data_string: Option<String>,
    pub(super) update_time: DateTime<Utc>,
    pub(super) flatten_ms: i64,
}

pub(super) struct BnComputeInput {
    pub(super) parsed: ParsedSave,
    pub(super) chart_constants: Arc<ChartConstantsMap>,
    pub(super) song_catalog: Arc<SongCatalog>,
    pub(super) n: u32,
}

pub(super) fn build_bn_compute_output(input: BnComputeInput) -> BnComputeOutput {
    let BnComputeInput {
        parsed,
        chart_constants,
        song_catalog,
        n,
    } = input;

    let t_flatten = Instant::now();
    let total_records = parsed.game_record.values().map(Vec::len).sum();
    let mut all: Vec<RenderRecord> = Vec::with_capacity(total_records);
    for (song_id, diffs) in &parsed.game_record {
        // 查定数与曲名
        let chart = chart_constants.get(song_id);
        let name = song_catalog
            .by_id
            .get(song_id)
            .map_or_else(|| song_id.clone(), |s| s.name.clone());

        for rec in diffs {
            let Some(dv) = chart_constant_for_difficulty(chart, rec.difficulty) else {
                continue;
            };

            let acc_percent = f64::from(rec.accuracy);
            let rks = crate::rks_contract::engine::calculate_chart_rks(acc_percent, dv);

            all.push(RenderRecord {
                song_id: song_id.clone(),
                song_name: name.clone(),
                difficulty: difficulty_label(rec.difficulty).to_string(),
                score: Some(f64::from(rec.score)),
                acc: acc_percent,
                rks,
                difficulty_value: dv,
                is_fc: rec.is_full_combo,
            });
        }
    }

    let data_process_duration = t_flatten.elapsed();
    let data_record_count = all.len();
    tracing::info!(target: "bestn_performance", "数据扁平化完成，记录数: {}, 耗时: {:?}ms", data_record_count, data_process_duration.as_millis());

    let t_sort_start = Instant::now();
    // 按 RKS 降序
    sort_render_records_by_rks_desc(&mut all);
    let sort_duration = t_sort_start.elapsed();

    let top_len = usize_from_u32(n).min(all.len());
    tracing::info!(target: "bestn_performance", "排序完成，目标TopN: {}, 排序耗时: {:?}ms", n, sort_duration.as_millis());

    let t_push_start = Instant::now();
    // 预计算推分 ACC（批量求解：避免每谱面重复扫描全量 records）
    let engine_all = build_engine_records_from_render_records(&all);
    let push_acc_map = calculate_push_acc_map(&all, &engine_all, top_len);
    let push_acc_duration = t_push_start.elapsed();
    tracing::info!(target: "bestn_performance", "推分ACC计算完成，计算数量: {}, 耗时: {:?}ms", push_acc_map.len(), push_acc_duration.as_millis());

    let flatten_ms = duration_ms_i64(t_flatten.elapsed());
    let t_stats_start = Instant::now();

    // 统计计算：RKS 详情与平均值
    let (exact_rks, _rounded) =
        crate::rks_contract::engine::calculate_player_rks_details(&engine_all);
    let ap_top_3_avg = calculate_ap_top_3_avg(&all);
    let best_27_avg = calculate_best_27_avg(&all);
    let stats_duration = t_stats_start.elapsed();
    tracing::info!(target: "bestn_performance", "统计数据计算完成，精确RKS: {:?}, AP Top3: {:?}, Best27: {:?}, 耗时: {:?}ms",
                   exact_rks, ap_top_3_avg, best_27_avg, stats_duration.as_millis());

    // 课题模式等级（优先使用 summaryParsed，其次使用 gameProgress.challengeModeRank）
    let t_challenge_start = Instant::now();
    let challenge_rank = if let Some(sum) = parsed.summary_parsed.as_ref() {
        Some(i64::from(sum.challenge_mode_rank))
    } else {
        parsed
            .game_progress
            .as_ref()
            .and_then(|progress| progress.challenge_mode_rank)
            .map(i64::from)
    }
    .and_then(parse_challenge_rank);
    let challenge_duration = t_challenge_start.elapsed();
    tracing::info!(target: "bestn_performance", "挑战等级解析完成: {:?}, 耗时: {:?}ms", challenge_rank, challenge_duration.as_millis());

    let t_data_string_start = Instant::now();
    // Data 数（money）展示
    let data_string = parsed
        .game_progress
        .as_ref()
        .and_then(|progress| progress.money.as_ref())
        .and_then(format_data_string);
    let data_string_duration = t_data_string_start.elapsed();
    tracing::info!(target: "bestn_performance", "Data字符串解析完成: {:?}, 耗时: {:?}ms", data_string, data_string_duration.as_millis());

    let t_time_start = Instant::now();
    let update_time = parse_update_time_or_now(parsed.updated_at.as_deref());
    let time_parse_duration = t_time_start.elapsed();
    tracing::info!(target: "bestn_performance", "更新时间解析完成, 耗时: {:?}ms", time_parse_duration.as_millis());

    let ap_top_3_scores = collect_ap_top_3_scores(&all);

    let top: Vec<RenderRecord> = all.drain(..top_len).collect();

    BnComputeOutput {
        top,
        push_acc_map,
        exact_rks,
        ap_top_3_avg,
        best_27_avg,
        ap_top_3_scores,
        challenge_rank,
        data_string,
        update_time,
        flatten_ms,
    }
}
