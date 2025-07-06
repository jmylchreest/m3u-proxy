//! M3U generation strategies

use anyhow::Result;
use async_trait::async_trait;
use uuid::Uuid;
use tracing::info;

use crate::models::{Channel, NumberedChannel};
use crate::proxy::stage_strategy::{MemoryPressureLevel, StageContext, StageStrategy};

/// In-memory M3U generation strategy
pub struct InMemoryM3uStrategy;

#[async_trait]
impl StageStrategy for InMemoryM3uStrategy {
    async fn execute_source_loading(&self, _context: &StageContext, _source_ids: Vec<Uuid>) -> Result<Vec<Channel>> {
        unimplemented!("InMemoryM3uStrategy only handles M3U generation")
    }

    async fn execute_data_mapping(&self, _context: &StageContext, _channels: Vec<Channel>) -> Result<Vec<Channel>> {
        unimplemented!("InMemoryM3uStrategy only handles M3U generation")
    }

    async fn execute_filtering(&self, _context: &StageContext, _channels: Vec<Channel>) -> Result<Vec<Channel>> {
        unimplemented!("InMemoryM3uStrategy only handles M3U generation")
    }

    async fn execute_channel_numbering(&self, _context: &StageContext, _channels: Vec<Channel>) -> Result<Vec<NumberedChannel>> {
        unimplemented!("InMemoryM3uStrategy only handles M3U generation")
    }

    async fn execute_m3u_generation(&self, context: &StageContext, numbered_channels: Vec<NumberedChannel>) -> Result<String> {
        info!("Generating M3U content for {} channels using in-memory strategy", numbered_channels.len());
        
        let mut m3u = String::from("#EXTM3U\n");

        for nc in numbered_channels.iter() {
            tracing::debug!(
                "M3U Generation (InMemory) - Channel #{}: id={}, channel_name='{}', tvg_name='{:?}', stream_url='{}'",
                nc.assigned_number, nc.channel.id, nc.channel.channel_name, nc.channel.tvg_name, nc.channel.stream_url
            );
            let extinf = format!(
                "#EXTINF:-1 tvg-id=\"{}\" tvg-name=\"{}\" tvg-logo=\"{}\" tvg-chno=\"{}\" group-title=\"{}\",{}",
                nc.channel.tvg_id.as_deref().unwrap_or(""),
                nc.channel.tvg_name.as_deref().unwrap_or(""),
                nc.channel.tvg_logo.as_deref().unwrap_or(""),
                nc.assigned_number,
                nc.channel.group_title.as_deref().unwrap_or(""),
                nc.channel.channel_name
            );
            tracing::debug!("M3U Generation (InMemory) - Generated EXTINF: '{}'", extinf);

            let proxy_stream_url = format!(
                "{}/stream/{}/{}",
                context.base_url.trim_end_matches('/'),
                context.proxy_config.proxy.ulid,
                nc.channel.id
            );

            m3u.push_str(&format!("{}\n{}\n", extinf, proxy_stream_url));
        }

        Ok(m3u)
    }

    fn can_handle_memory_pressure(&self, level: MemoryPressureLevel) -> bool {
        matches!(level, MemoryPressureLevel::Optimal | MemoryPressureLevel::Moderate | MemoryPressureLevel::High)
    }

    fn supports_mid_stage_switching(&self) -> bool {
        false
    }

    fn strategy_name(&self) -> &str {
        "inmemory_m3u"
    }

    fn estimated_memory_usage(&self, input_size: usize) -> Option<usize> {
        // M3U content is typically larger than channel data
        Some(input_size * 2048)
    }
}

/// Streaming M3U generation strategy for memory efficiency
pub struct StreamingM3uStrategy;

#[async_trait]
impl StageStrategy for StreamingM3uStrategy {
    async fn execute_source_loading(&self, _context: &StageContext, _source_ids: Vec<Uuid>) -> Result<Vec<Channel>> {
        unimplemented!("StreamingM3uStrategy only handles M3U generation")
    }

    async fn execute_data_mapping(&self, _context: &StageContext, _channels: Vec<Channel>) -> Result<Vec<Channel>> {
        unimplemented!("StreamingM3uStrategy only handles M3U generation")
    }

    async fn execute_filtering(&self, _context: &StageContext, _channels: Vec<Channel>) -> Result<Vec<Channel>> {
        unimplemented!("StreamingM3uStrategy only handles M3U generation")
    }

    async fn execute_channel_numbering(&self, _context: &StageContext, _channels: Vec<Channel>) -> Result<Vec<NumberedChannel>> {
        unimplemented!("StreamingM3uStrategy only handles M3U generation")
    }

    async fn execute_m3u_generation(&self, context: &StageContext, numbered_channels: Vec<NumberedChannel>) -> Result<String> {
        info!("Generating M3U content for {} channels using streaming strategy", numbered_channels.len());
        
        let mut m3u = String::from("#EXTM3U\n");

        // Process channels in smaller chunks to reduce memory pressure
        for (i, nc) in numbered_channels.iter().enumerate() {
            tracing::debug!(
                "M3U Generation (Streaming) - Channel #{}: id={}, channel_name='{}', tvg_name='{:?}', stream_url='{}'",
                nc.assigned_number, nc.channel.id, nc.channel.channel_name, nc.channel.tvg_name, nc.channel.stream_url
            );
            let extinf = format!(
                "#EXTINF:-1 tvg-id=\"{}\" tvg-name=\"{}\" tvg-logo=\"{}\" tvg-chno=\"{}\" group-title=\"{}\",{}",
                nc.channel.tvg_id.as_deref().unwrap_or(""),
                nc.channel.tvg_name.as_deref().unwrap_or(""),
                nc.channel.tvg_logo.as_deref().unwrap_or(""),
                nc.assigned_number,
                nc.channel.group_title.as_deref().unwrap_or(""),
                nc.channel.channel_name
            );
            tracing::debug!("M3U Generation (Streaming) - Generated EXTINF: '{}'", extinf);

            let proxy_stream_url = format!(
                "{}/stream/{}/{}",
                context.base_url.trim_end_matches('/'),
                context.proxy_config.proxy.ulid,
                nc.channel.id
            );

            m3u.push_str(&format!("{}\n{}\n", extinf, proxy_stream_url));

            // Yield control every 100 channels
            if i % 100 == 0 {
                tokio::task::yield_now().await;
            }
        }

        Ok(m3u)
    }

    fn can_handle_memory_pressure(&self, _level: MemoryPressureLevel) -> bool {
        true // Can handle any memory pressure
    }

    fn supports_mid_stage_switching(&self) -> bool {
        true
    }

    fn strategy_name(&self) -> &str {
        "streaming_m3u"
    }

    fn estimated_memory_usage(&self, _input_size: usize) -> Option<usize> {
        // Streaming approach uses minimal memory
        Some(1024 * 100) // ~100KB buffer
    }
}