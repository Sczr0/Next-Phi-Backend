use super::text::escape_xml;
use super::{DEFAULT_PLAYER_NAME, PlayerStats};

pub(super) struct BnHeaderText {
    pub(super) player_title: String,
    pub(super) ap_text: String,
    pub(super) bn_text: String,
}

pub(super) fn build_bn_header_text(stats: &PlayerStats) -> BnHeaderText {
    let player_name = stats.player_name.as_deref().unwrap_or(DEFAULT_PLAYER_NAME);
    let real_rks = stats.real_rks.unwrap_or(0.0);
    let player_title = format!("{player_name}({real_rks:.6})");

    let ap_text = stats.ap_top_3_avg.map_or_else(
        || "AP Top 3 Avg: N/A".to_string(),
        |avg| format!("AP Top 3 Avg: {avg:.4}"),
    );
    let bn_text = stats.best_27_avg.map_or_else(
        || "Best 27 Avg: N/A".to_string(),
        |avg| format!("Best 27 Avg: {avg:.4}"),
    );

    BnHeaderText {
        player_title,
        ap_text,
        bn_text,
    }
}

pub(super) fn build_challenge_rank_inner_xml(
    color: &str,
    level: &str,
    fallback_text_color: &str,
) -> String {
    let color_hex = match color {
        "Green" => "#51AF44",
        "Blue" => "#3173B3",
        "Red" => "#BE2D23",
        "Gold" => "#D1913C",
        "Rainbow" => "url(#ap-gradient)",
        _ => fallback_text_color,
    };

    format!(
        "Challenge: <tspan fill=\"{}\">{}</tspan> {}",
        color_hex,
        escape_xml(color),
        escape_xml(level)
    )
}
