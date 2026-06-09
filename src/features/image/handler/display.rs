use chrono::{DateTime, Utc};

pub(super) fn parse_update_time_or_now(updated_at: Option<&str>) -> DateTime<Utc> {
    updated_at
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
        .map_or_else(Utc::now, |datetime| datetime.with_timezone(&Utc))
}

pub(super) fn parse_challenge_rank(rank_num: i64) -> Option<(String, String)> {
    if rank_num <= 0 {
        return None;
    }
    let rank_text = rank_num.to_string();
    if rank_text.is_empty() {
        return None;
    }
    let (color_char, level_str) = rank_text.split_at(1);
    let color = match color_char {
        "1" => "Green",
        "2" => "Blue",
        "3" => "Red",
        "4" => "Gold",
        "5" => "Rainbow",
        _ => return None,
    };
    Some((color.to_string(), level_str.to_string()))
}

pub(super) fn format_data_string(money: &[i32; 5]) -> Option<String> {
    let units = ["KB", "MB", "GB", "TB"];
    let mut parts: Vec<String> = money
        .iter()
        .zip(units.iter())
        .filter_map(|(value, unit)| {
            let amount = i64::from(*value);
            (amount > 0).then(|| format!("{amount} {unit}"))
        })
        .collect();
    parts.reverse();

    if parts.is_empty() {
        None
    } else {
        Some(format!("Data: {}", parts.join(", ")))
    }
}
