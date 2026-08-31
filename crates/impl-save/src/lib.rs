// 原根 crate lint 策略随迁移搬入（代码源自根包；与根 lib.rs 的 allow 列表一致）。
#![allow(
    clippy::similar_names,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::too_many_lines,
    clippy::doc_markdown,
    clippy::struct_excessive_bools,
    clippy::items_after_statements,
    clippy::module_name_repetitions
)]

//! impl-save：存档实现（Charter §3.2——存档：codec 解密 + 整理成领域模型）。
//! Phase 1 纯搬迁 2026-09：client/provider/decryptor/parser/record_parser/
//! summary_parser/inspector 自 features/save 迁入；handler（HTTP 编排）留根。

// 承托被迁移代码的路径约定（与根 crate 等价的模块路径，零编辑搬迁）：
pub mod error {
    //! 错误 re-export。
    pub use phi_http::error::*;
}

pub mod config {
    //! 配置 re-export。
    pub use phi_common::config::*;
}

pub mod startup {
    //! 启动期类型 re-export。
    pub mod chart_loader {
        pub use phi_contract::chart::*;
    }
}

pub mod models {
    //! 领域模型 re-export（provider 的 super::models::DifficultyRecord 路径）。
    pub use phi_contract::save::*;
}

pub mod features {
    //! 业务私有路径 re-export（record_parser 的 crate::features::save::models 路径）。
    pub mod save {
        pub mod models {
            pub use phi_contract::save::*;
        }
    }
}

pub mod http {
    //! 全局 HTTP Client 复用（provider/client 的 crate::http 路径）。
    pub use phi_http::http::*;
}

pub mod client;
pub mod decryptor;
pub mod inspector;
pub mod parser;
pub mod provider;
pub mod record_parser;
pub mod summary_parser;

// ── 解码管线并发预算（自 features/save/mod.rs 迁入，Phase 1 纯搬迁）──
// 根 crate 以 pub(crate) use 链回（handler 使用）。

fn default_save_blocking_parallelism_total() -> usize {
    // /save 路径包含多类 CPU 密集任务；总并发预算需要受控，避免高并发下 blocking 线程池被打满。
    let cpu = std::thread::available_parallelism().map_or(4, std::num::NonZero::get);
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

/// 解码/解密阶段并发信号量。
/// `pub`（原 `pub(crate)`）：跨 crate 后由根 crate 的 mod.rs 以
/// `pub(crate) use` 链回，handler 调用点不变。
#[must_use]
pub fn save_decode_blocking_semaphore() -> &'static std::sync::Arc<tokio::sync::Semaphore> {
    static SAVE_DECODE_BLOCKING_SEMAPHORE: std::sync::LazyLock<
        std::sync::Arc<tokio::sync::Semaphore>,
    > = std::sync::LazyLock::new(|| {
        std::sync::Arc::new(tokio::sync::Semaphore::new(
            default_save_decode_blocking_parallelism(),
        ))
    });
    &SAVE_DECODE_BLOCKING_SEMAPHORE
}

/// RKS 计算阶段并发信号量。
#[must_use]
pub fn save_rks_blocking_semaphore() -> &'static std::sync::Arc<tokio::sync::Semaphore> {
    static SAVE_RKS_BLOCKING_SEMAPHORE: std::sync::LazyLock<
        std::sync::Arc<tokio::sync::Semaphore>,
    > = std::sync::LazyLock::new(|| {
        std::sync::Arc::new(tokio::sync::Semaphore::new(
            default_save_rks_blocking_parallelism(),
        ))
    });
    &SAVE_RKS_BLOCKING_SEMAPHORE
}
