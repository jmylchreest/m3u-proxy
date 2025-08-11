//! New Pipeline Orchestrator Implementation
//!
//! This is the refactored orchestrator that uses the new trait-based architecture
//! with clean ProgressManager integration and simplified stage management.

use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{info, warn, error, debug};
use crate::pipeline::models::{PipelineExecution, PipelineStatus};
use crate::pipeline::traits::{PipelineStage, ProgressAware, ProgressReporter};
use crate::pipeline::error::PipelineError;
use crate::services::progress_service::ProgressManager;
use sandboxed_file_manager::SandboxedManager;

/// Default suspension duration for pipeline operations (5 minutes)
const DEFAULT_PIPELINE_SUSPENSION_DURATION: Duration = Duration::from_secs(5 * 60);

/// Pipeline orchestrator using trait-based architecture with ProgressManager
pub struct PipelineOrchestrator {
    execution: PipelineExecution,
    file_manager: SandboxedManager,
    _proxy_output_file_manager: SandboxedManager,
    progress_manager: Option<Arc<ProgressManager>>,
    stages: Vec<Box<dyn PipelineStage>>,
}

impl PipelineOrchestrator {
    /// Create a new orchestrator with the given configuration
    pub fn new(
        execution: PipelineExecution,
        file_manager: SandboxedManager,
        proxy_output_file_manager: SandboxedManager,
        progress_manager: Option<Arc<ProgressManager>>,
    ) -> Self {
        Self {
            execution,
            file_manager,
            _proxy_output_file_manager: proxy_output_file_manager,
            progress_manager,
            stages: Vec::new(),
        }
    }
    
    /// Create orchestrator with all dependencies injected (legacy compatibility)
    pub fn new_with_dependencies(
        proxy_config: crate::models::StreamProxy,
        file_manager: SandboxedManager,
        proxy_output_file_manager: SandboxedManager,
        logo_service: Arc<crate::logo_assets::service::LogoAssetService>,
        logo_config: crate::pipeline::stages::logo_caching::LogoCachingConfig,
        db_pool: sqlx::SqlitePool,
    ) -> Self {
        let execution = PipelineExecution::new(proxy_config.id);
        let mut orchestrator = Self {
            execution,
            file_manager,
            _proxy_output_file_manager: proxy_output_file_manager,
            progress_manager: None, // Will be set later if needed
            stages: Vec::new(),
        };
        
        // Create and add all pipeline stages in order
        orchestrator.create_and_add_all_stages(proxy_config, logo_service, logo_config, db_pool);
        
        orchestrator
    }
    
    /// Add a stage to the pipeline
    pub fn add_stage(&mut self, stage: Box<dyn PipelineStage>) {
        self.stages.push(stage);
    }
    
