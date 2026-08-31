//! 存档二进制格式解析 — 委托给 phi-save-codec crate

pub use phi_save_codec::{
    GameKeyParsed, GameProgressParsed, SettingsParsed, UserParsed, parse_game_key_entry,
    parse_game_progress_entry, parse_settings_entry, parse_user_entry,
};
