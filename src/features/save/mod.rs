pub mod client;
pub mod decryptor;
pub mod handler;
pub mod inspector;
pub mod models;
pub mod parser;
pub mod provider;
pub mod record_parser;
pub mod summary_parser;

use once_cell::sync::Lazy;
use std::sync::Arc;
use tokio::sync::Semaphore;

// Re-exports for external use (main.rs, OpenAPI, etc.)
pub use client::ExternalApiCredentials;
pub use handler::{create_save_router, get_save_data};
pub use models::{SaveResponse, UnifiedSaveRequest};
pub use provider::SaveSource;

fn default_save_blocking_parallelism() -> usize {
    // /save 路径包含解压/解密/解析等 CPU 密集任务，使用固定上限避免高并发下 blocking 线程池被打满。
    let cpu = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);
    cpu.clamp(2, 16)
}

pub(crate) fn save_blocking_semaphore() -> &'static Arc<Semaphore> {
    static SAVE_BLOCKING_SEMAPHORE: Lazy<Arc<Semaphore>> =
        Lazy::new(|| Arc::new(Semaphore::new(default_save_blocking_parallelism())));
    &SAVE_BLOCKING_SEMAPHORE
}
