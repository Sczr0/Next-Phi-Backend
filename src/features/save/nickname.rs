//! 会话昵称解析（LeanCloud `users/me`）——`/save` 响应与图片链路共用的单一实现。
//!
//! 决策记录见 `docs/adr/0004-save-api-nickname-field.md`：
//! - 昵称是账号侧数据，存档格式不含，只能携会话令牌请求 `users/me`；
//! - 失败/超时一律降级为 `None`（响应省略字段），绝不向上传播错误；
//! - 进程内 moka 缓存限界上游调用频率，仅缓存成功结果（瞬态失败下次重试）。

use std::time::Duration;

use moka::future::Cache;
use once_cell::sync::OnceCell;
use sha2::{Digest, Sha256};

/// 昵称缓存 TTL：用户改名最迟约 10 分钟反映（ADR-0004）。
const NICKNAME_CACHE_TTL: Duration = Duration::from_secs(600);
/// 昵称缓存容量：DAU 100-200 量级足够，不设配置面（ADR-0004）。
const NICKNAME_CACHE_MAX_ENTRIES: u64 = 1024;
/// `users/me` 总超时：`client_default` 仅有 10s 连接超时、无总超时，
/// 必须显式限定，防止上游挂起拖住 /save 响应（ADR-0004）。
const USERS_ME_TIMEOUT: Duration = Duration::from_secs(2);

fn nickname_cache() -> &'static Cache<String, String> {
    static CACHE: OnceCell<Cache<String, String>> = OnceCell::new();
    CACHE.get_or_init(|| {
        Cache::builder()
            .max_capacity(NICKNAME_CACHE_MAX_ENTRIES)
            .time_to_live(NICKNAME_CACHE_TTL)
            .build()
    })
}

/// 缓存 key：令牌的 SHA-256 前 16 字节 hex。
/// 不以原始令牌为 key（避免敏感凭证明文驻留内存诊断面），也不依赖
/// `user_hash_salt` 配置（未配 salt 时 user_hash 为 None，不影响昵称解析）。
fn nickname_cache_key(session_token: &str) -> String {
    let digest = Sha256::digest(session_token.as_bytes());
    hex::encode(&digest[..16])
}

/// 带缓存解析入口（`/save` handler 与图片链路共用）。
/// 仅缓存成功结果；失败/超时返回 `None` 且不落缓存（下次请求重试）。
pub(crate) async fn resolve_session_nickname(
    session_token: &str,
    taptap_version: Option<&str>,
) -> Option<String> {
    let key = nickname_cache_key(session_token);
    cached_resolve(&key, || {
        fetch_nickname_uncached(session_token, taptap_version)
    })
    .await
}

/// 缓存包裹（独立出来便于注入 fetcher 做单测）。
async fn cached_resolve<F, Fut>(key: &str, fetch: F) -> Option<String>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Option<String>>,
{
    if let Some(cached) = nickname_cache().get(key).await {
        return Some(cached);
    }
    let fetched = fetch().await?;
    nickname_cache()
        .insert(key.to_string(), fetched.clone())
        .await;
    Some(fetched)
}

/// 上游请求（无缓存）：`users/me` 取 `nickname`，失败/超时一律 `None`。
async fn fetch_nickname_uncached(
    session_token: &str,
    taptap_version: Option<&str>,
) -> Option<String> {
    #[derive(serde::Deserialize)]
    struct UserMe {
        nickname: Option<String>,
    }
    let tap_config = crate::config::AppConfig::global()
        .taptap
        .resolve(taptap_version);
    let url = format!("{}/users/me", tap_config.leancloud_base_url);
    // 复用全局连接池，避免每次请求创建 Client。
    let client = crate::http::client_default().ok()?;
    let resp = tokio::time::timeout(USERS_ME_TIMEOUT, async {
        client
            .get(url)
            .header("X-LC-Id", &tap_config.leancloud_app_id)
            .header("X-LC-Key", &tap_config.leancloud_app_key)
            .header("X-LC-Session", session_token)
            .send()
            .await
    })
    .await
    .ok()?
    .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let me: UserMe = tokio::time::timeout(USERS_ME_TIMEOUT, resp.json())
        .await
        .ok()?
        .ok()?;
    me.nickname.filter(|n| !n.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn nickname_cache_key_is_stable_hex_and_not_raw_token() {
        let key = nickname_cache_key("r:abcdefg.hijklmn");
        assert_eq!(key.len(), 32);
        assert!(key.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(key, nickname_cache_key("r:abcdefg.hijklmn"));
        assert_ne!(key, nickname_cache_key("r:abcdefg.hijklmo"));
        assert!(!key.contains("abcdefg"));
    }

    #[tokio::test]
    async fn cached_resolve_hits_cache_within_ttl_and_only_caches_success() {
        // 唯一 key，避免与其它测试共享进程级缓存造成顺序依赖。
        let key = format!("test-{}", uuid::Uuid::new_v4());
        let calls = Arc::new(AtomicUsize::new(0));

        let calls1 = Arc::clone(&calls);
        let first = cached_resolve(&key, || {
            let calls = Arc::clone(&calls1);
            async move {
                calls.fetch_add(1, Ordering::SeqCst);
                Some("Alice".to_string())
            }
        })
        .await;
        assert_eq!(first.as_deref(), Some("Alice"));

        // 第二次命中缓存，fetcher 不再被调用。
        let calls2 = Arc::clone(&calls);
        let second = cached_resolve(&key, || {
            let calls = Arc::clone(&calls2);
            async move {
                calls.fetch_add(1, Ordering::SeqCst);
                panic!("cache hit must not invoke fetcher");
            }
        })
        .await;
        assert_eq!(second.as_deref(), Some("Alice"));

        // 失败结果不落缓存：None 之后再次调用仍会尝试 fetch。
        let miss_key = format!("test-{}", uuid::Uuid::new_v4());
        let calls3 = Arc::clone(&calls);
        let missed = cached_resolve(&miss_key, || {
            let calls = Arc::clone(&calls3);
            async move {
                calls.fetch_add(1, Ordering::SeqCst);
                None
            }
        })
        .await;
        assert_eq!(missed, None);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }
}
