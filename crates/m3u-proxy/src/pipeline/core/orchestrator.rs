use crate::pipeline::models::{PipelineExecution, PipelineStatus, PipelineArtifact};
use crate::pipeline::stages::{DataMappingStage, FilteringStage, LogoCachingStage, NumberingStage, GenerationStage, CleanupStage, logo_caching::LogoCachingConfig};
use crate::pipeline::core::performance_tracker::PipelinePerformanceTracker;
use tracing::{info, warn};
use crate::logo_assets::service::LogoAssetService;
use crate::models::StreamProxy;
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;
use uuid;
use sandboxed_file_manager::SandboxedManager;
use std::time::Duration;

/// Default suspension duration for pipeline operations (5 minutes)
const DEFAULT_PIPELINE_SUSPENSION_DURATION: Duration = Duration::from_secs(5 * 60);

pub struct PipelineOrchestrator {
    db_pool: SqlitePool,
    execution: PipelineExecution,
    file_manager: SandboxedManager,
    proxy_output_file_manager: SandboxedManager,
    proxy_config: StreamProxy,
    logo_service: Arc<LogoAssetService>,
    logo_config: LogoCachingConfig,
    performance_tracker: PipelinePerformanceTracker,
}

impl PipelineOrchestrator {
    /// Helper method to send progress updates with both overall and stage progress
    fn send_progress_update(
        progress_callback: &Option<Arc<crate::services::progress_service::UniversalProgressCallback>>,
        proxy_id: uuid::Uuid,
        step: &str,
        overall_percentage: f64,
    ) {
        Self::send_progress_update_with_stage(progress_callback, proxy_id, step, overall_percentage, None, None);
    }

