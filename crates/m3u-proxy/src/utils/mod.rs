//! Utility modules for the m3u-proxy application
//!
//! This module contains reusable utilities that can be used
//! across different parts of the system.

pub mod cron_helper;
pub mod database_operations;
pub mod database_retry;
pub mod datetime;
pub mod decompression;
pub mod deterministic_uuid;
pub mod http_client;
pub mod human_format;
pub mod jitter;
pub mod log_capture;
pub mod logo;
pub mod memory_cleanup;
pub mod memory_config;
pub mod memory_stats;
// pub mod memory_context; // Temporarily disabled - needs simplification
// pub mod memory_pressure_calculator; // Removed - no longer needed
pub mod pressure_monitor;
pub mod regex_preprocessor;
pub mod sandbox_health;
pub mod system_manager;
pub mod time;
pub mod url;
pub mod uuid_parser;
pub mod validation;
pub mod xmltv_parser;

// Re-export commonly used types for convenience
pub use cron_helper::{calculate_next_scheduled_time, calculate_next_scheduled_time_validated};
pub use database_operations::DatabaseOperations;
pub use database_retry::{RetryConfig, with_retry};
pub use decompression::{CompressionFormat, DecompressionService};
pub use deterministic_uuid::{generate_channel_uuid, generate_deterministic_uuid, generate_proxy_config_uuid, generate_relay_config_uuid};
pub use http_client::{DecompressingHttpClient, StandardHttpClient, FallbackHttpClient};
pub use human_format::{format_duration, format_memory, format_memory_delta};
pub use memory_cleanup::{
    CleanupStrategy, MemoryCleanable, MemoryCleanupCoordinator, StageTransition,
};
// Memory monitoring modules available for future pipeline integration
// but not exposed to prevent accidental usage
pub use regex_preprocessor::{RegexPreprocessor, RegexPreprocessorConfig};
pub use system_manager::SystemManager;
pub use url::UrlUtils;
pub use uuid_parser::{resolve_proxy_id, uuid_to_base64, uuid_to_hex32, deserialize_optional_uuid};
