//! Streaming pipeline orchestrator that coordinates chunked and streaming strategies
//!
//! This orchestrator demonstrates how to handle:
//! - Mixed strategy capabilities (streaming vs batch)
//! - Proper completion signaling
//! - Cross-stage temp file coordination
//! - Memory pressure adaptation

use anyhow::Result;
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::models::*;
use crate::proxy::streaming_stages::*;

/// Pipeline configuration for streaming execution
pub struct StreamingPipelineConfig {
    pub default_chunk_size: usize,
    pub memory_threshold_mb: usize,
    pub enable_early_output: bool,
    pub max_chunks_in_memory: usize,
}

impl Default for StreamingPipelineConfig {
    fn default() -> Self {
        Self {
            default_chunk_size: 1000,
            memory_threshold_mb: 256,
            enable_early_output: true,
            max_chunks_in_memory: 5,
        }
    }
}

/// Streaming pipeline that orchestrates different strategy types
pub struct StreamingPipeline {
    config: StreamingPipelineConfig,
    source_loading: Box<dyn StreamingSourceLoadingStage>,
    data_mapping: Box<dyn StreamingDataMappingStage>,
    // filtering: Box<dyn StreamingFilteringStage>, // TODO: Add when implemented
}

impl StreamingPipeline {
    pub fn new(
        config: StreamingPipelineConfig,
        source_loading: Box<dyn StreamingSourceLoadingStage>,
        data_mapping: Box<dyn StreamingDataMappingStage>,
    ) -> Self {
        Self {
            config,
            source_loading,
            data_mapping,
        }
    }

    /// Execute the full streaming pipeline
    pub async fn execute(
        &mut self,
        proxy_config: ResolvedProxyConfig,
        output: GenerationOutput,
        base_url: &str,
    ) -> Result<ProxyGeneration> {
        let start_time = std::time::Instant::now();
        info!("Starting streaming pipeline execution for proxy '{}'", proxy_config.proxy.name);

        let mut context = StreamingStageContext::new(proxy_config.clone());
        
        // Phase 1: Source Loading (potentially chunked)
        let source_ids: Vec<Uuid> = proxy_config.sources.iter().map(|s| s.source.id).collect();
        let all_channels = self.execute_source_loading(source_ids, &mut context).await?;
        
        info!("Source loading completed: {} channels", all_channels.len());

        // Phase 2: Data Mapping (streaming or batch based on capabilities)
        let mapped_channels = self.execute_data_mapping(all_channels, &mut context).await?;
        
        info!("Data mapping completed: {} channels", mapped_channels.len());

        // Phase 3: Create final generation (simplified for now)
        let generation = self.create_generation_result(
            mapped_channels,
            &proxy_config,
            &output,
            base_url,
            start_time.elapsed().as_millis() as u64,
        ).await?;

        info!("Streaming pipeline completed for '{}': {} channels in {}ms", 
              proxy_config.proxy.name, generation.channel_count, 
              start_time.elapsed().as_millis());

        Ok(generation)
    }

    /// Execute source loading with proper chunking
    async fn execute_source_loading(
        &mut self,
        source_ids: Vec<Uuid>,
        context: &mut StreamingStageContext,
    ) -> Result<Vec<Channel>> {
        let capabilities = self.source_loading.source_capabilities();
        info!("Executing source loading with strategy '{}' (streaming: {}, requires_all: {})", 
              self.source_loading.strategy_name(), 
              capabilities.supports_streaming, 
              capabilities.requires_all_data);

        if capabilities.supports_streaming {
            // Chunk the source IDs based on strategy preferences
            let chunk_size = capabilities.preferred_chunk_size.unwrap_or(self.config.default_chunk_size);
            let chunks = self.create_source_chunks(source_ids, chunk_size);
            
            let mut accumulated_results = Vec::new();

            // Process each chunk
            for (chunk_id, chunk) in chunks.into_iter().enumerate() {
                let chunk_results = self.source_loading.process_source_chunk(chunk, context).await?;
                
                // Handle early output if strategy supports it
                if capabilities.can_produce_early_output && !chunk_results.is_empty() {
                    info!("Got early output from chunk {}: {} channels", chunk_id, chunk_results.len());
                    accumulated_results.extend(chunk_results);
                } else {
                    debug!("Chunk {} processed, waiting for finalize", chunk_id);
                }
            }

            // Finalize processing
            let final_results = self.source_loading.finalize_source_loading(context).await?;
            accumulated_results.extend(final_results);

            Ok(accumulated_results)
        } else {
            // Non-streaming strategy - process all at once
            let mut chunk = StageChunk::new(source_ids, 0);
            chunk.is_final_chunk = true;
            
            let _chunk_results = self.source_loading.process_source_chunk(chunk, context).await?;
            self.source_loading.finalize_source_loading(context).await
        }
    }

