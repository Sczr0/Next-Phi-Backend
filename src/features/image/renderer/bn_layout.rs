use super::math::{round_non_negative_to_u32, u32_from_usize};

pub(super) struct BnLayout {
    pub(super) width: u32,
    pub(super) header_height: u32,
    pub(super) footer_height: u32,
    pub(super) main_card_padding_outer: u32,
    pub(super) ap_card_padding_outer: u32,
    pub(super) columns: u32,
    pub(super) main_card_width: u32,
    pub(super) calculated_card_height: u32,
    pub(super) ap_card_start_y: u32,
    pub(super) ap_section_height: u32,
    pub(super) total_height: u32,
}

impl BnLayout {
    pub(super) fn new(score_count: usize, ap_scores_empty: bool) -> Self {
        let width = 1200;
        let header_height = 120;
        // 84px：容纳三行 14px 底栏正文（生成时间行 + 签名两行），
        // footer_y 公式使生成行到卡片底保持原始 45px 间距。
        let footer_height = 84;
        let main_card_padding_outer = 12;
        let ap_card_padding_outer = 12;
        let columns = 3;

        let main_card_width = (width - main_card_padding_outer * (columns + 1)) / columns;
        let card_padding_inner = 10.0;
        let text_line_height_song = 22.0;
        let text_line_height_score = 30.0;
        let text_line_height_acc = 18.0;
        let text_line_height_level = 18.0;
        let text_block_spacing = 4.0;
        let text_block_height = text_line_height_song
            + text_line_height_score
            + text_line_height_acc
            + text_line_height_level
            + text_block_spacing * 3.0;
        let calculated_card_height =
            round_non_negative_to_u32(text_block_height + card_padding_inner * 2.0);
        let ap_card_start_y = ap_card_padding_outer;
        let ap_section_height = if ap_scores_empty {
            0
        } else {
            ap_card_start_y + calculated_card_height + ap_card_padding_outer
        };
        let rows = u32_from_usize(score_count).div_ceil(columns);
        let content_height = (calculated_card_height + main_card_padding_outer) * rows.max(1);
        let total_height = header_height + ap_section_height + content_height + footer_height + 10;

        Self {
            width,
            header_height,
            footer_height,
            main_card_padding_outer,
            ap_card_padding_outer,
            columns,
            main_card_width,
            calculated_card_height,
            ap_card_start_y,
            ap_section_height,
            total_height,
        }
    }
}
