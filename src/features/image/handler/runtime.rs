use std::time::Instant;

use crate::{error::AppError, state::AppState};

pub(super) fn duration_ms_i64(duration: std::time::Duration) -> i64 {
    i64::try_from(duration.as_millis()).unwrap_or(i64::MAX)
}

fn i64_from_usize(value: usize) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

pub(super) fn blocking_join_error(error: tokio::task::JoinError) -> AppError {
    let error_text = error.to_string();
    if let Ok(panic) = error.try_into_panic() {
        std::panic::resume_unwind(panic);
    }
    AppError::Internal(format!("spawn_blocking cancelled: {error_text}"))
}

pub(super) fn track_image_event(
    stats: &crate::stats_contract::StatsHandle,
    route: &'static str,
    feature: &'static str,
    action: &'static str,
    duration_ms: Option<i64>,
    user_hash: Option<String>,
    extra_json: serde_json::Value,
) {
    stats.track(crate::stats_contract::EventInsert {
        ts_utc: chrono::Utc::now(),
        route: Some(route.into()),
        feature: Some(feature.into()),
        action: Some(action.into()),
        method: Some("POST".into()),
        status: None,
        duration_ms,
        user_hash,
        client_ip_hash: None,
        instance: None,
        extra_json: Some(extra_json),
    });
}

pub(super) struct RenderPermitTiming {
    pub(super) _permit: tokio::sync::OwnedSemaphorePermit,
    pub(super) permits_avail: i64,
    pub(super) wait_ms: i64,
    pub(super) wait_elapsed: std::time::Duration,
}

pub(super) async fn acquire_render_permit(
    state: &AppState,
) -> Result<RenderPermitTiming, AppError> {
    let sem = state.render_semaphore.clone();
    let permits_avail = i64_from_usize(sem.available_permits());
    let started_at = Instant::now();
    let permit = sem
        .acquire_owned()
        .await
        .map_err(|e| AppError::Internal(format!("获取渲染信号量失败: {e}")))?;
    let wait_elapsed = started_at.elapsed();

    Ok(RenderPermitTiming {
        _permit: permit,
        permits_avail,
        wait_ms: duration_ms_i64(wait_elapsed),
        wait_elapsed,
    })
}

pub(super) async fn spawn_blocking_svg_generation<F>(generate: F) -> Result<String, AppError>
where
    F: FnOnce() -> Result<String, AppError> + Send + 'static,
{
    tokio::task::spawn_blocking(generate)
        .await
        .map_err(|e| AppError::Internal(format!("阻塞 SVG 生成任务执行失败: {e}")))?
}
