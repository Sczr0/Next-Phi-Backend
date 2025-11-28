//! 优雅退出管理模块
//!
//! 提供跨平台的信号处理和优雅退出协调机制
//! 支持SIGINT、SIGTERM信号和Windows Ctrl+C处理

use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Notify, broadcast};
use tokio::time::timeout;
use tracing::{debug, error, info, warn};

/// 优雅退出管理器
#[derive(Debug, Clone)]
pub struct ShutdownManager {
    /// 内部状态
    inner: Arc<ShutdownInner>,
}

#[derive(Debug)]
struct ShutdownInner {
    /// 退出信号通知器
    notify: Notify,
    /// 退出原因广播通道
    reason_tx: broadcast::Sender<ShutdownReason>,
    /// 最近一次退出原因（用于新订阅者或先触发后等待的场景）
    last_reason: std::sync::Mutex<Option<ShutdownReason>>,
    /// 是否已经开始优雅退出
    shutting_down: std::sync::atomic::AtomicBool,
}

/// 退出原因
#[derive(Debug, Clone)]
pub enum ShutdownReason {
    /// 用户中断信号 (Ctrl+C)
    Interrupt,
    /// 终止信号 (SIGTERM)
    Terminate,
    /// 应用请求退出
    Application,
    /// 超时强制退出
    Timeout,
}

impl ShutdownManager {
    /// 创建新的优雅退出管理器
    pub fn new() -> Self {
        let (reason_tx, _) = broadcast::channel(16);

        Self {
            inner: Arc::new(ShutdownInner {
                notify: Notify::new(),
                reason_tx,
                last_reason: std::sync::Mutex::new(None),
                shutting_down: std::sync::atomic::AtomicBool::new(false),
            }),
        }
    }

    /// 等待退出信号
    ///
    /// # Returns
    ///
    /// 退出原因
    pub async fn wait_for_shutdown(&self) -> ShutdownReason {
        debug!("等待退出信号...");
        // 如果已经触发过关闭，直接返回最后一次原因
        if self.is_shutting_down() {
            if let Ok(guard) = self.inner.last_reason.lock() {
                return guard.clone().unwrap_or(ShutdownReason::Application);
            }
            return ShutdownReason::Application;
        }

        // 等待通知到来，然后读取最后一次原因
        self.inner.notify.notified().await;
        if let Ok(guard) = self.inner.last_reason.lock() {
            guard.clone().unwrap_or(ShutdownReason::Application)
        } else {
            ShutdownReason::Application
        }
    }

    /// 带超时的等待退出信号
    ///
    /// # Arguments
    ///
    /// * `duration` - 超时时间
    ///
    /// # Returns
    ///
    /// 退出原因或超时错误
    pub async fn wait_for_shutdown_with_timeout(
        &self,
        duration: Duration,
    ) -> Result<ShutdownReason, ShutdownError> {
        match timeout(duration, self.wait_for_shutdown()).await {
            Ok(reason) => Ok(reason),
            Err(_) => {
                warn!("优雅退出超时，准备强制退出");
                Err(ShutdownError::Timeout)
            }
        }
    }

    /// 触发优雅退出
    ///
    /// # Arguments
    ///
    /// * `reason` - 退出原因
    pub fn trigger_shutdown(&self, reason: ShutdownReason) {
        // 使用原子操作确保只触发一次
        let was_shutting_down = self
            .inner
            .shutting_down
            .compare_exchange(
                false,
                true,
                std::sync::atomic::Ordering::SeqCst,
                std::sync::atomic::Ordering::SeqCst,
            )
            .unwrap_or(true);

        if !was_shutting_down {
            info!("触发优雅退出: {:?}", reason);

            // 发送退出原因
            if let Err(e) = self.inner.reason_tx.send(reason.clone()) {
                warn!("发送退出原因失败: {}", e);
            }

            // 记录最后一次退出原因
            if let Ok(mut guard) = self.inner.last_reason.lock() {
                *guard = Some(reason);
            }

            // 通知所有等待者
            self.inner.notify.notify_waiters();
        } else {
            debug!("重复的退出信号被忽略");
        }
    }

    /// 检查是否正在关闭
    pub fn is_shutting_down(&self) -> bool {
        self.inner
            .shutting_down
            .load(std::sync::atomic::Ordering::SeqCst)
    }

    /// 创建退出原因接收器
    ///
    /// 用于其他组件监听退出事件
    pub fn subscribe(&self) -> broadcast::Receiver<ShutdownReason> {
        self.inner.reason_tx.subscribe()
    }

    /// 启动信号处理器
    ///
    /// 在Linux/macOS上监听SIGINT和SIGTERM
    /// 在Windows上监听Ctrl+C
    pub async fn start_signal_handler(&self) -> Result<(), ShutdownError> {
        #[cfg(unix)]
        {
            self.start_unix_signal_handler().await
        }

        #[cfg(windows)]
        {
            self.start_windows_signal_handler().await
        }
    }

