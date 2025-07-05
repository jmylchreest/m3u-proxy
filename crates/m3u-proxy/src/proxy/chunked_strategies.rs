//! Real chunking strategies that demonstrate streaming, temp files, and completion signaling
//!
//! These strategies show how to implement memory-efficient processing that can:
//! - Process data in chunks
//! - Spill to temporary files when memory pressure is high
//! - Signal completion properly to downstream stages
//! - Pass context between stages

use anyhow::Result;
use async_trait::async_trait;
use std::collections::HashMap;
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::database::Database;
use crate::models::*;
use crate::proxy::streaming_stages::*;

/// Chunked source loading strategy that demonstrates real WASM-style processing
pub struct ChunkedSourceLoader {
    database: Database,
    chunk_size: usize,
    accumulated_channels: Vec<Channel>,
    spilled_files: Vec<String>,
    chunks_processed: usize,
    memory_threshold_mb: usize,
}

impl ChunkedSourceLoader {
    pub fn new(database: Database, chunk_size: usize, memory_threshold_mb: usize) -> Self {
        Self {
            database,
            chunk_size,
            accumulated_channels: Vec::new(),
            spilled_files: Vec::new(),
            chunks_processed: 0,
            memory_threshold_mb,
        }
    }

    /// Check if we should spill to disk based on memory usage
    async fn should_spill(&self, context: &StreamingStageContext) -> bool {
        // Estimate memory usage
        let estimated_memory_mb = self.accumulated_channels.len() * 1024 / (1024 * 1024); // Rough estimate
        
        // Check memory pressure from context
        match context.memory_pressure {
            crate::proxy::stage_strategy::MemoryPressureLevel::Critical | 
            crate::proxy::stage_strategy::MemoryPressureLevel::Emergency => true,
            _ => estimated_memory_mb >= self.memory_threshold_mb,
        }
    }

    /// Spill accumulated channels to temp file
    async fn spill_to_temp_file(&mut self, context: &mut StreamingStageContext) -> Result<()> {
        if self.accumulated_channels.is_empty() {
            return Ok(());
        }

        let file_id = format!("chunked_source_{}", self.chunks_processed);
        info!("Spilling {} channels to temp file: {}", self.accumulated_channels.len(), file_id);

        // Serialize channels to JSON
        let serialized = serde_json::to_vec(&self.accumulated_channels)?;
        
        // Write to temp file via host interface (in real WASM this would be host.write_temp_file)
        if let Some(ref host) = context.host_interface {
            host.write_temp_file(&file_id, &serialized).await?;
        } else {
            // Fallback for non-WASM testing - just simulate
            debug!("Simulating temp file write: {} bytes", serialized.len());
        }

        // Create temp file reference
        let temp_file_ref = TempFileRef {
            file_id: file_id.clone(),
            size_bytes: serialized.len(),
            chunk_count: 1,
            content_type: "channels".to_string(),
            metadata: {
                let mut meta = HashMap::new();
                meta.insert("source_chunk".to_string(), self.chunks_processed.to_string());
                meta.insert("channel_count".to_string(), self.accumulated_channels.len().to_string());
                meta
            },
        };

        // Add to context using standard key for next stages
        context.add_stage_output(
            crate::proxy::streaming_stages::stage_output_keys::SOURCE_CHANNELS, 
            temp_file_ref
        );
        
        // Track spilled file
        self.spilled_files.push(file_id);
        
        // Clear accumulated channels to free memory
        self.accumulated_channels.clear();
        
        Ok(())
    }

    /// Load channels from a spilled temp file
    async fn load_from_temp_file(&self, file_id: &str, context: &StreamingStageContext) -> Result<Vec<Channel>> {
        debug!("Loading channels from temp file: {}", file_id);
        
        if let Some(ref host) = context.host_interface {
            let data = host.read_temp_file(file_id).await?;
            let channels: Vec<Channel> = serde_json::from_slice(&data)?;
            Ok(channels)
        } else {
            // Fallback for non-WASM testing
            warn!("No host interface available, returning empty channels");
            Ok(Vec::new())
        }
    }

    /// Clean up temp files
    async fn cleanup_temp_files(&self, context: &StreamingStageContext) -> Result<()> {
        if let Some(ref host) = context.host_interface {
            for file_id in &self.spilled_files {
                host.delete_temp_file(file_id).await?;
            }
        }
        Ok(())
    }
}

