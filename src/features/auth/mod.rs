pub mod bearer;
pub mod client;
pub mod handler;
pub mod models;
pub mod qrcode_service;

// 对外导出路由构建函数，便于 main.rs 引用
pub use handler::create_auth_router;