    /// Helper method to send progress updates with stage-specific progress
    fn send_progress_update_with_stage(
        progress_callback: &Option<Arc<crate::services::progress_service::UniversalProgressCallback>>,
        proxy_id: uuid::Uuid,
        step: &str,
        overall_percentage: f64,
        stage_name: Option<&str>,
        stage_percentage: Option<f64>,
    ) {
        if let Some(callback) = progress_callback {
            tracing::debug!("Orchestrator sending progress update: {} (overall: {}%{})", 
                          step, overall_percentage,
                          if let (Some(stage), Some(stage_pct)) = (stage_name, stage_percentage) {
                              format!(", {}: {}%", stage, stage_pct)
                          } else {
                              String::new()
                          });
                          
            let mut progress = crate::services::progress_service::UniversalProgress::new(
                proxy_id,
                crate::services::progress_service::OperationType::ProxyRegeneration,
                format!("Regenerate Proxy {}", proxy_id),
            )
            .set_state(crate::services::progress_service::UniversalState::Processing)
            .update_step(step.to_string())
            .update_percentage(overall_percentage);
            
            // Add stage-specific progress to metadata
            if let (Some(stage), Some(stage_pct)) = (stage_name, stage_percentage) {
                progress = progress
                    .add_metadata("current_stage".to_string(), serde_json::Value::String(stage.to_string()))
                    .add_metadata("stage_percentage".to_string(), serde_json::Value::Number(serde_json::Number::from_f64(stage_pct).unwrap_or_else(|| serde_json::Number::from(0))));
            }
            
            callback(progress);
            tracing::debug!("Orchestrator progress callback completed for: {}", step);
        } else {
            tracing::debug!("Orchestrator: no progress callback available for step: {}", step);
        }
    }
    /// Create orchestrator with all dependencies injected (use factory instead)
    pub fn new_with_dependencies(
        proxy_config: StreamProxy,
        file_manager: SandboxedManager,
        proxy_output_file_manager: SandboxedManager,
        logo_service: Arc<LogoAssetService>,
        logo_config: LogoCachingConfig,
        db_pool: SqlitePool,
    ) -> Self {
        let execution = PipelineExecution::new(proxy_config.id);
        let performance_tracker = PipelinePerformanceTracker::new(
            execution.id.to_string(),
            execution.execution_prefix.clone(),
        );
        
        Self {
            db_pool,
            execution,
            file_manager,
            proxy_output_file_manager,
            proxy_config,
            logo_service,
            logo_config,
            performance_tracker,
        }
    }


    
    pub async fn execute_pipeline(&mut self, progress_callback: &Option<Arc<crate::services::progress_service::UniversalProgressCallback>>) -> Result<PipelineExecution, Box<dyn std::error::Error>> {
        let pipeline_start = std::time::Instant::now();
        
        // Configuration is already injected via factory
        
        // Suspend cleanup during pipeline execution (5 minutes initially)
        // This prevents intermediate files from being cleaned up during long-running operations like logo downloading
        self.file_manager.suspend_cleanup(DEFAULT_PIPELINE_SUSPENSION_DURATION).await?;
        tracing::info!("Pipeline cleanup suspended for execution {}", self.execution.execution_prefix);
        
        // Start background task to periodically extend suspension (every 1 minute, add 5 minutes from now)
        let stop_extension = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let _extension_task = self.start_suspension_extension_task(stop_extension.clone());
        
        // Create a guard that will stop extension task on drop
        let _extension_guard = SuspensionExtensionGuard { 
            stop_flag: stop_extension,
            file_manager: self.file_manager.clone(),
        };
        
        // Initialize pipeline stages
        self.execution.add_stage("data_mapping".to_string());
        self.execution.add_stage("filtering".to_string());
        self.execution.add_stage("logo_caching".to_string());
        self.execution.add_stage("numbering".to_string());
        self.execution.add_stage("generation".to_string());
        self.execution.add_stage("publish_content".to_string());
        
        // Stage 1: Data Mapping (removing data_loading as specified)
        self.execution.status = PipelineStatus::DataMapping;
        self.execution.start_stage("data_mapping");
        self.performance_tracker.start_stage("data_mapping".to_string());
        
        // Send progress update: 0% - Starting data mapping
        Self::send_progress_update(&progress_callback, self.execution.proxy_id, "Starting data mapping stage", 0.0);
        
        let artifacts = self.execute_data_mapping_stage(&progress_callback).await?;
        let mut metrics = HashMap::new();
        metrics.insert("artifacts_created".to_string(), serde_json::json!(artifacts.len()));
        self.execution.complete_stage_with_artifacts("data_mapping", artifacts.clone(), metrics);
        self.performance_tracker.complete_stage("data_mapping", artifacts.len());
        
        // Send progress update: 16% - Data mapping completed
        Self::send_progress_update(&progress_callback, self.execution.proxy_id, "Data mapping completed", 16.0);
        
        // Stage 2: Filtering
        self.execution.status = PipelineStatus::Filtering;
        self.execution.start_stage("filtering");
        self.performance_tracker.start_stage("filtering".to_string());
        
        // Send progress update: 17% - Starting filtering
        Self::send_progress_update(&progress_callback, self.execution.proxy_id, "Starting filtering stage", 17.0);
        
        let filtering_input = artifacts;
        let artifacts = self.execute_filtering_stage(filtering_input, &progress_callback).await?;
        let mut metrics = HashMap::new();
        metrics.insert("artifacts_processed".to_string(), serde_json::json!(artifacts.len()));
        self.execution.complete_stage_with_artifacts("filtering", artifacts.clone(), metrics);
        self.performance_tracker.complete_stage("filtering", artifacts.len());
        
        // Send progress update: 33% - Filtering completed
        Self::send_progress_update(&progress_callback, self.execution.proxy_id, "Filtering completed", 33.0);
        
        // Stage 3: Logo Caching
        self.execution.status = PipelineStatus::LogoCaching;
        self.execution.start_stage("logo_caching");
        self.performance_tracker.start_stage("logo_caching".to_string());
        
        // Send progress update: 34% - Starting logo caching
        Self::send_progress_update(&progress_callback, self.execution.proxy_id, "Starting logo caching stage", 34.0);
        
        let logo_caching_input = artifacts;
        let artifacts = self.execute_logo_caching_stage(logo_caching_input, &progress_callback).await?;
        let mut metrics = HashMap::new();
        metrics.insert("artifacts_processed".to_string(), serde_json::json!(artifacts.len()));
        self.execution.complete_stage_with_artifacts("logo_caching", artifacts.clone(), metrics);
        self.performance_tracker.complete_stage("logo_caching", artifacts.len());
        
        // Send progress update: 50% - Logo caching completed
        Self::send_progress_update(&progress_callback, self.execution.proxy_id, "Logo caching completed", 50.0);
        
        // Stage 4: Numbering
        self.execution.status = PipelineStatus::Numbering;
        self.execution.start_stage("numbering");
        self.performance_tracker.start_stage("numbering".to_string());
        
        // Send progress update: 51% - Starting numbering
        Self::send_progress_update(&progress_callback, self.execution.proxy_id, "Starting numbering stage", 51.0);
        
        let numbering_input = artifacts;
        let input_len = numbering_input.len(); // Store length before move
        let _artifacts = self.execute_numbering_stage(numbering_input, &progress_callback).await?;
        let mut metrics = HashMap::new();
        metrics.insert("artifacts_processed".to_string(), serde_json::json!(input_len));
        self.execution.complete_stage_with_artifacts("numbering", _artifacts.clone(), metrics);
        self.performance_tracker.complete_stage("numbering", _artifacts.len());
        
        // Send progress update: 66% - Numbering completed
        Self::send_progress_update(&progress_callback, self.execution.proxy_id, "Numbering completed", 66.0);
        
        // Stage 5: Generation
        self.execution.status = PipelineStatus::Generation;
        self.execution.start_stage("generation");
        self.performance_tracker.start_stage("generation".to_string());
        
        // Send progress update: 67% - Starting generation
        Self::send_progress_update(&progress_callback, self.execution.proxy_id, "Starting generation stage", 67.0);
        
        let generation_input = _artifacts;
        let input_len = generation_input.len(); // Store length before move  
        info!("About to execute generation stage with {} input artifacts", input_len);
        let (generated_artifacts, channels_count, programs_count) = match self.execute_generation_stage(generation_input, &progress_callback).await {
            Ok((artifacts, channels_count, programs_count)) => {
                info!("Generation stage completed successfully with {} output artifacts", artifacts.len());
                (artifacts, channels_count, programs_count)
            }
            Err(e) => {
                tracing::error!("Generation stage failed: {}", e);
                return Err(e);
            }
        };
        let mut metrics = HashMap::new();
        metrics.insert("artifacts_processed".to_string(), serde_json::json!(input_len));
        self.execution.complete_stage_with_artifacts("generation", generated_artifacts.clone(), metrics);
        self.performance_tracker.complete_stage("generation", generated_artifacts.len());
        
        // Send progress update: 83% - Generation completed
        Self::send_progress_update(&progress_callback, self.execution.proxy_id, "Generation completed", 83.0);
        
        // Stage 6: Publish Content
        self.execution.status = PipelineStatus::Publishing;
        self.execution.start_stage("publish_content");
        self.performance_tracker.start_stage("publish_content".to_string());
        
        // Send progress update: 84% - Starting publishing
        Self::send_progress_update(&progress_callback, self.execution.proxy_id, "Starting publishing stage", 84.0);
        
        let publish_input = generated_artifacts;
        let input_len = publish_input.len(); // Store length before move
        info!("About to call publish stage with {} artifacts", input_len);
        for (i, artifact) in publish_input.iter().enumerate() {
            info!("Publish input artifact {}: type={:?} file_path={} size={}KB", 
                  i, artifact.artifact_type.content, artifact.file_path,
                  artifact.file_size.unwrap_or(0) / 1024);
        }
        
        let published_artifacts = self.execute_publish_content_stage(publish_input, &progress_callback).await?;
        let mut metrics = HashMap::new();
        metrics.insert("artifacts_published".to_string(), serde_json::json!(input_len));
        self.execution.complete_stage_with_artifacts("publish_content", published_artifacts.clone(), metrics);
        self.performance_tracker.complete_stage("publish_content", published_artifacts.len());
        
        // Send progress update: 100% - Publishing completed
        Self::send_progress_update(&progress_callback, self.execution.proxy_id, "Publishing completed - Pipeline finished", 100.0);
        
        // Complete performance tracking before cleanup
        self.performance_tracker.complete_pipeline();
        
        let cleanup_stage = CleanupStage::success(
            self.file_manager.clone(),
            self.execution.execution_prefix.clone(),
        );
        
        let all_artifacts: Vec<PipelineArtifact> = self.execution.stages
            .keys()
            .flat_map(|stage_name| self.execution.get_stage_artifacts(stage_name))
            .cloned()
            .collect();
        
        let _ = cleanup_stage.process(all_artifacts, None).await;
        
        // Generate comprehensive pipeline summary
        let total_duration = pipeline_start.elapsed();
        self.log_pipeline_summary_optimized(channels_count, programs_count, &published_artifacts, total_duration).await;
        
        // Log performance report after cleanup
        tracing::info!("Pipeline execution completed");
        
        // CRITICAL: Update database to mark proxy as successfully generated
        // This prevents continuous re-runs by recording completion timestamp
        if let Err(e) = self.update_proxy_completion_timestamp().await {
            tracing::error!("Failed to update proxy completion timestamp: {}", e);
            // Don't fail the entire pipeline for this, but log it prominently
        }
        
        // Extension guard will automatically resume cleanup when function returns
        info!("Pipeline execution completed successfully - all 7 stages finished");
        self.execution.complete();
        info!("About to return from execute_pipeline - SuspensionExtensionGuard will drop now");
        Ok(self.execution.clone())
    }
    
