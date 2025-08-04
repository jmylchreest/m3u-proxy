//! Publish content stage - atomically moves temporary files to final locations
//!
//! This stage handles the atomic publishing of generated M3U and XMLTV files from 
//! temporary pipeline storage to the final proxy output location. It ensures that
//! clients never receive incomplete files during generation by using atomic rename operations.

use anyhow::Result;
use sandboxed_file_manager::SandboxedManager;
use std::sync::Arc;
use std::time::Instant;
use tracing::{info, debug, warn};
use uuid::Uuid;

use crate::pipeline::models::{PipelineArtifact, ArtifactType, ContentType, ProcessingStage};
use crate::pipeline::traits::{PipelineStage, ProgressAware};
use crate::pipeline::error::PipelineError;
use crate::services::progress_service::ProgressManager;

/// Publish content stage - atomically publishes temporary files to final locations
pub struct PublishContentStage {
    pipeline_file_manager: SandboxedManager,  // Pipeline temporary storage
    proxy_output_file_manager: SandboxedManager,  // Final proxy output storage
    proxy_id: Uuid,
    enable_versioning: bool,
    progress_manager: Option<Arc<ProgressManager>>,
}

impl PublishContentStage {
    pub fn new(
        pipeline_file_manager: SandboxedManager,
        proxy_output_file_manager: SandboxedManager,
        proxy_id: Uuid,
        enable_versioning: bool,
        progress_manager: Option<Arc<ProgressManager>>,
    ) -> Self {
        Self {
            pipeline_file_manager,
            proxy_output_file_manager,
            proxy_id,
            enable_versioning,
            progress_manager,
        }
    }
    
    /// Helper method for reporting progress
    async fn report_progress(&self, percentage: f64, message: &str) {
        if let Some(pm) = &self.progress_manager {
            if let Some(updater) = pm.get_stage_updater("publish_content").await {
                updater.update_progress(percentage, message).await;
            }
        }
    }
    
    /// Set the progress manager for this stage
    pub fn set_progress_manager(&mut self, progress_manager: Arc<ProgressManager>) {
        self.progress_manager = Some(progress_manager);
    }

    /// Publish artifacts atomically from temporary to final location
    pub async fn process(&self, input_artifacts: Vec<PipelineArtifact>) -> Result<Vec<PipelineArtifact>> {
        let stage_start = Instant::now();
        info!(
            "Publish content stage STARTED: proxy_id={} artifacts={} versioning={}",
            self.proxy_id, input_artifacts.len(), self.enable_versioning
        );
        
        // Debug: Print details of all input artifacts
        for (i, artifact) in input_artifacts.iter().enumerate() {
            debug!(
                "Input artifact {}: type={:?} file_path={} size={}KB",
                i, artifact.artifact_type.content, artifact.file_path,
                artifact.file_size.unwrap_or(0) / 1024
            );
        }

        let mut published_artifacts = Vec::new();
        let mut total_bytes_published = 0u64;
        let mut files_published = 0;

        let total_artifacts = input_artifacts.len();
        for (artifact_index, artifact) in input_artifacts.into_iter().enumerate() {
            let progress_percentage = 50.0 + (artifact_index as f64 / total_artifacts as f64 * 40.0); // 50% to 90%
            
            match artifact.artifact_type.content {
                ContentType::M3uPlaylist | ContentType::XmltvGuide => {
                    self.report_progress(progress_percentage, &format!("Publishing file {}/{}: {:?}", artifact_index + 1, total_artifacts, artifact.artifact_type.content)).await;
                    let published_artifact = self.publish_file_artifact(artifact).await?;
                    
                    if let Some(file_size) = published_artifact.file_size {
                        total_bytes_published += file_size;
                    }
                    files_published += 1;
                    
                    published_artifacts.push(published_artifact);
                }
                _ => {
                    self.report_progress(progress_percentage, &format!("Processing artifact {}/{}", artifact_index + 1, total_artifacts)).await;
                    // Pass through non-publishable artifacts unchanged
                    published_artifacts.push(artifact);
                }
            }
        }

        let stage_duration = stage_start.elapsed();
        info!(
            "Publish content completed: proxy_id={} files_published={} bytes_published={}KB duration={}",
            self.proxy_id,
            files_published,
            total_bytes_published / 1024,
            crate::utils::human_format::format_duration_precise(stage_duration)
        );

        Ok(published_artifacts)
    }

