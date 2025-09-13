//! Cleanup stage for pipeline processing
//!
//! This stage handles cleanup of temporary files and resources after pipeline execution.
//! It supports both normal cleanup (after successful processing) and error cleanup
//! (when pipeline stages fail).

use anyhow::Result;
use sandboxed_file_manager::SandboxedManager;
use std::time::Instant;
use tracing::{debug, error, info, warn};

use crate::pipeline::models::PipelineArtifact;

/// Cleanup mode determines what cleanup actions to perform
#[derive(Debug, Clone, Copy)]
pub enum CleanupMode {
    /// Normal cleanup after successful pipeline completion
    Success,
    /// Error cleanup when pipeline stages fail
    Error,
}

/// Enhanced cleanup stage with error handling and recovery
pub struct CleanupStage {
    pipeline_file_manager: SandboxedManager,
    pipeline_execution_prefix: String,
    cleanup_mode: CleanupMode,
}

impl CleanupStage {
    pub fn new(
        pipeline_file_manager: SandboxedManager,
        pipeline_execution_prefix: String,
        cleanup_mode: CleanupMode,
    ) -> Self {
        Self {
            pipeline_file_manager,
            pipeline_execution_prefix,
            cleanup_mode,
        }
    }

    /// Execute cleanup based on the configured mode and pipeline state
    pub async fn process(
        &self,
        input_artifacts: Vec<PipelineArtifact>,
        stage_error: Option<String>,
    ) -> Result<Vec<PipelineArtifact>> {
        let cleanup_start = Instant::now();

        match self.cleanup_mode {
            CleanupMode::Success => {
                info!(
                    "Cleanup stage (success): execution_prefix={} artifacts={}",
                    self.pipeline_execution_prefix,
                    input_artifacts.len()
                );
                self.cleanup_successful_pipeline(input_artifacts).await
            }
            CleanupMode::Error => {
                error!(
                    "Cleanup stage (error): execution_prefix={} artifacts={} error={:?}",
                    self.pipeline_execution_prefix,
                    input_artifacts.len(),
                    stage_error.as_ref().unwrap_or(&"Unknown error".to_string())
                );
                self.cleanup_failed_pipeline(input_artifacts, stage_error)
                    .await
            }
        }?;

        let cleanup_duration = cleanup_start.elapsed();
        info!(
            "Cleanup completed: mode={:?} duration={}",
            self.cleanup_mode,
            crate::utils::human_format::format_duration_precise(cleanup_duration)
        );

        // Return empty artifacts list as cleanup is final stage
        Ok(Vec::new())
    }

    /// Clean up after successful pipeline completion
    async fn cleanup_successful_pipeline(&self, artifacts: Vec<PipelineArtifact>) -> Result<()> {
        let mut temp_files_cleaned = 0;
        let mut total_bytes_cleaned = 0u64;

        // Clean up temporary files from all stages
        for artifact in &artifacts {
            if self.should_cleanup_artifact(artifact, true) {
                match self.cleanup_artifact_file(artifact).await {
                    Ok(bytes) => {
                        temp_files_cleaned += 1;
                        total_bytes_cleaned += bytes;
                    }
                    Err(e) => {
                        warn!(
                            "Failed to cleanup artifact file '{}': {}",
                            artifact.file_path, e
                        );
                    }
                }
            }
        }

        // Clean up any remaining temporary files with our execution prefix
        let additional_cleaned = self.cleanup_execution_prefix_files().await?;

        info!(
            "Success cleanup completed: temp_files_cleaned={} additional_files_cleaned={} total_bytes_cleaned={}KB",
            temp_files_cleaned,
            additional_cleaned,
            total_bytes_cleaned / 1024
        );

        Ok(())
    }

    /// Clean up after pipeline failure
    async fn cleanup_failed_pipeline(
        &self,
        artifacts: Vec<PipelineArtifact>,
        stage_error: Option<String>,
    ) -> Result<()> {
        let mut temp_files_cleaned = 0;
        let mut published_files_reverted = 0;
        let mut total_bytes_cleaned = 0u64;

        // More aggressive cleanup for failed pipelines
        for artifact in &artifacts {
            if self.should_cleanup_artifact(artifact, false) {
                match self.cleanup_artifact_file(artifact).await {
                    Ok(bytes) => {
                        temp_files_cleaned += 1;
                        total_bytes_cleaned += bytes;

                        // Check if this was a published file that needs reverting
                        if matches!(
                            artifact.artifact_type.stage,
                            crate::pipeline::models::ProcessingStage::Published
                        ) {
                            published_files_reverted += 1;
                            debug!("Reverted published file: {}", artifact.file_path);
                        }
                    }
                    Err(e) => {
                        warn!(
                            "Failed to cleanup failed pipeline artifact '{}': {}",
                            artifact.file_path, e
                        );
                    }
                }
            }
        }

        // Clean up any remaining temporary files with our execution prefix
        let additional_cleaned = self.cleanup_execution_prefix_files().await?;

        error!(
            "Error cleanup completed: error={} temp_files_cleaned={} published_files_reverted={} additional_files_cleaned={} total_bytes_cleaned={}KB",
            stage_error.as_ref().unwrap_or(&"Unknown error".to_string()),
            temp_files_cleaned,
            published_files_reverted,
            additional_cleaned,
            total_bytes_cleaned / 1024
        );

        Ok(())
    }