    async fn execute_data_mapping_stage(&mut self, progress_callback: &Option<Arc<crate::services::progress_service::UniversalProgressCallback>>) -> Result<Vec<PipelineArtifact>, Box<dyn std::error::Error>> {
        let mut data_mapping_stage = DataMappingStage::new(
            self.db_pool.clone(),
            self.execution.execution_prefix.clone(),
            self.file_manager.clone(),
        ).await?;
        
        // Send progress: 25% of data mapping stage (4% overall)
        Self::send_progress_update_with_stage(progress_callback, self.execution.proxy_id, "Processing channels data", 4.0, Some("data_mapping"), Some(25.0));
        
        // Process channels (streams)
        let channels_output = data_mapping_stage.process_channels().await?;
        
        // Send progress: 75% of data mapping stage (12% overall)
        Self::send_progress_update_with_stage(progress_callback, self.execution.proxy_id, "Processing EPG programs data", 12.0, Some("data_mapping"), Some(75.0));
        
        // Process programs (EPG)
        let programs_output = data_mapping_stage.process_programs().await?;
        
        // Send progress: 100% of data mapping stage (16% overall)
        Self::send_progress_update_with_stage(progress_callback, self.execution.proxy_id, "Data mapping cleanup", 15.5, Some("data_mapping"), Some(100.0));
        
        // Cleanup stage resources
        data_mapping_stage.cleanup()?;
        
        Ok(vec![channels_output, programs_output])
    }
    
