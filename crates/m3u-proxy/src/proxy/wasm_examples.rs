//! WASM Plugin Examples and Strategies
//!
//! This module contains example WASM plugin implementations that demonstrate
//! advanced processing strategies like chunking, memory-efficient spilling,
//! and streaming data processing.
//!
//! These examples show how to:
//! - Create WASM-compatible strategies
//! - Handle cross-stage communication via temp files  
//! - Implement memory pressure monitoring
//! - Build and deploy WASM plugins
//!
//! For development guide, see: `docs/WASM_PLUGIN_DEVELOPMENT.md`

use anyhow::Result;
use async_trait::async_trait;
use std::collections::HashMap;
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::database::Database;
use crate::models::*;
use crate::proxy::streaming_stages::*;

/// Example: Chunked source loading strategy that demonstrates real WASM-style processing
/// 
/// This strategy shows how to:
/// - Process data in configurable chunks
/// - Spill to temporary files when memory pressure is high
/// - Signal completion properly to downstream stages
/// - Pass context between stages via StreamingStageContext
pub struct WasmChunkedSourceLoader {
    database: Database,
    chunk_size: usize,
    accumulated_channels: Vec<Channel>,
    spilled_files: Vec<String>,
    chunks_processed: usize,
    memory_threshold_mb: usize,
}

impl WasmChunkedSourceLoader {
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
impl StreamingSourceLoadingStage for WasmChunkedSourceLoader {
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
        "wasm_chunked_source_loader"
    }
}

/// Example: Pure WASM-compatible chunked strategy with serialized boundaries
/// 
/// This demonstrates how a real WASM plugin would work with serialized input/output
/// and host interface calls for system services.
pub struct PureWasmChunkedLoader {
    memory_threshold_mb: usize,
    chunks_processed: usize,
    spilled_files: Vec<String>,
}

impl PureWasmChunkedLoader {
    pub fn new(memory_threshold_mb: usize) -> Self {
        Self {
            memory_threshold_mb,
            chunks_processed: 0,
            spilled_files: Vec::new(),
        }
    }
}

#[async_trait]
impl WasmStreamingStage for PureWasmChunkedLoader {
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
        "pure_wasm_chunked_loader"
    }
}

/// Streaming data mapper for demonstration purposes
pub struct WasmStreamingDataMapper {
    data_mapping_service: crate::data_mapping::service::DataMappingService,
    logo_service: crate::logo_assets::service::LogoAssetService,
}

impl WasmStreamingDataMapper {
    pub fn new(
        data_mapping_service: crate::data_mapping::service::DataMappingService,
        logo_service: crate::logo_assets::service::LogoAssetService,
    ) -> Self {
        Self {
            data_mapping_service,
            logo_service,
        }
    }
}

#[async_trait]
impl StreamingDataMappingStage for WasmStreamingDataMapper {
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
        "wasm_streaming_data_mapper"
    }
}

/// WASM File Spill Strategy for Large Datasets
/// 
/// This strategy demonstrates how to handle datasets that exceed memory limits
/// by intelligently spilling intermediate results to temporary files.
/// 
/// Key features:
/// - Automatic memory pressure detection
/// - Configurable spill thresholds
/// - Cross-stage temp file coordination
/// - Efficient file I/O via host interface
/// 
/// This strategy would be deployed as a WASM plugin in production.
pub struct WasmFileSpillSourceLoader {
    memory_threshold_mb: usize,
    accumulated_channels: Vec<Channel>,
    spilled_file_count: usize,
    temp_files: Vec<String>,
}

impl WasmFileSpillSourceLoader {
    pub fn new(memory_threshold_mb: usize) -> Self {
        Self {
            memory_threshold_mb,
            accumulated_channels: Vec::new(),
            spilled_file_count: 0,
            temp_files: Vec::new(),
        }
    }

    /// Estimate current memory usage
    fn estimate_memory_usage(&self) -> usize {
        // Rough estimate: each channel ~1KB
        self.accumulated_channels.len() * 1024
    }

    /// Check if we should spill to disk
    async fn should_spill(&self, context: &StreamingStageContext) -> bool {
        let memory_mb = self.estimate_memory_usage() / (1024 * 1024);
        
        // Check context memory pressure or threshold
        match context.memory_pressure {
            crate::proxy::stage_strategy::MemoryPressureLevel::Critical | 
            crate::proxy::stage_strategy::MemoryPressureLevel::Emergency => true,
            _ => memory_mb >= self.memory_threshold_mb,
        }
    }

