//! Streaming stage interfaces that support chunking and partial processing
//!
//! This module provides the improved stage architecture that can handle
//! streaming inputs, chunked processing, and proper completion signaling.

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

use crate::models::*;

/// Streaming chunk with completion information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StageChunk<T> {
    pub data: Vec<T>,
    pub chunk_id: usize,
    pub is_final_chunk: bool,
    pub total_chunks: Option<usize>,
    pub total_items: Option<usize>,
}

/// Strategy capabilities for determining processing approach
#[derive(Debug, Clone)]
pub struct StageCapabilities {
    pub supports_streaming: bool,
    pub requires_all_data: bool,
    pub can_produce_early_output: bool,
    pub preferred_chunk_size: Option<usize>,
    pub memory_efficient: bool,
}

/// Reference to a temporary file created during processing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TempFileRef {
    pub file_id: String,
    pub size_bytes: usize,
    pub chunk_count: usize,
    pub content_type: String, // "channels", "mapped_channels", etc.
    pub metadata: HashMap<String, String>,
}

/// Progress tracking for a stage
#[derive(Debug, Clone)]
pub struct StageProgress {
    pub stage_name: String,
    pub chunks_processed: usize,
    pub total_chunks: Option<usize>,
    pub items_processed: usize,
    pub total_items: Option<usize>,
    pub start_time: std::time::Instant,
}

/// Standard output keys for cross-stage communication
pub mod stage_output_keys {
    pub const SOURCE_CHANNELS: &str = "source_channels";
    pub const MAPPED_CHANNELS: &str = "mapped_channels"; 
    pub const FILTERED_CHANNELS: &str = "filtered_channels";
    pub const NUMBERED_CHANNELS: &str = "numbered_channels";
    pub const M3U_CONTENT: &str = "m3u_content";
}

/// Shared context for cross-stage communication and streaming coordination
#[derive(Debug)]
pub struct StreamingStageContext {
    pub temp_files: HashMap<String, TempFileRef>,
    pub metadata: HashMap<String, serde_json::Value>,
    pub memory_pressure: crate::proxy::stage_strategy::MemoryPressureLevel,
    pub progress: HashMap<String, StageProgress>,
    pub proxy_config: ResolvedProxyConfig,
}

impl StreamingStageContext {
    pub fn new(proxy_config: ResolvedProxyConfig) -> Self {
        Self {
            temp_files: HashMap::new(),
            metadata: HashMap::new(),
            memory_pressure: crate::proxy::stage_strategy::MemoryPressureLevel::Optimal,
            progress: HashMap::new(),
            proxy_config,
        }
    }

    /// Add a temp file reference for cross-stage communication
    pub fn add_temp_file(&mut self, key: String, temp_file: TempFileRef) {
        self.temp_files.insert(key, temp_file);
    }

    /// Get a temp file reference
    pub fn get_temp_file(&self, key: &str) -> Option<&TempFileRef> {
        self.temp_files.get(key)
    }
    
    /// Get previous stage output by standard key
    pub fn get_stage_output(&self, stage_output_key: &str) -> Option<&TempFileRef> {
        self.temp_files.get(stage_output_key)
    }
    
    /// Add stage output with standard key
    pub fn add_stage_output(&mut self, stage_output_key: &str, temp_file: TempFileRef) {
        self.temp_files.insert(stage_output_key.to_string(), temp_file);
    }
    
    /// Get all temp files for a specific content type
    pub fn get_temp_files_by_type(&self, content_type: &str) -> Vec<(&String, &TempFileRef)> {
        self.temp_files
            .iter()
            .filter(|(_, temp_file)| temp_file.content_type == content_type)
            .collect()
    }

    /// Update stage progress
    pub fn update_progress(&mut self, stage_name: String, chunks_processed: usize, items_processed: usize) {
        if let Some(progress) = self.progress.get_mut(&stage_name) {
            progress.chunks_processed = chunks_processed;
            progress.items_processed = items_processed;
        } else {
            self.progress.insert(stage_name.clone(), StageProgress {
                stage_name,
                chunks_processed,
                total_chunks: None,
                items_processed,
                total_items: None,
                start_time: std::time::Instant::now(),
            });
        }
    }
}

/// Streaming stage interface that supports chunked processing
#[async_trait]
pub trait StreamingSourceLoadingStage: Send + Sync {
    /// Process a chunk of source IDs
    async fn process_source_chunk(
        &mut self,
        chunk: StageChunk<Uuid>,
        context: &mut StreamingStageContext,
    ) -> Result<Vec<Channel>>;