    #[cfg(unix)]
    async fn start_unix_signal_handler(&self) -> Result<(), ShutdownError> {
        use tokio::signal::unix::{SignalKind, signal};

        info!("启动Unix信号处理器");

        // 创建SIGINT处理器 (Ctrl+C)
        let mut sigint = signal(SignalKind::interrupt())
            .map_err(|e| ShutdownError::SignalSetup(e.to_string()))?;

        // 创建SIGTERM处理器
        let mut sigterm = signal(SignalKind::terminate())
            .map_err(|e| ShutdownError::SignalSetup(e.to_string()))?;

        let manager = self.clone();

        // 启动信号监听任务
        tokio::spawn(async move {
            tokio::select! {
                // SIGINT信号 (Ctrl+C)
                _ = sigint.recv() => {
                    info!("接收到SIGINT信号 (Ctrl+C)");
                    manager.trigger_shutdown(ShutdownReason::Interrupt);
                }
                // SIGTERM信号
                _ = sigterm.recv() => {
                    info!("接收到SIGTERM信号");
                    manager.trigger_shutdown(ShutdownReason::Terminate);
                }
            }
        });

        Ok(())
    }

    #[cfg(windows)]
    async fn start_windows_signal_handler(&self) -> Result<(), ShutdownError> {
        info!("启动Windows信号处理器");

        let manager = self.clone();

        // 启动Ctrl+C监听任务
        tokio::spawn(async move {
            if let Err(e) = tokio::signal::ctrl_c().await {
                error!("监听Ctrl+C信号失败: {}", e);
                return;
            }

            info!("接收到Ctrl+C信号");
            manager.trigger_shutdown(ShutdownReason::Interrupt);
        });

        Ok(())
    }
}

impl Default for ShutdownManager {
    fn default() -> Self {
        Self::new()
    }
}

/// 优雅退出错误类型
#[derive(Debug, thiserror::Error)]
pub enum ShutdownError {
    #[error("信号设置失败: {0}")]
    SignalSetup(String),

    #[error("优雅退出超时")]
    Timeout,

    #[error("资源清理失败: {0}")]
    CleanupError(String),
}

/// 优雅退出句柄
///
/// 提供给需要执行清理操作的组件使用
#[derive(Debug)]
pub struct ShutdownHandle {
    /// 退出信号接收器
    reason_rx: broadcast::Receiver<ShutdownReason>,
    /// 退出管理器
    manager: ShutdownManager,
}

impl ShutdownHandle {
    /// 创建新的退出句柄
    pub fn new(manager: &ShutdownManager) -> Self {
        Self {
            reason_rx: manager.subscribe(),
            manager: manager.clone(),
        }
    }

    /// 等待退出信号
    ///
    /// # Returns
    ///
    /// 退出原因，如果通道关闭则返回None
    pub async fn wait(&mut self) -> Option<ShutdownReason> {
        self.reason_rx.recv().await.ok()
    }

    /// 检查是否正在关闭
    pub fn is_shutting_down(&self) -> bool {
        self.manager.is_shutting_down()
    }

    /// 带超时的执行清理操作
    ///
    /// # Arguments
    ///
    /// * `cleanup_fn` - 清理操作闭包
    /// * `timeout_duration` - 超时时间
    ///
    /// # Returns
    ///
    /// 清理操作结果
    pub async fn cleanup_with_timeout<F, T>(
        &self,
        cleanup_fn: F,
        timeout_duration: Duration,
    ) -> Result<T, ShutdownError>
    where
        F: std::future::Future<Output = T>,
    {
        match timeout(timeout_duration, cleanup_fn).await {
            Ok(result) => Ok(result),
            Err(_) => {
                error!("清理操作超时");
                Err(ShutdownError::Timeout)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn test_shutdown_manager_basic() {
        let manager = ShutdownManager::new();

        // 初始状态不应该正在关闭
        assert!(!manager.is_shutting_down());

        // 触发退出
        manager.trigger_shutdown(ShutdownReason::Application);

        // 现在应该正在关闭
        assert!(manager.is_shutting_down());

        // 等待退出信号应该立即返回
        let reason = manager.wait_for_shutdown().await;
        matches!(reason, ShutdownReason::Application);
    }

    #[tokio::test]
    async fn test_multiple_triggers() {
        let manager = ShutdownManager::new();

        // 多次触发退出，只有第一次生效
        manager.trigger_shutdown(ShutdownReason::Interrupt);
        manager.trigger_shutdown(ShutdownReason::Terminate);

        let reason = manager.wait_for_shutdown().await;
        // 应该是第一个触发的原因
        matches!(reason, ShutdownReason::Interrupt);
    }

    #[tokio::test]
    async fn test_shutdown_handle() {
        let manager = ShutdownManager::new();
        let mut handle = ShutdownHandle::new(&manager);

        // 初始状态不应该正在关闭
        assert!(!handle.is_shutting_down());

        // 触发退出
        manager.trigger_shutdown(ShutdownReason::Application);

        // 句柄应该能接收到退出信号
        let reason = handle.wait().await;
        assert!(reason.is_some());
        matches!(reason.unwrap(), ShutdownReason::Application);
    }

    #[tokio::test]
    async fn test_timeout_functionality() {
        let manager = ShutdownManager::new();

        // 等待超时（没有触发退出）
        let result = manager
            .wait_for_shutdown_with_timeout(Duration::from_millis(100))
            .await;
        assert!(result.is_err());
        matches!(result.unwrap_err(), ShutdownError::Timeout);
    }
}

impl Clone for ShutdownHandle {
    fn clone(&self) -> Self {
        Self {
            reason_rx: self.manager.subscribe(),
            manager: self.manager.clone(),
        }
    }
}