    /// Spill accumulated data to temp file
    async fn spill_to_file(&mut self, context: &mut StreamingStageContext) -> Result<()> {
        if self.accumulated_channels.is_empty() {
            return Ok(());
        }

        let file_id = format!("filespill_source_{}", self.spilled_file_count);
        let data = serde_json::to_vec(&self.accumulated_channels)?;
        
        info!("FileSpill: Spilling {} channels ({} MB) to {}", 
              self.accumulated_channels.len(), 
              data.len() / (1024 * 1024),
              file_id);

        // Write via WASM host interface
        if let Some(ref host) = context.host_interface {
            host.write_temp_file(&file_id, &data).await?;
            host.log(
                crate::proxy::wasm_host_interface::PluginLogLevel::Info,
                &format!("Spilled {} channels to temp file", self.accumulated_channels.len())
            );
        }

        // Register temp file for cross-stage access
        let temp_file_ref = TempFileRef {
            file_id: file_id.clone(),
            size_bytes: data.len(),
            chunk_count: 1,
            content_type: "channels".to_string(),
            metadata: {
                let mut meta = HashMap::new();
                meta.insert("spill_reason".to_string(), "memory_pressure".to_string());
                meta.insert("channel_count".to_string(), self.accumulated_channels.len().to_string());
                meta
            },
        };

        context.add_stage_output(
            crate::proxy::streaming_stages::stage_output_keys::SOURCE_CHANNELS,
            temp_file_ref
        );

        self.temp_files.push(file_id);
        self.spilled_file_count += 1;
        self.accumulated_channels.clear();

        Ok(())
    }

    /// Load data from all spilled files
    async fn load_spilled_data(&self, context: &StreamingStageContext) -> Result<Vec<Channel>> {
        let mut all_channels = Vec::new();

        for file_id in &self.temp_files {
            if let Some(ref host) = context.host_interface {
                let data = host.read_temp_file(file_id).await?;
                let channels: Vec<Channel> = serde_json::from_slice(&data)?;
                let channel_count = channels.len();
                all_channels.extend(channels);
                
                host.log(
                    crate::proxy::wasm_host_interface::PluginLogLevel::Debug,
                    &format!("Loaded {} channels from spill file {}", channel_count, file_id)
                );
            }
        }

        Ok(all_channels)
    }

    /// Cleanup all temp files
    async fn cleanup(&self, context: &StreamingStageContext) -> Result<()> {
        if let Some(ref host) = context.host_interface {
            for file_id in &self.temp_files {
                host.delete_temp_file(file_id).await?;
            }
        }
        Ok(())
    }
}

#[async_trait]
impl WasmStreamingStage for WasmFileSpillSourceLoader {
    async fn process_chunk(
        &mut self,
        chunk_data: &[u8],
        chunk_metadata: &StageChunk<()>,
        context: &mut StreamingStageContext,
    ) -> Result<Vec<u8>> {
        // Deserialize source IDs
        let source_ids: Vec<Uuid> = serde_json::from_slice(chunk_data)?;
        
        info!("FileSpill: Processing chunk {} with {} sources", 
              chunk_metadata.chunk_id, source_ids.len());

        // In a real WASM plugin, we would load channels from database via host calls
        // For this example, we simulate the process
        
        // Simulate loading channels (in real plugin: host.database_query())
        let simulated_channels: Vec<Channel> = Vec::new(); // Placeholder
        self.accumulated_channels.extend(simulated_channels);

        // Check if we need to spill
        if self.should_spill(context).await {
            self.spill_to_file(context).await?;
        }

        // Return empty (accumulating for finalize)
        Ok(serde_json::to_vec(&Vec::<Channel>::new())?)
    }

    async fn finalize(
        &mut self,
        context: &mut StreamingStageContext,
    ) -> Result<Vec<u8>> {
        info!("FileSpill: Finalizing - {} spilled files, {} in memory", 
              self.temp_files.len(), self.accumulated_channels.len());

        // Combine memory + spilled data
        let mut all_channels = self.accumulated_channels.clone();
        let spilled_channels = self.load_spilled_data(context).await?;
        all_channels.extend(spilled_channels);

        // Cleanup
        self.cleanup(context).await?;

        info!("FileSpill: Completed with {} total channels", all_channels.len());
        Ok(serde_json::to_vec(&all_channels)?)
    }

