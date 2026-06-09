use std::fmt::Write;

use crate::error::AppError;

use super::math::i32_from_usize;
use super::svg_error::svg_fmt_error;
use super::text::{escape_xml, truncate_chars_with_ellipsis};
use super::{LeaderboardEntry, LeaderboardRenderData};

pub(super) fn generate_leaderboard_svg_string(
    data: &LeaderboardRenderData,
) -> Result<String, AppError> {
    let width = 1200;
    let row_height = 60;
    let header_height = 120;
    let footer_height = 40;
    let total_height =
        header_height + (i32_from_usize(data.entries.len()) * row_height) + footer_height;

    let mut svg = String::with_capacity(20000);
    write!(svg, r#"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{total_height}" viewBox="0 0 {width} {total_height}">"#)
        .map_err(svg_fmt_error)?;

    // 添加渐变背景和样式
    // 使用 r##"..."## 来避免 # 颜色值与原始字符串分隔符冲突
    svg.push_str(r##"
    <defs>
        <linearGradient id="bg-gradient" x1="0%" y1="0%" x2="100%" y2="100%">
            <stop offset="0%" stop-color="#1a1a2e" />
            <stop offset="100%" stop-color="#16213e" />
        </linearGradient>
        <style>
            @font-face {
                font-family: 'NotoSansSC';
                src: url('https://fonts.gstatic.com/s/notosanssc/v36/k3kXo84MPvpLmixcA63oeALhLIiP-Q-87KaAavc.woff2') format('woff2');
            }
            .header-text {
                font-family: 'NotoSansSC', sans-serif;
                font-size: 48px;
                fill: white;
                text-anchor: middle;
                font-weight: bold; /* 加粗标题 */
            }
            .rank-text {
                font-family: 'NotoSansSC', sans-serif;
                font-size: 32px;
                fill: white;
                text-anchor: middle;
                font-weight: bold;
            }
            .name-text {
                font-family: 'NotoSansSC', sans-serif;
                font-size: 32px;
                fill: white;
                text-anchor: start;
            }
            .rks-text {
                font-family: 'NotoSansSC', sans-serif;
                font-size: 32px;
                fill: white;
                text-anchor: end;
                font-weight: bold;
            }
            .footer-text {
                font-family: 'NotoSansSC', sans-serif;
                font-size: 20px;
                fill: #aaaaaa;
                text-anchor: end;
            }
        </style>
    </defs>
"##);

    // 绘制背景
    write!(
        svg,
        r#"<rect width="{width}" height="{total_height}" fill="url(#bg-gradient)" />"#
    )
    .map_err(svg_fmt_error)?;

    // 绘制标题
    let title_xml = escape_xml(&data.title);
    write!(
        svg,
        r#"<text x="{}" y="{}" class="header-text">{}</text>"#,
        width / 2,
        header_height / 2 + 16,
        title_xml
    )
    .map_err(svg_fmt_error)?;

    // 绘制表头分隔线
    write!(
        svg,
        r##"<line x1="20" y1="{}" x2="{}" y2="{}" stroke="#4a5568" stroke-width="2" />"##,
        header_height,
        width - 20,
        header_height
    )
    .map_err(svg_fmt_error)?;

    // 绘制排行榜条目
    for (i, entry) in data.entries.iter().enumerate() {
        let y_pos = header_height + (i32_from_usize(i) * row_height);
        write_leaderboard_entry(
            svg,
            entry,
            LeaderboardEntryRenderLayout {
                rank: i + 1,
                y_pos,
                row_height,
                width,
                is_last: i == data.entries.len() - 1,
            },
        )?;
    }

    // 绘制底部更新时间
    let time_str = data.update_time.format("%Y-%m-%d %H:%M:%S").to_string();
    write!(
        svg,
        r#"<text x="{}" y="{}" class="footer-text">更新时间: {} UTC</text>"#,
        width - 60,
        total_height - 15,
        time_str
    )
    .map_err(svg_fmt_error)?;

    svg.push_str("</svg>");
    Ok(svg)
}

#[derive(Debug, Clone, Copy)]
struct LeaderboardEntryRenderLayout {
    rank: usize,
    y_pos: i32,
    row_height: i32,
    width: i32,
    is_last: bool,
}

fn write_leaderboard_entry(
    svg: &mut String,
    entry: &LeaderboardEntry,
    layout: LeaderboardEntryRenderLayout,
) -> Result<(), AppError> {
    let text_y = layout.y_pos + (layout.row_height / 2) + 10;

    write!(
        svg,
        r#"<text x="60" y="{text_y}" class="rank-text">#{}</text>"#,
        layout.rank
    )
    .map_err(svg_fmt_error)?;

    let name_display = leaderboard_name_display(&entry.player_name);
    let name_display_xml = escape_xml(&name_display);
    write!(
        svg,
        r#"<text x="120" y="{text_y}" class="name-text">{name_display_xml}</text>"#
    )
    .map_err(svg_fmt_error)?;

    write!(
        svg,
        r#"<text x="{}" y="{text_y}" class="rks-text">{:.2}</text>"#,
        layout.width - 60,
        entry.rks
    )
    .map_err(svg_fmt_error)?;

    if !layout.is_last {
        let line_y = layout.y_pos + layout.row_height;
        write!(
            svg,
            r##"<line x1="100" y1="{line_y}" x2="{}" y2="{line_y}" stroke="#2d3748" stroke-width="1" />"##,
            layout.width - 100,
        )
        .map_err(svg_fmt_error)?;
    }

    Ok(())
}

fn leaderboard_name_display(player_name: &str) -> String {
    truncate_chars_with_ellipsis(player_name, 20, 17)
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::*;

    #[test]
    fn leaderboard_escapes_title_and_player_name() {
        let svg = generate_leaderboard_svg_string(&LeaderboardRenderData {
            title: "A&B <Top> \"Q\"".to_string(),
            update_time: Utc::now(),
            entries: vec![LeaderboardEntry {
                player_name: "玩家<&>\"".to_string(),
                rks: 15.234,
            }],
        })
        .expect("render leaderboard svg");

        assert!(svg.contains("A&amp;B &lt;Top&gt; &quot;Q&quot;"));
        assert!(svg.contains("玩家&lt;&amp;&gt;&quot;"));
        assert!(!svg.contains("A&B <Top> \"Q\""));
        assert!(!svg.contains("玩家<&>\""));
    }
}
