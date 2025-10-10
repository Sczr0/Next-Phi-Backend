use std::collections::HashMap;

use axum::{
    Router,
    extract::{Query, State},
    response::{IntoResponse, Json},
    routing::get,
};

use crate::error::AppError;
use crate::features::song::models::SongInfo;
use crate::state::AppState;

/// oneOf 响应：单个 SongInfo 或 Vec<SongInfo>
#[derive(serde::Serialize, utoipa::ToSchema)]
#[serde(untagged)]
pub enum SongSearchResult {
    Single(SongInfo),
    Multiple(Vec<SongInfo>),
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
        ("unique" = bool, Query, description = "是否强制唯一匹配（可选）")
    ),
    responses(
        (status = 200, description = "查询成功（unique=true 时返回单个对象，否则为列表）", body = SongSearchResult),
        (status = 404, description = "未找到匹配项", body = AppError),
        (status = 409, description = "结果不唯一（提供候选）", body = AppError),
        (status = 400, description = "请求参数错误", body = AppError),
    ),
    tag = "Song"
)]
pub async fn search_songs(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<axum::response::Response, AppError> {
    let q = match params.get("q").map(|s| s.trim()).filter(|s| !s.is_empty()) {
        Some(q) => q.to_string(),
        None => return Err(AppError::Json("缺少查询参数 q".into())),
    };

    let unique = params.get("unique").map(|v| parse_bool(v)).unwrap_or(false);

    if unique {
        let item = state.song_catalog.search_unique(&q)?;
        Ok(Json::<SongInfo>(item.as_ref().clone()).into_response())
    } else {
        let items = state.song_catalog.search(&q);
        let list: Vec<SongInfo> = items.into_iter().map(|a| a.as_ref().clone()).collect();
        Ok(Json(list).into_response())
    }
}

pub fn create_song_router() -> Router<AppState> {
    Router::new().route("/songs/search", get(search_songs))
}
