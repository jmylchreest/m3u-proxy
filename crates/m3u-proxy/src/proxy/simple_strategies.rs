//! Simple in-memory strategies for all stages
//!
//! These are the default, high-performance strategies that process everything in memory.
//! Complex strategies (chunking, spill, compression) are handled by WASM plugins.

use anyhow::Result;
use async_trait::async_trait;
use std::collections::HashMap;
use std::time::Instant;
use tracing::{debug, info};

use crate::data_mapping::service::DataMappingService;
use crate::database::Database;
use crate::logo_assets::service::LogoAssetService;
use crate::models::*;
use crate::proxy::filter_engine::FilterEngine;
use crate::proxy::stage_contracts::*;
use crate::utils::MemoryContext;

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
    async fn execute(
        &self,
        input: SourceLoadingInput,
        memory_context: &mut MemoryContext,
    ) -> Result<SourceLoadingOutput> {
        let start_time = Instant::now();
        debug!(
            "Loading {} sources using simple in-memory strategy",
            input.source_ids.len()
        );

        let mut all_channels = Vec::new();
        let mut source_stats = HashMap::new();

        for source_id in &input.source_ids {
            let source_start = Instant::now();
            let channels = self.database.get_source_channels(*source_id).await?;
            let source_duration = source_start.elapsed().as_millis() as u64;

            // Track memory usage after loading each source
            let (memory_snapshot, _pressure) =
                memory_context.observe(&format!("source_{}", source_id)).await?;

            debug!(
                "Loaded {} channels from source {} in {}ms (Memory: {:.1}MB)",
                channels.len(),
                source_id,
                source_duration,
                memory_snapshot.rss_mb
            );

            source_stats.insert(
                *source_id,
                SourceStats {
                    channels_loaded: channels.len(),
                    load_duration_ms: source_duration,
                    memory_used_mb: Some(memory_snapshot.rss_mb),
                    errors: Vec::new(),
                },
            );

            all_channels.extend(channels);
        }

        let total_duration = start_time.elapsed().as_millis() as u64;
        let final_memory_stats = memory_context.get_memory_statistics();
        let total_stats = SourceStats {
            channels_loaded: all_channels.len(),
            load_duration_ms: total_duration,
            memory_used_mb: Some(final_memory_stats.peak_mb),
            errors: Vec::new(),
        };

        debug!(
            "Simple source loading completed: {} total channels in {}ms (Peak Memory: {:.1}MB)",
            all_channels.len(),
            total_duration,
            final_memory_stats.peak_mb
        );

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
    async fn execute(
        &self,
        input: DataMappingInput,
        memory_context: &mut MemoryContext,
    ) -> Result<DataMappingOutput> {
        let start_time = Instant::now();
        debug!(
            "Applying simple data mapping to {} channels",
            input.channels.len()
        );

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

            // Track memory usage after processing each source
            memory_context.observe(&format!("mapping_source_{}", source_id)).await?;
        }

        let total_duration = start_time.elapsed().as_millis() as u64;
        let final_memory_stats = memory_context.get_memory_statistics();
        let mapping_stats = MappingStats {
            channels_processed: original_channel_count,
            channels_transformed: all_mapped_channels.len(),
            transformations_applied: total_transformations,
            mapping_duration_ms: total_duration,
            memory_used_mb: Some(final_memory_stats.peak_mb),
        };

        debug!(
            "Simple data mapping completed: {} channels processed in {}ms (Peak Memory: {:.1}MB)",
            all_mapped_channels.len(),
            total_duration,
            final_memory_stats.peak_mb
        );

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
    async fn execute(
        &self,
        input: FilteringInput,
        memory_context: &mut MemoryContext,
    ) -> Result<FilteringOutput> {
        let start_time = Instant::now();
        let original_channel_count = input.channels.len();
        debug!(
            "Applying simple filtering to {} channels with {} filters",
            input.channels.len(),
            input.filters.len()
        );

        let filtered_channels = if !input.filters.is_empty() {
            let mut filter_engine = FilterEngine::new();

            let filter_tuples: Vec<_> = input
                .filters
                .iter()
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

            let result = filter_engine
                .apply_filters(input.channels, filter_tuples)
                .await?;
            memory_context.observe("filter_application").await?;
            result
        } else {
            input.channels
        };

        let total_duration = start_time.elapsed().as_millis() as u64;
        let channels_filtered_out = original_channel_count - filtered_channels.len();
        let final_memory_stats = memory_context.get_memory_statistics();

        let filter_stats = FilterStats {
            channels_input: original_channel_count,
            channels_output: filtered_channels.len(),
            channels_filtered_out,
            filters_applied: input
                .filters
                .iter()
                .filter(|f| f.is_active)
                .map(|f| f.filter.name.clone())
                .collect(),
            filter_duration_ms: total_duration,
            memory_used_mb: Some(final_memory_stats.peak_mb),
        };

        info!(
            "Simple filtering completed: {}/{} channels passed ({} filtered out) in {}ms (Peak Memory: {:.1}MB)",
            filtered_channels.len(),
            original_channel_count,
            channels_filtered_out,
            total_duration,
            final_memory_stats.peak_mb
        );

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
    async fn execute(
        &self,
        input: ChannelNumberingInput,
        memory_context: &mut MemoryContext,
    ) -> Result<ChannelNumberingOutput> {
        let start_time = Instant::now();
        debug!(
            "Applying simple channel numbering to {} channels starting from {}",
            input.channels.len(),
            input.starting_number
        );

        let numbered_channels: Vec<NumberedChannel> = input
            .channels
            .into_iter()
            .enumerate()
            .map(|(i, channel)| NumberedChannel {
                channel,
                assigned_number: input.starting_number + i as i32,
                assignment_type: ChannelNumberAssignmentType::Sequential,
            })
            .collect();

        memory_context.observe("channel_numbering").await?;

        let total_duration = start_time.elapsed().as_millis() as u64;
        let final_memory_stats = memory_context.get_memory_statistics();
        let numbering_stats = NumberingStats {
            channels_numbered: numbered_channels.len(),
            starting_number: input.starting_number,
            numbering_strategy: "sequential".to_string(),
            numbering_duration_ms: total_duration,
        };

        debug!(
            "Simple channel numbering completed: {} channels numbered in {}ms (Peak Memory: {:.1}MB)",
            numbered_channels.len(),
            total_duration,
            final_memory_stats.peak_mb
        );

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

