//! systemd 看门狗模块
//!
//! 提供systemd看门狗协议支持，定期向systemd发送心跳通知

use crate::config::WatchdogConfig;
use crate::shutdown::{ShutdownHandle, ShutdownManager};
use std::time::Duration;
use tracing::{debug, error, info};

/// systemd 看门狗管理器
#[derive(Debug, Clone)]
pub struct SystemdWatchdog {
    config: WatchdogConfig,
    shutdown_handle: ShutdownHandle,
}

#[cfg(target_os = "linux")]
mod systemd_impl {
    use sd_notify::{NotifyState, notify};
    use tracing::{debug, error};

    /// 通知systemd服务状态
    fn notify_systemd(state: NotifyState) -> Result<(), Box<dyn std::error::Error>> {
        if std::env::var_os("NOTIFY_SOCKET").is_none() {
            debug!("不在systemd环境下运行，忽略通知");
            return Ok(());
        }

        // 发送前记录一次日志，随后将 `state` 移入 `notify`
        debug!("准备发送systemd通知: {:?}", state);

        if let Err(e) = notify(false, &[state]) {
            error!("systemd通知失败: {}", e);
            return Err(Box::new(e));
        }

        Ok(())
    }

    /// 发送ready信号
    pub fn notify_ready() -> Result<(), Box<dyn std::error::Error>> {
        notify_systemd(NotifyState::Ready)
    }

    /// 发送reloading信号
    pub fn notify_reloading() -> Result<(), Box<dyn std::error::Error>> {
        notify_systemd(NotifyState::Reloading)
    }

    /// 发送stopping信号
    pub fn notify_stopping() -> Result<(), Box<dyn std::error::Error>> {
        notify_systemd(NotifyState::Stopping)
    }

    /// 发送看门狗心跳
    pub fn notify_watchdog() -> Result<(), Box<dyn std::error::Error>> {
        notify_systemd(NotifyState::Watchdog)
    }

    /// 获取看门狗超时时间
    pub fn get_watchdog_timeout_us() -> Option<u64> {
        let mut usec: u64 = 0;
        if sd_notify::watchdog_enabled(false, &mut usec) {
            Some(usec)
        } else {
            None
        }
    }
}

#[cfg(not(target_os = "linux"))]
mod systemd_impl {
    use tracing::debug;
    /// 发送ready信号（非Linux平台）
    pub fn notify_ready() -> Result<(), Box<dyn std::error::Error>> {
        debug!("非Linux平台，忽略systemd ready通知");
        Ok(())
    }

    /// 发送reloading信号（非Linux平台）
    pub fn notify_reloading() -> Result<(), Box<dyn std::error::Error>> {
        debug!("非Linux平台，忽略systemd reloading通知");
        Ok(())
    }

    /// 发送stopping信号（非Linux平台）
    pub fn notify_stopping() -> Result<(), Box<dyn std::error::Error>> {
        debug!("非Linux平台，忽略systemd stopping通知");
        Ok(())
    }

    /// 发送看门狗心跳（非Linux平台）
    pub fn notify_watchdog() -> Result<(), Box<dyn std::error::Error>> {
        debug!("非Linux平台，忽略systemd watchdog通知");
        Ok(())
    }
}

impl SystemdWatchdog {
    /// 创建新的看门狗管理器
    pub fn new(config: WatchdogConfig, shutdown_manager: &ShutdownManager) -> Self {
        let shutdown_handle = ShutdownHandle::new(shutdown_manager);

        Self {
            config,
            shutdown_handle,
        }
    }

    /// 发送ready信号到systemd
    pub fn notify_ready(&self) -> Result<(), Box<dyn std::error::Error>> {
        systemd_impl::notify_ready()
    }

    /// 发送reloading信号到systemd
    pub fn notify_reloading(&self) -> Result<(), Box<dyn std::error::Error>> {
        systemd_impl::notify_reloading()
    }

    /// 发送stopping信号到systemd
    pub fn notify_stopping(&self) -> Result<(), Box<dyn std::error::Error>> {
        systemd_impl::notify_stopping()
    }

