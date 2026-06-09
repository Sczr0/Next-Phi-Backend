use axum::{
    extract::{Query, State},
    response::Json,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::{error::AppError, state::AppState};

use super::super::archive::archive_one_day;

#[derive(Deserialize)]
pub struct ArchiveQuery {
    pub(super) date: Option<String>,
}

#[derive(Serialize, utoipa::ToSchema)]
#[schema(example = json!({"ok": true, "date": "2025-12-23"}))]
#[serde(rename_all = "camelCase")]
pub struct ArchiveNowResponse {
    pub ok: bool,
    pub date: String,
}

#[utoipa::path(
    post,
    path = "/stats/archive/now",
    summary = "手动触发某日归档",
    description = "将指定日期（默认昨天）的明细导出为 Parquet 文件，落地到配置的归档目录",
    params(("date" = Option<String>, Query, description = "归档日期 YYYY-MM-DD，默认为昨天")),
    responses(
        (status = 200, description = "归档已触发", body = ArchiveNowResponse),
        (
            status = 422,
            description = "参数校验失败（日期格式等）",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        ),
        (
            status = 500,
            description = "统计存储未初始化/归档失败",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        )
    ),
    tag = "Stats"
)]
pub async fn trigger_archive_now(
    State(state): State<AppState>,
    Query(q): Query<ArchiveQuery>,
) -> Result<Json<ArchiveNowResponse>, AppError> {
    let day = if let Some(d) = q.date {
        chrono::NaiveDate::parse_from_str(&d, "%Y-%m-%d")
            .map_err(|e| AppError::Validation(format!("date 无效（期望 YYYY-MM-DD）: {e}")))?
    } else {
        (Utc::now() - chrono::Duration::days(1)).date_naive()
    };
    let storage = state
        .stats_storage
        .as_ref()
        .ok_or_else(|| AppError::Internal("统计存储未初始化".into()))?;
    archive_one_day(storage, &crate::config::StatsArchiveConfig::default(), day).await?;
    Ok(Json(ArchiveNowResponse {
        ok: true,
        date: day.to_string(),
    }))
}
