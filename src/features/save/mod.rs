pub mod client;
pub mod decryptor;
pub mod handler;
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
