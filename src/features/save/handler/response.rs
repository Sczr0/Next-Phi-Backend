use axum::{
    body::Bytes,
    http::{StatusCode, header::CONTENT_TYPE},
    response::{IntoResponse, Response},
};
use std::collections::HashMap;
use std::time::Instant;

use crate::error::AppError;
use crate::rks_contract::engine::{ChartRankingScore, PlayerRksResult};
use crate::state::AppState;

use super::super::{
    models::{SaveAndRksResponseDoc, SaveResponseDoc},
    provider,
};
use super::{RksComputeResult, SaveWithCache, duration_ms_i64};

/// oneOf 响应：仅解析存档，或解析存档并计算 RKS。
#[derive(serde::Serialize, utoipa::ToSchema)]
#[serde(untagged)]
pub enum SaveApiResponse {
    Save(SaveResponseDoc),
    SaveAndRks(SaveAndRksResponseDoc),
}

#[derive(serde::Serialize)]
struct SaveDataBody<'a> {
    data: &'a provider::ParsedSave,
}

#[derive(Debug, serde::Serialize)]
pub struct SaveAndRksResponse {
    pub save: provider::ParsedSave,
    pub rks: PlayerRksResult,
    #[serde(rename = "gradeCounts")]
    pub grade_counts: super::super::models::CfcPCountsByDifficulty,
}

pub(super) fn serialize_save_data_body(parsed: &provider::ParsedSave) -> Result<Bytes, AppError> {
    serde_json::to_vec(&SaveDataBody { data: parsed })
        .map(Bytes::from)
        .map_err(|e| AppError::Internal(format!("serialize save data response failed: {e}")))
}

fn serialize_json_bytes<T: serde::Serialize>(value: &T, label: &str) -> Result<Bytes, AppError> {
    serde_json::to_vec(value)
        .map(Bytes::from)
        .map_err(|e| AppError::Internal(format!("serialize {label} failed: {e}")))
}

fn json_bytes_response(body: Bytes) -> Response {
    (StatusCode::OK, [(CONTENT_TYPE, "application/json")], body).into_response()
}

pub(super) fn build_save_response(
    data: &SaveWithCache,
    rks_opt: Option<(&RksComputeResult, &provider::ParsedSave)>,
) -> Result<Response, AppError> {
    let (body, calc_rks, calc_ms) = if let Some((rks_result, full_save)) = rks_opt {
        let grade_counts = compute_grade_counts(&rks_result.game_record);
        let resp = SaveAndRksResponse {
            save: provider::ParsedSave {
                game_record: rks_result.game_record.clone(),
                game_progress: full_save.game_progress.clone(),
                user: full_save.user.clone(),
                settings: full_save.settings.clone(),
                game_key: full_save.game_key.clone(),
                summary_parsed: full_save.summary_parsed.clone(),
                updated_at: full_save.updated_at.clone(),
            },
            rks: rks_result.rks.clone(),
            grade_counts,
        };
        let body = serialize_json_bytes(&resp, "save+rks response")?;
        (body, true, rks_result.calc_ms)
    } else {
        (data.data_body.clone(), false, 0_i64)
    };
    let t_serialize = Instant::now();
    let serialize_ms = duration_ms_i64(t_serialize.elapsed());
    tracing::info!(
        target: "phi_backend::save::performance",
        route = "/save",
        phase = "serialize",
        status = "ok",
        calculate_rks = calc_rks,
        cache_status = data.cache_status,
        dur_ms = serialize_ms,
        "save performance"
    );
    let _ = (calc_rks, calc_ms);
    Ok(json_bytes_response(body))
}

fn compute_grade_counts(
    records: &HashMap<String, Vec<super::super::models::DifficultyRecord>>,
) -> super::super::models::CfcPCountsByDifficulty {
    use super::super::models::{CfcPCounts, CfcPCountsByDifficulty, Difficulty};

    #[derive(Debug, Clone, Copy)]
    enum Tier {
        C,
        FC,
        P,
    }

    let mut counts = CfcPCountsByDifficulty::default();

    for diffs in records.values() {
        for rec in diffs {
            let tier = if rec.score == 1_000_000 {
                Tier::P
            } else if rec.is_full_combo {
                Tier::FC
            } else if rec.score > 700_000 {
                Tier::C
            } else {
                continue;
            };

            let bucket: &mut CfcPCounts = match &rec.difficulty {
                Difficulty::EZ => &mut counts.ez,
                Difficulty::HD => &mut counts.hd,
                Difficulty::IN => &mut counts.in_,
                Difficulty::AT => &mut counts.at,
            };

            match tier {
                Tier::P => {
                    bucket.p += 1;
                    bucket.fc += 1;
                    bucket.c += 1;
                }
                Tier::FC => {
                    bucket.fc += 1;
                    bucket.c += 1;
                }
                Tier::C => {
                    bucket.c += 1;
                }
            }
        }
    }

    counts
}

fn normalize_accuracy_percent(acc: f32) -> f64 {
    f64::from(acc)
}

