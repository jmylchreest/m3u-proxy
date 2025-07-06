//! Production-ready pipeline infrastructure
//!
//! This module contains the core pipeline infrastructure for processing
//! data through multiple stages with dynamic chunk size management,
//! buffered iterators, and efficient memory usage.

pub mod buffered_iterator;
pub mod chunk_manager;
pub mod generic_iterator;
pub mod iterator_traits;
pub mod orchestrator;
pub mod stages;

// Re-export key types for easier access
pub use buffered_iterator::{BufferedIterator, DataSource};
pub use chunk_manager::{ChunkSizeManager, ChunkSizeStats, StageChunkStats};
pub use iterator_traits::{IteratorResult, PluginIterator};
pub use orchestrator::{
    OrderedChannelAggregateIterator, OrderedDataMappingIterator, 
    OrderedEpgAggregateIterator, OrderedFilterIterator
};

/// Pipeline stage names for consistent naming across the system
pub mod stage_names {
    pub const SOURCE_LOADING: &str = "source_loading";
    pub const DATA_MAPPING: &str = "data_mapping";
    pub const FILTERING: &str = "filtering";
    pub const LOGO_PREFETCH: &str = "logo_prefetch";
    pub const CHANNEL_NUMBERING: &str = "channel_numbering";
    pub const M3U_GENERATION: &str = "m3u_generation";
    pub const EPG_PROCESSING: &str = "epg_processing";
}

/// Pipeline configuration for production use
#[derive(Debug, Clone)]
pub struct PipelineConfig {
    /// Default chunk size for iterators
    pub default_chunk_size: usize,
    /// Maximum allowed chunk size (safety limit)
    pub max_chunk_size: usize,
    /// Default buffer size multiplier (buffer = chunk_size * multiplier)
    pub buffer_size_multiplier: usize,
    /// Maximum memory usage per stage (in MB)
    pub max_memory_per_stage_mb: usize,
    /// Enable chunk size cascade optimization
    pub enable_chunk_cascade: bool,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            default_chunk_size: 1000,
            max_chunk_size: 50000,
            buffer_size_multiplier: 2,
            max_memory_per_stage_mb: 256,
            enable_chunk_cascade: true,
        }
    }
}

/// Production-ready pipeline factory
pub struct PipelineFactory {
    config: PipelineConfig,
    chunk_manager: std::sync::Arc<ChunkSizeManager>,
}

impl PipelineFactory {
    /// Create new pipeline factory with configuration
    pub fn new(config: PipelineConfig) -> Self {
        let chunk_manager = std::sync::Arc::new(ChunkSizeManager::new(
            config.default_chunk_size,
            config.max_chunk_size,
        ));
        
        Self {
            config,
            chunk_manager,
        }
    }
    
    /// Get shared chunk manager for coordinating across pipeline stages
    pub fn chunk_manager(&self) -> std::sync::Arc<ChunkSizeManager> {
        self.chunk_manager.clone()
    }
    
    /// Get pipeline configuration
    pub fn config(&self) -> &PipelineConfig {
        &self.config
    }
    
    /// Create a buffered iterator for a specific stage
    pub fn create_buffered_iterator<T>(
        &self,
        stage_name: String,
        data_source: std::sync::Arc<dyn DataSource<T>>,
    ) -> BufferedIterator<T>
    where
        T: Clone + Send + Sync + 'static,
    {
        BufferedIterator::new(
            stage_name,
            self.chunk_manager.clone(),
            data_source,
            self.config.default_chunk_size * self.config.buffer_size_multiplier,
            self.config.default_chunk_size,
        )
    }
}