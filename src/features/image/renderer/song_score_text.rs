use super::{SongDifficultyScore, engine};

pub(super) const INACTIVE_ACC_TEXT: &str = "Acc: N/A";
pub(super) const INACTIVE_RKS_TEXT: &str = "Lv.?? -> ??";

#[derive(Debug, Clone)]
pub(super) struct SongScoreText {
    pub(super) score_text: String,
    pub(super) acc_text: String,
    pub(super) rks_text: String,
    pub(super) constant_text: Option<String>,
}

#[derive(Debug, Clone, Copy)]
enum SongPushAccDisplay {
    AlreadyPhi,
    TargetAcc { acc: f64 },
    PhiTarget,
    Unreachable,
}

pub(super) fn build_song_score_text(score_data: &SongDifficultyScore) -> SongScoreText {
    let score_text = score_data
        .score
        .map_or_else(|| "N/A".to_string(), |score| format!("{score:.0}"));
    let acc_text = build_song_acc_text(score_data);
    let rks_value = score_data.rks.unwrap_or(0.0);
    let dv_value = score_data.difficulty_value.unwrap_or(0.0);
    let rks_text = format!("Lv.{dv_value:.1} -> {rks_value:.2}");
    let constant_text = build_song_constant_text(score_data);

    SongScoreText {
        score_text,
        acc_text,
        rks_text,
        constant_text,
    }
}

pub(super) fn build_song_constant_text(score_data: &SongDifficultyScore) -> Option<String> {
    score_data
        .difficulty_value
        .map(|difficulty_value| format!("Lv. {difficulty_value:.1}"))
}

pub(super) fn build_song_acc_handwritten_xml(score_data: &SongDifficultyScore) -> String {
    let acc_value = score_data.acc.unwrap_or(0.0);
    let mut acc_text = format!("Acc: {acc_value:.2}%");
    if let Some(display) = push_acc_display(score_data) {
        acc_text.push_str(&push_acc_handwritten_xml_suffix(display));
    }
    acc_text
}

fn build_song_acc_text(score_data: &SongDifficultyScore) -> String {
    let acc_value = score_data.acc.unwrap_or(0.0);
    let mut acc_text = format!("Acc: {acc_value:.2}%");
    if let Some(display) = push_acc_display(score_data) {
        acc_text.push_str(&push_acc_plain_suffix(display));
    }
    acc_text
}

fn push_acc_display(score_data: &SongDifficultyScore) -> Option<SongPushAccDisplay> {
    if score_data.is_phi == Some(true) {
        return Some(SongPushAccDisplay::AlreadyPhi);
    }

    score_data.player_push_acc.map(|hint| match hint {
        engine::PushAccHint::TargetAcc { acc } => SongPushAccDisplay::TargetAcc { acc },
        engine::PushAccHint::PhiOnly | engine::PushAccHint::AlreadyPhi => {
            SongPushAccDisplay::PhiTarget
        }
        engine::PushAccHint::Unreachable => SongPushAccDisplay::Unreachable,
    })
}

fn push_acc_plain_suffix(display: SongPushAccDisplay) -> String {
    match display {
        SongPushAccDisplay::AlreadyPhi => " (已 Phi)".to_string(),
        SongPushAccDisplay::TargetAcc { acc } => format!(" -> {acc:.2}%"),
        SongPushAccDisplay::PhiTarget => " -> 100.00%".to_string(),
        SongPushAccDisplay::Unreachable => " -> 无法推分".to_string(),
    }
}

fn push_acc_handwritten_xml_suffix(display: SongPushAccDisplay) -> String {
    match display {
        SongPushAccDisplay::AlreadyPhi => {
            "<tspan class='text-push-acc' fill='gold'> (已 Phi)</tspan>".to_string()
        }
        SongPushAccDisplay::TargetAcc { acc } => {
            format!(
                r"<tspan class='text-push-acc' fill='url(#rks-gradient-push)'> -> {acc:.2}%</tspan>"
            )
        }
        SongPushAccDisplay::PhiTarget => {
            "<tspan class='text-push-acc' fill='gold'> -> 100.00%</tspan>".to_string()
        }
        SongPushAccDisplay::Unreachable => {
            "<tspan class='text-push-acc' fill='#9E9E9E'> -> 无法推分</tspan>".to_string()
        }
    }
}
