//! Simple in-memory strategies for all stages
//!
//! These are the default, high-performance strategies that process everything in memory.
//! Complex strategies (chunking, spill, compression) are handled by WASM plugins.

use anyhow::Result;
use async_trait::async_trait;
use std::collections::HashMap;
use std::time::Instant;
use tracing::info;

use crate::database::Database;
use crate::data_mapping::service::DataMappingService;
use crate::logo_assets::service::LogoAssetService;
use crate::models::*;
use crate::proxy::filter_engine::FilterEngine;
use crate::proxy::stage_contracts::*;

/// Simple in-memory source loading strategy
pub struct SimpleSourceLoader {
    database: Database,
}

impl SimpleSourceLoader {
    pub fn new(database: Database) -> Self {
        Self { database }
    }
}

#[async_trait]
impl SourceLoadingStage for SimpleSourceLoader {
    async fn execute(&self, input: SourceLoadingInput) -> Result<SourceLoadingOutput> {
        let start_time = Instant::now();
        info!("Loading {} sources using simple in-memory strategy", input.source_ids.len());

        let mut all_channels = Vec::new();
        let mut source_stats = HashMap::new();

        for source_id in &input.source_ids {
            let source_start = Instant::now();
            let channels = self.database.get_source_channels(*source_id).await?;
            let source_duration = source_start.elapsed().as_millis() as u64;

            info!("Loaded {} channels from source {} in {}ms", 
                  channels.len(), source_id, source_duration);

            source_stats.insert(*source_id, SourceStats {
                channels_loaded: channels.len(),
                load_duration_ms: source_duration,
                memory_used_mb: None, // Simple strategy doesn't track detailed memory
                errors: Vec::new(),
            });

            all_channels.extend(channels);
        }

        let total_duration = start_time.elapsed().as_millis() as u64;
        let total_stats = SourceStats {
            channels_loaded: all_channels.len(),
            load_duration_ms: total_duration,
            memory_used_mb: None,
            errors: Vec::new(),
        };

        info!("Simple source loading completed: {} total channels in {}ms", 
              all_channels.len(), total_duration);

        Ok(SourceLoadingOutput {
            channels: all_channels,
            source_stats,
            total_stats,
        })
    }

    fn strategy_name(&self) -> &str {
        "simple_inmemory"
    }

    fn estimated_memory_usage(&self, input: &SourceLoadingInput) -> Option<usize> {
        // Rough estimate: assume 1KB per channel per source
        Some(input.source_ids.len() * 1000 * 1024)
    }
}

/// Simple in-memory data mapping strategy
pub struct SimpleDataMapper {
    data_mapping_service: DataMappingService,
    logo_service: LogoAssetService,
}

impl SimpleDataMapper {
    pub fn new(data_mapping_service: DataMappingService, logo_service: LogoAssetService) -> Self {
        Self {
            data_mapping_service,
            logo_service,
        }
    }
}

#[async_trait]
impl DataMappingStage for SimpleDataMapper {
    async fn execute(&self, input: DataMappingInput) -> Result<DataMappingOutput> {
        let start_time = Instant::now();
        info!("Applying simple data mapping to {} channels", input.channels.len());

        // Store original count before processing
        let original_channel_count = input.channels.len();
        
        // Group channels by source for efficient processing
        let mut channels_by_source = HashMap::new();
        for channel in input.channels {
            channels_by_source
                .entry(channel.source_id)
                .or_insert_with(Vec::new)
                .push(channel);
        }

        let mut all_mapped_channels = Vec::new();
        let mut total_transformations = 0;

        // Process each source's channels
        for (source_id, source_channels) in channels_by_source {
            let mapped_channels = self
                .data_mapping_service
                .apply_mapping_for_proxy(
                    source_channels.clone(),
                    source_id,
                    &self.logo_service,
                    &input.base_url,
                    input.engine_config.clone(),
                )
                .await?;

            total_transformations += source_channels.len();
            all_mapped_channels.extend(mapped_channels);
        }

        let total_duration = start_time.elapsed().as_millis() as u64;
        let mapping_stats = MappingStats {
            channels_processed: original_channel_count,
            channels_transformed: all_mapped_channels.len(),
            transformations_applied: total_transformations,
            mapping_duration_ms: total_duration,
            memory_used_mb: None,
        };

        info!("Simple data mapping completed: {} channels processed in {}ms", 
              all_mapped_channels.len(), total_duration);

        Ok(DataMappingOutput {
            mapped_channels: all_mapped_channels,
            mapping_stats,
        })
    }

    fn strategy_name(&self) -> &str {
        "simple_inmemory"
    }

    fn estimated_memory_usage(&self, input: &DataMappingInput) -> Option<usize> {
        // Data mapping typically doesn't increase memory much
        Some(input.channels.len() * 1024)
    }
}

/// Simple in-memory filtering strategy
pub struct SimpleFilter;

