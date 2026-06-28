use super::RenderRecord;

pub(super) fn bn_score_text(score: &RenderRecord) -> String {
    score.score.map_or("N/A".to_string(), |s| format!("{s:.0}"))
}

pub(super) fn bn_template_score_text(score: &RenderRecord) -> String {
    score.score.map_or_else(
        || "N/A".to_string(),
        |s| format!("{:.0}", s.max(0.0).round()),
    )
}

pub(super) fn bn_level_text(score: &RenderRecord) -> String {
    format!("Lv.{:.1} → {:.2}", score.difficulty_value, score.rks)
}

pub(super) fn bn_rank_text(index: usize) -> String {
    format!("#{}", index + 1)
}
