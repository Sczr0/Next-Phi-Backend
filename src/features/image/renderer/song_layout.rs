pub(super) struct SongLayout {
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) padding: f64,
    pub(super) player_info_height: f64,
    pub(super) illust_height: f64,
    pub(super) illust_width: f64,
    pub(super) song_name_height: f64,
    pub(super) difficulty_card_width: f64,
    pub(super) difficulty_card_height: f64,
    pub(super) difficulty_card_spacing: f64,
}

impl SongLayout {
    pub(super) fn new() -> Self {
        let width = 1400;
        let height = 800;
        let padding = 40.0;
        let player_info_height = 78.0;

        // 曲绘尺寸保持 2048x1080 比例，并限制最大宽度。
        let illust_height = f64::from(height) - padding * 3.0 - player_info_height - 80.0;
        let illust_width = (illust_height * (2048.0 / 1080.0)).min(f64::from(width) * 0.60);
        let song_name_height = 50.0;

        // 成绩卡总高度与曲绘高度对齐。
        let card_area_width = f64::from(width) - illust_width - padding * 3.0;
        let difficulty_spacing_total = padding * 0.8 * 3.0;
        let difficulty_card_height = (illust_height - difficulty_spacing_total) / 4.0;
        let difficulty_card_spacing = padding * 0.8;

        Self {
            width,
            height,
            padding,
            player_info_height,
            illust_height,
            illust_width,
            song_name_height,
            difficulty_card_width: card_area_width,
            difficulty_card_height,
            difficulty_card_spacing,
        }
    }
}
