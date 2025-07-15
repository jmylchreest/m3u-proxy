//! Utility modules for the m3u-proxy application
//!
//! This module contains reusable utilities that can be used
//! across different parts of the system.

pub mod datetime;
pub mod logo;
pub mod memory_cleanup;
pub mod memory_config;
pub mod memory_context;
pub mod memory_monitor;
pub mod sqlite;
pub mod time;
pub mod url;
pub mod uuid_parser;
pub mod validation;

// Re-export commonly used types for convenience
pub use memory_cleanup::{
    CleanupStrategy, MemoryCleanable, MemoryCleanupCoordinator, StageTransition,
};
pub use memory_config::{MemoryMonitoringConfig, MemoryVerbosity};
pub use memory_context::{MemoryAnalysis, MemoryContext, MemoryEfficiencyTrend, StageMemoryInfo};
pub use memory_monitor::{MemoryLimitStatus, MemorySnapshot, MemoryStats, SimpleMemoryMonitor};
pub use uuid_parser::{resolve_proxy_id, uuid_to_base64, uuid_to_hex32};