    /// Publish a single file artifact atomically
    async fn publish_file_artifact(&self, artifact: PipelineArtifact) -> Result<PipelineArtifact> {
        let publish_start = Instant::now();
        
        // Extract target filename from artifact metadata
        let target_filename = artifact.metadata
            .get("target_filename")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing target_filename in artifact metadata"))?
            .to_string();

        info!(
            "Publishing file: temp_file={} target_filename={} content_type={:?} size={}KB",
            artifact.file_path, target_filename, artifact.artifact_type.content,
            artifact.file_size.unwrap_or(0) / 1024
        );

        // Create backup of existing file if versioning is enabled
        if self.enable_versioning {
            debug!("Versioning enabled - creating backup for {}", target_filename);
            self.create_backup(&target_filename).await?;
        } else {
            debug!("Versioning disabled - no backup created for {}", target_filename);
        }

        // Perform atomic move from temporary to final location
        self.atomic_move(&artifact.file_path, &target_filename).await?;

        let publish_duration = publish_start.elapsed();
        
        // Create published artifact with updated metadata
        let published_artifact = PipelineArtifact::new(
            ArtifactType::new(artifact.artifact_type.content, ProcessingStage::Published),
            target_filename.clone(),
            "publish_content".to_string(),
        )
        .with_record_count(artifact.record_count.unwrap_or(0))
        .with_file_size(artifact.file_size.unwrap_or(0))
        .with_metadata("proxy_id".to_string(), self.proxy_id.to_string().into())
        .with_metadata("original_temp_file".to_string(), artifact.file_path.clone().into())
        .with_metadata("publish_duration_ms".to_string(), publish_duration.as_millis().to_string().into());

        info!(
            "File published: {} -> {} ({} bytes) duration={}",
            artifact.file_path,
            target_filename,
            artifact.file_size.unwrap_or(0),
            crate::utils::human_format::format_duration_precise(publish_duration)
        );

        Ok(published_artifact)
    }

    /// Create backup of existing file if it exists
    async fn create_backup(&self, target_filename: &str) -> Result<()> {
        // Check if target file exists
        let exists = match self.proxy_output_file_manager.exists(target_filename).await {
            Ok(exists) => exists,
            Err(_) => false, // Assume doesn't exist if we can't check
        };

        if exists {
            let backup_filename = format!("{}.backup.{}", target_filename, chrono::Utc::now().timestamp());
            
            // Read existing file
            let existing_content = self.proxy_output_file_manager.read(target_filename).await
                .map_err(|e| anyhow::anyhow!("Failed to read existing file for backup: {}", e))?;
            
            // Write backup file
            self.proxy_output_file_manager.write(&backup_filename, &existing_content).await
                .map_err(|e| anyhow::anyhow!("Failed to create backup file: {}", e))?;
            
            info!(
                "Backup created: {} -> {} ({}KB)",
                target_filename, backup_filename, existing_content.len() / 1024
            );
        }

        Ok(())
    }

    /// Perform atomic move from temporary to final location
    async fn atomic_move(&self, temp_file_path: &str, target_filename: &str) -> Result<()> {
        info!("Atomic move: {} -> {}", temp_file_path, target_filename);
        
        // Read from temporary location
        let content = self.pipeline_file_manager.read(temp_file_path).await
            .map_err(|e| anyhow::anyhow!("Failed to read temporary file '{}': {}", temp_file_path, e))?;
        
        debug!("Read {} bytes from temporary file: {}", content.len(), temp_file_path);

        // Write to final location (this is atomic at the filesystem level)
        self.proxy_output_file_manager.write(target_filename, &content).await
            .map_err(|e| anyhow::anyhow!("Failed to write to final location '{}': {}", target_filename, e))?;

        info!(
            "Atomic move completed successfully: {} -> {} ({} bytes)",
            temp_file_path, target_filename, content.len()
        );

        Ok(())
    }

    /// Get published file path for a given content type
    pub fn get_published_file_path(&self, content_type: ContentType) -> String {
        match content_type {
            ContentType::M3uPlaylist => format!("{}.m3u8", self.proxy_id),
            ContentType::XmltvGuide => format!("{}.xmltv", self.proxy_id),
            _ => format!("{}_{:?}.unknown", self.proxy_id, content_type),
        }
    }

    /// Clean up any published files (for error recovery)
    pub async fn cleanup_published_files(&self, published_artifacts: &[PipelineArtifact]) -> Result<()> {
        for artifact in published_artifacts {
            if matches!(artifact.artifact_type.stage, ProcessingStage::Published) {
                match self.proxy_output_file_manager.remove_file(&artifact.file_path).await {
                    Ok(_) => debug!("Cleaned up published file: {}", artifact.file_path),
                    Err(e) => warn!("Failed to clean up published file '{}': {}", artifact.file_path, e),
                }
            }
        }
        Ok(())
    }
}

impl ProgressAware for PublishContentStage {
    fn get_progress_manager(&self) -> Option<&Arc<ProgressManager>> {
        self.progress_manager.as_ref()
    }
}

#[async_trait::async_trait]
impl PipelineStage for PublishContentStage {
    async fn execute(&mut self, input: Vec<PipelineArtifact>) -> Result<Vec<PipelineArtifact>, PipelineError> {
        self.report_progress(25.0, "Initializing file publishing").await;
        let result = self.process(input).await
            .map_err(|e| PipelineError::stage_error("publish_content", format!("Publishing failed: {}", e)))?;
        self.report_progress(100.0, "Publishing completed").await;
        Ok(result)
    }
    
    fn stage_id(&self) -> &'static str {
        "publish_content"
    }
    
    fn stage_name(&self) -> &'static str {
        "Publish Content"
    }
    
    async fn cleanup(&mut self) -> Result<(), PipelineError> {
        Ok(())
    }
    
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}