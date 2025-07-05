//! Channel numbering strategies

use anyhow::Result;
use async_trait::async_trait;
use uuid::Uuid;
use tracing::info;

use crate::models::{Channel, NumberedChannel, ChannelNumberAssignmentType};
use crate::proxy::stage_strategy::{MemoryPressureLevel, StageContext, StageStrategy};

/// In-memory channel numbering strategy
pub struct InMemoryNumberingStrategy;

#[async_trait]
impl StageStrategy for InMemoryNumberingStrategy {
    async fn execute_source_loading(&self, _context: &StageContext, _source_ids: Vec<Uuid>) -> Result<Vec<Channel>> {
        unimplemented!("InMemoryNumberingStrategy only handles channel numbering")
    }

    async fn execute_data_mapping(&self, _context: &StageContext, _channels: Vec<Channel>) -> Result<Vec<Channel>> {
        unimplemented!("InMemoryNumberingStrategy only handles channel numbering")
    }

    async fn execute_filtering(&self, _context: &StageContext, _channels: Vec<Channel>) -> Result<Vec<Channel>> {
        unimplemented!("InMemoryNumberingStrategy only handles channel numbering")
    }

    async fn execute_channel_numbering(&self, context: &StageContext, channels: Vec<Channel>) -> Result<Vec<NumberedChannel>> {
        info!("Applying in-memory channel numbering to {} channels", channels.len());
        
        let starting_number = context.proxy_config.proxy.starting_channel_number;
        
        let numbered_channels = channels
            .into_iter()
            .enumerate()
            .map(|(i, channel)| NumberedChannel {
                channel,
                assigned_number: starting_number + i as i32,
                assignment_type: ChannelNumberAssignmentType::Sequential,
            })
            .collect();
        
        Ok(numbered_channels)
    }

    async fn execute_m3u_generation(&self, _context: &StageContext, _numbered_channels: Vec<NumberedChannel>) -> Result<String> {
        unimplemented!("InMemoryNumberingStrategy only handles channel numbering")
    }

    fn can_handle_memory_pressure(&self, level: MemoryPressureLevel) -> bool {
        matches!(level, MemoryPressureLevel::Optimal | MemoryPressureLevel::Moderate | MemoryPressureLevel::High)
    }

    fn supports_mid_stage_switching(&self) -> bool {
        false
    }

    fn strategy_name(&self) -> &str {
        "inmemory_numbering"
    }

    fn estimated_memory_usage(&self, input_size: usize) -> Option<usize> {
        Some(input_size * 1024)
    }
}