    fn capabilities(&self) -> StageCapabilities {
        StageCapabilities::memory_efficient_spill()
    }

    fn strategy_name(&self) -> &str {
        "wasm_filespill_source_loader"
    }
}

/// WASM File Spill Data Mapping Strategy
/// 
/// Demonstrates memory-efficient data mapping with intelligent spilling
/// for large channel datasets that don't fit in memory.
pub struct WasmFileSpillDataMapper {
    memory_threshold_mb: usize,
    mapped_channels: Vec<Channel>,
    spill_count: usize,
    temp_files: Vec<String>,
}

impl WasmFileSpillDataMapper {
    pub fn new(memory_threshold_mb: usize) -> Self {
        Self {
            memory_threshold_mb,
            mapped_channels: Vec::new(),
            spill_count: 0,
            temp_files: Vec::new(),
        }
    }

    /// Check if we should spill mapped results
    async fn should_spill(&self) -> bool {
        let memory_mb = (self.mapped_channels.len() * 1024) / (1024 * 1024);
        memory_mb >= self.memory_threshold_mb
    }

    /// Spill mapped channels to temp file
    async fn spill_mapped_data(&mut self, context: &mut StreamingStageContext) -> Result<()> {
        if self.mapped_channels.is_empty() {
            return Ok(());
        }

        let file_id = format!("filespill_mapped_{}", self.spill_count);
        let data = serde_json::to_vec(&self.mapped_channels)?;

        if let Some(ref host) = context.host_interface {
            host.write_temp_file(&file_id, &data).await?;
            host.log(
                crate::proxy::wasm_host_interface::PluginLogLevel::Info,
                &format!("Spilled {} mapped channels to {}", self.mapped_channels.len(), file_id)
            );
        }

        let temp_file_ref = TempFileRef {
            file_id: file_id.clone(),
            size_bytes: data.len(),
            chunk_count: 1,
            content_type: "mapped_channels".to_string(),
            metadata: HashMap::new(),
        };

        context.add_stage_output(
            crate::proxy::streaming_stages::stage_output_keys::MAPPED_CHANNELS,
            temp_file_ref
        );

        self.temp_files.push(file_id);
        self.spill_count += 1;
        self.mapped_channels.clear();

        Ok(())
    }
}

#[async_trait]
impl WasmStreamingStage for WasmFileSpillDataMapper {
    async fn process_chunk(
        &mut self,
        chunk_data: &[u8],
        chunk_metadata: &StageChunk<()>,
        context: &mut StreamingStageContext,
    ) -> Result<Vec<u8>> {
        // Deserialize channels for mapping
        let channels: Vec<Channel> = serde_json::from_slice(chunk_data)?;
        
        info!("FileSpill DataMapper: Processing chunk {} with {} channels", 
              chunk_metadata.chunk_id, channels.len());

        // In real WASM: apply data mapping via host interface
        // For now, simulate mapping
        let mapped = channels; // Placeholder - no actual mapping

        self.mapped_channels.extend(mapped);

        // Check if we should spill
        if self.should_spill().await {
            self.spill_mapped_data(context).await?;
        }

        // Return empty (accumulating)
        Ok(serde_json::to_vec(&Vec::<Channel>::new())?)
    }

    async fn finalize(
        &mut self,
        context: &mut StreamingStageContext,
    ) -> Result<Vec<u8>> {
        info!("FileSpill DataMapper: Finalizing with {} files, {} in memory", 
              self.temp_files.len(), self.mapped_channels.len());

        // Combine all mapped data
        let mut all_mapped = self.mapped_channels.clone();
        
        // Load from spilled files
        for file_id in &self.temp_files {
            if let Some(ref host) = context.host_interface {
                let data = host.read_temp_file(file_id).await?;
                let mapped: Vec<Channel> = serde_json::from_slice(&data)?;
                all_mapped.extend(mapped);
                host.delete_temp_file(file_id).await?;
            }
        }

        info!("FileSpill DataMapper: Completed with {} mapped channels", all_mapped.len());
        Ok(serde_json::to_vec(&all_mapped)?)
    }

    fn capabilities(&self) -> StageCapabilities {
        StageCapabilities::memory_efficient_spill()
    }

    fn strategy_name(&self) -> &str {
        "wasm_filespill_data_mapper"
    }
}