use axum::{
    Router,
    routing::{get, post, put},
};
use serde::Serialize;

pub(crate) mod admin;
mod cursor;
pub(crate) mod profile;
pub(crate) mod ranking;

use crate::{error::AppError, state::AppState};

#[cfg(test)]
pub(crate) use self::admin::require_admin_with_cfg;
pub use self::admin::{
    AdminLeaderboardUserItem, AdminLeaderboardUsersResponse, AdminSetUserStatusRequest,
    AdminUserStatusQuery, AdminUserStatusResponse, AdminUsersQuery, ForceAliasRequest,
    ResolveRequest, SuspiciousItem, get_admin_leaderboard_users, get_admin_user_status,
    get_suspicious, post_admin_user_status, post_alias_force, post_resolve,
};
pub use self::profile::{get_public_profile, put_alias, put_profile};
pub use self::ranking::{RankQuery, TopQuery, get_by_rank, get_top, post_me};

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(example = json!({"ok": true}))]
pub struct OkResponse {
    pub ok: bool,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(example = json!({"ok": true, "alias": "Alice"}))]
pub struct OkAliasResponse {
    pub ok: bool,
    pub alias: String,
}

pub(super) fn normalize_moderation_status(raw: &str) -> Result<(&'static str, i64), AppError> {
    let st = raw.trim().to_lowercase();
    let mapped = match st.as_str() {
        "active" | "approved" => ("active", 0_i64),
        "shadow" => ("shadow", 1_i64),
        "banned" => ("banned", 1_i64),
        "rejected" => ("rejected", 1_i64),
        _ => {
            return Err(AppError::Validation(
                "status 必须为 active|approved|shadow|banned|rejected".into(),
            ));
        }
    };
    Ok(mapped)
}

async fn ensure_not_banned(
    storage: &crate::stats_contract::StatsStorage,
    user_hash: &str,
) -> Result<(), AppError> {
    storage.ensure_user_not_banned(user_hash).await
}

pub(super) async fn apply_user_status(
    storage: &crate::stats_contract::StatsStorage,
    user_hash: &str,
    status_raw: &str,
    reason: Option<&str>,
    admin: &str,
    now: &str,
) -> Result<String, AppError> {
    let (status, hide) = normalize_moderation_status(status_raw)?;
    storage.set_leaderboard_hidden(user_hash, hide != 0).await?;
    storage
        .set_user_moderation_status(user_hash, status, reason, admin, now)
        .await?;
    Ok(status.to_string())
}

pub(super) fn mask_user_prefix(hash: &str) -> String {
    let prefix_end = hash.char_indices().nth(4).map_or(hash.len(), |(i, _)| i);

    let mut out = String::with_capacity(prefix_end + 4);
    out.push_str(&hash[..prefix_end]);
    out.push_str("****");
    out
}

/// 检查字符是否为中日韩（CJK）字符
/// 包括：CJK统一汉字、扩展区A/B、兼容汉字、日文平假名/片假名、韩文音节
pub(super) fn is_cjk_char(c: char) -> bool {
    matches!(c,
        '\u{4E00}'..='\u{9FFF}'   // CJK 统一汉字
        | '\u{3400}'..='\u{4DBF}' // CJK 扩展 A
        | '\u{20000}'..='\u{2A6DF}' // CJK 扩展 B
        | '\u{F900}'..='\u{FAFF}' // CJK 兼容汉字
        | '\u{3040}'..='\u{309F}' // 日文平假名
        | '\u{30A0}'..='\u{30FF}' // 日文片假名
        | '\u{AC00}'..='\u{D7AF}' // 韩文音节
    )
}

pub(super) fn validate_alias_format(alias: &str) -> Result<(), AppError> {
    let char_count = alias.chars().count();
    if !(2..=20).contains(&char_count) {
        return Err(AppError::Validation("别名长度需在 2~20 字符之间".into()));
    }
    if !alias
        .chars()
        .all(|c| c.is_alphanumeric() || c == '.' || c == '_' || c == '-' || is_cjk_char(c))
    {
        return Err(AppError::Validation(
            "别名仅允许字母、数字、中日韩文字和 . _ -".into(),
        ));
    }
    Ok(())
}