fn build_chart_acc_index(
    records: &HashMap<String, Vec<super::super::models::DifficultyRecord>>,
) -> (HashMap<String, f64>, usize, usize) {
    let mut acc_by_chart = HashMap::new();
    let mut valid_count = 0usize;
    let mut ap_count = 0usize;

    for (song_id, diffs) in records {
        for rec in diffs {
            if rec.chart_constant.is_none() {
                continue;
            }
            let acc = normalize_accuracy_percent(rec.accuracy);
            let key = format!("{song_id}-{}", rec.difficulty);
            acc_by_chart.insert(key, acc);
            valid_count = valid_count.saturating_add(1);
            if acc >= 100.0 {
                ap_count = ap_count.saturating_add(1);
            }
        }
    }

    (acc_by_chart, valid_count, ap_count)
}

pub(super) fn build_textual_details_from_rks(
    records: &HashMap<String, Vec<super::super::models::DifficultyRecord>>,
    rks_result: &PlayerRksResult,
    state: &AppState,
) -> (
    Vec<crate::leaderboard_contract::ChartTextItem>,
    Vec<crate::leaderboard_contract::ChartTextItem>,
    crate::leaderboard_contract::RksCompositionText,
) {
    use crate::leaderboard_contract::{ChartTextItem, RksCompositionText};
    let (acc_by_chart, valid_count, ap_count) = build_chart_acc_index(records);
    let total_charts = rks_result.b30_charts.len();
    let best27_len = valid_count.min(27).min(total_charts);
    let ap3_len = ap_count.min(3).min(total_charts.saturating_sub(best27_len));

    let best_slice = &rks_result.b30_charts[..best27_len];
    let ap_slice = &rks_result.b30_charts[best27_len..best27_len + ap3_len];

    let name_of = |sid: &str| -> String {
        state
            .song_catalog
            .by_id
            .get(sid)
            .map_or_else(|| sid.to_string(), |s| s.name.clone())
    };

    let to_text = |v: &[ChartRankingScore]| -> Vec<ChartTextItem> {
        v.iter()
            .take(3)
            .map(|score| {
                let key = format!("{}-{}", score.song_id, score.difficulty);
                let acc = acc_by_chart.get(&key).copied().unwrap_or(0.0);
                ChartTextItem {
                    song: name_of(&score.song_id),
                    difficulty: score.difficulty.to_string(),
                    acc,
                    rks: score.rks,
                }
            })
            .collect()
    };
    let best_top3 = to_text(best_slice);
    let ap_top3 = to_text(ap_slice);
    let best27_sum: f64 = best_slice.iter().map(|v| v.rks).sum();
    let ap3_sum: f64 = ap_slice.iter().map(|v| v.rks).sum();

    (
        best_top3,
        ap_top3,
        RksCompositionText {
            best27_sum,
            ap_top3_sum: ap3_sum,
        },
    )
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::compute_grade_counts;
    use crate::features::save::models::{Difficulty, DifficultyRecord};

    fn rec(difficulty: Difficulty, score: u32, is_full_combo: bool) -> DifficultyRecord {
        DifficultyRecord {
            difficulty,
            score,
            accuracy: 0.0,
            is_full_combo,
            chart_constant: None,
            push_acc: None,
            push_acc_hint: None,
        }
    }

    #[test]
    fn compute_grade_counts_counts_c_fc_p_with_cumulative_rule() {
        let mut records: HashMap<String, Vec<DifficultyRecord>> = HashMap::new();
        records.insert(
            "song_a".to_string(),
            vec![
                rec(Difficulty::EZ, 700_001, false),
                rec(Difficulty::HD, 500_000, true),
                rec(Difficulty::IN, 1_000_000, false),
                rec(Difficulty::AT, 700_000, false),
            ],
        );
        records.insert(
            "song_b".to_string(),
            vec![
                rec(Difficulty::EZ, 1_000_000, true),
                rec(Difficulty::AT, 700_000, true),
            ],
        );

        let counts = compute_grade_counts(&records);

        assert_eq!(counts.ez.c, 2);
        assert_eq!(counts.ez.fc, 1);
        assert_eq!(counts.ez.p, 1);

        assert_eq!(counts.hd.c, 1);
        assert_eq!(counts.hd.fc, 1);
        assert_eq!(counts.hd.p, 0);

        assert_eq!(counts.in_.c, 1);
        assert_eq!(counts.in_.fc, 1);
        assert_eq!(counts.in_.p, 1);

        assert_eq!(counts.at.c, 1);
        assert_eq!(counts.at.fc, 1);
        assert_eq!(counts.at.p, 0);
    }

    #[test]
    fn grade_counts_serializes_with_expected_keys() {
        let records: HashMap<String, Vec<DifficultyRecord>> = HashMap::new();
        let counts = compute_grade_counts(&records);
        let v = serde_json::to_value(counts).expect("serialize");

        assert!(v.get("EZ").is_some());
        assert!(v.get("HD").is_some());
        assert!(v.get("IN").is_some());
        assert!(v.get("AT").is_some());

        let ez = v.get("EZ").expect("EZ exists");
        assert!(ez.get("C").is_some());
        assert!(ez.get("FC").is_some());
        assert!(ez.get("P").is_some());
    }
}