    /// Create and add all pipeline stages in the correct order
    fn create_and_add_all_stages(
        &mut self,
        proxy_config: crate::models::StreamProxy,
        logo_service: Arc<crate::logo_assets::service::LogoAssetService>,
        logo_config: crate::pipeline::stages::logo_caching::LogoCachingConfig,
        db_pool: sqlx::SqlitePool,
    ) {
        info!("Creating pipeline stages for proxy: {}", proxy_config.id);
        
        // Create each pipeline stage in the correct order
        
        // 1. Data Mapping Stage
        if let Ok(data_mapping_stage) = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                crate::pipeline::stages::data_mapping::DataMappingStage::new(
                    db_pool.clone(),
                    self.execution.execution_prefix.clone(),
                    self.file_manager.clone(),
                    self.progress_manager.clone(),
                ).await
            })
        }) {
            self.add_stage(Box::new(data_mapping_stage));
        } else {
            warn!("Failed to create DataMappingStage");
        }
        
        // 2. Filtering Stage
        if let Ok(filtering_stage) = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                crate::pipeline::stages::filtering::FilteringStage::new(
                    db_pool.clone(),
                    self.file_manager.clone(),
                    self.execution.execution_prefix.clone(),
                    Some(proxy_config.id),
                    self.progress_manager.clone(),
                ).await
            })
        }) {
            self.add_stage(Box::new(filtering_stage));
        } else {
            warn!("Failed to create FilteringStage");
        }
        
        // 3. Logo Caching Stage
        if let Ok(logo_caching_stage) = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                crate::pipeline::stages::logo_caching::LogoCachingStage::new(
                    self.file_manager.clone(),
                    self.execution.execution_prefix.clone(),
                    logo_service.clone(),
                    logo_config.clone(),
                    self.progress_manager.clone(),
                ).await
            })
        }) {
            self.add_stage(Box::new(logo_caching_stage));
        } else {
            warn!("Failed to create LogoCachingStage");
        }
        
        // 4. Numbering Stage  
        let starting_channel_number = if proxy_config.starting_channel_number >= 0 {
            proxy_config.starting_channel_number as u32
        } else {
            warn!(
                "Proxy {} has negative starting_channel_number ({}), using default 50000", 
                proxy_config.id, proxy_config.starting_channel_number
            );
            50000u32
        };
        
        let numbering_stage = crate::pipeline::stages::numbering::NumberingStage::new(
            self.file_manager.clone(),
            self.execution.execution_prefix.clone(),
            starting_channel_number,
            self.progress_manager.clone(),
        );
        self.add_stage(Box::new(numbering_stage));
        
        // 5. Generation Stage
        if let Ok(generation_stage) = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                crate::pipeline::stages::generation::GenerationStage::new(
                    db_pool.clone(),
                    self.file_manager.clone(),
                    self.execution.execution_prefix.clone(),
                    proxy_config.id,
                    "http://localhost:8080".to_string(), // TODO: Get this from config
                    self.progress_manager.clone(),
                ).await
            })
        }) {
            self.add_stage(Box::new(generation_stage));
        } else {
            warn!("Failed to create GenerationStage");
        }
        
        // 6. Publish Content Stage
        let publish_content_stage = crate::pipeline::stages::publish_content::PublishContentStage::new(
            self.file_manager.clone(),              // pipeline file manager (for reading temp files)
            self._proxy_output_file_manager.clone(), // proxy output file manager (for final served files)
            proxy_config.id,                       // proxy_id
            false,                                  // enable_versioning (disabled for now)
            self.progress_manager.clone(),
        );
        self.add_stage(Box::new(publish_content_stage));
        
        info!("Created {} pipeline stages for proxy {}", self.stages.len(), proxy_config.id);
    }
    
    /// Get the execution ID for this pipeline
    pub fn get_execution_id(&self) -> uuid::Uuid {
        self.execution.id
    }
    
    /// Set the progress manager for this orchestrator and inject it into all stages
    pub fn set_progress_manager(&mut self, progress_manager: Option<Arc<ProgressManager>>) {
        self.progress_manager = progress_manager.clone();
        
        // Update existing stages that need progress managers
        if let Some(ref pm) = progress_manager {
            for stage in &mut self.stages {
                let stage_id = stage.stage_id();
                debug!("Setting progress manager on {} stage after construction", stage_id);
                
                // Update progress manager for each stage type
                match stage_id {
                    "data_mapping" => {
                        if let Some(data_mapping_stage) = stage.as_any_mut().downcast_mut::<crate::pipeline::stages::data_mapping::DataMappingStage>() {
                            data_mapping_stage.set_progress_manager(pm.clone());
                        }
                    }
                    "filtering" => {
                        if let Some(filtering_stage) = stage.as_any_mut().downcast_mut::<crate::pipeline::stages::filtering::FilteringStage>() {
                            filtering_stage.set_progress_manager(pm.clone());
                        }
                    }
                    "logo_caching" => {
                        if let Some(logo_caching_stage) = stage.as_any_mut().downcast_mut::<crate::pipeline::stages::logo_caching::LogoCachingStage>() {
                            logo_caching_stage.set_progress_manager(pm.clone());
                        }
                    }
                    "numbering" => {
                        if let Some(numbering_stage) = stage.as_any_mut().downcast_mut::<crate::pipeline::stages::numbering::NumberingStage>() {
                            numbering_stage.set_progress_manager(pm.clone());
                        }
                    }
                    "generation" => {
                        if let Some(generation_stage) = stage.as_any_mut().downcast_mut::<crate::pipeline::stages::generation::GenerationStage>() {
                            generation_stage.set_progress_manager(pm.clone());
                        }
                    }
                    "publish_content" => {
                        if let Some(publish_content_stage) = stage.as_any_mut().downcast_mut::<crate::pipeline::stages::publish_content::PublishContentStage>() {
                            publish_content_stage.set_progress_manager(pm.clone());
                        }
                    }
                    _ => {
                        debug!("Unknown stage type: {}, cannot set progress manager", stage_id);
                    }
                }
            }
        }
    }
    
    /// Initialize all pipeline stages in the ProgressManager
    async fn initialize_progress_stages(&self) -> Result<(), PipelineError> {
        if let Some(ref progress_mgr) = self.progress_manager {
            info!("Initializing {} pipeline stages in ProgressManager", self.stages.len());
            
            let mut stage_manager = progress_mgr.clone();
            for stage in &self.stages {
                stage_manager = stage_manager.add_stage(stage.stage_id(), stage.stage_name()).await;
            }
            
            info!("Successfully initialized all pipeline stages in ProgressManager for execution {}", self.execution.execution_prefix);
        }
        
        Ok(())
    }
    
    /// Execute the entire pipeline
    pub async fn execute_pipeline(&mut self) -> Result<PipelineExecution, PipelineError> {
        let pipeline_start = Instant::now();
        let _reporter = ProgressReporter::new(self);
        
        info!("Starting pipeline execution: {}", self.execution.execution_prefix);
        
        // Initialize progress tracking
        self.initialize_progress_stages().await?;
        
        // Suspend cleanup during pipeline execution
        self.file_manager.suspend_cleanup(DEFAULT_PIPELINE_SUSPENSION_DURATION).await
            .map_err(|e| PipelineError::FileSystem(std::io::Error::other(e.to_string())))?;
        info!("Pipeline cleanup suspended for execution {}", self.execution.execution_prefix);
        
        // Start background task to extend suspension periodically
        let stop_extension = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let _extension_task = self.start_suspension_extension_task(stop_extension.clone());
        let _extension_guard = SuspensionExtensionGuard {
            stop_flag: stop_extension,
            _file_manager: self.file_manager.clone(),
        };
        
        // Execute all stages in sequence
        let mut artifacts = Vec::new();
        let total_stages = self.stages.len();
        
        for stage_index in 0..self.stages.len() {
            let stage_start = Instant::now();
            
            // Get stage info first to avoid borrow issues
            let stage_id = self.stages[stage_index].stage_id();
            let stage_name = self.stages[stage_index].stage_name();
            
            info!("Executing stage {}/{}: {} ({})", stage_index + 1, total_stages, stage_name, stage_id);
            
            // Update execution status
            self.execution.status = self.get_pipeline_status_for_stage(stage_id);
            self.execution.start_stage(stage_id);
            
            // CRITICAL: Set the current active stage in progress manager
            if let Some(ref progress_mgr) = self.progress_manager {
                progress_mgr.set_current_stage(stage_id).await;
            }
            
            // Execute the stage (split borrow to avoid conflicts)
            let stage_result = {
                let stage = &mut self.stages[stage_index];
                stage.execute(artifacts).await
            };
            
            match stage_result {
                Ok(stage_artifacts) => {
                    let stage_duration = stage_start.elapsed();
                    info!("Stage {} completed successfully in {:?}", stage_name, stage_duration);
                    
                    // Update execution tracking
                    let mut metrics = std::collections::HashMap::new();
                    metrics.insert("artifacts_created".to_string(), serde_json::json!(stage_artifacts.len()));
                    self.execution.complete_stage_with_artifacts(stage_id, stage_artifacts.clone(), metrics);
                    
                    artifacts = stage_artifacts;
                }
                Err(e) => {
                    error!("Stage {} failed: {}", stage_name, e);
                    self.execution.status = PipelineStatus::Failed;
                    return Err(PipelineError::stage_error(stage_id, format!("Stage execution failed: {e}")));
                }
            }
            
            // Cleanup stage resources (separate borrow)
            if let Err(e) = self.stages[stage_index].cleanup().await {
                warn!("Stage {} cleanup failed: {}", stage_name, e);
            }
        }
        
        // Pipeline completed successfully
        let total_duration = pipeline_start.elapsed();
        self.execution.status = PipelineStatus::Completed;
        
        info!(
            "Pipeline execution completed successfully: {} stages, {} artifacts, duration: {:?}",
            total_stages, artifacts.len(), total_duration
        );
        
        Ok(self.execution.clone())
    }
    
    /// Get the appropriate PipelineStatus for a given stage ID
    fn get_pipeline_status_for_stage(&self, stage_id: &str) -> PipelineStatus {
        match stage_id {
            "data_mapping" => PipelineStatus::DataMapping,
            "filtering" => PipelineStatus::Filtering,
            "logo_caching" => PipelineStatus::LogoCaching,
            "numbering" => PipelineStatus::Numbering,
            "generation" => PipelineStatus::Generation,
            "publish_content" => PipelineStatus::Publishing,
            _ => PipelineStatus::DataMapping, // Default fallback
        }
    }
    
    /// Start background task to extend file cleanup suspension
    fn start_suspension_extension_task(&self, stop_flag: Arc<std::sync::atomic::AtomicBool>) -> tokio::task::JoinHandle<()> {
        let _file_manager = self.file_manager.clone();
        
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(60)); // Check every minute
            
            while !stop_flag.load(std::sync::atomic::Ordering::Relaxed) {
                interval.tick().await;
                
                if !stop_flag.load(std::sync::atomic::Ordering::Relaxed) {
                    // Note: extend_suspension method not available in current SandboxedManager API
                    // This would be needed for production use to prevent cleanup during long operations
                }
            }
        })
    }
}

