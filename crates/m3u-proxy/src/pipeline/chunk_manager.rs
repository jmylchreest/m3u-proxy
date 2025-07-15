//! Production-ready dynamic chunk size management with upstream cascade
//!
//! This module provides a complete solution for managing chunk sizes across
//! the entire pipeline, allowing plugins to request optimal chunk sizes while
//! ensuring efficient memory usage and smooth data flow.

use anyhow::Result;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

/// Production-ready chunk size manager with upstream cascade
#[derive(Debug)]
pub struct ChunkSizeManager {
    /// Track the largest requested chunk size per pipeline stage
    max_requested_sizes: Arc<RwLock<HashMap<String, usize>>>,

    /// Minimum buffer size (always at least 1 chunk worth)
    min_buffer_size: usize,

    /// Current buffer allocation per iterator/stage
    current_buffer_sizes: Arc<RwLock<HashMap<String, usize>>>,

    /// Default chunk size for new iterators
    default_chunk_size: usize,

    /// Maximum allowed chunk size (safety limit)
    max_chunk_size: usize,

    /// Stage dependency graph for upstream cascade
    stage_dependencies: HashMap<String, Vec<String>>,
}

impl ChunkSizeManager {
    /// Create a new production-ready chunk size manager
    pub fn new(default_chunk_size: usize, max_chunk_size: usize) -> Self {
        let mut stage_dependencies = HashMap::new();

        // Define the pipeline stage dependencies for upstream cascade
        stage_dependencies.insert("source_loading".to_string(), vec![]);
        stage_dependencies.insert(
            "data_mapping".to_string(),
            vec!["source_loading".to_string()],
        );
        stage_dependencies.insert(
            "filtering".to_string(),
            vec!["source_loading".to_string(), "data_mapping".to_string()],
        );
        stage_dependencies.insert(
            "logo_prefetch".to_string(),
            vec![
                "source_loading".to_string(),
                "data_mapping".to_string(),
                "filtering".to_string(),
            ],
        );
        stage_dependencies.insert(
            "program_logo_prefetch".to_string(),
            vec![
                "source_loading".to_string(),
                "data_mapping".to_string(),
                "filtering".to_string(),
            ],
        );
        stage_dependencies.insert(
            "channel_numbering".to_string(),
            vec![
                "source_loading".to_string(),
                "data_mapping".to_string(),
                "filtering".to_string(),
                "logo_prefetch".to_string(),
            ],
        );
        stage_dependencies.insert(
            "m3u_generation".to_string(),
            vec![
                "source_loading".to_string(),
                "data_mapping".to_string(),
                "filtering".to_string(),
                "logo_prefetch".to_string(),
                "channel_numbering".to_string(),
            ],
        );
        stage_dependencies.insert(
            "epg_processing".to_string(),
            vec![
                "source_loading".to_string(),
                "data_mapping".to_string(),
                "filtering".to_string(),
                "program_logo_prefetch".to_string(),
            ],
        );

        Self {
            max_requested_sizes: Arc::new(RwLock::new(HashMap::new())),
            min_buffer_size: default_chunk_size.max(100), // At least 100 items
            current_buffer_sizes: Arc::new(RwLock::new(HashMap::new())),
            default_chunk_size,
            max_chunk_size,
            stage_dependencies,
        }
    }

    /// Request a specific chunk size for a stage (production-ready with cascade)
    pub async fn request_chunk_size(&self, stage: &str, requested_size: usize) -> Result<usize> {
        // Validate requested size
        let clamped_size = requested_size.clamp(1, self.max_chunk_size);
        if clamped_size != requested_size {
            warn!(
                "Chunk size {} clamped to {} for stage {}",
                requested_size, clamped_size, stage
            );
        }

        let mut max_sizes = self.max_requested_sizes.write().await;
        let mut buffer_sizes = self.current_buffer_sizes.write().await;

        // Update maximum requested size for this stage
        let current_max = max_sizes
            .get(stage)
            .copied()
            .unwrap_or(self.default_chunk_size);
        let new_max = current_max.max(clamped_size);

        if new_max > current_max {
            info!(
                "├─ Stage '{}' chunk size increased: {} → {}",
                stage, current_max, new_max
            );
            max_sizes.insert(stage.to_string(), new_max);

            // Calculate new buffer size (at least 2x chunk size for smooth flow)
            let new_buffer_size = (new_max * 2).max(self.min_buffer_size);
            buffer_sizes.insert(stage.to_string(), new_buffer_size);

            // Cascade upstream to increase buffer sizes of dependent stages
            if let Some(upstream_stages) = self.stage_dependencies.get(stage) {
                for upstream_stage in upstream_stages {
                    let upstream_buffer = buffer_sizes
                        .get(upstream_stage)
                        .copied()
                        .unwrap_or(self.min_buffer_size);
                    if new_buffer_size > upstream_buffer {
                        info!(
                            "└─ Cascading buffer resize to '{}': {} → {}",
                            upstream_stage, upstream_buffer, new_buffer_size
                        );
                        buffer_sizes.insert(upstream_stage.to_string(), new_buffer_size);
                        max_sizes.insert(upstream_stage.to_string(), new_max);
                    }
                }
            }
        }

        Ok(clamped_size)
    }