#[async_trait]
impl FilteringStage for SimpleFilter {
    async fn execute(&self, input: FilteringInput) -> Result<FilteringOutput> {
        let start_time = Instant::now();
        let original_channel_count = input.channels.len();
        info!("Applying simple filtering to {} channels with {} filters", 
              original_channel_count, input.filters.len());

        let filtered_channels = if !input.filters.is_empty() {
            let mut filter_engine = FilterEngine::new();
            
            let filter_tuples: Vec<_> = input.filters.iter()
                .filter(|f| f.is_active)
                .map(|f| {
                    // Create a proxy filter - we'll need to get the actual proxy ID from context
                    // For now, we'll use a placeholder since we don't have access to the full proxy config here
                    let proxy_filter = ProxyFilter {
                        proxy_id: uuid::Uuid::nil(), // TODO: Get actual proxy ID from context
                        filter_id: f.filter.id,
                        priority_order: f.priority_order,
                        is_active: f.is_active,
                        created_at: chrono::Utc::now(),
                    };
                    (f.filter.clone(), proxy_filter)
                })
                .collect();

            filter_engine.apply_filters(input.channels, filter_tuples).await?
        } else {
            input.channels
        };

        let total_duration = start_time.elapsed().as_millis() as u64;
        let channels_filtered_out = original_channel_count - filtered_channels.len();
        
        let filter_stats = FilterStats {
            channels_input: original_channel_count,
            channels_output: filtered_channels.len(),
            channels_filtered_out,
            filters_applied: input.filters.iter()
                .filter(|f| f.is_active)
                .map(|f| f.filter.name.clone())
                .collect(),
            filter_duration_ms: total_duration,
            memory_used_mb: None,
        };

        info!("Simple filtering completed: {}/{} channels passed ({} filtered out) in {}ms", 
              filtered_channels.len(), original_channel_count, channels_filtered_out, total_duration);

        Ok(FilteringOutput {
            filtered_channels,
            filter_stats,
        })
    }

    fn strategy_name(&self) -> &str {
        "simple_inmemory"
    }

    fn estimated_memory_usage(&self, input: &FilteringInput) -> Option<usize> {
        // Filtering doesn't typically increase memory usage
        Some(input.channels.len() * 1024)
    }
}

/// Simple in-memory channel numbering strategy
pub struct SimpleChannelNumbering;

#[async_trait]
impl ChannelNumberingStage for SimpleChannelNumbering {
    async fn execute(&self, input: ChannelNumberingInput) -> Result<ChannelNumberingOutput> {
        let start_time = Instant::now();
        info!("Applying simple channel numbering to {} channels starting from {}", 
              input.channels.len(), input.starting_number);

        let numbered_channels: Vec<NumberedChannel> = input.channels
            .into_iter()
            .enumerate()
            .map(|(i, channel)| NumberedChannel {
                channel,
                assigned_number: input.starting_number + i as i32,
                assignment_type: ChannelNumberAssignmentType::Sequential,
            })
            .collect();

        let total_duration = start_time.elapsed().as_millis() as u64;
        let numbering_stats = NumberingStats {
            channels_numbered: numbered_channels.len(),
            starting_number: input.starting_number,
            numbering_strategy: "sequential".to_string(),
            numbering_duration_ms: total_duration,
        };

        info!("Simple channel numbering completed: {} channels numbered in {}ms", 
              numbered_channels.len(), total_duration);

        Ok(ChannelNumberingOutput {
            numbered_channels,
            numbering_stats,
        })
    }

    fn strategy_name(&self) -> &str {
        "simple_inmemory"
    }

    fn estimated_memory_usage(&self, input: &ChannelNumberingInput) -> Option<usize> {
        // Channel numbering creates numbered channel objects
        Some(input.channels.len() * 1200) // Slightly larger than base channels
    }
}

/// Simple in-memory M3U generation strategy
pub struct SimpleM3uGenerator;

#[async_trait]
impl M3uGenerationStage for SimpleM3uGenerator {
    async fn execute(&self, input: M3uGenerationInput) -> Result<M3uGenerationOutput> {
        let start_time = Instant::now();
        info!("Generating M3U content for {} channels using simple strategy", 
              input.numbered_channels.len());

        let mut m3u = String::from("#EXTM3U\n");

        for nc in &input.numbered_channels {
            let extinf = format!(
                "#EXTINF:-1 tvg-id=\"{}\" tvg-name=\"{}\" tvg-logo=\"{}\" tvg-chno=\"{}\" group-title=\"{}\",{}",
                nc.channel.tvg_id.as_deref().unwrap_or(""),
                nc.channel.tvg_name.as_deref().unwrap_or(""),
                nc.channel.tvg_logo.as_deref().unwrap_or(""),
                nc.assigned_number,
                nc.channel.group_title.as_deref().unwrap_or(""),
                nc.channel.channel_name
            );

            let proxy_stream_url = format!(
                "{}/stream/{}/{}",
                input.base_url.trim_end_matches('/'),
                input.proxy_ulid,
                nc.channel.id
            );

            m3u.push_str(&format!("{}\n{}\n", extinf, proxy_stream_url));
        }

        let total_duration = start_time.elapsed().as_millis() as u64;
        let m3u_stats = M3uStats {
            channels_processed: input.numbered_channels.len(),
            m3u_size_bytes: m3u.len(),
            m3u_lines: m3u.lines().count(),
            generation_duration_ms: total_duration,
            memory_used_mb: None,
        };

        info!("Simple M3U generation completed: {} bytes generated in {}ms", 
              m3u.len(), total_duration);

        Ok(M3uGenerationOutput {
            m3u_content: m3u,
            m3u_stats,
        })
    }

    fn strategy_name(&self) -> &str {
        "simple_inmemory"
    }

    fn estimated_memory_usage(&self, input: &M3uGenerationInput) -> Option<usize> {
        // M3U content is typically larger than channel data
        Some(input.numbered_channels.len() * 2048)
    }
}

