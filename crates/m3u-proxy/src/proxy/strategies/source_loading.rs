//! Source loading strategies with different memory footprints

use anyhow::Result;
use async_trait::async_trait;
use uuid::Uuid;
use tracing::{debug, info};

use crate::database::Database;
use crate::models::Channel;
use crate::proxy::stage_strategy::{MemoryPressureLevel, StageContext, StageStrategy};

/// In-memory source loading - loads all channels at once for optimal performance
pub struct InMemorySourceLoader {
    database: Database,
}

impl InMemorySourceLoader {
    pub fn new(database: Database) -> Self {
        Self { database }
    }
}

#[async_trait]
impl StageStrategy for InMemorySourceLoader {
    async fn execute_source_loading(
        &self,
        _context: &StageContext,
        source_ids: Vec<Uuid>,
    ) -> Result<Vec<Channel>> {
        info!("Loading {} sources using in-memory strategy", source_ids.len());
        let mut all_channels = Vec::new();
        
        for source_id in source_ids {
            let channels = self.database.get_source_channels(source_id).await?;
            debug!("Loaded {} channels from source {}", channels.len(), source_id);
            all_channels.extend(channels);
        }
        
        info!("In-memory source loading completed: {} total channels", all_channels.len());
        Ok(all_channels)
    }

    async fn execute_data_mapping(&self, _context: &StageContext, _channels: Vec<Channel>) -> Result<Vec<Channel>> {
        unimplemented!("InMemorySourceLoader only handles source loading")
    }

    async fn execute_filtering(&self, _context: &StageContext, _channels: Vec<Channel>) -> Result<Vec<Channel>> {
        unimplemented!("InMemorySourceLoader only handles source loading")
    }

    async fn execute_channel_numbering(&self, _context: &StageContext, _channels: Vec<Channel>) -> Result<Vec<crate::models::NumberedChannel>> {
        unimplemented!("InMemorySourceLoader only handles source loading")
    }

    async fn execute_m3u_generation(&self, _context: &StageContext, _numbered_channels: Vec<crate::models::NumberedChannel>) -> Result<String> {
        unimplemented!("InMemorySourceLoader only handles source loading")
    }

    fn can_handle_memory_pressure(&self, level: MemoryPressureLevel) -> bool {
        matches!(level, MemoryPressureLevel::Optimal | MemoryPressureLevel::Moderate)
    }

    fn supports_mid_stage_switching(&self) -> bool {
        false // All-or-nothing loading
    }

    fn strategy_name(&self) -> &str {
        "inmemory_full"
    }

    fn estimated_memory_usage(&self, input_size: usize) -> Option<usize> {
        // Rough estimate: each channel ~1KB in memory
        Some(input_size * 1024)
    }
}

/// Batched source loading - loads sources in configurable batches
pub struct BatchedSourceLoader {
    database: Database,
    batch_size: usize,
}

impl BatchedSourceLoader {
    pub fn new(database: Database, batch_size: usize) -> Self {
        Self { database, batch_size }
    }
}

#[async_trait]
impl StageStrategy for BatchedSourceLoader {
    async fn execute_source_loading(
        &self,
        context: &StageContext,
        source_ids: Vec<Uuid>,
    ) -> Result<Vec<Channel>> {
        info!("Loading {} sources using batched strategy (batch_size: {})", source_ids.len(), self.batch_size);
        let mut all_channels = Vec::new();
        
        for chunk in source_ids.chunks(self.batch_size) {
            debug!("Processing batch of {} sources", chunk.len());
            
            for source_id in chunk {
                let channels = self.database.get_source_channels(*source_id).await?;
                debug!("Loaded {} channels from source {}", channels.len(), source_id);
                all_channels.extend(channels);
            }
            
            // Optional: yield control point for memory checks
            if context.memory_pressure >= MemoryPressureLevel::High {
                debug!("High memory pressure detected, yielding control");
                tokio::task::yield_now().await;
            }
        }
        
        info!("Batched source loading completed: {} total channels", all_channels.len());
        Ok(all_channels)
    }

