//! 歌曲搜索相关的对外契约类型（纯类型，无逻辑）。

/// 搜索候选预览（用于歧义查询时的提示）。
///
/// 从 `features/song/models.rs` 下沉至此（Phase 1 纯搬迁）：
/// `AppError/ProblemDetails`（phi-http）与业务层都依赖它，物理上必须先
/// 脱离根 crate，否则 phi-http 与业务实现无法独立成 crate。
#[derive(Debug, Clone, utoipa::ToSchema, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SongCandidatePreview {
    pub id: String,
    pub name: String,
}
