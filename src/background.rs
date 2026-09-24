//! 后台长驻任务注册表（Phase 5）。
//!
//! 现状问题：每日聚合 / 归档维护 / 启动补齐 / 水印口令等循环此前各自 `tokio::spawn`，
//! 既不保留 `JoinHandle`、也无协作式停止——关停时进行中的维护被直接丢弃。
//!
//! 本模块统一登记这些任务的句柄，并提供**协作式关停**：先发关停信号，再在超时内
//! 等待任务从 `select!` 的安全点退出，超时才强制中止。任务自身只需在 `select!` 中
//! 监听 `shutdown_receiver()`，即可在两次周期动作之间干净退出（不会中断进行中的事务）。

use std::sync::Mutex;
use std::time::Duration;

use tokio::sync::watch;
use tokio::task::JoinSet;

/// 后台任务注册表。以 `Arc<BackgroundTasks>` 在组合根共享。
pub struct BackgroundTasks {
    shutdown_tx: watch::Sender<bool>,
    tasks: Mutex<JoinSet<()>>,
}

impl BackgroundTasks {
    #[must_use]
    pub fn new() -> Self {
        let (shutdown_tx, _rx) = watch::channel(false);
        Self {
            shutdown_tx,
            tasks: Mutex::new(JoinSet::new()),
        }
    }

    /// 订阅关停信号。任务循环应在 `select!` 中监听它，收到后从安全点退出。
    #[must_use]
    pub fn shutdown_receiver(&self) -> watch::Receiver<bool> {
        self.shutdown_tx.subscribe()
    }

    /// 登记并启动一个长驻任务：闭包接收关停接收器，返回要 spawn 的 future。
    pub fn spawn<F, Fut>(&self, f: F)
    where
        F: FnOnce(watch::Receiver<bool>) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = ()> + Send + 'static,
    {
        let rx = self.shutdown_receiver();
        let mut guard = self
            .tasks
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        guard.spawn(f(rx));
    }

    /// 协作式关停：发信号 → 有界等待退出 → 超时强制中止。
    pub async fn shutdown(&self, timeout: Duration) {
        let _ = self.shutdown_tx.send(true);
        let mut set = {
            let mut guard = self
                .tasks
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            std::mem::take(&mut *guard)
        };
        if set.is_empty() {
            return;
        }
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                set.abort_all();
                break;
            }
            match tokio::time::timeout(remaining, set.join_next()).await {
                Ok(Some(_)) => {}
                Ok(None) => break,
                Err(_) => {
                    set.abort_all();
                    break;
                }
            }
        }
        // 收尾被中止的任务，确保句柄全部结束。
        while set.join_next().await.is_some() {}
    }
}

impl Default for BackgroundTasks {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::BackgroundTasks;
    use std::time::Duration;

    #[tokio::test]
    async fn shutdown_stops_cooperative_task_promptly() {
        let tasks = BackgroundTasks::new();
        tasks.spawn(|mut shutdown| async move {
            // 长睡眠，但 select! 监听关停信号 → 应被唤醒退出。
            tokio::select! {
                () = tokio::time::sleep(Duration::from_secs(3600)) => {}
                _ = shutdown.changed() => {}
            }
        });

        let started = std::time::Instant::now();
        tasks.shutdown(Duration::from_secs(5)).await;
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "协作式任务应在收到关停信号后立即退出"
        );
    }

    #[tokio::test]
    async fn shutdown_is_noop_without_tasks() {
        let tasks = BackgroundTasks::new();
        tasks.shutdown(Duration::from_millis(50)).await;
    }
}
