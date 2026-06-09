use crate::features::image::Theme;

pub(super) struct BnThemePalette {
    pub(super) bg_color: &'static str,
    pub(super) text_color: &'static str,
    pub(super) card_bg_color: &'static str,
    pub(super) card_stroke_color: &'static str,
    pub(super) text_secondary_color: &'static str,
    pub(super) fc_stroke_color: &'static str,
    pub(super) ap_stroke_color: &'static str,
    pub(super) ap_card_fill: String,
    pub(super) fc_card_fill: String,
    pub(super) normal_card_stroke_color: String,
}

impl BnThemePalette {
    #[allow(clippy::trivially_copy_pass_by_ref)]
    pub(super) fn from_theme(theme: &Theme) -> Self {
        let (
            bg_color,
            text_color,
            card_bg_color,
            card_stroke_color,
            text_secondary_color,
            fc_stroke_color,
            ap_stroke_color,
        ) = match theme {
            Theme::White => (
                "#F7FAFF",
                "#000000",
                "#ECEFF4",
                "#D0D4DD",
                "#555555",
                "#4682B4",
                "url(#ap-gradient)",
            ),
            Theme::Black => (
                "#141826",
                "#FFFFFF",
                "#1A1E2A",
                "#333848",
                "#BBBBBB",
                "#87CEEB",
                "url(#ap-gradient)",
            ),
        };
        let (ap_card_fill, fc_card_fill) = match theme {
            Theme::White => ("#FFFBEB".to_string(), "#E6F2FF".to_string()),
            Theme::Black => (card_bg_color.to_string(), card_bg_color.to_string()),
        };
        let normal_card_stroke_color = match theme {
            Theme::White => "url(#normal-card-stroke-gradient)".to_string(),
            Theme::Black => "#252A38".to_string(),
        };

        Self {
            bg_color,
            text_color,
            card_bg_color,
            card_stroke_color,
            text_secondary_color,
            fc_stroke_color,
            ap_stroke_color,
            ap_card_fill,
            fc_card_fill,
            normal_card_stroke_color,
        }
    }
}
