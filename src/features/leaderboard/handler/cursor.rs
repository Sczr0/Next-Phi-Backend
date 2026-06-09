use base64::Engine;
use serde::{Deserialize, Serialize};

use crate::error::AppError;

#[derive(Debug, Clone)]
pub(super) struct LeaderboardCursor {
    pub(super) score: f64,
    pub(super) updated_at: String,
    pub(super) user_hash: String,
    pub(super) rank_base: Option<i64>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LeaderboardCursorEnvelope {
    version: u8,
    score: f64,
    updated_at: String,
    user_hash: String,
    #[serde(default)]
    rank_base: Option<i64>,
}

fn leaderboard_cursor_secret() -> Option<String> {
    let cfg = crate::config::AppConfig::global();
    let session_secret = cfg.session.jwt_secret.trim();
    if !session_secret.is_empty() {
        return Some(session_secret.to_string());
    }
    cfg.stats
        .user_hash_salt
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned)
}

fn derive_leaderboard_cursor_key(secret: &str) -> [u8; 32] {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;

    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("hmac key");
    mac.update(b"phi-backend/leaderboard/cursor-v1");
    let bytes = mac.finalize().into_bytes();
    let mut key = [0u8; 32];
    key.copy_from_slice(&bytes[..32]);
    key
}

fn validate_leaderboard_cursor(cursor: LeaderboardCursor) -> Result<LeaderboardCursor, AppError> {
    if !cursor.score.is_finite()
        || cursor.updated_at.trim().is_empty()
        || cursor.user_hash.trim().is_empty()
        || cursor.rank_base.is_some_and(|rank| rank <= 0)
    {
        return Err(AppError::Validation(
            "cursor 无效（排序键不能为空、score 必须为有限值且 rankBase 必须为正数）".into(),
        ));
    }
    Ok(cursor)
}

fn seal_leaderboard_cursor_with_secret(
    cursor: &LeaderboardCursor,
    secret: &str,
) -> Result<String, AppError> {
    use aes_gcm::aead::{Aead, KeyInit};

    let cursor = validate_leaderboard_cursor(cursor.clone())?;
    let envelope = LeaderboardCursorEnvelope {
        version: 1,
        score: cursor.score,
        updated_at: cursor.updated_at,
        user_hash: cursor.user_hash,
        rank_base: cursor.rank_base,
    };
    let payload = serde_json::to_vec(&envelope)
        .map_err(|e| AppError::Internal(format!("序列化排行榜游标失败: {e}")))?;
    let key = derive_leaderboard_cursor_key(secret);
    let cipher = aes_gcm::Aes256Gcm::new(aes_gcm::Key::<aes_gcm::Aes256Gcm>::from_slice(&key));
    let nonce_bytes = uuid::Uuid::new_v4().as_bytes().to_owned();
    let nonce = aes_gcm::Nonce::from_slice(&nonce_bytes[..12]);
    let encrypted = cipher
        .encrypt(
            nonce,
            aes_gcm::aead::Payload {
                msg: &payload,
                aad: b"leaderboard-rks-top",
            },
        )
        .map_err(|e| AppError::Internal(format!("加密排行榜游标失败: {e}")))?;

    let mut sealed = Vec::with_capacity(12 + encrypted.len());
    sealed.extend_from_slice(&nonce_bytes[..12]);
    sealed.extend_from_slice(&encrypted);
    Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(sealed))
}