// Implement ProgressAware for the orchestrator
impl ProgressAware for PipelineOrchestrator {
    fn get_progress_manager(&self) -> Option<&Arc<ProgressManager>> {
        self.progress_manager.as_ref()
    }
}

/// Guard that stops the suspension extension task when dropped
struct SuspensionExtensionGuard {
    stop_flag: Arc<std::sync::atomic::AtomicBool>,
    _file_manager: SandboxedManager,
}

impl Drop for SuspensionExtensionGuard {
    fn drop(&mut self) {
        self.stop_flag.store(true, std::sync::atomic::Ordering::Relaxed);
        
        // Note: resume_cleanup method not available in current SandboxedManager API
        // This would be needed for production use to resume cleanup after pipeline completion
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::models::PipelineArtifact;
    use uuid::Uuid;
    use std::sync::Arc;
    
    // Simple test stage for orchestrator testing
    struct TestStage {
        progress_manager: Option<Arc<ProgressManager>>,
    }
    
    impl TestStage {
        fn new() -> Self {
            Self { progress_manager: None }
        }
    }
    
    impl ProgressAware for TestStage {
        fn get_progress_manager(&self) -> Option<&Arc<ProgressManager>> {
            self.progress_manager.as_ref()
        }
    }
    
    #[async_trait::async_trait]
    impl PipelineStage for TestStage {
        async fn execute(&mut self, input: Vec<PipelineArtifact>) -> Result<Vec<PipelineArtifact>, PipelineError> {
            // Just pass through input artifacts
            Ok(input)
        }
        
        fn stage_id(&self) -> &'static str {
            "test_stage"
        }
        
        fn stage_name(&self) -> &'static str {
            "Test Stage"
        }
        
        fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
            self
        }
    }
    
    #[tokio::test]
    async fn test_orchestrator_basic_execution() {
        // Create a simple test execution
        let execution = PipelineExecution::new(Uuid::new_v4());
        
        // Create file manager with proper test temp directory
        let file_manager = SandboxedManager::builder()
            .base_directory(std::env::temp_dir().join("orchestrator_test"))
            .build()
            .await
            .unwrap();
        let output_manager = file_manager.clone();
        
        // Create orchestrator
        let mut orchestrator = PipelineOrchestrator::new(
            execution,
            file_manager,
            output_manager,
            None, // No progress manager for basic test
        );
        
        // Add a simple test stage
        let test_stage = TestStage::new();
        orchestrator.add_stage(Box::new(test_stage));
        
        // Execute pipeline
        let result = orchestrator.execute_pipeline().await;
        assert!(result.is_ok());
        
        let completed_execution = result.unwrap();
        assert_eq!(completed_execution.status, PipelineStatus::Completed);
    }
}