    /// 启动看门狗心跳任务
    ///
    /// 如果启用了看门狗，将定期发送心跳信号到systemd
    pub async fn start_watchdog_task(&self) -> Result<(), Box<dyn std::error::Error>> {
        if !self.config.enabled {
            info!("看门狗功能已禁用");
            return Ok(());
        }

        // 检查systemd是否支持看门狗
        #[cfg(target_os = "linux")]
        {
            use tracing::warn;

            let watchdog_timeout_us = systemd_impl::get_watchdog_timeout_us();
            if watchdog_timeout_us.is_none() {
                warn!("systemd看门狗未启用或不在systemd环境下运行");
                return Ok(());
            }

            info!("systemd看门狗超时时间: {}μs", watchdog_timeout_us.unwrap());
        }

        info!(
            "启动看门狗任务，间隔: {:?}",
            self.config.interval_duration()
        );

        let interval = self.config.interval_duration();
        let shutdown_handle = self.shutdown_handle.clone();

        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);

            loop {
                tokio::select! {
                    // 定期发送心跳
                    _ = ticker.tick() => {
                        if let Err(e) = systemd_impl::notify_watchdog() {
                            error!("看门狗心跳发送失败: {}", e);
                        } else {
                            debug!("看门狗心跳发送成功");
                        }
                    }
                    // 定期检查退出信号
                    _ = tokio::time::sleep(Duration::from_secs(1)) => {
                        if shutdown_handle.is_shutting_down() {
                            info!("看门狗任务检测到退出信号，停止发送心跳");
                            break;
                        }
                    }
                }
            }
        });

        Ok(())
    }

    /// 验证看门狗配置
    pub fn validate_config(&self) -> Result<(), String> {
        if !self.config.enabled {
            return Ok(());
        }

        // 检查间隔时间是否合理
        if self.config.interval_secs == 0 {
            return Err("看门狗间隔时间不能为0".to_string());
        }

        #[cfg(target_os = "linux")]
        {
            // 在Linux下检查systemd看门狗超时时间
            if let Some(watchdog_timeout_us) = systemd_impl::get_watchdog_timeout_us() {
                let watchdog_timeout_secs = watchdog_timeout_us / 1_000_000;
                let interval_secs = self.config.interval_secs;

                // 建议心跳间隔不超过看门狗超时时间的一半
                if interval_secs >= watchdog_timeout_secs {
                    info!(
                        "看门狗间隔时间({}s)建议小于systemd看门狗超时时间({}s)的一半",
                        interval_secs, watchdog_timeout_secs
                    );
                }

                if interval_secs * 2 >= watchdog_timeout_secs {
                    return Err(format!(
                        "看门狗间隔时间({}s)过大，应小于systemd看门狗超时时间({}s)的一半",
                        interval_secs, watchdog_timeout_secs
                    ));
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shutdown::ShutdownManager;

    #[tokio::test]
    async fn test_watchdog_creation() {
        let config = WatchdogConfig::default();
        let shutdown_manager = ShutdownManager::new();
        let watchdog = SystemdWatchdog::new(config, &shutdown_manager);

        assert!(!watchdog.config.enabled);
    }

    #[test]
    fn test_watchdog_config_validation() {
        let mut config = WatchdogConfig::default();
        let shutdown_manager = ShutdownManager::new();

        // 禁用状态应该验证通过
        config.enabled = false;
        let watchdog = SystemdWatchdog::new(config.clone(), &shutdown_manager);
        assert!(watchdog.validate_config().is_ok());

        // 启用但间隔时间为0应该失败
        config.enabled = true;
        config.interval_secs = 0;
        let watchdog = SystemdWatchdog::new(config.clone(), &shutdown_manager);
        assert!(watchdog.validate_config().is_err());

        // 启用且间隔时间合理应该通过
        config.interval_secs = 10;
        let watchdog = SystemdWatchdog::new(config.clone(), &shutdown_manager);
        assert!(watchdog.validate_config().is_ok());
    }

    #[test]
    fn test_systemd_notifications() {
        let shutdown_manager = ShutdownManager::new();
        let watchdog = SystemdWatchdog::new(WatchdogConfig::default(), &shutdown_manager);

        // 这些调用在任何平台上都不应该panic
        let _ = watchdog.notify_ready();
        let _ = watchdog.notify_reloading();
        let _ = watchdog.notify_stopping();
    }
}