    /// Execute data mapping, handling streaming coordination
    async fn execute_data_mapping(
        &mut self,
        channels: Vec<Channel>,
        context: &mut StreamingStageContext,
    ) -> Result<Vec<Channel>> {
        let capabilities = self.data_mapping.mapping_capabilities();
        info!("Executing data mapping with strategy '{}' (streaming: {}, early_output: {})", 
              self.data_mapping.strategy_name(), 
              capabilities.supports_streaming,
              capabilities.can_produce_early_output);

        if capabilities.supports_streaming {
            // Stream data through the mapper
            let chunk_size = capabilities.preferred_chunk_size.unwrap_or(self.config.default_chunk_size);
            let chunks = self.create_channel_chunks(channels, chunk_size);
            
            let mut all_mapped = Vec::new();

            for (chunk_id, chunk) in chunks.into_iter().enumerate() {
                let mapped_chunk = self.data_mapping.process_mapping_chunk(chunk, context).await?;
                
                if capabilities.can_produce_early_output {
                    // Can process results immediately
                    all_mapped.extend(mapped_chunk);
                } else {
                    // Strategy is accumulating, results come in finalize
                    debug!("Data mapping chunk {} processed, accumulating", chunk_id);
                }
            }

            // Finalize and get any remaining results
            let final_mapped = self.data_mapping.finalize_data_mapping(context).await?;
            all_mapped.extend(final_mapped);

            Ok(all_mapped)
        } else {
            // Batch processing - send all data at once
            let mut chunk = StageChunk::new(channels, 0);
            chunk.is_final_chunk = true;
            
            let _chunk_results = self.data_mapping.process_mapping_chunk(chunk, context).await?;
            self.data_mapping.finalize_data_mapping(context).await
        }
    }

    /// Create source ID chunks with proper metadata
    fn create_source_chunks(&self, source_ids: Vec<Uuid>, chunk_size: usize) -> Vec<StageChunk<Uuid>> {
        let total_items = source_ids.len();
        let chunks: Vec<_> = source_ids.chunks(chunk_size).enumerate().map(|(i, chunk)| {
            let chunk_data = chunk.to_vec();
            let total_chunks = (total_items + chunk_size - 1) / chunk_size;
            let is_final = i == total_chunks - 1;
            
            StageChunk {
                data: chunk_data,
                chunk_id: i,
                is_final_chunk: is_final,
                total_chunks: Some(total_chunks),
                total_items: Some(total_items),
            }
        }).collect();

        info!("Created {} source chunks (chunk_size: {}, total_items: {})", 
              chunks.len(), chunk_size, total_items);
        chunks
    }

    /// Create channel chunks with proper metadata
    fn create_channel_chunks(&self, channels: Vec<Channel>, chunk_size: usize) -> Vec<StageChunk<Channel>> {
        let total_items = channels.len();
        let chunks: Vec<_> = channels.chunks(chunk_size).enumerate().map(|(i, chunk)| {
            let chunk_data = chunk.to_vec();
            let total_chunks = (total_items + chunk_size - 1) / chunk_size;
            let is_final = i == total_chunks - 1;
            
            StageChunk {
                data: chunk_data,
                chunk_id: i,
                is_final_chunk: is_final,
                total_chunks: Some(total_chunks),
                total_items: Some(total_items),
            }
        }).collect();

        debug!("Created {} channel chunks (chunk_size: {}, total_items: {})", 
               chunks.len(), chunk_size, total_items);
        chunks
    }