    async fn execute_filtering_stage(&mut self, input_artifacts: Vec<PipelineArtifact>, progress_callback: &Option<Arc<crate::services::progress_service::UniversalProgressCallback>>) -> Result<Vec<PipelineArtifact>, Box<dyn std::error::Error>> {
        // Send progress: 25% of filtering stage (25% overall)
        Self::send_progress_update_with_stage(progress_callback, self.execution.proxy_id, "Initializing filters", 25.0, Some("filtering"), Some(25.0));
        
        let mut filtering_stage = FilteringStage::new(
            self.db_pool.clone(),
            self.file_manager.clone(),
            self.execution.execution_prefix.clone(),
            Some(self.execution.proxy_id),
        ).await?;
        
        // Send progress: 75% of filtering stage (30% overall)
        Self::send_progress_update_with_stage(progress_callback, self.execution.proxy_id, "Processing channel filters", 30.0, Some("filtering"), Some(75.0));
        
        // Process artifacts through filtering stage
        let filtered_artifacts = filtering_stage.process(input_artifacts).await?;
        
        // Send progress: 100% of filtering stage (33% overall)
        Self::send_progress_update_with_stage(progress_callback, self.execution.proxy_id, "Filtering cleanup", 32.5, Some("filtering"), Some(100.0));
        
        // Cleanup stage resources
        filtering_stage.cleanup()?;
        
        Ok(filtered_artifacts)
    }
    
    async fn execute_logo_caching_stage(&mut self, input_artifacts: Vec<PipelineArtifact>, progress_callback: &Option<Arc<crate::services::progress_service::UniversalProgressCallback>>) -> Result<Vec<PipelineArtifact>, Box<dyn std::error::Error>> {
        // Send progress: 10% of logo caching stage (37% overall)
        Self::send_progress_update_with_stage(progress_callback, self.execution.proxy_id, "Initializing logo cache", 37.0, Some("logo_caching"), Some(10.0));
        
        // Use injected logo service and configuration
        let mut logo_caching_stage = LogoCachingStage::new(
            self.file_manager.clone(),
            self.execution.execution_prefix.clone(),
            self.logo_service.clone(),
            self.logo_config.clone(),
        ).await?;
        
        // Send progress: 50% of logo caching stage (42% overall)
        Self::send_progress_update_with_stage(progress_callback, self.execution.proxy_id, "Processing channel logos", 42.0, Some("logo_caching"), Some(50.0));
        
        // Process artifacts through logo caching stage
        let cached_artifacts = logo_caching_stage.process(input_artifacts).await?;
        
        // Send progress: 100% of logo caching stage (50% overall)
        Self::send_progress_update_with_stage(progress_callback, self.execution.proxy_id, "Logo caching completed", 49.5, Some("logo_caching"), Some(100.0));
        
        Ok(cached_artifacts)
    }
    
