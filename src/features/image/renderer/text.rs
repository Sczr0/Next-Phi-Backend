pub(super) fn escape_xml(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

pub(super) fn estimate_bn_song_name_width_px(input: &str) -> f64 {
    const FULL_WIDTH_CHAR_PX: f64 = 19.0;
    const HALF_WIDTH_CHAR_PX: f64 = 10.5;

    input
        .chars()
        .map(|ch| {
            if is_full_width_for_svg_estimate(ch) {
                FULL_WIDTH_CHAR_PX
            } else {
                HALF_WIDTH_CHAR_PX
            }
        })
        .sum()
}

pub(super) fn truncate_chars_with_ellipsis(
    input: &str,
    max_chars: usize,
    keep_chars: usize,
) -> String {
    if max_chars == 0 {
        return String::new();
    }

    if input.chars().count() <= max_chars {
        return input.to_string();
    }

    let keep_chars = keep_chars.min(max_chars);
    let mut output = String::new();
    output.extend(input.chars().take(keep_chars));
    output.push_str("...");
    output
}

fn is_full_width_for_svg_estimate(ch: char) -> bool {
    // 保持旧手写 SVG 的粗略像素估算范围，避免改变 textLength 触发条件。
    ('\u{4E00}'..='\u{9FFF}').contains(&ch)
        || ('\u{3040}'..='\u{30FF}').contains(&ch)
        || ('\u{FF00}'..='\u{FFEF}').contains(&ch)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_chars_with_ellipsis_preserves_short_text() {
        assert_eq!(truncate_chars_with_ellipsis("short", 20, 17), "short");
    }

    #[test]
    fn truncate_chars_with_ellipsis_keeps_ascii_legacy_shape() {
        assert_eq!(
            truncate_chars_with_ellipsis("abcdefghijklmnopqrstu", 20, 17),
            "abcdefghijklmnopq..."
        );
    }

    #[test]
    fn truncate_chars_with_ellipsis_handles_multibyte_text() {
        assert_eq!(
            truncate_chars_with_ellipsis("玩家一二三四五六七八九十甲乙丙丁戊己庚辛壬癸", 20, 17),
            "玩家一二三四五六七八九十甲乙丙丁戊..."
        );
    }
}
