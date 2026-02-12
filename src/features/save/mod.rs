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

fn default_save_blocking_parallelism_total() -> usize {
    // /save 路径包含多类 CPU 密集任务；总并发预算需要受控，避免高并发下 blocking 线程池被打满。
    let cpu = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);
    cpu.clamp(2, 16)
}

fn default_save_decode_blocking_parallelism() -> usize {
    // 解压/解密阶段更容易放大内存占用，分配半数预算。
    (default_save_blocking_parallelism_total() / 2).max(1)
}

fn default_save_rks_blocking_parallelism() -> usize {
    // RKS 阶段使用剩余预算，保证两类任务总预算不超过 total。
    let total = default_save_blocking_parallelism_total();
    let decode = default_save_decode_blocking_parallelism();
    total.saturating_sub(decode).max(1)
}

pub(crate) fn save_decode_blocking_semaphore() -> &'static Arc<Semaphore> {
    static SAVE_DECODE_BLOCKING_SEMAPHORE: Lazy<Arc<Semaphore>> =
        Lazy::new(|| Arc::new(Semaphore::new(default_save_decode_blocking_parallelism())));
    &SAVE_DECODE_BLOCKING_SEMAPHORE
}

pub(crate) fn save_rks_blocking_semaphore() -> &'static Arc<Semaphore> {
    static SAVE_RKS_BLOCKING_SEMAPHORE: Lazy<Arc<Semaphore>> =
        Lazy::new(|| Arc::new(Semaphore::new(default_save_rks_blocking_parallelism())));
    &SAVE_RKS_BLOCKING_SEMAPHORE
}
