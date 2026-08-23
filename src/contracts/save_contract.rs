/// OpenAPI 文档用响应类型（避免 open_platform 跨 feature 引用）
pub use crate::features::save::handler::SaveApiResponse;
pub use crate::features::save::models::{Difficulty, DifficultyRecord};
pub use crate::features::save::provider::{
    ParsedSave, SaveMeta, SaveSource, fetch_save_meta, get_decrypted_save_from_meta,
};