    async fn execute_numbering_stage(&mut self, input_artifacts: Vec<PipelineArtifact>, progress_callback: &Option<Arc<crate::services::progress_service::UniversalProgressCallback>>) -> Result<Vec<PipelineArtifact>, Box<dyn std::error::Error>> {
        // Send progress: 20% of numbering stage (53% overall)
        Self::send_progress_update_with_stage(progress_callback, self.execution.proxy_id, "Initializing channel numbering", 53.0, Some("numbering"), Some(20.0));
        
        // Use proxy-specific starting channel number
        let starting_channel_number = if self.proxy_config.starting_channel_number >= 0 {
            self.proxy_config.starting_channel_number as u32
        } else {
            tracing::warn!(
                "Proxy {} has negative starting_channel_number ({}), using default 50000", 
                self.proxy_config.id, self.proxy_config.starting_channel_number
            );
            50000u32
        };
        
        let numbering_stage = NumberingStage::new(
            self.file_manager.clone(),
            self.execution.execution_prefix.clone(),
            starting_channel_number,
        );
        
        // Send progress: 80% of numbering stage (62% overall) 
        Self::send_progress_update_with_stage(progress_callback, self.execution.proxy_id, "Assigning channel numbers", 62.0, Some("numbering"), Some(80.0));
        
        // Process artifacts through numbering stage
        let numbered_artifacts = numbering_stage.process(input_artifacts).await?;
        
        // Send progress: 100% of numbering stage (66% overall)
        Self::send_progress_update_with_stage(progress_callback, self.execution.proxy_id, "Channel numbering completed", 65.5, Some("numbering"), Some(100.0));
        
        Ok(numbered_artifacts)
    }
    
