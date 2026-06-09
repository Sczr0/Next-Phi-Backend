use std::time::Instant;

use crate::config::AppConfig;

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

/// 从 LeanCloud users/me 获取昵称（复用 phigros.cxx 的请求头部）
async fn fetch_nickname(session_token: &str, taptap_version: Option<&str>) -> Option<String> {
    #[derive(serde::Deserialize)]
    struct UserMe {
        nickname: Option<String>,
    }
    let tap_config = AppConfig::global().taptap.resolve(taptap_version);
    let url = format!("{}/users/me", tap_config.leancloud_base_url);
    // 复用全局连接池，避免每次请求创建 Client。
    let client = crate::http::client_default().ok()?;
    let resp = client
        .get(url)
        .header("X-LC-Id", &tap_config.leancloud_app_id)
        .header("X-LC-Key", &tap_config.leancloud_app_key)
        .header("X-LC-Session", session_token)
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let me: UserMe = resp.json().await.ok()?;
    me.nickname
}
