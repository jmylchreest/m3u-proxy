//! Utility modules for the m3u-proxy application
//!
//! This module contains reusable utilities that can be used
//! across different parts of the system.

pub mod database_operations;
pub mod datetime;
pub mod decompression;
pub mod deterministic_uuid;
pub mod http_client;
pub mod human_format;
pub mod log_capture;
pub mod logo;
pub mod memory_cleanup;
pub mod memory_config;
pub mod memory_context;
pub mod pressure_monitor;
pub mod regex_preprocessor;
pub mod sqlite;
pub mod system_manager;
pub mod time;
pub mod url;
pub mod uuid_parser;
pub mod validation;
pub mod xmltv_parser;

// Re-export commonly used types for convenience
pub use database_operations::DatabaseOperations;
pub use decompression::{CompressionFormat, DecompressionService};
pub use deterministic_uuid::{generate_channel_uuid, generate_deterministic_uuid, generate_proxy_config_uuid, generate_relay_config_uuid};
pub use http_client::{DecompressingHttpClient, StandardHttpClient, FallbackHttpClient};
pub use human_format::{format_duration, format_memory, format_memory_delta};
pub use memory_cleanup::{
    CleanupStrategy, MemoryCleanable, MemoryCleanupCoordinator, StageTransition,
};
pub use memory_config::{MemoryMonitoringConfig, MemoryVerbosity};
pub use memory_context::{MemoryAnalysis, MemoryContext, MemoryEfficiencyTrend, StageMemoryInfo};
pub use pressure_monitor::{
    MemoryLimitStatus, MemorySnapshot, MemoryStats, SimpleMemoryMonitor, SystemPressureAssessment,
    SystemPressureMetrics,
};
pub use regex_preprocessor::{RegexPreprocessor, RegexPreprocessorConfig};
pub use system_manager::SystemManager;
pub use uuid_parser::{resolve_proxy_id, uuid_to_base64, uuid_to_hex32};
