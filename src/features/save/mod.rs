pub mod client;
pub mod decryptor;
pub mod handler;
pub mod inspector;
pub mod models;
pub mod parser;
pub mod provider;
pub mod record_parser;
pub mod summary_parser;

// Re-exports for external use (main.rs, OpenAPI, etc.)
pub use client::ExternalApiCredentials;
pub use handler::{create_save_router, get_save_data};
pub use models::{SaveResponse, UnifiedSaveRequest};
pub use provider::SaveSource;

// Phase 1 纯搬迁：并发预算信号量定义已迁至 impl-save（解码管线职责），
// 此处 pub(crate) use 链回，handler 调用点不变（root 只用 RKS 阶段信号量；
// 解码阶段信号量由 impl-save 内部使用）。
pub(crate) use impl_save::save_rks_blocking_semaphore;