    async fn execute_generation_stage(&mut self, input_artifacts: Vec<PipelineArtifact>, progress_callback: &Option<Arc<crate::services::progress_service::UniversalProgressCallback>>) -> Result<(Vec<PipelineArtifact>, usize, usize), Box<dyn std::error::Error>> {
        info!("Generation stage starting with {} input artifacts", input_artifacts.len());
        
        // Send progress: 10% of generation stage (69% overall)
        Self::send_progress_update_with_stage(progress_callback, self.execution.proxy_id, "Loading generation data", 69.0, Some("generation"), Some(10.0));
        
        // Log all input artifacts for debugging
        for (i, artifact) in input_artifacts.iter().enumerate() {
            info!(
                "Input artifact {}: type={:?} stage={:?} file_path={} record_count={:?}",
                i, artifact.artifact_type.content, artifact.artifact_type.stage, artifact.file_path, artifact.record_count
            );
        }
        
        // Load data from artifacts - separate channels and EPG data
        let mut numbered_channels = Vec::new();
        let mut epg_programs = Vec::new();
        
        for artifact in &input_artifacts {
            // Build full file path for reading - use the sandboxed path
            match self.file_manager.get_full_path(&artifact.file_path) {
                Ok(full_path) => {
                    info!("Attempting to read artifact file: {} -> {}", artifact.file_path, full_path.display());
                    // Read and deserialize based on artifact type
                    match &artifact.artifact_type.content {
                        crate::pipeline::models::ContentType::Channels => {
                            match tokio::fs::read_to_string(&full_path).await {
                                Ok(content) => {
                                    info!("Successfully read channels file, content length: {} bytes", content.len());
                                    // Parse JSONL format (one JSON object per line)
                                    let mut parsed_channels = Vec::new();
                                    for (line_num, line) in content.lines().enumerate() {
                                        if line.trim().is_empty() {
                                            continue;
                                        }
                                        match serde_json::from_str::<crate::models::Channel>(line) {
                                            Ok(channel) => {
                                                // JSONL files contain Channel objects directly - wrap in NumberedChannel for compatibility
                                                let numbered_channel = crate::models::NumberedChannel {
                                                    channel,
                                                    assigned_number: 0, // This will be set by the numbering stage
                                                    assignment_type: crate::models::ChannelNumberAssignmentType::Sequential,
                                                };
                                                parsed_channels.push(numbered_channel);
                                            }
                                            Err(e) => {
                                                warn!("Failed to deserialize channel at line {}: {}", line_num + 1, e);
                                            }
                                        }
                                    }
                                    info!("Successfully deserialized {} channels from JSONL file", parsed_channels.len());
                                    numbered_channels.extend(parsed_channels);
                                }
                                Err(e) => {
                                    warn!("Failed to read channels file {}: {}", artifact.file_path, e);
                                }
                            }
                        }
                        crate::pipeline::models::ContentType::EpgPrograms => {
                            match tokio::fs::read_to_string(&full_path).await {
                                Ok(content) => {
                                    info!("Successfully read EPG programs file, content length: {} bytes", content.len());
                                    // Parse JSONL format (one JSON object per line)
                                    let mut parsed_programs = Vec::new();
                                    for (line_num, line) in content.lines().enumerate() {
                                        if line.trim().is_empty() {
                                            continue;
                                        }
                                        match serde_json::from_str::<crate::pipeline::engines::rule_processor::EpgProgram>(line) {
                                            Ok(pipeline_program) => {
                                                // JSONL files contain pipeline format directly - no conversion needed
                                                parsed_programs.push(pipeline_program);
                                            }
                                            Err(e) => {
                                                warn!("Failed to deserialize EPG program at line {}: {}", line_num + 1, e);
                                            }
                                        }
                                    }
                                    info!("Successfully deserialized {} EPG programs from JSONL file", parsed_programs.len());
                                    epg_programs.extend(parsed_programs);
                                }
                                Err(e) => {
                                    warn!("Failed to read EPG programs file {}: {}", artifact.file_path, e);
                                }
                            }
                        }
                        _ => {
                            // Skip other artifact types
                            info!("Skipping artifact with content type: {:?}", artifact.artifact_type.content);
                            continue;
                        }
                    }
                }
                Err(e) => {
                    warn!("Failed to get full path for artifact {}: {}", artifact.file_path, e);
                }
            }
        }
        
        info!(
            "Loaded artifacts for generation: numbered_channels={} epg_programs={}",
            numbered_channels.len(),
            epg_programs.len()
        );
        
        // Send progress: 30% of generation stage (73% overall)
        Self::send_progress_update_with_stage(progress_callback, self.execution.proxy_id, "Initializing M3U/XMLTV generation", 73.0, Some("generation"), Some(30.0));
        
        // Create GenerationStage instance
        let generation_stage = GenerationStage::new(
            self.db_pool.clone(),
            self.file_manager.clone(),
            self.execution.execution_prefix.clone(),
            self.proxy_config.id,
            format!("http://localhost:8080"), // TODO: Get this from config
        ).await?;
        
        // Store length before moving
        let has_epg_programs = !epg_programs.is_empty();
        
        // Process through generation stage
        info!("Calling process_channels_and_programs with {} channels and {} programs", 
              numbered_channels.len(), epg_programs.len());
        
        // Send progress: 60% of generation stage (78% overall)
        Self::send_progress_update_with_stage(progress_callback, self.execution.proxy_id, "Generating M3U and XMLTV files", 78.0, Some("generation"), Some(60.0));
        
        // Store counts for summary reporting instead of cloning full data structures
        let channels_count = numbered_channels.len();
        let programs_count = epg_programs.len();
        
        let _generated_files = match generation_stage.process_channels_and_programs(
            numbered_channels,
            epg_programs,
            true, // cache_channel_logos
            &self.logo_service,
        ).await {
            Ok(files) => {
                info!("Generation stage process_channels_and_programs completed successfully");
                files
            }
            Err(e) => {
                tracing::error!("Generation stage process_channels_and_programs failed: {}", e);
                return Err(e.into());
            }
        };
        
        // Send progress: 90% of generation stage (81% overall)
        Self::send_progress_update_with_stage(progress_callback, self.execution.proxy_id, "Finalizing generated files", 81.0, Some("generation"), Some(90.0));
        
        // Create output artifacts for generated files with actual file sizes
        let mut generated_artifacts = Vec::new();
        
        // Add M3U playlist artifact with actual file size
        let m3u_file_path = format!("{}_temp.m3u8", self.execution.execution_prefix);
        let m3u_file_size = match self.file_manager.get_full_path(&m3u_file_path) {
            Ok(full_path) => {
                match tokio::fs::metadata(&full_path).await {
                    Ok(metadata) => {
                        let size = metadata.len();
                        info!("M3U file size determined: {} bytes ({}KB)", size, size / 1024);
                        Some(size)
                    }
                    Err(e) => {
                        warn!("Failed to get M3U file size for {}: {}", m3u_file_path, e);
                        None
                    }
                }
            }
            Err(e) => {
                warn!("Failed to get M3U full path for {}: {}", m3u_file_path, e);
                None
            }
        };
        
        let mut m3u_artifact = PipelineArtifact::new(
            crate::pipeline::models::ArtifactType::generated_m3u(),
            m3u_file_path,
            "generation".to_string(),
        )
        .with_metadata("target_filename".to_string(), format!("{}.m3u8", self.proxy_config.id).into());
        
        if let Some(size) = m3u_file_size {
            m3u_artifact = m3u_artifact.with_file_size(size);
        }
        generated_artifacts.push(m3u_artifact);
        
        // Add XMLTV artifact if EPG was generated, with actual file size
        if has_epg_programs {
            let xmltv_file_path = format!("{}_temp.xmltv", self.execution.execution_prefix);
            let xmltv_file_size = match self.file_manager.get_full_path(&xmltv_file_path) {
                Ok(full_path) => {
                    match tokio::fs::metadata(&full_path).await {
                        Ok(metadata) => {
                            let size = metadata.len();
                            info!("XMLTV file size determined: {} bytes ({}KB)", size, size / 1024);
                            Some(size)
                        }
                        Err(e) => {
                            warn!("Failed to get XMLTV file size for {}: {}", xmltv_file_path, e);
                            None
                        }
                    }
                }
                Err(e) => {
                    warn!("Failed to get XMLTV full path for {}: {}", xmltv_file_path, e);
                    None
                }
            };
            
            let mut xmltv_artifact = PipelineArtifact::new(
                crate::pipeline::models::ArtifactType::generated_xmltv(),
                xmltv_file_path,
                "generation".to_string(),
            )
            .with_metadata("target_filename".to_string(), format!("{}.xmltv", self.proxy_config.id).into());
            
            if let Some(size) = xmltv_file_size {
                xmltv_artifact = xmltv_artifact.with_file_size(size);
            }
            generated_artifacts.push(xmltv_artifact);
        }
        
        // Send progress: 100% of generation stage (83% overall)
        Self::send_progress_update_with_stage(progress_callback, self.execution.proxy_id, "Generation stage completed", 82.5, Some("generation"), Some(100.0));
        
        Ok((generated_artifacts, channels_count, programs_count))
    }
    
