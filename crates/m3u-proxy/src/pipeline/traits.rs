//! Pipeline Traits
//!
//! Core traits for the pipeline system that provide clean abstractions
//! for progress reporting and stage execution.

use std::sync::Arc;
use crate::services::progress_service::ProgressManager;
use crate::pipeline::models::PipelineArtifact;
use crate::pipeline::error::PipelineError;

/// Trait for components that can report progress through ProgressManager
pub trait ProgressAware {
    /// Get the progress manager for this component
    fn get_progress_manager(&self) -> Option<&Arc<ProgressManager>>;
}

/// Helper struct that provides progress reporting methods
/// This solves the trait object compatibility issue by moving async methods out of the trait
pub struct ProgressReporter<'a> {
    progress_manager: Option<&'a Arc<ProgressManager>>,
}

impl<'a> ProgressReporter<'a> {
    /// Create a new progress reporter from a ProgressAware component
    pub fn new(component: &'a dyn ProgressAware) -> Self {
        Self {
            progress_manager: component.get_progress_manager(),
        }
    }
    
    /// Report progress for a specific stage
    pub async fn report_stage_progress(&self, stage_id: &str, percentage: f64, message: &str) {
        if let Some(pm) = self.progress_manager {
            if let Some(updater) = pm.get_stage_updater(stage_id).await {
                updater.update_progress(percentage, message).await;
            } else {
                tracing::debug!("Stage '{}' not found in ProgressManager, skipping progress update", stage_id);
            }
        }
    }
    
    /// Report item-based progress (e.g., "processed 150 of 1000 channels")
    pub async fn report_item_progress(&self, stage_id: &str, processed: usize, total: usize, message: &str) {
        let percentage = if total > 0 {
            (processed as f64 / total as f64 * 100.0).clamp(0.0, 100.0)
        } else {
            0.0
        };
        
        let detailed_message = format!("{} ({}/{} items)", message, processed, total);
        self.report_stage_progress(stage_id, percentage, &detailed_message).await;
    }
    
    /// Mark a stage as completed
    pub async fn complete_stage(&self, stage_id: &str, message: &str) {
        self.report_stage_progress(stage_id, 100.0, message).await;
        
        if let Some(pm) = self.progress_manager {
            if let Some(updater) = pm.get_stage_updater(stage_id).await {
                updater.complete_stage().await;
            }
        }
    }
}

/// Trait for pipeline stages that can be executed in sequence
#[async_trait::async_trait]
pub trait PipelineStage: ProgressAware + Send + Sync {
    /// Execute this stage with the given input artifacts
    async fn execute(&mut self, input: Vec<PipelineArtifact>) -> Result<Vec<PipelineArtifact>, PipelineError>;
    
    /// Get the unique identifier for this stage
    fn stage_id(&self) -> &'static str;
    
    /// Get the human-readable name for this stage
    fn stage_name(&self) -> &'static str;
    
    /// Cleanup any resources used by this stage
    async fn cleanup(&mut self) -> Result<(), PipelineError> {
        // Default implementation does nothing
        Ok(())
    }
    
    /// Allow downcasting to concrete types for progress manager injection
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any;
}

/// Factory trait for creating pipeline stages
#[async_trait::async_trait]
pub trait PipelineStageFactory {
    /// Create a new instance of a pipeline stage with progress manager injection
    async fn create_stage(
        &self, 
        stage_id: &str,
        progress_manager: Option<Arc<ProgressManager>>
    ) -> Result<Box<dyn PipelineStage>, PipelineError>;
}