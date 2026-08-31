// 原根 crate lint 策略随迁移搬入（代码源自根包；与根 lib.rs 的 allow 列表一致）。
#![allow(
    clippy::similar_names,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::too_many_lines,
    clippy::doc_markdown,
    clippy::struct_excessive_bools,
    clippy::items_after_statements,
    clippy::module_name_repetitions
)]

//! phi-save-codec: Phigros Cloud Save Binary Format Codec
//!
//! 提供 Phigros 存档二进制格式的解析能力，不依赖异步运行时。
//! 使用方式：解析后的 struct 可通过 serde 序列化为 JSON 等格式。

#![no_std]

extern crate alloc;

mod reader;

pub mod error;
pub mod game_key;
pub mod game_progress;
pub mod game_record;
pub mod settings;
pub mod summary;
pub mod types;
pub mod user;

// Re-exports
pub use error::CodecError;
pub use types::*;

pub use game_key::GameKeyParsed;
pub use game_key::parse_game_key_entry;
pub use game_progress::GameProgressParsed;
pub use game_progress::parse_game_progress_entry;
pub use game_record::{
    LevelRecord, SongLevelRecord, parse_game_record_bytes, parse_game_record_json,
};
pub use settings::SettingsParsed;
pub use settings::parse_settings_entry;
pub use summary::SummaryParsed;
pub use summary::parse_summary_base64;
pub use user::UserParsed;
pub use user::parse_user_entry;
