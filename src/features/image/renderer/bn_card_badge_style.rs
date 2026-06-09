use crate::features::image::Theme;

use super::RenderRecord;

pub(super) struct DifficultyBadgeStyle {
    pub(super) text: &'static str,
    pub(super) fill: &'static str,
}

pub(super) struct FcApBadgeStyle {
    pub(super) fill: &'static str,
    pub(super) text_fill: &'static str,
    pub(super) text: &'static str,
}

pub(super) fn difficulty_badge_style(difficulty: &str) -> DifficultyBadgeStyle {
    match difficulty {
        diff if diff.eq_ignore_ascii_case("EZ") => DifficultyBadgeStyle {
            text: "EZ",
            fill: "#51AF44",
        },
        diff if diff.eq_ignore_ascii_case("HD") => DifficultyBadgeStyle {
            text: "HD",
            fill: "#3173B3",
        },
        diff if diff.eq_ignore_ascii_case("IN") => DifficultyBadgeStyle {
            text: "IN",
            fill: "#BE2D23",
        },
        diff if diff.eq_ignore_ascii_case("AT") => DifficultyBadgeStyle {
            text: "AT",
            fill: "#383838",
        },
        _ => DifficultyBadgeStyle {
            text: "??",
            fill: "#888888",
        },
    }
}

#[allow(clippy::trivially_copy_pass_by_ref)]
pub(super) fn fc_ap_badge_style(score: &RenderRecord, theme: &Theme) -> Option<FcApBadgeStyle> {
    let (ap_fill, fc_fill, ap_text_fill, fc_text_fill) = match theme {
        Theme::White => ("url(#ap-gradient-white)", "#4682B4", "white", "white"),
        Theme::Black => ("url(#ap-gradient)", "#87CEEB", "white", "white"),
    };

    if (score.acc - 100.0).abs() < f64::EPSILON {
        Some(FcApBadgeStyle {
            fill: ap_fill,
            text_fill: ap_text_fill,
            text: "AP",
        })
    } else if score.is_fc {
        Some(FcApBadgeStyle {
            fill: fc_fill,
            text_fill: fc_text_fill,
            text: "FC",
        })
    } else {
        None
    }
}
