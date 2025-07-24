//! Data Mapping Service Compatibility Layer
//!
//! This module provides backward compatibility for the old data_mapping module
//! by re-exporting the new engine-based implementation.

// Re-export the new engine-based service under the old name for compatibility
pub use crate::pipeline::services::data_mapping::EngineBasedDataMappingService as DataMappingService;

// Re-export engine components that may still be needed
pub use crate::pipeline::engines::{DataMappingEngine, ChannelDataMappingEngine, ProgramDataMappingEngine};