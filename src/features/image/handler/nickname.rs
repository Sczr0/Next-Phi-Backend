use std::time::Instant;

use super::runtime::duration_ms_i64;

pub(super) async fn resolve_display_name(
    nickname: Option<String>,
    session_token: Option<String>,
    taptap_version: Option<&str>,
) -> (String, i64) {
    if let Some(name) = nickname {
        return (name, 0);
    }

    if let Some(token) = session_token {
        let started_at = Instant::now();
        let name = fetch_nickname(&token, taptap_version)
            .await
            .unwrap_or_else(|| "Phigros Player".into());
        return (name, duration_ms_i64(started_at.elapsed()));
    }

    ("Phigros Player".into(), 0)
}

/// 从 LeanCloud users/me 获取昵称（复用 phigros.cxx 的请求头部）。
/// 委托顶层共享实现（ADR-0004）：带进程内缓存与总超时，
/// 图片链路不再每次渲染都打上游。
async fn fetch_nickname(session_token: &str, taptap_version: Option<&str>) -> Option<String> {
    crate::nickname::resolve_session_nickname(session_token, taptap_version).await
}
