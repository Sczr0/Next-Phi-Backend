use super::{SongDifficultyScore, SongRenderData};

pub(super) const SONG_DIFFICULTIES: [&str; 4] = ["EZ", "HD", "IN", "AT"];

pub(super) fn score_for_difficulty<'a>(
    data: &'a SongRenderData,
    key: &str,
) -> Option<&'a SongDifficultyScore> {
    data.difficulty_scores.get(key).and_then(Option::as_ref)
}

pub(super) fn has_score(score: Option<&SongDifficultyScore>) -> bool {
    score.is_some_and(|score| score.acc.is_some())
}

pub(super) fn has_chart(score: Option<&SongDifficultyScore>) -> bool {
    score.is_some_and(|score| score.difficulty_value.is_some())
}

pub(super) fn card_class(score: Option<&SongDifficultyScore>) -> &'static str {
    let Some(score) = score.filter(|score| score.acc.is_some()) else {
        return "difficulty-card-inactive";
    };

    if score.is_phi == Some(true) {
        "difficulty-card-phi"
    } else if score.is_fc == Some(true) {
        "difficulty-card-fc"
    } else {
        "difficulty-card"
    }
}
