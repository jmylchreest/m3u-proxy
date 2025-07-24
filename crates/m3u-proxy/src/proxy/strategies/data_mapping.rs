//! Data mapping strategies with different performance/memory trade-offs

use anyhow::Result;
use async_trait::async_trait;
use uuid::Uuid;
use tracing::{debug, info};

use crate::data_mapping::DataMappingService;
use crate::logo_assets::service::LogoAssetService;
use crate::models::Channel;
use crate::proxy::stage_strategy::{MemoryPressureLevel, StageContext, StageStrategy};

/// High-performance parallel data mapping
pub struct ParallelDataMapper {
    data_mapping_service: DataMappingService,
    logo_service: LogoAssetService,
}

impl ParallelDataMapper {
    pub fn new(data_mapping_service: DataMappingService, logo_service: LogoAssetService) -> Self {
        Self {
            data_mapping_service,
            logo_service,
        }
    }
}

#[async_trait]
impl StageStrategy for ParallelDataMapper {
    async fn execute_source_loading(&self, _context: &StageContext, _source_ids: Vec<Uuid>) -> Result<Vec<Channel>> {
        unimplemented!("ParallelDataMapper only handles data mapping")
    }

    async fn execute_data_mapping(
        &self,
        context: &StageContext,
        channels: Vec<Channel>,
    ) -> Result<Vec<Channel>> {
        info!("Applying parallel data mapping to {} channels", channels.len());
        
        // Group channels by source for efficient processing
        let mut channels_by_source = std::collections::HashMap::new();
        for channel in channels {
            channels_by_source
                .entry(channel.source_id)
                .or_insert_with(Vec::new)
                .push(channel);
        }
        
        let mut all_mapped_channels = Vec::new();
        
        // Process each source's channels
        for (source_id, source_channels) in channels_by_source {
            debug!("Mapping {} channels from source {}", source_channels.len(), source_id);
            
            let mapped_channels = self
                .data_mapping_service
                .apply_mapping_for_proxy(
                    source_channels,
                    source_id,
                    &self.logo_service,
                    &context.base_url,
                    context.engine_config.clone(),
                )
                .await?;
            
            all_mapped_channels.extend(mapped_channels);
        }
        
        info!("Parallel data mapping completed: {} channels processed", all_mapped_channels.len());
        Ok(all_mapped_channels)
    }

    async fn execute_filtering(&self, _context: &StageContext, _channels: Vec<Channel>) -> Result<Vec<Channel>> {
        unimplemented!("ParallelDataMapper only handles data mapping")
    }

    async fn execute_channel_numbering(&self, _context: &StageContext, _channels: Vec<Channel>) -> Result<Vec<crate::models::NumberedChannel>> {
        unimplemented!("ParallelDataMapper only handles data mapping")
    }

    async fn execute_m3u_generation(&self, _context: &StageContext, _numbered_channels: Vec<crate::models::NumberedChannel>) -> Result<String> {
        unimplemented!("ParallelDataMapper only handles data mapping")
    }

    fn can_handle_memory_pressure(&self, level: MemoryPressureLevel) -> bool {
        matches!(level, MemoryPressureLevel::Optimal | MemoryPressureLevel::Moderate)
    }

    fn supports_mid_stage_switching(&self) -> bool {
        false // Processes all channels at once for optimal performance
    }

    fn strategy_name(&self) -> &str {
        "parallel_mapping"
    }

    fn estimated_memory_usage(&self, input_size: usize) -> Option<usize> {
        // Parallel processing requires holding all channels in memory
        Some(input_size * 1024)
    }
}

/// Memory-efficient batched data mapping
pub struct BatchedDataMapper {
    data_mapping_service: DataMappingService,
    logo_service: LogoAssetService,
    batch_size: usize,
}

impl BatchedDataMapper {
    pub fn new(
        data_mapping_service: DataMappingService,
        logo_service: LogoAssetService,
        batch_size: usize,
    ) -> Self {
        Self {
            data_mapping_service,
            logo_service,
            batch_size,
        }
    }
}

#[async_trait]
impl StageStrategy for BatchedDataMapper {
    async fn execute_source_loading(&self, _context: &StageContext, _source_ids: Vec<Uuid>) -> Result<Vec<Channel>> {
        unimplemented!("BatchedDataMapper only handles data mapping")
    }

    async fn execute_data_mapping(
        &self,
        context: &StageContext,
        channels: Vec<Channel>,
    ) -> Result<Vec<Channel>> {
        info!("Applying batched data mapping to {} channels (batch_size: {})", channels.len(), self.batch_size);
        
        let mut all_mapped_channels = Vec::new();
        let mut processed = 0;
        
        // Group by source first for efficiency
        let mut channels_by_source = std::collections::HashMap::new();
        for channel in channels {
            channels_by_source
                .entry(channel.source_id)
                .or_insert_with(Vec::new)
                .push(channel);
        }
        
        // Process each source in batches
        for (source_id, source_channels) in channels_by_source {
            for batch in source_channels.chunks(self.batch_size) {
                debug!("Processing batch of {} channels from source {}", batch.len(), source_id);
                
                let mapped_channels = self
                    .data_mapping_service
                    .apply_mapping_for_proxy(
                        batch.to_vec(),
                        source_id,
                        &self.logo_service,
                        &context.base_url,
                        context.engine_config.clone(),
                    )
                    .await?;
                
                all_mapped_channels.extend(mapped_channels);
                processed += batch.len();
                
                // Yield control point for memory monitoring
                if context.memory_pressure >= MemoryPressureLevel::High {
                    tokio::task::yield_now().await;
                }
            }
        }
        
        info!("Batched data mapping completed: {} channels processed", processed);
        Ok(all_mapped_channels)
    }

