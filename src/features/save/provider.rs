//! 存档提供器（Phase 1 已迁至 impl-save）——shim 保持路径不变
//! （contracts/save_contract.rs 的 ParsedSave/SaveMeta/SaveSource/fetch_save_meta
//! /get_decrypted_save_from_meta 经此处链出）。
pub use impl_save::provider::*;
