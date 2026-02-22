/// 统一错误处理模块
pub mod error;

/// 配置模块
pub mod config;

/// CORS 构建工具
pub mod cors;

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

/// 请求 request_id 中间件与上下文工具
pub mod request_id;

#[path = "contracts/auth_contract.rs"]
pub mod auth_contract;
#[path = "api/auth_qrcode_api.rs"]
pub mod auth_qrcode_api;
pub mod auth_services;
pub mod identity_hash;
#[path = "api/image_api.rs"]
pub mod image_api;
#[path = "api/leaderboard_api.rs"]
pub mod leaderboard_api;
#[path = "contracts/leaderboard_contract.rs"]
pub mod leaderboard_contract;
/// OpenAPI 文档（utoipa）
pub mod openapi;
#[path = "api/rks_api.rs"]
pub mod rks_api;
#[path = "contracts/rks_contract.rs"]
pub mod rks_contract;
#[path = "api/save_api.rs"]
pub mod save_api;
#[path = "contracts/save_contract.rs"]
pub mod save_contract;
pub mod session_auth;
#[path = "api/song_api.rs"]
pub mod song_api;
#[path = "contracts/song_contract.rs"]
pub mod song_contract;
#[path = "contracts/stats_contract.rs"]
pub mod stats_contract;

// 导出常用类型供外部使用
pub use config::AppConfig;
pub use error::AppError;
pub use shutdown::{ShutdownHandle, ShutdownManager, ShutdownReason};
pub use watchdog::SystemdWatchdog;