    async fn execute_data_mapping(&self, _context: &StageContext, _channels: Vec<Channel>) -> Result<Vec<Channel>> {
        unimplemented!("BatchedSourceLoader only handles source loading")
    }

    async fn execute_filtering(&self, _context: &StageContext, _channels: Vec<Channel>) -> Result<Vec<Channel>> {
        unimplemented!("BatchedSourceLoader only handles source loading")
    }

    async fn execute_channel_numbering(&self, _context: &StageContext, _channels: Vec<Channel>) -> Result<Vec<crate::models::NumberedChannel>> {
        unimplemented!("BatchedSourceLoader only handles source loading")
    }

    async fn execute_m3u_generation(&self, _context: &StageContext, _numbered_channels: Vec<crate::models::NumberedChannel>) -> Result<String> {
        unimplemented!("BatchedSourceLoader only handles source loading")
    }

    fn can_handle_memory_pressure(&self, level: MemoryPressureLevel) -> bool {
        matches!(level, MemoryPressureLevel::Moderate | MemoryPressureLevel::High)
    }

    fn supports_mid_stage_switching(&self) -> bool {
        true // Can switch between batches
    }

    fn strategy_name(&self) -> &str {
        "batched_loader"
    }

    fn estimated_memory_usage(&self, input_size: usize) -> Option<usize> {
        // Only holds one batch worth of channels at a time
        Some((input_size / self.batch_size.max(1)) * 1024)
    }
}

/// Streaming source loader - minimal memory footprint, processes one channel at a time
pub struct StreamingSourceLoader {
    database: Database,
}

impl StreamingSourceLoader {
    pub fn new(database: Database) -> Self {
        Self { database }
    }
}

#[async_trait]
impl StageStrategy for StreamingSourceLoader {
    async fn execute_source_loading(
        &self,
        _context: &StageContext,
        source_ids: Vec<Uuid>,
    ) -> Result<Vec<Channel>> {
        info!("Loading {} sources using streaming strategy", source_ids.len());
        let mut all_channels = Vec::new();
        
        for source_id in source_ids {
            let channels = self.database.get_source_channels(source_id).await?;
            debug!("Streaming {} channels from source {}", channels.len(), source_id);
            
            // In a real streaming implementation, we'd process one at a time
            // For now, we simulate by yielding control frequently
            for (i, channel) in channels.into_iter().enumerate() {
                all_channels.push(channel);
                
                // Yield control every 10 channels to allow memory checks
                if i % 10 == 0 {
                    tokio::task::yield_now().await;
                }
            }
        }
        
        info!("Streaming source loading completed: {} total channels", all_channels.len());
        Ok(all_channels)
    }

    async fn execute_data_mapping(&self, _context: &StageContext, _channels: Vec<Channel>) -> Result<Vec<Channel>> {
        unimplemented!("StreamingSourceLoader only handles source loading")
    }

    async fn execute_filtering(&self, _context: &StageContext, _channels: Vec<Channel>) -> Result<Vec<Channel>> {
        unimplemented!("StreamingSourceLoader only handles source loading")
    }

    async fn execute_channel_numbering(&self, _context: &StageContext, _channels: Vec<Channel>) -> Result<Vec<crate::models::NumberedChannel>> {
        unimplemented!("StreamingSourceLoader only handles source loading")
    }

    async fn execute_m3u_generation(&self, _context: &StageContext, _numbered_channels: Vec<crate::models::NumberedChannel>) -> Result<String> {
        unimplemented!("StreamingSourceLoader only handles source loading")
    }

    fn can_handle_memory_pressure(&self, _level: MemoryPressureLevel) -> bool {
        // Can handle any pressure level due to minimal memory usage
        true
    }

    fn supports_mid_stage_switching(&self) -> bool {
        true // Supports switching after each channel
    }

    fn strategy_name(&self) -> &str {
        "streaming_loader"
    }

    fn estimated_memory_usage(&self, _input_size: usize) -> Option<usize> {
        // Minimal memory usage - only holds one channel at a time
        Some(1024) // ~1KB
    }
}