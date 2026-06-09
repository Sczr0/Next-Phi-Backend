use std::collections::HashMap;

use crate::rks_contract::engine;

use super::{RenderRecord, score::calculate_push_acc};

pub(super) fn pre_calculated_push_acc_for_score<S>(
    score: &RenderRecord,
    push_acc_map: Option<&HashMap<String, engine::PushAccHint, S>>,
) -> Option<engine::PushAccHint>
where
    S: std::hash::BuildHasher,
{
    push_acc_map.and_then(|map| {
        let key = push_acc_key(score);
        map.get(&key).copied()
    })
}

pub(super) fn resolve_push_acc_hint(
    score: &RenderRecord,
    pre_calculated_push_acc: Option<engine::PushAccHint>,
    all_engine_records: &[engine::RksRecord],
) -> Option<engine::PushAccHint> {
    pre_calculated_push_acc.or_else(|| {
        let target_chart_id = push_acc_key(score);
        calculate_push_acc(&target_chart_id, score.difficulty_value, all_engine_records)
    })
}

pub(super) fn format_acc_text(
    score: &RenderRecord,
    is_ap_score: bool,
    pre_calculated_push_acc: Option<engine::PushAccHint>,
    all_engine_records: &[engine::RksRecord],
) -> String {
    let base_text = base_acc_text(score);
    if !is_ap_score && score.acc < 100.0 && score.difficulty_value > 0.0 {
        let push_hint = resolve_push_acc_hint(score, pre_calculated_push_acc, all_engine_records);

        match push_hint {
            Some(engine::PushAccHint::TargetAcc { acc: push_acc }) => {
                if push_acc > 99.995 {
                    format!("{base_text} <tspan class='push-acc'>-> 100.00%</tspan>")
                } else if (push_acc - score.acc).abs() < 0.005 {
                    format!("{base_text} <tspan class='push-acc'>-> {push_acc:.3}%</tspan>")
                } else {
                    format!("{base_text} <tspan class='push-acc'>-> {push_acc:.2}%</tspan>")
                }
            }
            Some(engine::PushAccHint::PhiOnly | engine::PushAccHint::AlreadyPhi) => {
                format!("{base_text} <tspan class='push-acc push-acc-phi-only'>-> 100.00%</tspan>")
            }
            Some(engine::PushAccHint::Unreachable) => format!(
                "{base_text} <tspan class='push-acc push-acc-unreachable'>-> 无法推分</tspan>"
            ),
            None => base_text,
        }
    } else {
        base_text
    }
}

pub(super) fn format_plain_acc_text(
    score: &RenderRecord,
    push_hint: Option<engine::PushAccHint>,
) -> String {
    let mut acc_text = base_acc_text(score);
    if let Some(hint) = push_hint {
        acc_text.push_str(&plain_push_acc_suffix(hint));
    }
    acc_text
}

fn plain_push_acc_suffix(hint: engine::PushAccHint) -> String {
    match hint {
        engine::PushAccHint::TargetAcc { acc } => format!(" -> {acc:.2}%"),
        engine::PushAccHint::PhiOnly | engine::PushAccHint::AlreadyPhi => " -> 100.00%".to_string(),
        engine::PushAccHint::Unreachable => " -> 无法推分".to_string(),
    }
}

fn push_acc_key(score: &RenderRecord) -> String {
    format!("{}-{}", score.song_id, score.difficulty)
}

fn base_acc_text(score: &RenderRecord) -> String {
    format!("Acc: {:.2}%", score.acc)
}