    /// Determine if an artifact file should be cleaned up
    fn should_cleanup_artifact(&self, artifact: &PipelineArtifact, success_mode: bool) -> bool {
        use crate::pipeline::models::ProcessingStage;

        if success_mode {
            // In success mode, clean up temporary files but leave published files
            match artifact.artifact_type.stage {
                ProcessingStage::Generated => true, // Clean up temporary generated files
                ProcessingStage::Published => false, // Keep published files
                _ => {
                    artifact.file_path.contains("temp")
                        || artifact.file_path.contains(&self.pipeline_execution_prefix)
                }
            }
        } else {
            // In error mode, clean up all pipeline-related files including published ones
            match artifact.artifact_type.stage {
                ProcessingStage::Generated => true, // Clean up temporary files
                ProcessingStage::Published => true, // Revert published files on error
                _ => {
                    artifact.file_path.contains("temp")
                        || artifact.file_path.contains(&self.pipeline_execution_prefix)
                }
            }
        }
    }

    /// Clean up a single artifact file
    async fn cleanup_artifact_file(&self, artifact: &PipelineArtifact) -> Result<u64> {
        // Get file size before deletion for reporting
        let file_size = artifact.file_size.unwrap_or({
            // Use recorded file size if available, otherwise default to 0
            // TODO: We could make this async to get actual file size, but it's not critical
            0
        });

        // Delete the file
        self.pipeline_file_manager
            .remove_file(&artifact.file_path)
            .await
            .map_err(|e| {
                anyhow::anyhow!(
                    "Failed to delete artifact file '{}': {}",
                    artifact.file_path,
                    e
                )
            })?;

        debug!(
            "Cleaned up artifact file: {} ({} bytes) stage={:?}",
            artifact.file_path, file_size, artifact.artifact_type.stage
        );

        Ok(file_size)
    }

    /// Clean up any remaining files with our execution prefix
    async fn cleanup_execution_prefix_files(&self) -> Result<usize> {
        // List files that match our execution prefix pattern
        let temp_file_patterns = vec![
            format!("{}_temp.m3u8", self.pipeline_execution_prefix),
            format!("{}_temp.xmltv", self.pipeline_execution_prefix),
            format!("{}_final_output.m3u", self.pipeline_execution_prefix),
            format!("{}*.jsonl", self.pipeline_execution_prefix),
        ];

        let mut additional_cleaned = 0;

        for pattern in &temp_file_patterns {
            // Try to delete files matching this pattern
            // Note: SandboxedManager doesn't have glob support, so we try exact matches
            match self.pipeline_file_manager.remove_file(pattern).await {
                Ok(_) => {
                    debug!("Cleaned up pattern file: {}", pattern);
                    additional_cleaned += 1;
                }
                Err(_) => {
                    // File probably doesn't exist, which is fine
                    debug!("Pattern file not found (ok): {}", pattern);
                }
            }
        }

        if additional_cleaned > 0 {
            info!(
                "Additional cleanup: removed {} files matching execution prefix {}",
                additional_cleaned, self.pipeline_execution_prefix
            );
        }

        Ok(additional_cleaned)
    }

    /// Create a cleanup stage for successful pipeline completion
    pub fn success(
        pipeline_file_manager: SandboxedManager,
        pipeline_execution_prefix: String,
    ) -> Self {
        Self::new(
            pipeline_file_manager,
            pipeline_execution_prefix,
            CleanupMode::Success,
        )
    }

    /// Create a cleanup stage for failed pipeline error recovery
    pub fn error(
        pipeline_file_manager: SandboxedManager,
        pipeline_execution_prefix: String,
    ) -> Self {
        Self::new(
            pipeline_file_manager,
            pipeline_execution_prefix,
            CleanupMode::Error,
        )
    }
}