    /// Finalize processing after all chunks
    async fn finalize_source_loading(
        &mut self,
        context: &mut StreamingStageContext,
    ) -> Result<Vec<Channel>>;

    /// Get strategy capabilities
    fn source_capabilities(&self) -> StageCapabilities;

    fn strategy_name(&self) -> &str;
}

/// Streaming data mapping stage
#[async_trait]
pub trait StreamingDataMappingStage: Send + Sync {
    /// Process a chunk of channels
    async fn process_mapping_chunk(
        &mut self,
        chunk: StageChunk<Channel>,
        context: &mut StreamingStageContext,
    ) -> Result<Vec<Channel>>;

    /// Finalize mapping after all chunks
    async fn finalize_data_mapping(
        &mut self,
        context: &mut StreamingStageContext,
    ) -> Result<Vec<Channel>>;

    /// Get strategy capabilities
    fn mapping_capabilities(&self) -> StageCapabilities;

    fn strategy_name(&self) -> &str;
}

/// Streaming filtering stage
#[async_trait]
pub trait StreamingFilteringStage: Send + Sync {
    /// Process a chunk of channels
    async fn process_filtering_chunk(
        &mut self,
        chunk: StageChunk<Channel>,
        context: &mut StreamingStageContext,
    ) -> Result<Vec<Channel>>;

    /// Finalize filtering after all chunks
    async fn finalize_filtering(
        &mut self,
        context: &mut StreamingStageContext,
    ) -> Result<Vec<Channel>>;

    /// Get strategy capabilities
    fn filtering_capabilities(&self) -> StageCapabilities;

    fn strategy_name(&self) -> &str;
}

/// Result of streaming stage processing
pub enum StreamingStageResult<T> {
    /// Partial output that can be passed to next stage immediately
    Partial(Vec<T>),
    /// Complete output, processing finished
    Complete(Vec<T>),
    /// No output yet, accumulating data (used by batch strategies)
    Pending,
    /// Spilled to temp file, contains file reference
    Spilled(TempFileRef),
}

/// Unified streaming stage trait for WASM plugins
#[async_trait]
pub trait WasmStreamingStage: Send + Sync {
    /// Process any type of chunk (source IDs, channels, etc.)
    async fn process_chunk(
        &mut self,
        chunk_data: &[u8], // Serialized chunk
        chunk_metadata: &StageChunk<()>, // Metadata without data
        context: &mut StreamingStageContext,
    ) -> Result<Vec<u8>>; // Serialized output

    /// Finalize processing
    async fn finalize(
        &mut self,
        context: &mut StreamingStageContext,
    ) -> Result<Vec<u8>>;

    /// Get capabilities
    fn capabilities(&self) -> StageCapabilities;

    fn strategy_name(&self) -> &str;
}

/// Helper functions for chunk management
impl<T> StageChunk<T> {
    pub fn new(data: Vec<T>, chunk_id: usize) -> Self {
        Self {
            data,
            chunk_id,
            is_final_chunk: false,
            total_chunks: None,
            total_items: None,
        }
    }

    pub fn final_chunk(data: Vec<T>, chunk_id: usize, total_chunks: usize) -> Self {
        Self {
            data,
            chunk_id,
            is_final_chunk: true,
            total_chunks: Some(total_chunks),
            total_items: None,
        }
    }

    pub fn with_totals(mut self, total_chunks: usize, total_items: usize) -> Self {
        self.total_chunks = Some(total_chunks);
        self.total_items = Some(total_items);
        self
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }
}

/// Default capabilities for different strategy types
impl StageCapabilities {
    /// Simple in-memory strategy
    pub fn simple_inmemory() -> Self {
        Self {
            supports_streaming: false,
            requires_all_data: true,
            can_produce_early_output: false,
            preferred_chunk_size: None,
            memory_efficient: false,
        }
    }

    /// Streaming strategy that can process chunks independently
    pub fn streaming() -> Self {
        Self {
            supports_streaming: true,
            requires_all_data: false,
            can_produce_early_output: true,
            preferred_chunk_size: Some(1000),
            memory_efficient: true,
        }
    }

    /// Chunked strategy that processes in batches but needs completion signal
    pub fn chunked(chunk_size: usize) -> Self {
        Self {
            supports_streaming: true,
            requires_all_data: false,
            can_produce_early_output: false,
            preferred_chunk_size: Some(chunk_size),
            memory_efficient: true,
        }
    }

    /// Memory-efficient strategy that spills to disk
    pub fn memory_efficient_spill() -> Self {
        Self {
            supports_streaming: true,
            requires_all_data: false,
            can_produce_early_output: false,
            preferred_chunk_size: Some(500),
            memory_efficient: true,
        }
    }
}