    async fn execute_publish_content_stage(&mut self, input_artifacts: Vec<PipelineArtifact>, progress_callback: &Option<Arc<crate::services::progress_service::UniversalProgressCallback>>) -> Result<Vec<PipelineArtifact>, Box<dyn std::error::Error>> {
        use crate::pipeline::stages::publish_content::PublishContentStage;
        
        // Send progress: 25% of publishing stage (86% overall)
        Self::send_progress_update_with_stage(progress_callback, self.execution.proxy_id, "Initializing file publishing", 86.0, Some("publishing"), Some(25.0));
        
        let publish_stage = PublishContentStage::new(
            self.file_manager.clone(),              // pipeline file manager (for reading temp files)
            self.proxy_output_file_manager.clone(), // proxy output file manager (for final served files)
            self.proxy_config.id,                   // proxy_id
            false,                                  // enable_versioning (disabled for now)
        );
        
        // Send progress: 75% of publishing stage (95% overall)
        Self::send_progress_update_with_stage(progress_callback, self.execution.proxy_id, "Publishing files to final location", 95.0, Some("publishing"), Some(75.0));
        
        // Process artifacts through publish content stage
        let published_artifacts = publish_stage.process(input_artifacts).await?;
        
        // Send progress: 100% of publishing stage (100% overall)
        Self::send_progress_update_with_stage(progress_callback, self.execution.proxy_id, "Publishing completed", 99.5, Some("publishing"), Some(100.0));
        
        Ok(published_artifacts)
    }
    
    /// Update database to mark proxy generation completion
    /// This is CRITICAL to prevent continuous re-runs of the pipeline
    async fn update_proxy_completion_timestamp(&self) -> Result<(), Box<dyn std::error::Error>> {
        // Update the last_generated_at timestamp in the database
        sqlx::query("UPDATE stream_proxies SET last_generated_at = datetime('now'), updated_at = datetime('now') WHERE id = ?")
            .bind(self.proxy_config.id.to_string())
            .execute(&self.db_pool)
            .await?;
        
        tracing::info!(
            "Proxy {} completion timestamp updated in database (last_generated_at = now)", 
            self.proxy_config.id
        );
        
        Ok(())
    }
    
    
    pub fn get_execution(&self) -> &PipelineExecution {
        &self.execution
    }
    
    pub fn get_execution_id(&self) -> Uuid {
        self.execution.id
    }
    
    pub fn get_execution_prefix(&self) -> &str {
        &self.execution.execution_prefix
    }