fn open_leaderboard_cursor_with_secret(
    raw: &str,
    secret: &str,
) -> Result<LeaderboardCursor, AppError> {
    use aes_gcm::aead::{Aead, KeyInit};

    let sealed = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(raw)
        .map_err(|_| AppError::Validation("cursor 格式无效".into()))?;
    if sealed.len() < 13 {
        return Err(AppError::Validation("cursor 长度无效".into()));
    }
    let key = derive_leaderboard_cursor_key(secret);
    let cipher = aes_gcm::Aes256Gcm::new(aes_gcm::Key::<aes_gcm::Aes256Gcm>::from_slice(&key));
    let nonce = aes_gcm::Nonce::from_slice(&sealed[..12]);
    let payload = cipher
        .decrypt(
            nonce,
            aes_gcm::aead::Payload {
                msg: &sealed[12..],
                aad: b"leaderboard-rks-top",
            },
        )
        .map_err(|_| AppError::Validation("cursor 无效或不属于当前服务实例".into()))?;
    let envelope: LeaderboardCursorEnvelope = serde_json::from_slice(&payload)
        .map_err(|_| AppError::Validation("cursor 内容无效".into()))?;
    if envelope.version != 1 {
        return Err(AppError::Validation("cursor 版本无效".into()));
    }
    validate_leaderboard_cursor(LeaderboardCursor {
        score: envelope.score,
        updated_at: envelope.updated_at,
        user_hash: envelope.user_hash,
        rank_base: envelope.rank_base,
    })
}

pub(super) fn parse_leaderboard_cursor(
    raw: Option<&str>,
) -> Result<Option<LeaderboardCursor>, AppError> {
    let Some(raw) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    let secret = leaderboard_cursor_secret()
        .ok_or_else(|| AppError::Internal("排行榜 cursor 密钥未配置".into()))?;
    open_leaderboard_cursor_with_secret(raw, &secret).map(Some)
}

pub(super) fn normalize_leaderboard_seek(
    cursor: Option<LeaderboardCursor>,
    after_score: Option<f64>,
    after_updated: Option<String>,
    after_user: Option<String>,
) -> Result<Option<LeaderboardCursor>, AppError> {
    if cursor.is_some() {
        return Ok(cursor);
    }
    let (Some(score), Some(updated_at), Some(user_hash)) = (after_score, after_updated, after_user)
    else {
        return Ok(None);
    };
    validate_leaderboard_cursor(LeaderboardCursor {
        score,
        updated_at,
        user_hash,
        rank_base: None,
    })
    .map(Some)
}

pub(super) fn seal_leaderboard_cursor(cursor: &LeaderboardCursor) -> Option<String> {
    let Some(secret) = leaderboard_cursor_secret() else {
        tracing::warn!(
            target: "phi_backend::leaderboard",
            "leaderboard cursor secret is not configured; nextCursor omitted"
        );
        return None;
    };
    match seal_leaderboard_cursor_with_secret(cursor, &secret) {
        Ok(cursor) => Some(cursor),
        Err(e) => {
            tracing::warn!(target: "phi_backend::leaderboard", "seal leaderboard cursor failed: {e}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leaderboard_cursor_seals_and_opens_sort_keys() {
        let cursor = LeaderboardCursor {
            score: 14.73,
            updated_at: "2025-09-20T04:10:44Z".to_string(),
            user_hash: "0123456789abcdef".to_string(),
            rank_base: Some(51),
        };
        let sealed = seal_leaderboard_cursor_with_secret(&cursor, "test-secret").unwrap();
        assert!(!sealed.contains(&cursor.user_hash));

        let opened = open_leaderboard_cursor_with_secret(&sealed, "test-secret").unwrap();
        assert_eq!(opened.score, cursor.score);
        assert_eq!(opened.updated_at, cursor.updated_at);
        assert_eq!(opened.user_hash, cursor.user_hash);
        assert_eq!(opened.rank_base, cursor.rank_base);
    }

    #[test]
    fn leaderboard_cursor_rejects_wrong_secret_and_invalid_keys() {
        let cursor = LeaderboardCursor {
            score: 14.73,
            updated_at: "2025-09-20T04:10:44Z".to_string(),
            user_hash: "0123456789abcdef".to_string(),
            rank_base: Some(51),
        };
        let sealed = seal_leaderboard_cursor_with_secret(&cursor, "test-secret").unwrap();
        assert!(open_leaderboard_cursor_with_secret(&sealed, "other-secret").is_err());

        let invalid = LeaderboardCursor {
            score: f64::NAN,
            updated_at: "2025-09-20T04:10:44Z".to_string(),
            user_hash: "0123456789abcdef".to_string(),
            rank_base: Some(51),
        };
        assert!(seal_leaderboard_cursor_with_secret(&invalid, "test-secret").is_err());
    }
}