#[async_trait]
impl StreamingSourceLoadingStage for ChunkedSourceLoader {
    async fn process_source_chunk(
        &mut self,
        chunk: StageChunk<Uuid>,
        context: &mut StreamingStageContext,
    ) -> Result<Vec<Channel>> {
        info!("Processing source chunk {}/{:?} with {} source IDs", 
              chunk.chunk_id, chunk.total_chunks, chunk.data.len());

        self.chunks_processed += 1;
        context.update_progress("chunked_source_loading".to_string(), self.chunks_processed, 0);

        // Load channels for this chunk of source IDs
        let mut chunk_channels = Vec::new();
        for source_id in &chunk.data {
            let channels = self.database.get_source_channels(*source_id).await?;
            debug!("Loaded {} channels from source {}", channels.len(), source_id);
            chunk_channels.extend(channels);
        }

        // Add to accumulated channels
        self.accumulated_channels.extend(chunk_channels);

        // Check if we should spill to temp file
        if self.should_spill(context).await {
            self.spill_to_temp_file(context).await?;
        }

        // For chunked strategy, we don't return data until finalize
        // This demonstrates a strategy that needs completion signal
        Ok(Vec::new())
    }

    async fn finalize_source_loading(
        &mut self,
        context: &mut StreamingStageContext,
    ) -> Result<Vec<Channel>> {
        info!("Finalizing chunked source loading - processed {} chunks, {} spilled files", 
              self.chunks_processed, self.spilled_files.len());

        // Collect all channels from memory and temp files
        let mut all_channels = self.accumulated_channels.clone();

        // Load channels from all spilled temp files
        for file_id in &self.spilled_files {
            let spilled_channels = self.load_from_temp_file(file_id, context).await?;
            info!("Loaded {} channels from spilled file: {}", spilled_channels.len(), file_id);
            all_channels.extend(spilled_channels);
        }

        // Clean up temp files
        self.cleanup_temp_files(context).await?;

        info!("Chunked source loading complete: {} total channels", all_channels.len());
        Ok(all_channels)
    }

    fn source_capabilities(&self) -> StageCapabilities {
        StageCapabilities::chunked(self.chunk_size)
    }

    fn strategy_name(&self) -> &str {
        "chunked_source_loader"
    }
}

/// Streaming data mapper that can process channels as they come
pub struct StreamingDataMapper {
    data_mapping_service: crate::data_mapping::service::DataMappingService,
    logo_service: crate::logo_assets::service::LogoAssetService,
    chunk_size: usize,
}

impl StreamingDataMapper {
    pub fn new(
        data_mapping_service: crate::data_mapping::service::DataMappingService,
        logo_service: crate::logo_assets::service::LogoAssetService,
        chunk_size: usize,
    ) -> Self {
        Self {
            data_mapping_service,
            logo_service,
            chunk_size,
        }
    }
}

#[async_trait]
impl StreamingDataMappingStage for StreamingDataMapper {
    async fn process_mapping_chunk(
        &mut self,
        chunk: StageChunk<Channel>,
        context: &mut StreamingStageContext,
    ) -> Result<Vec<Channel>> {
        info!("Processing data mapping chunk {}/{:?} with {} channels", 
              chunk.chunk_id, chunk.total_chunks, chunk.data.len());

        // Check if previous stage spilled data to temp files
        if let Some(source_temp_file) = context.get_stage_output(
            crate::proxy::streaming_stages::stage_output_keys::SOURCE_CHANNELS
        ) {
            info!("Found spilled source data: {} ({} bytes)", 
                  source_temp_file.file_id, source_temp_file.size_bytes);
            // Could load additional data if needed
        }

        // Group channels by source for efficient processing
        let mut channels_by_source = HashMap::new();
        for channel in chunk.data {
            channels_by_source
                .entry(channel.source_id)
                .or_insert_with(Vec::new)
                .push(channel);
        }

        let mut mapped_channels = Vec::new();

        // Process each source's channels in this chunk
        for (source_id, source_channels) in channels_by_source {
            // Find the source config for this source
            let source_config = context.proxy_config.sources
                .iter()
                .find(|s| s.source.id == source_id);

            if let Some(_config) = source_config {
                let mapped = self.data_mapping_service
                    .apply_mapping_for_proxy(
                        source_channels,
                        source_id,
                        &self.logo_service,
                        "http://localhost:8080", // TODO: Get from context
                        None, // TODO: Get engine config from context
                    )
                    .await?;
                
                mapped_channels.extend(mapped);
            } else {
                warn!("No source config found for source {}, skipping", source_id);
            }
        }

        // Streaming mapper can return results immediately
        info!("Mapped {} channels in chunk {}", mapped_channels.len(), chunk.chunk_id);
        Ok(mapped_channels)
    }