    /// Start a background task to periodically extend cleanup suspension during pipeline execution
    fn start_suspension_extension_task(&self, stop_flag: std::sync::Arc<std::sync::atomic::AtomicBool>) -> tokio::task::JoinHandle<()> {
        let file_manager = self.file_manager.clone();
        let prefix = self.execution.execution_prefix.clone();
        
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(60)); // Update every 1 minute
            
            loop {
                if stop_flag.load(std::sync::atomic::Ordering::Relaxed) {
                    tracing::debug!("Suspension extension task shutting down for pipeline {}", prefix);
                    break;
                }
                
                interval.tick().await;
                
                // Update suspension to 5 minutes from now
                if let Err(e) = file_manager.update_suspension(DEFAULT_PIPELINE_SUSPENSION_DURATION).await {
                    tracing::warn!("Failed to update pipeline suspension for {}: {}", prefix, e);
                } else {
                    tracing::trace!("Updated pipeline suspension for {}", prefix);
                }
            }
        })
    }



    /// Log comprehensive pipeline summary with channels/programs counts and stage breakdown (optimized version)
    async fn log_pipeline_summary_optimized(
        &self, 
        channels_count: usize, 
        programs_count: usize,
        published_artifacts: &[crate::pipeline::models::PipelineArtifact],
        total_duration: std::time::Duration
    ) {
        // Count published files by type
        let mut m3u_files = 0;
        let mut xmltv_files = 0;
        let mut total_published_bytes = 0u64;
        
        for artifact in published_artifacts {
            match artifact.artifact_type.content {
                crate::pipeline::models::ContentType::M3uPlaylist => {
                    m3u_files += 1;
                    total_published_bytes += artifact.file_size.unwrap_or(0);
                }
                crate::pipeline::models::ContentType::XmltvGuide => {
                    xmltv_files += 1;
                    total_published_bytes += artifact.file_size.unwrap_or(0);
                }
                _ => {}
            }
        }

        // Get stage performance metrics for summary
        let stages = self.performance_tracker.get_stage_summaries();
        
        info!("=== PIPELINE EXECUTION SUMMARY ===");
        info!("Proxy ID: {}", self.execution.proxy_id);
        info!("Total Execution Time: {}", crate::utils::human_format::format_duration_precise(total_duration));
        info!("Content Processed:");
        info!("   Channels: {}", channels_count);
        info!("   EPG Programs: {}", programs_count);
        info!("Files Published:");
        info!("   M3U Playlists: {}", m3u_files);
        info!("   XMLTV Guides: {}", xmltv_files);
        info!("   Total Size: {}KB", total_published_bytes / 1024);
        
        info!("Stage Performance Breakdown:");
        for (stage_name, metrics) in stages {
            if let Some(duration) = metrics.duration {
                let memory_delta = metrics.memory_delta_mb.unwrap_or(0.0);
                let peak_memory_mb = metrics.memory_after
                    .as_ref()
                    .map(|m| m.memory_mb)
                    .unwrap_or(metrics.memory_before.memory_mb);
                
                info!(
                    "   {}: duration={} peak_memory={:.1}MB memory_delta={:.1}MB",
                    stage_name,
                    crate::utils::human_format::format_duration_precise(duration),
                    peak_memory_mb,
                    memory_delta
                );
            }
        }
        
        let final_memory = self.performance_tracker.get_final_memory_snapshot();
        if let Some(memory) = final_memory {
            info!("Final Memory Usage: {:.1}MB ({:.1}% of system)", 
                memory.memory_mb, memory.memory_usage_percent);
        }
        
        info!("Pipeline execution completed successfully");
        info!("==========================================");
    }
    
}

/// Guard that automatically stops suspension extension and resumes cleanup when dropped
struct SuspensionExtensionGuard {
    stop_flag: std::sync::Arc<std::sync::atomic::AtomicBool>,
    file_manager: sandboxed_file_manager::SandboxedManager,
}

impl Drop for SuspensionExtensionGuard {
    fn drop(&mut self) {
        // Stop the extension task
        self.stop_flag.store(true, std::sync::atomic::Ordering::Relaxed);
        
        // Resume cleanup immediately (non-blocking)
        let file_manager = self.file_manager.clone();
        tokio::spawn(async move {
            file_manager.resume_cleanup().await;
            tracing::debug!("Pipeline cleanup resumed via suspension guard");
        });
        
        tracing::debug!("Suspension extension guard dropped, stopping background extension and resuming cleanup");
    }
}