    /// Get the current optimal chunk size for a stage
    pub async fn get_chunk_size(&self, stage: &str) -> usize {
        let max_sizes = self.max_requested_sizes.read().await;
        max_sizes
            .get(stage)
            .copied()
            .unwrap_or(self.default_chunk_size)
    }

    /// Get the current buffer size for a stage
    pub async fn get_buffer_size(&self, stage: &str) -> usize {
        let buffer_sizes = self.current_buffer_sizes.read().await;
        buffer_sizes
            .get(stage)
            .copied()
            .unwrap_or(self.min_buffer_size)
    }

    /// Set buffer size for a specific stage (with cascade if needed)
    pub async fn set_buffer_size(&self, stage: &str, buffer_size: usize) -> Result<()> {
        let clamped_buffer = buffer_size.clamp(self.min_buffer_size, self.max_chunk_size * 4);

        let mut buffer_sizes = self.current_buffer_sizes.write().await;
        buffer_sizes.insert(stage.to_string(), clamped_buffer);

        debug!("Buffer size set for stage '{}': {}", stage, clamped_buffer);
        Ok(())
    }

    /// Get current statistics for monitoring
    pub async fn get_stats(&self) -> ChunkSizeStats {
        let max_sizes = self.max_requested_sizes.read().await;
        let buffer_sizes = self.current_buffer_sizes.read().await;

        ChunkSizeStats {
            total_stages: max_sizes.len(),
            max_chunk_size: max_sizes
                .values()
                .copied()
                .max()
                .unwrap_or(self.default_chunk_size),
            total_buffer_memory: buffer_sizes.values().sum::<usize>(),
            stage_stats: max_sizes
                .iter()
                .map(|(stage, &chunk_size)| {
                    let buffer_size = buffer_sizes
                        .get(stage)
                        .copied()
                        .unwrap_or(self.min_buffer_size);
                    (
                        stage.clone(),
                        StageChunkStats {
                            chunk_size,
                            buffer_size,
                        },
                    )
                })
                .collect(),
        }
    }

    /// Reset all chunk sizes (useful for testing or reconfiguration)
    pub async fn reset(&self) {
        let mut max_sizes = self.max_requested_sizes.write().await;
        let mut buffer_sizes = self.current_buffer_sizes.write().await;

        max_sizes.clear();
        buffer_sizes.clear();

        info!("Chunk size manager reset to defaults");
    }
}

/// Statistics for monitoring chunk size management
#[derive(Debug, Clone)]
pub struct ChunkSizeStats {
    pub total_stages: usize,
    pub max_chunk_size: usize,
    pub total_buffer_memory: usize,
    pub stage_stats: HashMap<String, StageChunkStats>,
}

/// Per-stage chunk size statistics
#[derive(Debug, Clone)]
pub struct StageChunkStats {
    pub chunk_size: usize,
    pub buffer_size: usize,
}

impl Default for ChunkSizeManager {
    fn default() -> Self {
        Self::new(
            1500,  // 1.5K default chunk size
            50000, // 50K max chunk size (safety limit)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_chunk_size_request_and_cascade() {
        let manager = ChunkSizeManager::new(100, 10000);

        // Request larger chunk size for filtering stage
        let result = manager.request_chunk_size("filtering", 2000).await.unwrap();
        assert_eq!(result, 2000);

        // Check that upstream stages got cascaded buffer increases
        let source_buffer = manager.get_buffer_size("source_loading").await;
        let mapping_buffer = manager.get_buffer_size("data_mapping").await;

        assert!(source_buffer >= 4000); // At least 2x chunk size
        assert!(mapping_buffer >= 4000);

        // Check stats
        let stats = manager.get_stats().await;
        assert!(stats.max_chunk_size >= 2000);
        assert!(stats.total_buffer_memory > 0);
    }

    #[tokio::test]
    async fn test_chunk_size_clamping() {
        let manager = ChunkSizeManager::new(100, 1000);

        // Request size above maximum
        let result = manager.request_chunk_size("filtering", 5000).await.unwrap();
        assert_eq!(result, 1000); // Should be clamped to max

        // Request size below minimum
        let result = manager.request_chunk_size("filtering", 0).await.unwrap();
        assert_eq!(result, 1); // Should be clamped to minimum
    }
}