    async fn execute_filtering(&self, _context: &StageContext, _channels: Vec<Channel>) -> Result<Vec<Channel>> {
        unimplemented!("BatchedDataMapper only handles data mapping")
    }

    async fn execute_channel_numbering(&self, _context: &StageContext, _channels: Vec<Channel>) -> Result<Vec<crate::models::NumberedChannel>> {
        unimplemented!("BatchedDataMapper only handles data mapping")
    }

    async fn execute_m3u_generation(&self, _context: &StageContext, _numbered_channels: Vec<crate::models::NumberedChannel>) -> Result<String> {
        unimplemented!("BatchedDataMapper only handles data mapping")
    }

    fn can_handle_memory_pressure(&self, level: MemoryPressureLevel) -> bool {
        matches!(
            level,
            MemoryPressureLevel::Moderate | MemoryPressureLevel::High | MemoryPressureLevel::Critical
        )
    }

    fn supports_mid_stage_switching(&self) -> bool {
        true // Can switch strategy between batches
    }

    fn strategy_name(&self) -> &str {
        "batched_mapping"
    }

    fn estimated_memory_usage(&self, input_size: usize) -> Option<usize> {
        // Only holds one batch worth of channels at a time
        Some((input_size / self.batch_size.max(1)) * 1024)
    }
}

/// Ultra-low memory streaming data mapper
pub struct StreamingDataMapper {
    data_mapping_service: DataMappingService,
    logo_service: LogoAssetService,
}

impl StreamingDataMapper {
    pub fn new(data_mapping_service: DataMappingService, logo_service: LogoAssetService) -> Self {
        Self {
            data_mapping_service,
            logo_service,
        }
    }
}

#[async_trait]
impl StageStrategy for StreamingDataMapper {
    async fn execute_source_loading(&self, _context: &StageContext, _source_ids: Vec<Uuid>) -> Result<Vec<Channel>> {
        unimplemented!("StreamingDataMapper only handles data mapping")
    }

    async fn execute_data_mapping(
        &self,
        context: &StageContext,
        channels: Vec<Channel>,
    ) -> Result<Vec<Channel>> {
        info!("Applying streaming data mapping to {} channels", channels.len());
        
        let mut all_mapped_channels = Vec::new();
        let mut current_source_id = None;
        let mut source_batch = Vec::new();
        
        // Process channels one by one, batching by source for efficiency
        for (i, channel) in channels.into_iter().enumerate() {
            if current_source_id.is_none() || current_source_id != Some(channel.source_id) {
                // Process previous source batch if it exists
                if !source_batch.is_empty() {
                    let mapped = self
                        .data_mapping_service
                        .apply_mapping_for_proxy(
                            source_batch,
                            current_source_id.unwrap(),
                            &self.logo_service,
                            &context.base_url,
                            context.engine_config.clone(),
                        )
                        .await?;
                    all_mapped_channels.extend(mapped);
                }
                
                // Start new source batch
                current_source_id = Some(channel.source_id);
                source_batch = vec![channel];
            } else {
                source_batch.push(channel);
            }
            
            // Yield control frequently for memory monitoring
            if i % 10 == 0 {
                tokio::task::yield_now().await;
            }
        }
        
        // Process final batch
        if !source_batch.is_empty() {
            let mapped = self
                .data_mapping_service
                .apply_mapping_for_proxy(
                    source_batch,
                    current_source_id.unwrap(),
                    &self.logo_service,
                    &context.base_url,
                    context.engine_config.clone(),
                )
                .await?;
            all_mapped_channels.extend(mapped);
        }
        
        info!("Streaming data mapping completed: {} channels processed", all_mapped_channels.len());
        Ok(all_mapped_channels)
    }

    async fn execute_filtering(&self, _context: &StageContext, _channels: Vec<Channel>) -> Result<Vec<Channel>> {
        unimplemented!("StreamingDataMapper only handles data mapping")
    }

    async fn execute_channel_numbering(&self, _context: &StageContext, _channels: Vec<Channel>) -> Result<Vec<crate::models::NumberedChannel>> {
        unimplemented!("StreamingDataMapper only handles data mapping")
    }

    async fn execute_m3u_generation(&self, _context: &StageContext, _numbered_channels: Vec<crate::models::NumberedChannel>) -> Result<String> {
        unimplemented!("StreamingDataMapper only handles data mapping")
    }

    fn can_handle_memory_pressure(&self, _level: MemoryPressureLevel) -> bool {
        true // Can handle any memory pressure due to minimal footprint
    }

    fn supports_mid_stage_switching(&self) -> bool {
        true // Can switch after each channel
    }

    fn strategy_name(&self) -> &str {
        "streaming_mapping"
    }

    fn estimated_memory_usage(&self, _input_size: usize) -> Option<usize> {
        // Minimal memory - only holds small batches of channels per source
        Some(10 * 1024) // ~10KB for small batches
    }
}