/// 统一错误处理模块
pub mod error;

/// 配置模块
pub mod config;

/// 启动检查模块
pub mod startup;

/// 功能聚合模块
pub mod features;

/// 应用状态聚合模块
pub mod state;

/// 优雅退出管理模块
pub mod shutdown;

/// systemd 看门狗模块
pub mod watchdog;

/// HTTP Client 复用工具
pub mod http;

// 导出常用类型供外部使用
pub use config::AppConfig;
pub use error::AppError;
pub use shutdown::{ShutdownHandle, ShutdownManager, ShutdownReason};
pub use watchdog::SystemdWatchdog;
