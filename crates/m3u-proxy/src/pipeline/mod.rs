//! Clean, refactored pipeline infrastructure
//!
//! This module contains the new refactored pipeline infrastructure with
//! clean separation of concerns:
//! 
//! - **Core**: Pipeline orchestration and execution coordination
//! - **Engines**: Data mapping engines with rule-based processing
//! - **Models**: Pipeline execution tracking and state management  
//! - **Stages**: Individual pipeline stages (data mapping, filtering, etc.)
//!
//! The new architecture removes all legacy iterator/accumulator/chunking complexity
//! in favor of a simpler, more maintainable approach using engines that process
//! records one at a time through ordered rule processors.

pub mod core;
pub mod engines;
pub mod models;
pub mod stages;
pub mod services;
pub mod traits;
pub mod error;

// Re-export key types for easier access
pub use core::{PipelineBuilder, PipelineConfig, PipelineOrchestrator, PipelineOrchestratorFactory};
pub use engines::{
    DataMappingEngine, ChannelDataMappingEngine, ProgramDataMappingEngine, 
    RuleProcessor, RuleResult, FieldModification, DataMappingTestService, DataMappingTestResult,
    RuleValidationResult, PipelineStageType, StageValidator,
    DataMappingValidator, FilteringValidator, NumberingValidator, GenerationValidator,
    ValidationFactory, RuleValidationService
};
pub use models::{PipelineExecution, PipelineStageExecution, PipelineStatus, StageStatus};
pub use stages::{DataMappingStage, FilteringStage, LogoCachingStage, LogoCachingConfig, NumberingStage, GenerationStage};
pub use services::{EngineBasedDataMappingService, PipelineValidationService, ApiValidationService};
pub use traits::{ProgressAware, PipelineStage, PipelineStageFactory};
pub use error::PipelineError;

/// Pipeline stage names for consistent naming across the system
pub mod stage_names {
    pub const DATA_MAPPING: &str = "data_mapping";
    pub const FILTERING: &str = "filtering";
    pub const NUMBERING: &str = "numbering";
    pub const GENERATION: &str = "generation";
}