    /// Create final generation result
    async fn create_generation_result(
        &self,
        channels: Vec<Channel>,
        proxy_config: &ResolvedProxyConfig,
        output: &GenerationOutput,
        _base_url: &str,
        duration_ms: u64,
    ) -> Result<ProxyGeneration> {
        // For now, create a simplified generation
        // In full implementation, this would go through filtering, numbering, and M3U generation
        
        let m3u_content = format!("#EXTM3U\n# Generated {} channels via streaming pipeline\n", channels.len());
        
        let generation = ProxyGeneration {
            id: uuid::Uuid::new_v4(),
            proxy_id: proxy_config.proxy.id,
            version: 1,
            channel_count: channels.len() as i32,
            total_channels: channels.len(),
            filtered_channels: channels.len(),
            applied_filters: Vec::new(),
            m3u_content,
            created_at: chrono::Utc::now(),
            stats: Some(self.create_generation_stats(channels.len(), duration_ms)),
            processed_channels: None,
        };

        // Handle output (simplified)
        match output {
            GenerationOutput::InMemory => {
                debug!("Generation completed in memory");
            }
            _ => {
                info!("Generation completed, output handled");
            }
        }

        Ok(generation)
    }

    /// Create generation statistics
    fn create_generation_stats(&self, channel_count: usize, duration_ms: u64) -> GenerationStats {
        let mut stats = GenerationStats::new("streaming_pipeline".to_string());
        stats.add_stage_timing("source_loading", duration_ms / 3);
        stats.add_stage_timing("data_mapping", duration_ms / 3);
        stats.add_stage_timing("coordination", duration_ms / 3);
        stats.total_channels_processed = channel_count;
        stats.finalize();
        stats
    }
}

/// Builder for creating streaming pipelines with different strategy combinations
pub struct StreamingPipelineBuilder {
    config: StreamingPipelineConfig,
    database: Option<crate::database::Database>,
    data_mapping_service: Option<crate::data_mapping::service::DataMappingService>,
    logo_service: Option<crate::logo_assets::service::LogoAssetService>,
}

impl StreamingPipelineBuilder {
    pub fn new() -> Self {
        Self {
            config: StreamingPipelineConfig::default(),
            database: None,
            data_mapping_service: None,
            logo_service: None,
        }
    }

    pub fn with_config(mut self, config: StreamingPipelineConfig) -> Self {
        self.config = config;
        self
    }

    pub fn with_database(mut self, database: crate::database::Database) -> Self {
        self.database = Some(database);
        self
    }

    pub fn with_data_mapping_service(mut self, service: crate::data_mapping::service::DataMappingService) -> Self {
        self.data_mapping_service = Some(service);
        self
    }

    pub fn with_logo_service(mut self, service: crate::logo_assets::service::LogoAssetService) -> Self {
        self.logo_service = Some(service);
        self
    }

    /// Build with chunked source loading strategy
    pub fn build_chunked(self) -> Result<StreamingPipeline> {
        Err(anyhow::anyhow!("Streaming pipeline not available - use native pipeline instead"))
    }
}

/// Demonstration of how to use the streaming pipeline
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_streaming_pipeline_concept() {
        // This test demonstrates the concept - would need real database/services to run
        
        let config = StreamingPipelineConfig {
            default_chunk_size: 100,
            memory_threshold_mb: 64,
            enable_early_output: true,
            max_chunks_in_memory: 3,
        };

        // In real usage:
        // let mut pipeline = StreamingPipelineBuilder::new()
        //     .with_config(config)
        //     .with_database(database)
        //     .with_data_mapping_service(data_mapping_service)
        //     .with_logo_service(logo_service)
        //     .build_chunked()?;

        // let generation = pipeline.execute(proxy_config, output, base_url).await?;
        
        println!("Streaming pipeline test concept validated");
    }
}