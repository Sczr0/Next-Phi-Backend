use axum::{
    Router,
    extract::{Query, State},
    response::{IntoResponse, Json},
    routing::get,
};

use crate::error::AppError;
use crate::features::song::models::SongInfo;
use crate::state::AppState;

const DEFAULT_LIMIT: u32 = 20;
const MAX_LIMIT: u32 = 100;
const MAX_QUERY_CHARS: usize = 128;

/// 分页响应（用于非 unique 查询）。
#[derive(serde::Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(example = json!({
  "items": [
    {"id": "97f9466b2e77", "name": "Arcahv", "composer": "Feryquitous", "illustrator": "Catrong", "chartConstants": {"ez": 4.5, "hd": 7.9, "in": 9.6, "at": 12.3}}
  ],
  "total": 123,
  "limit": 20,
  "offset": 0,
  "hasMore": true,
  "nextOffset": 20
}))]
pub struct SongSearchPage {
    pub items: Vec<SongInfo>,
    pub total: u32,
    pub limit: u32,
    pub offset: u32,
    pub has_more: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_offset: Option<u32>,
}

/// oneOf 响应：单个 SongInfo 或 SongSearchPage
#[derive(serde::Serialize, utoipa::ToSchema)]
#[serde(untagged)]
pub enum SongSearchResult {
    Single(SongInfo),
    Page(SongSearchPage),
}

#[derive(serde::Deserialize)]
pub struct SongSearchQuery {
    /// 查询字符串
    q: String,
    /// 是否强制唯一匹配（可选，支持 1/true/yes/on）
    unique: Option<String>,
    /// 最大返回条数（可选，默认 20，上限 100，最小 1）
    limit: Option<u32>,
    /// 结果偏移（可选，默认 0）
    offset: Option<u32>,
}

fn parse_bool(s: &str) -> bool {
    matches!(s.to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on")
}

#[utoipa::path(
    get,
    path = "/songs/search",
    summary = "歌曲检索（支持别名与模糊匹配）",
    description = "按 ID/官方名/别名进行模糊搜索。`unique=true` 时期望唯一命中，未命中返回 404，多命中返回 409。",
    params(
        ("q" = String, Query, description = "查询字符串"),
        ("unique" = Option<bool>, Query, description = "是否强制唯一匹配（可选）"),
        ("limit" = Option<u32>, Query, description = "最大返回条数（可选，默认 20，上限 100，最小 1）"),
        ("offset" = Option<u32>, Query, description = "结果偏移（可选，默认 0）")
    ),
    responses(
        (status = 200, description = "查询成功（unique=true 时返回单个对象，否则为分页对象）", body = SongSearchResult),
        (
            status = 400,
            description = "请求参数错误（缺少 q 等）",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        ),
        (
            status = 422,
            description = "参数校验错误（q 过长 / limit 无效等）",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        ),
        (
            status = 404,
            description = "未找到匹配项",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        ),
        (
            status = 409,
            description = "结果不唯一",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        ),
        (
            status = 500,
            description = "服务器内部错误",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        )
    ),
    tag = "Song"
)]
pub async fn search_songs(
    State(state): State<AppState>,
    Query(params): Query<SongSearchQuery>,
) -> Result<axum::response::Response, AppError> {
    let q = params.q.trim().to_string();
    if q.is_empty() {
        return Err(AppError::Json("缺少查询参数 q".into()));
    }
    let q_len = q.chars().count();
    if q_len > MAX_QUERY_CHARS {
        return Err(AppError::Validation(format!(
            "查询参数 q 过长（最大 {MAX_QUERY_CHARS} 字符）"
        )));
    }

    let unique = params.unique.as_deref().map(parse_bool).unwrap_or(false);

    let limit = match params.limit {
        None => DEFAULT_LIMIT,
        Some(0) => return Err(AppError::Validation("limit 必须 >= 1".into())),
        Some(v) => v.min(MAX_LIMIT),
    };
    let offset = params.offset.unwrap_or(0);

    // 统计：歌曲搜索（不记录原始查询词，避免敏感信息）
    if let Some(stats_handle) = state.stats.as_ref() {
        let extra = serde_json::json!({
            "unique": unique,
            "q_len": q_len,
            "limit": limit,
            "offset": offset
        });
        stats_handle
            .track_feature("song_search", "search", None, Some(extra))
            .await;
    }

    if unique {
        let item = state.song_catalog.search_unique(&q)?;
        Ok(Json::<SongInfo>(item.as_ref().clone()).into_response())
    } else {
        let items = state.song_catalog.search(&q);
        let total = items.len();
        let total_u32 = u32::try_from(total).unwrap_or(u32::MAX);

        let start = offset as usize;
        let end = start.saturating_add(limit as usize).min(total);
        let slice = if start >= total {
            &[]
        } else {
            &items[start..end]
        };

        let page_items: Vec<SongInfo> = slice.iter().map(|a| a.as_ref().clone()).collect();

        let has_more = end < total;
        let next_offset = if has_more {
            u32::try_from(end).ok()
        } else {
            None
        };

        let page = SongSearchPage {
            items: page_items,
            total: total_u32,
            limit,
            offset,
            has_more,
            next_offset,
        };
        Ok(Json(page).into_response())
    }
}

pub fn create_song_router() -> Router<AppState> {
    Router::new().route("/songs/search", get(search_songs))
}
