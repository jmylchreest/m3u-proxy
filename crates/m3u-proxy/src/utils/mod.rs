//! Utility modules for the m3u-proxy application
//!
//! This module contains reusable utilities that can be used
//! across different parts of the system.

pub mod datetime;
pub mod human_format;
pub mod logo;
pub mod memory_cleanup;
pub mod memory_config;
pub mod memory_context;
pub mod pressure_monitor;
pub mod sqlite;
pub mod system_manager;
pub mod time;
pub mod url;
pub mod uuid_parser;
pub mod validation;

// Re-export commonly used types for convenience
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
pub use system_manager::SystemManager;
pub use uuid_parser::{resolve_proxy_id, uuid_to_base64, uuid_to_hex32};