pub fn create_leaderboard_router() -> Router<AppState> {
    Router::new()
        .route("/leaderboard/rks/top", get(get_top))
        .route("/leaderboard/rks/by-rank", get(get_by_rank))
        .route("/leaderboard/rks/me", post(post_me))
        .route("/leaderboard/alias", put(put_alias))
        .route("/leaderboard/profile", put(put_profile))
        .route("/public/profile/:alias", get(get_public_profile))
        .route("/admin/leaderboard/suspicious", get(get_suspicious))
        .route("/admin/leaderboard/users", get(get_admin_leaderboard_users))
        .route("/admin/leaderboard/resolve", post(post_resolve))
        .route("/admin/users/status", get(get_admin_user_status))
        .route("/admin/users/status", post(post_admin_user_status))
        .route("/admin/leaderboard/alias/force", post(post_alias_force))
}

#[cfg(test)]
mod tests {
    use axum::http::HeaderMap;

    use super::*;

    #[test]
    fn test_mask_user_prefix() {
        assert_eq!(mask_user_prefix("abcd1234"), "abcd****");
        assert_eq!(mask_user_prefix("ab"), "ab****");
        assert_eq!(mask_user_prefix(""), "****");
    }

    #[test]
    fn test_is_cjk_char() {
        // 中文字符
        assert!(is_cjk_char('中'));
        assert!(is_cjk_char('文'));
        assert!(is_cjk_char('测'));
        assert!(is_cjk_char('试'));

        // 日文平假名
        assert!(is_cjk_char('あ'));
        assert!(is_cjk_char('い'));

        // 日文片假名
        assert!(is_cjk_char('ア'));
        assert!(is_cjk_char('イ'));

        // 韩文
        assert!(is_cjk_char('한'));
        assert!(is_cjk_char('글'));

        // 非 CJK 字符
        assert!(!is_cjk_char('a'));
        assert!(!is_cjk_char('Z'));
        assert!(!is_cjk_char('1'));
        assert!(!is_cjk_char('.'));
        assert!(!is_cjk_char('_'));
        assert!(!is_cjk_char('-'));
    }

    #[test]
    fn test_alias_validation_with_chinese() {
        // 测试别名验证逻辑（模拟）
        let valid_aliases = vec![
            "测试用户",
            "Alice测试",
            "用户123",
            "test_用户",
            "玩家.名",
            "日本語テスト",
            "한글테스트",
        ];

        for alias in valid_aliases {
            let char_count = alias.chars().count();
            let is_valid = (2..=20).contains(&char_count)
                && alias.chars().all(|c| {
                    c.is_alphanumeric() || c == '.' || c == '_' || c == '-' || is_cjk_char(c)
                });
            assert!(is_valid, "别名 '{alias}' 应该有效");
        }

        // 无效别名
        let invalid_aliases = vec![
            "a",          // 太短
            "测",         // 太短
            "test@user",  // 包含非法字符 @
            "user#name",  // 包含非法字符 #
            "name space", // 包含空格
        ];

        for alias in invalid_aliases {
            let char_count = alias.chars().count();
            let is_valid = (2..=20).contains(&char_count)
                && alias.chars().all(|c| {
                    c.is_alphanumeric() || c == '.' || c == '_' || c == '-' || is_cjk_char(c)
                });
            assert!(!is_valid, "别名 '{alias}' 应该无效");
        }
    }

    #[test]
    fn test_normalize_moderation_status() {
        assert_eq!(normalize_moderation_status("active").unwrap().0, "active");
        assert_eq!(normalize_moderation_status("approved").unwrap().0, "active");
        assert_eq!(normalize_moderation_status("shadow").unwrap().0, "shadow");
        assert_eq!(normalize_moderation_status("banned").unwrap().0, "banned");
        assert!(normalize_moderation_status("unknown").is_err());
    }

    #[test]
    fn test_require_admin_env() {
        // 避免测试间共享全局配置导致的竞态：直接构造 cfg 注入。
        let mut cfg = crate::config::AppConfig::default();
        cfg.leaderboard.admin_tokens = vec!["t1".into(), "t2".into()];

        let mut headers = HeaderMap::new();
        headers.insert("x-admin-token", axum::http::HeaderValue::from_static("t2"));
        assert!(require_admin_with_cfg(&cfg, &headers).is_ok());
        headers.insert("x-admin-token", axum::http::HeaderValue::from_static("bad"));
        assert!(require_admin_with_cfg(&cfg, &headers).is_err());
    }
}
