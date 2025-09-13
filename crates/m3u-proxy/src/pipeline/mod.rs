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
pub mod error;
pub mod models;
pub mod services;
pub mod stages;
pub mod traits;

// Re-export key types for easier access
pub use core::{
    PipelineBuilder, PipelineConfig, PipelineOrchestrator, PipelineOrchestratorFactory,
};
pub use engines::{
    ChannelDataMappingEngine, DataMappingEngine, DataMappingTestResult, DataMappingTestService,
    DataMappingValidator, EpgDataMappingTestResult, EpgDataMappingTestService,
    EpgProgramTestResult, FieldModification, FilteringValidator, GenerationValidator,
    NumberingValidator, PipelineStageType, ProgramDataMappingEngine, RuleProcessor, RuleResult,
    RuleValidationResult, RuleValidationService, StageValidator, ValidationFactory,
};
pub use error::PipelineError;
pub use models::{PipelineExecution, PipelineStageExecution, PipelineStatus, StageStatus};
pub use services::{ApiValidationService, PipelineValidationService, SeaOrmDataMappingService};
pub use stages::{
    DataMappingStage, FilteringStage, GenerationStage, LogoCachingConfig, LogoCachingStage,
    NumberingStage,
};
pub use traits::{PipelineStage, PipelineStageFactory, ProgressAware};

/// Pipeline stage names for consistent naming across the system
pub mod stage_names {
    pub const DATA_MAPPING: &str = "data_mapping";
    pub const FILTERING: &str = "filtering";
    pub const NUMBERING: &str = "numbering";
    pub const GENERATION: &str = "generation";
}
