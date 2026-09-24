//! 进程内「单飞」计算锁（Phase 3）。
//!
//! 解决缓存击穿/惊群：同一缓存 key 的并发 miss 若各自完整重算（多秒 SQL 聚合、
//! 存档下载解密），会在流量尖峰下成倍放大后端压力。调用方约定：
//!
//! ```ignore
//! if let Some(v) = cache.get(key).await { return v; }      // 快路径（命中不碰锁）
//! let lock = keyed_lock(key).await;
//! let _guard = lock.lock().await;
//! if let Some(v) = cache.get(key).await { return v; }      // 二次检查：别的任务已填好
//! let v = expensive().await?;
//! cache.insert(key, v).await;
//! ```
//!
//! 锁本身用 moka 承载并带 TTI 回收，避免无界增长。

use std::sync::Arc;
use std::time::Duration;

use moka::future::Cache;
use once_cell::sync::Lazy;
use tokio::sync::Mutex;

/// 单飞锁表：key → 该 key 的计算互斥锁。TTI 到期后回收，避免长期驻留。
static LOCKS: Lazy<Cache<String, Arc<Mutex<()>>>> = Lazy::new(|| {
    Cache::builder()
        .max_capacity(4096)
        .time_to_idle(Duration::from_secs(300))
        .build()
});

/// 获取指定 key 的单飞锁。调用方须在获得锁后**二次检查**缓存再决定是否计算。
pub async fn keyed_lock(key: &str) -> Arc<Mutex<()>> {
    LOCKS
        .get_with(key.to_string(), async { Arc::new(Mutex::new(())) })
        .await
}

#[cfg(test)]
mod tests {
    use super::keyed_lock;
    use moka::future::Cache;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    #[tokio::test]
    async fn same_key_shares_lock_distinct_keys_do_not() {
        let a = keyed_lock("k-a").await;
        let a2 = keyed_lock("k-a").await;
        let b = keyed_lock("k-b").await;
        assert!(Arc::ptr_eq(&a, &a2), "同一 key 应返回同一把锁");
        assert!(!Arc::ptr_eq(&a, &b), "不同 key 应返回不同锁");
    }

    #[tokio::test]
    async fn concurrent_misses_compute_exactly_once() {
        let cache: Cache<String, u32> = Cache::builder().build();
        let computes = Arc::new(AtomicUsize::new(0));

        let handles: Vec<_> = (0..32)
            .map(|_| {
                let cache = cache.clone();
                let computes = computes.clone();
                tokio::spawn(async move {
                    let key = "hot".to_string();
                    if let Some(v) = cache.get(&key).await {
                        return v;
                    }
                    let lock = keyed_lock(&key).await;
                    let _guard = lock.lock().await;
                    if let Some(v) = cache.get(&key).await {
                        return v;
                    }
                    computes.fetch_add(1, Ordering::SeqCst);
                    tokio::time::sleep(Duration::from_millis(20)).await;
                    cache.insert(key, 42).await;
                    42
                })
            })
            .collect();

        for h in handles {
            assert_eq!(h.await.expect("join"), 42);
        }
        assert_eq!(
            computes.load(Ordering::SeqCst),
            1,
            "同一 key 的并发 miss 只应计算一次"
        );
    }
}
