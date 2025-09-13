//! Data Mapping Service Compatibility Layer
//!
//! This module provides backward compatibility for the old data_mapping module
//! by re-exporting the new SeaORM-based implementation.

// Re-export the new SeaORM-based service under the old name for compatibility
pub use crate::pipeline::services::seaorm_data_mapping::SeaOrmDataMappingService as DataMappingService;

// Re-export engine components that may still be needed
pub use crate::pipeline::engines::{
    ChannelDataMappingEngine, DataMappingEngine, ProgramDataMappingEngine,
};