    async fn finalize_data_mapping(
        &mut self,
        _context: &mut StreamingStageContext,
    ) -> Result<Vec<Channel>> {
        // Streaming mapper doesn't accumulate data, so nothing to finalize
        Ok(Vec::new())
    }

    fn mapping_capabilities(&self) -> StageCapabilities {
        StageCapabilities::streaming()
    }

    fn strategy_name(&self) -> &str {
        "streaming_data_mapper"
    }
}

/// WASM-compatible chunked strategy (demonstrates how a real WASM plugin would work)
pub struct WasmChunkedSourceLoader {
    chunk_size: usize,
    memory_threshold_mb: usize,
    chunks_processed: usize,
    spilled_files: Vec<String>,
}

impl WasmChunkedSourceLoader {
    pub fn new(chunk_size: usize, memory_threshold_mb: usize) -> Self {
        Self {
            chunk_size,
            memory_threshold_mb,
            chunks_processed: 0,
            spilled_files: Vec::new(),
        }
    }
}

#[async_trait]
impl WasmStreamingStage for WasmChunkedSourceLoader {
    async fn process_chunk(
        &mut self,
        chunk_data: &[u8],
        chunk_metadata: &StageChunk<()>,
        context: &mut StreamingStageContext,
    ) -> Result<Vec<u8>> {
        // Deserialize source IDs from chunk data
        let source_ids: Vec<Uuid> = serde_json::from_slice(chunk_data)?;
        
        info!("WASM: Processing chunk {}/{:?} with {} source IDs", 
              chunk_metadata.chunk_id, chunk_metadata.total_chunks, source_ids.len());

        self.chunks_processed += 1;

        // In a real WASM plugin, we would:
        // 1. Use host.get_memory_usage() to check memory pressure
        // 2. Load channels via host database calls
        // 3. Use host.write_temp_file() to spill if needed
        // 4. Return serialized empty result (accumulating for finalize)

        if let Some(ref host) = context.host_interface {
            let memory_usage = host.get_memory_usage().await;
            let memory_pressure = host.get_memory_pressure().await;
            
            debug!("WASM: Memory usage: {} bytes, pressure: {:?}", memory_usage, memory_pressure);

            // Simulate processing and potential spilling
            if memory_usage > (self.memory_threshold_mb * 1024 * 1024) as u64 {
                let spill_file = format!("wasm_spill_{}", self.chunks_processed);
                host.write_temp_file(&spill_file, b"simulated_channel_data").await?;
                self.spilled_files.push(spill_file);
                host.log(
                    crate::proxy::wasm_host_interface::PluginLogLevel::Info,
                    &format!("Spilled chunk {} to temp file", chunk_metadata.chunk_id)
                );
            }
        }

        // Return empty for now (accumulating)
        Ok(serde_json::to_vec(&Vec::<Channel>::new())?)
    }

    async fn finalize(
        &mut self,
        context: &mut StreamingStageContext,
    ) -> Result<Vec<u8>> {
        info!("WASM: Finalizing chunked processing - {} chunks, {} spilled files", 
              self.chunks_processed, self.spilled_files.len());

        // In real WASM plugin:
        // 1. Read all spilled temp files via host.read_temp_file()
        // 2. Combine all data
        // 3. Clean up temp files via host.delete_temp_file()
        // 4. Return final serialized result

        let all_channels = Vec::<Channel>::new(); // Placeholder

        if let Some(ref host) = context.host_interface {
            for file_id in &self.spilled_files {
                let _data = host.read_temp_file(file_id).await?;
                // In real implementation: deserialize and add to all_channels
                host.delete_temp_file(file_id).await?;
            }
            
            host.log(
                crate::proxy::wasm_host_interface::PluginLogLevel::Info,
                &format!("WASM finalized with {} total channels", all_channels.len())
            );
        }

        Ok(serde_json::to_vec(&all_channels)?)
    }

    fn capabilities(&self) -> StageCapabilities {
        StageCapabilities::memory_efficient_spill()
    }

    fn strategy_name(&self) -> &str {
        "wasm_chunked_source_loader"
    }
}