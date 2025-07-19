//! Unified Iterator-Based Pipeline
//!
//! This pipeline implements the complete 7-stage processing pipeline using orchestrator
//! iterators, chunk management, and sophisticated buffering. It provides native implementations
//! and coordinates memory usage across all stages.

use anyhow::Result;
use std::sync::Arc;
use std::time::Instant;
use tracing::{debug, info, warn};

use crate::data_mapping::service::DataMappingService;
use crate::database::Database;
use crate::logo_assets::service::LogoAssetService;
use crate::models::*;
use crate::pipeline::chunk_manager::ChunkSizeManager;
use crate::pipeline::generic_iterator::{
    MappingIterator, MultiSourceIterator, SingleSourceIterator,
};
use crate::pipeline::iterator_traits::PipelineIterator;
use crate::pipeline::orchestrator::OrchestratorIteratorFactory;
use crate::pipeline::rolling_buffer_iterator::BufferConfig;
use crate::pipeline::{AccumulatorFactory, AccumulationStrategy, IteratorAccumulator, IteratorRegistry};
use crate::proxy::stage_strategy::{MemoryPressureLevel, StageContext};
use crate::services::sandboxed_file::{SandboxedFileManager, SandboxedManagerAdapter};
use crate::utils::{MemoryCleanupCoordinator, MemoryContext, SimpleMemoryMonitor};
use sandboxed_file_manager::SandboxedManager;

// Simple data structures for pipeline stages
#[derive(Debug, Clone)]
struct SourceLoadingInput {
    source_ids: Vec<uuid::Uuid>,
    proxy_config: ResolvedProxyConfig,
}

#[derive(Debug, Clone)]
struct DataMappingInput {
    channels: Vec<Channel>,
    proxy_config: ResolvedProxyConfig,
    engine_config: Option<crate::config::DataMappingEngineConfig>,
    base_url: String,
}

#[derive(Debug, Clone)]
struct FilteringInput {
    channels: Vec<Channel>,
    filters: Vec<(Filter, ProxyFilter)>,
}

#[derive(Debug, Clone)]
struct ChannelNumberingInput {
    channels: Vec<Channel>,
    starting_number: i32,
    numbering_strategy: ChannelNumberingStrategy,
}

#[derive(Debug, Clone)]
enum ChannelNumberingStrategy {
    Sequential,
}

// Simple strategy implementations
struct SimpleSourceLoader {
    database: Database,
}

impl SimpleSourceLoader {
    fn new(database: Database) -> Self {
        Self { database }
    }
    
    async fn execute(&self, input: SourceLoadingInput, _memory_context: &MemoryContext) -> Result<Vec<Channel>> {
        let mut all_channels = Vec::new();
        for source_id in input.source_ids {
            let channels = self.database.get_source_channels(source_id).await?;
            all_channels.extend(channels);
        }
        Ok(all_channels)
    }
}

struct SimpleDataMapper {
    data_mapping_service: DataMappingService,
    logo_service: LogoAssetService,
}

impl SimpleDataMapper {
    fn new(data_mapping_service: DataMappingService, logo_service: LogoAssetService) -> Self {
        Self {
            data_mapping_service,
            logo_service,
        }
    }
    
    async fn execute(&self, input: DataMappingInput, _memory_context: &MemoryContext) -> Result<Vec<Channel>> {
        // Apply mapping for each source separately and then combine
        let mut all_mapped_channels = Vec::new();
        
        // Group channels by their source_id
        let mut channels_by_source: std::collections::HashMap<uuid::Uuid, Vec<Channel>> = std::collections::HashMap::new();
        for channel in input.channels {
            channels_by_source.entry(channel.source_id).or_insert_with(Vec::new).push(channel);
        }
        
        // Apply mapping for each source
        for (source_id, channels) in channels_by_source {
            let mapped_channels = self.data_mapping_service.apply_mapping_for_proxy(
                channels,
                source_id,
                &self.logo_service,
                &input.base_url,
                input.engine_config.clone(),
            ).await?;
            all_mapped_channels.extend(mapped_channels);
        }
        
        Ok(all_mapped_channels)
    }
}

struct SimpleFilter;

impl SimpleFilter {
    async fn execute(&self, input: FilteringInput, _memory_context: &MemoryContext) -> Result<Vec<Channel>> {
        let mut engine = crate::proxy::filter_engine::FilterEngine::new();
        engine.apply_filters(input.channels, input.filters).await
    }
}

struct SimpleChannelNumbering;

impl SimpleChannelNumbering {
    async fn execute(&self, input: ChannelNumberingInput, _memory_context: &MemoryContext) -> Result<Vec<NumberedChannel>> {
        let mut numbered_channels = Vec::new();
        let mut current_number = input.starting_number;
        
        for channel in input.channels {
            numbered_channels.push(NumberedChannel {
                channel,
                assigned_number: current_number,
                assignment_type: ChannelNumberAssignmentType::Sequential,
            });
            current_number += 1;
        }
        
        Ok(numbered_channels)
    }
}

/// Unified iterator-based pipeline with sophisticated buffering and native processing

pub struct NativePipeline {
    database: Database,
    data_mapping_service: DataMappingService,
    logo_service: LogoAssetService,
    memory_monitor: Option<SimpleMemoryMonitor>,
    temp_file_manager: SandboxedManager,
    chunk_manager: Arc<ChunkSizeManager>,
    memory_limit_mb: Option<usize>,
    #[allow(dead_code)]
    iterator_registry: Arc<IteratorRegistry>,
    system: Arc<tokio::sync::RwLock<sysinfo::System>>,
}

#[allow(dead_code)]
impl NativePipeline {
    pub fn new(
        database: Database,
        data_mapping_service: DataMappingService,
        logo_service: LogoAssetService,
        memory_limit_mb: Option<usize>,
        temp_file_manager: SandboxedManager,
        shared_memory_monitor: Option<SimpleMemoryMonitor>,
        system: Arc<tokio::sync::RwLock<sysinfo::System>>,
    ) -> Self {
        let memory_monitor = shared_memory_monitor;

        // Initialize chunk manager with stage dependencies
        let chunk_manager = Arc::new(ChunkSizeManager::new(
            1000,                                // default_chunk_size
            memory_limit_mb.unwrap_or(512) * 10, // max_chunk_size (10x memory limit)
        ));

        // Initialize iterator registry for pipeline data sharing
        let iterator_registry = Arc::new(IteratorRegistry::new());

        Self {
            database,
            data_mapping_service,
            logo_service,
            memory_monitor,
            temp_file_manager,
            chunk_manager,
            memory_limit_mb,
            iterator_registry,
            system,
        }
    }

    /// Initialize the pipeline
    pub async fn initialize(&mut self) -> Result<()> {
        info!("Initializing native pipeline");
        Ok(())
    }

    /// Generate proxy using unified iterator-based pipeline with all 7 stages
    pub async fn generate_with_dynamic_strategies(
        &mut self,
        config: ResolvedProxyConfig,
        base_url: &str,
        engine_config: Option<crate::config::DataMappingEngineConfig>,
        _app_config: &crate::config::Config,
    ) -> Result<ProxyGeneration> {
        // Using native pipeline only
        info!(
            "Starting unified pipeline generation proxy={} sources={} filters={}",
            config.proxy.name,
            config.sources.len(),
            config.filters.len()
        );

        let overall_start = Instant::now();

        // Initialize memory context for tracking
        let mut memory_context = MemoryContext::new(self.memory_limit_mb, None, self.system.clone());
        memory_context.initialize().await?;

        // Initialize cleanup coordinator
        let mut cleanup_coordinator =
            MemoryCleanupCoordinator::new(true, self.memory_limit_mb.map(|mb| mb as f64));

        // Execute all 7 pipeline stages using iterator-based approach
        let generation = self
            .execute_unified_pipeline_stages(
                config.clone(),
                base_url,
                engine_config,
                &mut memory_context,
                &mut cleanup_coordinator,
            )
            .await?;

        let total_duration = overall_start.elapsed().as_millis() as u64;

        info!(
            "Unified pipeline completed proxy={} total_time={} channels={} pipeline=iterator-based",
            config.proxy.name, crate::utils::format_duration(total_duration), generation.channel_count
        );

        // Log memory analysis
        let memory_analysis = memory_context.analyze_memory_patterns();
        debug!(
            "Memory analysis: growth={:.1}MB, stages={}, trend={:?}",
            memory_analysis.total_memory_growth_mb,
            memory_analysis.total_stages,
            memory_analysis.memory_efficiency_trend
        );

        Ok(generation)
    }

    /// Execute all 7 pipeline stages using unified iterator-based approach
    async fn execute_unified_pipeline_stages(
        &self,
        config: ResolvedProxyConfig,
        base_url: &str,
        engine_config: Option<crate::config::DataMappingEngineConfig>,
        memory_context: &mut MemoryContext,
        cleanup_coordinator: &mut MemoryCleanupCoordinator,
    ) -> Result<ProxyGeneration> {
        info!("Starting unified 6-stage iterator-based pipeline execution");

        // Load channels using RollingBufferChannelIterator with active source filtering
        info!(
            "Loading channels using RollingBufferChannelIterator for {} sources",
            config.sources.len()
        );

        let _memory_pressure = self.get_current_memory_pressure(memory_context).await;

        // Request optimal chunk size from chunk manager first
        let requested_chunk_size = self
            .chunk_manager
            .request_chunk_size("data_loading", 3000)
            .await?;

        info!("Data loading using chunk size: {}", requested_chunk_size);

        // Create RollingBufferChannelIterator with buffer size matching chunk size for efficiency
        let buffer_config = BufferConfig {
            initial_buffer_size: requested_chunk_size, // Match buffer to chunk size
            max_buffer_size: requested_chunk_size * 5, // Allow up to 5x chunk size maximum
            trigger_threshold: 0.8, // Load next when 80% consumed (more efficient)
            initial_chunk_size: requested_chunk_size,
            max_concurrent_sources: 2,
            enable_cascade_integration: true,
        };

        let channel_iterator = OrchestratorIteratorFactory::create_rolling_buffer_channel_iterator_from_configs_with_cascade(
            Arc::new(self.database.clone()),
            config.proxy.id,
            config.sources.clone(),
            buffer_config,
            Some(self.chunk_manager.clone()),
            "data_loading".to_string(),
        );

        // Use accumulator to collect channels with automatic memory management
        let file_manager = Arc::new(SandboxedManagerAdapter::new(self.temp_file_manager.clone()))
            as Arc<dyn SandboxedFileManager>;
        let mut channel_accumulator =
            AccumulatorFactory::create_channel_accumulator(file_manager);

        info!("Loading channels using accumulator with automatic spilling");

        // Convert channel iterator to JsonValue iterator
        let json_iterator = Box::new(MappingIterator::new(channel_iterator, |channel| {
            Ok(serde_json::to_value(channel)?)
        }));

        // Accumulate all channels from iterator
        channel_accumulator
            .accumulate_channels(json_iterator)
            .await?;

        // Get completed channel data
        let channels = channel_accumulator.get_channels().await?;
        let stats = channel_accumulator.get_stats();

        info!(
            "Channel accumulation completed:\n\
             ├─ Channels loaded: {}\n\
             ├─ Strategy used: {:?}\n\
             ├─ Memory estimate: {:.1}MB\n\
             └─ Spilled to disk: {}",
            stats.total_items,
            stats.strategy,
            stats.estimated_memory_mb,
            stats.estimated_memory_mb > 50.0 // Likely spilled if over 50MB
        );

        // Debug: Log first few channels to see the data structure
        if !channels.is_empty() {
            for (i, channel) in channels.iter().take(3).enumerate() {
                debug!(
                    "Channel #{}: id={}, channel_name='{}', tvg_name='{:?}', stream_url='{}'",
                    i + 1,
                    channel.id,
                    channel.channel_name,
                    channel.tvg_name,
                    channel.stream_url
                );
            }
        }

        info!("Data loading completed: {} channels loaded", channels.len());

        // Stage 1: Data Mapping (using OrderedDataMappingIterator)
        let mapped_channels = self
            .execute_data_mapping_with_iterator(
                channels,
                &config,
                base_url,
                engine_config,
                memory_context,
                cleanup_coordinator,
            )
            .await?;

        // Stage 2: Filtering (using OrderedFilterIterator)
        let filtered_channels = self
            .execute_filtering_with_iterator(
                mapped_channels,
                &config,
                memory_context,
                cleanup_coordinator,
            )
            .await?;

        // Stage 3: Logo Prefetch (NEW - with access to EPG data)
        let logo_processed_channels = self
            .execute_logo_prefetch_with_iterator(
                filtered_channels,
                &config,
                memory_context,
                cleanup_coordinator,
            )
            .await?;

        // Stage 4: Channel Numbering
        let numbered_channels = self
            .execute_channel_numbering_with_iterator(
                logo_processed_channels,
                &config,
                memory_context,
                cleanup_coordinator,
            )
            .await?;

        // Stage 5: M3U Generation
        let m3u_content = self
            .execute_m3u_generation_with_iterator(
                numbered_channels.clone(),
                &config,
                base_url,
                memory_context,
                cleanup_coordinator,
            )
            .await?;

        // Stage 6: EPG Processing (parallel to M3U generation, using OrderedEpgAggregateIterator)
        let _epg_content = self
            .execute_epg_processing_with_iterator(
                numbered_channels.clone(),
                &config,
                base_url,
                memory_context,
                cleanup_coordinator,
            )
            .await?;

        info!("Unified 6-stage pipeline execution completed successfully");

        // Debug: Log final channel structure before M3U generation record
        if !numbered_channels.is_empty() {
            debug!("=== FINAL PIPELINE OUTPUT ===");
            for (i, numbered_channel) in numbered_channels.iter().take(3).enumerate() {
                debug!(
                    "Final Channel #{}: number={}, name='{}', tvg_name='{:?}', group='{:?}'",
                    i + 1,
                    numbered_channel.assigned_number,
                    numbered_channel.channel.channel_name,
                    numbered_channel.channel.tvg_name,
                    numbered_channel.channel.group_title
                );
            }

            // Also log a preview of what the M3U content looks like
            debug!("=== M3U CONTENT PREVIEW ===");
            let preview_lines: Vec<&str> = m3u_content.lines().take(10).collect();
            for line in preview_lines {
                debug!("M3U Line: {}", line);
            }
        }

        // Create generation record with enhanced statistics
        let generation = ProxyGeneration {
            id: uuid::Uuid::new_v4(),
            proxy_id: config.proxy.id,
            version: 1,
            channel_count: numbered_channels.len() as i32,
            total_channels: numbered_channels.len(),
            filtered_channels: numbered_channels.len(),
            applied_filters: config
                .filters
                .iter()
                .map(|f| f.filter.name.clone())
                .collect(),
            m3u_content,
            created_at: chrono::Utc::now(),
            stats: None, // TODO: Integrate with chunk manager and iterator statistics
            processed_channels: Some(numbered_channels),
        };

        Ok(generation)
    }

    /// Execute data mapping stage using OrderedDataMappingIterator with native implementation
    async fn execute_data_mapping_with_iterator(
        &self,
        channels: Vec<Channel>,
        config: &ResolvedProxyConfig,
        base_url: &str,
        engine_config: Option<crate::config::DataMappingEngineConfig>,
        memory_context: &mut MemoryContext,
        _cleanup_coordinator: &mut MemoryCleanupCoordinator,
    ) -> Result<Vec<Channel>> {
        memory_context.start_stage("data_mapping").await?;

        info!(
            "Starting data mapping stage with OrderedDataMappingIterator for {} channels",
            channels.len()
        );

        let _memory_pressure = self.get_current_memory_pressure(memory_context).await;

        // Create OrderedDataMappingIterator for mapping rules
        let mapping_iterator = SingleSourceIterator::new(
            Arc::new(self.database.clone()),
            crate::pipeline::orchestrator::DataMappingLoader {},
            config.proxy.id,
            1000,
        );

        // Request optimal chunk size from chunk manager (coordinates with upstream)
        let default_chunk_size = self.chunk_manager.get_chunk_size("data_mapping").await;
        let requested_chunk_size = self
            .chunk_manager
            .request_chunk_size("data_mapping", channels.len().min(default_chunk_size))
            .await?;

        info!("Data mapping using chunk size: {}", requested_chunk_size);

        // Using native data mapping implementation

        // Use accumulator for mapping rules with memory management
        let file_manager = Arc::new(SandboxedManagerAdapter::new(self.temp_file_manager.clone()))
            as Arc<dyn SandboxedFileManager>;
        let mut rules_accumulator = IteratorAccumulator::new(
            AccumulationStrategy::default_hybrid(),
            file_manager,
        );

        info!("Loading mapping rules using accumulator");

        // Accumulate all mapping rules from iterator
        rules_accumulator
            .accumulate_from_iterator(Box::new(mapping_iterator))
            .await?;

        // Get stats before consuming the accumulator
        let rules_stats = rules_accumulator.get_stats();

        // Get completed rule data
        let all_mapping_rules = rules_accumulator.into_items().await?;

        info!(
            "Mapping rules accumulation completed:\n\
             ├─ Rules loaded: {}\n\
             ├─ Strategy used: {:?}\n\
             ├─ Memory estimate: {:.1}MB",
            rules_stats.total_items, rules_stats.strategy, rules_stats.estimated_memory_mb
        );

        // Apply data mapping using the service with loaded rules
        let original_count = channels.len();
        let mapped_channels = if all_mapping_rules.is_empty() {
            info!("No mapping rules found, channels passed through unchanged");
            channels
        } else {
            info!(
                "Applying {} mapping rules to {} channels",
                all_mapping_rules.len(),
                channels.len()
            );

            // Use DataMappingService to apply the mapping rules
            self.data_mapping_service
                .apply_mapping_for_proxy(
                    channels.clone(),
                    config.proxy.id,
                    &self.logo_service,
                    base_url,
                    engine_config,
                )
                .await?
        };

        let stage_info = memory_context.complete_stage("data_mapping").await?;

        info!(
            "Native data mapping completed:\n\
             ├─ Implementation: OrderedDataMappingIterator + DataMappingService\n\
             ├─ Channels processed: {} → {}\n\
             ├─ Mapping rules applied: {}\n\
             ├─ Chunk size used: {}\n\
             ├─ Memory used: {:.1}MB\n\
             ├─ Processing time: {}ms\n\
             └─ Memory pressure: {:?}",
            original_count,
            mapped_channels.len(),
            all_mapping_rules.len(),
            requested_chunk_size,
            stage_info.memory_delta_mb,
            stage_info.duration_ms,
            stage_info.pressure_level
        );

        Ok(mapped_channels)
    }

    /// Execute filtering stage using OrderedFilterIterator with native implementation
    async fn execute_filtering_with_iterator(
        &self,
        channels: Vec<Channel>,
        config: &ResolvedProxyConfig,
        memory_context: &mut MemoryContext,
        _cleanup_coordinator: &mut MemoryCleanupCoordinator,
    ) -> Result<Vec<Channel>> {
        memory_context.start_stage("filtering").await?;

        info!(
            "Starting filtering stage with OrderedFilterIterator for {} channels",
            channels.len()
        );

        let _memory_pressure = self.get_current_memory_pressure(memory_context).await;

        // Create OrderedFilterIterator for filter rules
        let mut filter_iterator = MultiSourceIterator::new(
            Arc::new(self.database.clone()),
            config.filters.clone(),
            crate::pipeline::orchestrator::FilterLoader {},
            1000,
        );

        // Request optimal chunk size from chunk manager (coordinates with upstream)
        let default_chunk_size = self.chunk_manager.get_chunk_size("filtering").await;
        let requested_chunk_size = self
            .chunk_manager
            .request_chunk_size("filtering", channels.len().min(default_chunk_size))
            .await?;

        info!("Filtering using chunk size: {}", requested_chunk_size);

        // Using native filtering implementation

        // Native implementation using direct filter configuration + FilterEngine
        // Convert filter configs to (Filter, ProxyFilter) tuples for FilterEngine
        let filter_tuples: Vec<(Filter, ProxyFilter)> = config
            .filters
            .iter()
            .map(|filter_config| {
                // Create ProxyFilter from the config data
                let proxy_filter = ProxyFilter {
                    proxy_id: config.proxy.id,
                    filter_id: filter_config.filter.id,
                    priority_order: filter_config.priority_order,
                    is_active: filter_config.is_active,
                    created_at: chrono::Utc::now(),
                };
                (filter_config.filter.clone(), proxy_filter)
            })
            .collect();

        // Close filter iterator
        filter_iterator.close().await?;

        // Apply filtering using FilterEngine with loaded rules
        let input_channel_count = channels.len();

        // Debug: Log first few channels entering filtering stage
        if !channels.is_empty() {
            debug!("=== FILTERING STAGE INPUT ===");
            for (i, channel) in channels.iter().take(3).enumerate() {
                debug!(
                    "Input Channel #{}: name='{}', tvg_name='{:?}', group='{:?}', stream_url='{}'",
                    i + 1,
                    channel.channel_name,
                    channel.tvg_name,
                    channel.group_title,
                    channel.stream_url
                );
            }
        }

        let filtered_channels = if filter_tuples.is_empty() {
            info!("No filter rules found, all channels passed through");
            channels
        } else {
            info!(
                "Applying {} filter rules to {} channels",
                filter_tuples.len(),
                channels.len()
            );

            // Use FilterEngine to apply the filter rules
            let mut filter_engine = crate::proxy::filter_engine::FilterEngine::new();
            let result = filter_engine
                .apply_filters(channels, filter_tuples.clone())
                .await?;

            // Debug: Log first few channels after filtering
            if !result.is_empty() {
                debug!("=== FILTERING STAGE OUTPUT ===");
                for (i, channel) in result.iter().take(3).enumerate() {
                    debug!(
                        "Output Channel #{}: name='{}', tvg_name='{:?}', group='{:?}', stream_url='{}'",
                        i + 1,
                        channel.channel_name,
                        channel.tvg_name,
                        channel.group_title,
                        channel.stream_url
                    );
                }
            }

            result
        };

        let stage_info = memory_context.complete_stage("filtering").await?;

        info!(
            "Native filtering completed:\n\
             ├─ Implementation: OrderedFilterIterator + FilterEngine\n\
             ├─ Channels filtered: {} → {} ({:.1}% passed)\n\
             ├─ Filter rules applied: {}\n\
             ├─ Chunk size used: {}\n\
             ├─ Memory used: {:.1}MB\n\
             ├─ Processing time: {}ms\n\
             └─ Memory pressure: {:?}",
            input_channel_count,
            filtered_channels.len(),
            if input_channel_count == 0 {
                0.0
            } else {
                (filtered_channels.len() as f64 / input_channel_count as f64) * 100.0
            },
            filter_tuples.len(),
            requested_chunk_size,
            stage_info.memory_delta_mb,
            stage_info.duration_ms,
            stage_info.pressure_level
        );

        Ok(filtered_channels)
    }

    /// Execute logo prefetch stage with access to both channels and EPG data
    async fn execute_logo_prefetch_with_iterator(
        &self,
        channels: Vec<Channel>,
        _config: &ResolvedProxyConfig,
        memory_context: &mut MemoryContext,
        _cleanup_coordinator: &mut MemoryCleanupCoordinator,
    ) -> Result<Vec<Channel>> {
        memory_context.start_stage("logo_prefetch").await?;

        info!(
            "Starting logo prefetch stage for {} channels (with EPG access)",
            channels.len()
        );

        let _memory_pressure = self.get_current_memory_pressure(memory_context).await;

        // Request optimal chunk size from chunk manager
        let default_chunk_size = self.chunk_manager.get_chunk_size("logo_prefetch").await;
        let requested_chunk_size = self
            .chunk_manager
            .request_chunk_size("logo_prefetch", channels.len().min(default_chunk_size))
            .await?;

        info!("Logo prefetch using chunk size: {}", requested_chunk_size);

        // Using native logo prefetch implementation

        // Native implementation using LogoAssetService
        // This stage processes channels in chunks to cache logos efficiently
        let mut processed_channels = Vec::new();
        let mut total_logos_cached = 0;

        // Process channels in chunks for memory efficiency
        for chunk in channels.chunks(requested_chunk_size) {
            let mut chunk_channels = chunk.to_vec();

            // Process each channel for logo caching
            for channel in &mut chunk_channels {
                // Cache channel logo if available
                if let Some(logo_url) = &channel.tvg_logo {
                    if !logo_url.is_empty() {
                        // Actually cache the logo from URL with channel metadata
                        match self
                            .logo_service
                            .cache_logo_from_url_with_metadata(
                                logo_url,
                                Some(channel.channel_name.clone()),
                                channel.group_title.clone(),
                                None,
                            )
                            .await
                        {
                            Ok(cache_id) => {
                                debug!(
                                    "Successfully cached logo for channel '{}': {} -> {}",
                                    channel.channel_name, logo_url, cache_id
                                );
                                total_logos_cached += 1;

                                // Update channel to use cached logo URL
                                let cached_logo_url = format!("/api/v1/logos/cached/{}", cache_id);
                                channel.tvg_logo = Some(cached_logo_url);
                            }
                            Err(e) => {
                                debug!(
                                    "Failed to cache logo for channel '{}' from '{}': {}",
                                    channel.channel_name, logo_url, e
                                );
                                // Keep original URL on failure
                            }
                        }
                    }
                }

                // TODO: In future, also access EPG data to cache program-specific logos
                // This would require integrating with OrderedEpgAggregateIterator
                // to get program information and cache logos for specific shows/movies
            }

            processed_channels.extend(chunk_channels);

            info!("Processed logo prefetch chunk: {} channels", chunk.len());
        }

        let stage_info = memory_context.complete_stage("logo_prefetch").await?;

        info!(
            "Native logo prefetch completed:\n\
             ├─ Implementation: LogoAssetService\n\
             ├─ Channels processed: {}\n\
             ├─ Logos cached: {} ({:.1}% success rate)\n\
             ├─ Chunk size used: {}\n\
             ├─ Memory used: {:.1}MB\n\
             ├─ Processing time: {}\n\
             ├─ Memory pressure: {:?}\n\
             └─ Note: EPG logo integration pending",
            processed_channels.len(),
            total_logos_cached,
            if processed_channels.is_empty() {
                0.0
            } else {
                (total_logos_cached as f64 / processed_channels.len() as f64) * 100.0
            },
            requested_chunk_size,
            stage_info.memory_delta_mb,
            crate::utils::format_duration(stage_info.duration_ms),
            stage_info.pressure_level
        );

        // Logo enrichment completed

        Ok(processed_channels)
    }

    /// Execute channel numbering stage with iterator-based processing
    async fn execute_channel_numbering_with_iterator(
        &self,
        channels: Vec<Channel>,
        config: &ResolvedProxyConfig,
        memory_context: &mut MemoryContext,
        _cleanup_coordinator: &mut MemoryCleanupCoordinator,
    ) -> Result<Vec<NumberedChannel>> {
        memory_context.start_stage("channel_numbering").await?;

        info!(
            "Starting channel numbering stage for {} channels",
            channels.len()
        );

        let _memory_pressure = self.get_current_memory_pressure(memory_context).await;

        // Request optimal chunk size from chunk manager
        let requested_chunk_size = self
            .chunk_manager
            .request_chunk_size("channel_numbering", channels.len().min(2000))
            .await?;

        info!(
            "Channel numbering using chunk size: {}",
            requested_chunk_size
        );

        // Using native channel numbering implementation

        // Native implementation using sequential numbering
        let mut numbered_channels = Vec::new();
        let starting_number = config.proxy.starting_channel_number;

        // Process channels in chunks for memory efficiency
        let mut current_number = starting_number;

        for chunk in channels.chunks(requested_chunk_size) {
            let mut chunk_numbered = Vec::new();

            for channel in chunk {
                let numbered_channel = NumberedChannel {
                    channel: channel.clone(),
                    assigned_number: current_number,
                    assignment_type: ChannelNumberAssignmentType::Sequential,
                };
                chunk_numbered.push(numbered_channel);
                current_number += 1;
            }

            numbered_channels.extend(chunk_numbered);
            info!(
                "Numbered chunk of {} channels (numbers {} to {})",
                chunk.len(),
                current_number - chunk.len() as i32,
                current_number - 1
            );
        }

        let stage_info = memory_context.complete_stage("channel_numbering").await?;

        info!(
            "Native channel numbering completed:\n\
             ├─ Implementation: Sequential numbering\n\
             ├─ Channels numbered: {}\n\
             ├─ Starting number: {}\n\
             ├─ Ending number: {}\n\
             ├─ Chunk size used: {}\n\
             ├─ Memory used: {:.1}MB\n\
             ├─ Processing time: {}ms\n\
             └─ Memory pressure: {:?}",
            numbered_channels.len(),
            starting_number,
            current_number - 1,
            requested_chunk_size,
            stage_info.memory_delta_mb,
            stage_info.duration_ms,
            stage_info.pressure_level
        );

        Ok(numbered_channels)
    }

    /// Execute M3U generation stage with iterator-based processing
    async fn execute_m3u_generation_with_iterator(
        &self,
        numbered_channels: Vec<NumberedChannel>,
        config: &ResolvedProxyConfig,
        base_url: &str,
        memory_context: &mut MemoryContext,
        _cleanup_coordinator: &mut MemoryCleanupCoordinator,
    ) -> Result<String> {
        memory_context.start_stage("m3u_generation").await?;

        info!(
            "Starting M3U generation stage for {} numbered channels",
            numbered_channels.len()
        );

        let _memory_pressure = self.get_current_memory_pressure(memory_context).await;

        // Request optimal chunk size from chunk manager
        let default_chunk_size = self.chunk_manager.get_chunk_size("m3u_generation").await;
        let requested_chunk_size = self
            .chunk_manager
            .request_chunk_size(
                "m3u_generation",
                numbered_channels.len().min(default_chunk_size),
            )
            .await?;

        info!("M3U generation using chunk size: {}", requested_chunk_size);

        // Using native M3U generation implementation
        // Native implementation follows

        // Native implementation using chunked M3U generation
        let mut m3u_content = String::from("#EXTM3U\n");
        let mut total_channels_processed = 0;

        // Process numbered channels in chunks for memory efficiency
        for chunk in numbered_channels.chunks(requested_chunk_size) {
            let mut chunk_content = String::new();

            for numbered_channel in chunk {
                let channel = &numbered_channel.channel;

                debug!(
                    "M3U Generation (Adaptive Native) - Channel #{}: id={}, channel_name='{}', tvg_name='{:?}', stream_url='{}'",
                    numbered_channel.assigned_number,
                    channel.id,
                    channel.channel_name,
                    channel.tvg_name,
                    channel.stream_url
                );

                // Build EXTINF line with all available metadata
                let mut extinf_parts = vec![format!("#EXTINF:-1")];

                // Add TVG attributes if available
                if let Some(tvg_id) = &channel.tvg_id {
                    if !tvg_id.is_empty() {
                        extinf_parts.push(format!("tvg-id=\"{}\"", tvg_id));
                    }
                }

                if let Some(tvg_name) = &channel.tvg_name {
                    if !tvg_name.is_empty() {
                        extinf_parts.push(format!("tvg-name=\"{}\"", tvg_name));
                    }
                }

                if let Some(tvg_logo) = &channel.tvg_logo {
                    if !tvg_logo.is_empty() {
                        extinf_parts.push(format!("tvg-logo=\"{}\"", tvg_logo));
                    }
                }

                if let Some(tvg_shift) = &channel.tvg_shift {
                    if !tvg_shift.is_empty() {
                        extinf_parts.push(format!("tvg-shift=\"{}\"", tvg_shift));
                    }
                }

                if let Some(group_title) = &channel.group_title {
                    if !group_title.is_empty() {
                        extinf_parts.push(format!("group-title=\"{}\"", group_title));
                    }
                }

                // Add channel number
                extinf_parts.push(format!("tvg-chno=\"{}\"", numbered_channel.assigned_number));

                // Join attributes with spaces
                let attributes = extinf_parts.join(" ");
                
                // Build complete EXTINF line with comma before channel name
                let complete_extinf = format!("{},{}", attributes, channel.channel_name);
                debug!(
                    "M3U Generation (Adaptive Native) - Generated EXTINF: '{}'",
                    complete_extinf
                );
                chunk_content.push_str(&complete_extinf);
                chunk_content.push('\n');

                // Add stream URL (always use proxy endpoints for consistency)
                let stream_url = format!("{}/stream/{}/{}", base_url, config.proxy.id, channel.id);
                chunk_content.push_str(&stream_url);
                chunk_content.push('\n');

                total_channels_processed += 1;
            }

            m3u_content.push_str(&chunk_content);
            info!(
                "Generated M3U chunk: {} channels, {} bytes",
                chunk.len(),
                chunk_content.len()
            );
        }

        let stage_info = memory_context.complete_stage("m3u_generation").await?;

        info!(
            "Native M3U generation completed:\n\
             ├─ Implementation: Native chunked generator\n\
             ├─ Channels processed: {}\n\
             ├─ M3U content size: {} bytes ({:.1} KB)\n\
             ├─ Avg bytes per channel: {:.1}\n\
             ├─ Chunk size used: {}\n\
             ├─ Memory used: {:.1}MB\n\
             ├─ Processing time: {}ms\n\
             └─ Memory pressure: {:?}",
            total_channels_processed,
            m3u_content.len(),
            m3u_content.len() as f64 / 1024.0,
            if total_channels_processed > 0 {
                m3u_content.len() as f64 / total_channels_processed as f64
            } else {
                0.0
            },
            requested_chunk_size,
            stage_info.memory_delta_mb,
            stage_info.duration_ms,
            stage_info.pressure_level
        );

        Ok(m3u_content)
    }

    /// Execute EPG processing stage using OrderedEpgAggregateIterator with native implementation
    async fn execute_epg_processing_with_iterator(
        &self,
        numbered_channels: Vec<NumberedChannel>,
        config: &ResolvedProxyConfig,
        _base_url: &str,
        memory_context: &mut MemoryContext,
        _cleanup_coordinator: &mut MemoryCleanupCoordinator,
    ) -> Result<String> {
        memory_context.start_stage("epg_processing").await?;

        info!(
            "Starting EPG processing stage for {} channels with {} EPG sources",
            numbered_channels.len(),
            config.epg_sources.len()
        );
        
        // Debug: Log each EPG source being used
        for (i, epg_source_config) in config.epg_sources.iter().enumerate() {
            info!(
                "EPG Source {}: {} (ID: {}, active: {}, priority: {})",
                i + 1,
                epg_source_config.epg_source.name,
                epg_source_config.epg_source.id,
                epg_source_config.epg_source.is_active,
                epg_source_config.priority_order
            );
        }

        let _memory_pressure = self.get_current_memory_pressure(memory_context).await;

        // Create OrderedEpgAggregateIterator for EPG data
        let epg_iterator = MultiSourceIterator::new(
            Arc::new(self.database.clone()),
            config.epg_sources.clone(),
            crate::pipeline::orchestrator::EpgLoader {},
            10000,
        );

        // Request optimal chunk size from chunk manager
        let default_chunk_size = self.chunk_manager.get_chunk_size("epg_processing").await;
        let requested_chunk_size = self
            .chunk_manager
            .request_chunk_size("epg_processing", default_chunk_size)
            .await?;

        info!("EPG processing using chunk size: {}", requested_chunk_size);

        // Using native EPG processing implementation

        // Use accumulator for EPG entries with file spilling for large datasets
        let file_manager = Arc::new(SandboxedManagerAdapter::new(self.temp_file_manager.clone()))
            as Arc<dyn SandboxedFileManager>;
        let mut epg_accumulator = IteratorAccumulator::new(
            AccumulationStrategy::default_hybrid(),
            file_manager,
        );

        info!("Loading EPG entries using accumulator with file spilling");

        // Accumulate all EPG entries from iterator
        epg_accumulator
            .accumulate_from_iterator(Box::new(epg_iterator))
            .await?;

        // Get stats before consuming the accumulator
        let epg_stats = epg_accumulator.get_stats();

        // Get completed EPG data
        let all_epg_entries = epg_accumulator.into_items().await?;

        info!(
            "EPG accumulation completed:\n\
             ├─ EPG entries loaded: {}\n\
             ├─ Strategy used: {:?}\n\
             ├─ Memory estimate: {:.1}MB\n\
             └─ Spilled to disk: {}",
            epg_stats.total_items,
            epg_stats.strategy,
            epg_stats.estimated_memory_mb,
            epg_stats.is_spilled
        );

        // Generate XMLTV content using the EPG generator
        let xmltv_content = if all_epg_entries.is_empty() {
            info!("No EPG entries found, generating minimal XMLTV");
            // Generate minimal XMLTV with just channel list
            let mut xmltv = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
            xmltv.push_str("<!DOCTYPE tv SYSTEM \"xmltv.dtd\">\n");
            xmltv.push_str("<tv>\n");

            // Add channels
            for numbered_channel in &numbered_channels {
                let channel = &numbered_channel.channel;
                if let Some(tvg_id) = &channel.tvg_id {
                    if !tvg_id.is_empty() {
                        xmltv.push_str(&format!(
                            "  <channel id=\"{}\">\n    <display-name>{}</display-name>\n  </channel>\n",
                            tvg_id, channel.channel_name
                        ));
                    }
                }
            }

            xmltv.push_str("</tv>\n");
            xmltv
        } else {
            info!(
                "Processing {} EPG entries for {} channels",
                all_epg_entries.len(),
                numbered_channels.len()
            );

            // Use EPG generator service to create XMLTV
            let epg_generator =
                crate::proxy::epg_generator::EpgGenerator::new(self.database.clone());

            // Extract channel IDs for EPG filtering
            let channel_ids: Vec<String> = numbered_channels
                .iter()
                .filter_map(|nc| nc.channel.tvg_id.clone())
                .filter(|id| !id.is_empty())
                .collect();

            // Use the new method that accepts resolved EPG sources instead of querying the database
            match epg_generator
                .generate_xmltv_with_resolved_sources(&config.proxy, &channel_ids, &config.epg_sources, None)
                .await
            {
                Ok((xmltv_content, stats)) => {
                    info!(
                        "EPG generation succeeded:\n\
                         ├─ Channels processed: {}\n\
                         ├─ Programs generated: {}\n\
                         ├─ XMLTV size: {} bytes\n\
                         └─ Duration: {}",
                        stats.matched_epg_channels,
                        stats.total_programs_after_filter,
                        xmltv_content.len(),
                        crate::utils::format_duration(stats.generation_time_ms)
                    );
                    xmltv_content
                }
                Err(e) => {
                    warn!("EPG generation failed: {}, using minimal XMLTV", e);
                    // Generate minimal XMLTV on error
                    format!(
                        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE tv SYSTEM \"xmltv.dtd\">\n<tv>\n<!-- EPG generation error: {} -->\n</tv>\n",
                        e
                    )
                }
            }
        };

        let stage_info = memory_context.complete_stage("epg_processing").await?;

        info!(
            "Native EPG processing completed:\n\
             ├─ Implementation: OrderedEpgAggregateIterator + EpgGenerator\n\
             ├─ EPG entries processed: {}\n\
             ├─ Channel count: {}\n\
             ├─ XMLTV content size: {} bytes ({:.1} KB)\n\
             ├─ Chunk size used: {}\n\
             ├─ Memory used: {:.1}MB\n\
             ├─ Processing time: {}ms\n\
             └─ Memory pressure: {:?}",
            all_epg_entries.len(),
            numbered_channels.len(),
            xmltv_content.len(),
            xmltv_content.len() as f64 / 1024.0,
            requested_chunk_size,
            stage_info.memory_delta_mb,
            stage_info.duration_ms,
            stage_info.pressure_level
        );

        Ok(xmltv_content)
    }

    // ==================== REUSABLE PIPELINE UTILITIES ====================

    /// Generic iterator processing with chunk management
    #[allow(dead_code)]
    async fn process_with_iterator<T, I>(
        &self,
        iterator: &mut I,
        stage_name: &str,
        chunk_size: usize,
    ) -> Result<Vec<T>>
    where
        I: crate::pipeline::iterator_traits::PipelineIterator<T>,
    {
        let mut all_items = Vec::new();

        info!(
            "{} using iterator with chunk size: {}",
            stage_name, chunk_size
        );

        loop {
            match iterator.next_chunk_with_size(chunk_size).await? {
                crate::pipeline::iterator_traits::IteratorResult::Chunk(chunk) => {
                    if chunk.is_empty() {
                        break;
                    }
                    info!(
                        "Loaded chunk of {} items from {} iterator",
                        chunk.len(),
                        stage_name
                    );
                    all_items.extend(chunk);
                }
                crate::pipeline::iterator_traits::IteratorResult::Exhausted => {
                    info!("{} iterator exhausted", stage_name);
                    break;
                }
            }
        }

        iterator.close().await?;
        Ok(all_items)
    }

    /// Generic chunk processing for memory efficiency
    #[allow(dead_code)]
    fn process_in_chunks<T, R, F>(
        &self,
        items: Vec<T>,
        chunk_size: usize,
        stage_name: &str,
        mut processor: F,
    ) -> Result<Vec<R>>
    where
        F: FnMut(&[T]) -> Result<Vec<R>>,
    {
        let mut results = Vec::new();

        for chunk in items.chunks(chunk_size) {
            let chunk_results = processor(chunk)?;
            results.extend(chunk_results);
            info!("Processed {} chunk: {} items", stage_name, chunk.len());
        }

        Ok(results)
    }

    /// Generic stage completion logging
    #[allow(dead_code)]
    fn log_stage_completion<T>(
        &self,
        stage_name: &str,
        implementation: &str,
        input_count: usize,
        output_count: usize,
        chunk_size: usize,
        stage_info: &crate::utils::StageMemoryInfo,
        extra_metrics: Option<&str>,
    ) {
        let mut log_msg = format!(
            "Native {} completed:\n\
             ├─ Implementation: {}\n\
             ├─ Items processed: {} → {}\n\
             ├─ Chunk size used: {}\n\
             ├─ Memory used: {:.1}MB\n\
             ├─ Processing time: {}ms\n\
             └─ Memory pressure: {:?}",
            stage_name,
            implementation,
            input_count,
            output_count,
            chunk_size,
            stage_info.memory_delta_mb,
            stage_info.duration_ms,
            stage_info.pressure_level
        );

        if let Some(extra) = extra_metrics {
            log_msg.push_str(&format!("\n{}", extra));
        }

        info!("{}", log_msg);
    }

    /// Create stage context for native execution
    #[allow(dead_code)]
    fn create_stage_context(
        &self,
        stage_name: &str,
        config: &ResolvedProxyConfig,
        base_url: &str,
        memory_pressure: MemoryPressureLevel,
        _channels: Option<Vec<Channel>>, // Unused parameter, kept for compatibility
    ) -> StageContext {
        StageContext {
            proxy_config: config.clone(),
            output: GenerationOutput::Production {
                file_manager: self.temp_file_manager.clone(),
                update_database: true,
            },
            base_url: base_url.to_string(),
            engine_config: None,
            memory_pressure,
            available_memory_mb: Some(512),
            current_stage: stage_name.to_string(),
            stats: GenerationStats::new("unified_iterator_native".to_string()),
            database: Some(Arc::new(self.database.clone())),
            logo_service: Some(Arc::new(self.logo_service.clone())),
            iterator_registry: Some(self.iterator_registry.clone()),
        }
    }

    /// Create temporary stage context for cases where config is not available
    fn create_temp_stage_context(
        &self,
        stage_name: &str,
        memory_pressure: MemoryPressureLevel,
    ) -> StageContext {
        StageContext {
            proxy_config: ResolvedProxyConfig {
                proxy: StreamProxy {
                    id: uuid::Uuid::new_v4(),
                    name: "temp".to_string(),
                    description: None,
                    created_at: chrono::Utc::now(),
                    updated_at: chrono::Utc::now(),
                    last_generated_at: None,
                    is_active: true,
                    auto_regenerate: false,
                    proxy_mode: crate::models::StreamProxyMode::Proxy,
                    upstream_timeout: None,
                    buffer_size: None,
                    max_concurrent_streams: None,
                    starting_channel_number: 1,
                    cache_channel_logos: true, // Default value, field was added later
                    cache_program_logos: false, // Default value, field was added later
                    relay_profile_id: None, // Not used for temp contexts
                },
                sources: vec![],
                epg_sources: vec![],
                filters: vec![],
            },
            output: GenerationOutput::Production {
                file_manager: self.temp_file_manager.clone(),
                update_database: true,
            },
            base_url: "http://localhost".to_string(),
            engine_config: None,
            memory_pressure,
            available_memory_mb: Some(512),
            current_stage: stage_name.to_string(),
            stats: GenerationStats::new("unified_iterator_native".to_string()),
            database: Some(Arc::new(self.database.clone())),
            logo_service: Some(Arc::new(self.logo_service.clone())),
            iterator_registry: Some(self.iterator_registry.clone()),
        }
    }

    /// Execute source loading stage with strategy selection
    async fn execute_source_loading_stage(
        &self,
        source_ids: Vec<uuid::Uuid>,
        memory_context: &mut MemoryContext,
        cleanup_coordinator: &mut MemoryCleanupCoordinator,
    ) -> Result<Vec<Channel>> {
        memory_context.start_stage("source_loading").await?;

        let _memory_pressure = self.get_current_memory_pressure(memory_context).await;

        // Using native source loading implementation

        // Use simple strategy (default)
        let strategy = SimpleSourceLoader::new(self.database.clone());
        let input = SourceLoadingInput {
            source_ids,
            proxy_config: ResolvedProxyConfig {
                proxy: StreamProxy {
                    id: uuid::Uuid::new_v4(),
                    name: "Temporary Proxy".to_string(),
                    description: None,
                    created_at: chrono::Utc::now(),
                    updated_at: chrono::Utc::now(),
                    last_generated_at: None,
                    is_active: true,
                    auto_regenerate: false,
                    proxy_mode: StreamProxyMode::Proxy,
                    upstream_timeout: None,
                    buffer_size: None,
                    max_concurrent_streams: None,
                    starting_channel_number: 1,
                    cache_channel_logos: true, // Default value, field was added later
                    cache_program_logos: false, // Default value, field was added later
                    relay_profile_id: None, // Not used for temporary proxies
                },
                sources: Vec::new(),
                filters: Vec::new(),
                epg_sources: Vec::new(),
            },
        };

        let mut output = strategy.execute(input, memory_context).await?;
        let stage_info = memory_context.complete_stage("source_loading").await?;

        info!(
            "Source loading completed: {} channels, {:.1}MB memory used, {:?} pressure",
            output.len(),
            stage_info.memory_delta_mb,
            stage_info.pressure_level
        );

        // Cleanup if needed
        if memory_context.should_cleanup().await? {
            cleanup_coordinator.cleanup_between_stages(
                "source_loading",
                &mut output,
                crate::utils::CleanupStrategy::Basic,
            )?;
        }

        Ok(output)
    }

    /// Execute data mapping stage with strategy selection
    async fn execute_data_mapping_stage(
        &self,
        channels: Vec<Channel>,
        config: &ResolvedProxyConfig,
        base_url: &str,
        engine_config: Option<crate::config::DataMappingEngineConfig>,
        memory_context: &mut MemoryContext,
        cleanup_coordinator: &mut MemoryCleanupCoordinator,
    ) -> Result<Vec<Channel>> {
        memory_context.start_stage("data_mapping").await?;

        let _memory_pressure = self.get_current_memory_pressure(memory_context).await;

        // Using native data mapping implementation

        // Use simple strategy (default)
        let strategy =
            SimpleDataMapper::new(self.data_mapping_service.clone(), self.logo_service.clone());
        let input = DataMappingInput {
            channels,
            proxy_config: config.clone(),
            engine_config,
            base_url: base_url.to_string(),
        };

        let mut output = strategy.execute(input, memory_context).await?;
        let stage_info = memory_context.complete_stage("data_mapping").await?;

        info!(
            "Data mapping completed: {} channels, {:.1}MB memory used",
            output.len(),
            stage_info.memory_delta_mb
        );

        // Cleanup if needed
        if memory_context.should_cleanup().await? {
            cleanup_coordinator.cleanup_between_stages(
                "data_mapping",
                &mut output,
                crate::utils::CleanupStrategy::Basic,
            )?;
        }

        Ok(output)
    }

    /// Execute filtering stage with strategy selection
    async fn execute_filtering_stage(
        &self,
        channels: Vec<Channel>,
        config: &ResolvedProxyConfig,
        memory_context: &mut MemoryContext,
        cleanup_coordinator: &mut MemoryCleanupCoordinator,
    ) -> Result<Vec<Channel>> {
        memory_context.start_stage("filtering").await?;

        let _memory_pressure = self.get_current_memory_pressure(memory_context).await;

        // Using native filtering implementation

        // Use simple strategy (default)
        let strategy = SimpleFilter;
        // Convert ProxyFilterConfig to (Filter, ProxyFilter) tuples
        let filters: Vec<(Filter, ProxyFilter)> = config.filters.iter().map(|filter_config| {
            let proxy_filter = ProxyFilter {
                proxy_id: config.proxy.id,
                filter_id: filter_config.filter.id,
                priority_order: filter_config.priority_order,
                is_active: filter_config.is_active,
                created_at: chrono::Utc::now(),
            };
            (filter_config.filter.clone(), proxy_filter)
        }).collect();
        
        let input = FilteringInput {
            channels,
            filters,
        };

        let mut output = strategy.execute(input, memory_context).await?;
        let stage_info = memory_context.complete_stage("filtering").await?;

        info!(
            "Filtering completed: {} channels, {:.1}MB memory used",
            output.len(),
            stage_info.memory_delta_mb
        );

        // Cleanup if needed
        if memory_context.should_cleanup().await? {
            cleanup_coordinator.cleanup_between_stages(
                "filtering",
                &mut output,
                crate::utils::CleanupStrategy::Basic,
            )?;
        }

        Ok(output)
    }

    /// Execute channel numbering stage with strategy selection
    async fn execute_channel_numbering_stage(
        &self,
        channels: Vec<Channel>,
        config: &ResolvedProxyConfig,
        memory_context: &mut MemoryContext,
        cleanup_coordinator: &mut MemoryCleanupCoordinator,
    ) -> Result<Vec<NumberedChannel>> {
        memory_context.start_stage("channel_numbering").await?;

        let _memory_pressure = self.get_current_memory_pressure(memory_context).await;

        // Using native channel numbering implementation

        // Use simple strategy (default)
        let strategy = SimpleChannelNumbering;
        let input = ChannelNumberingInput {
            channels,
            starting_number: config.proxy.starting_channel_number,
            numbering_strategy: ChannelNumberingStrategy::Sequential,
        };

        let mut output = strategy.execute(input, memory_context).await?;
        let stage_info = memory_context.complete_stage("channel_numbering").await?;

        info!(
            "Channel numbering completed: {} channels, {:.1}MB memory used",
            output.len(),
            stage_info.memory_delta_mb
        );

        // Cleanup if needed
        if memory_context.should_cleanup().await? {
            cleanup_coordinator.cleanup_between_stages(
                "channel_numbering",
                &mut output,
                crate::utils::CleanupStrategy::Basic,
            )?;
        }

        Ok(output)
    }

    /// Get current memory pressure level
    async fn get_current_memory_pressure(&self, memory_context: &MemoryContext) -> MemoryPressureLevel {
        if let Some(ref monitor) = self.memory_monitor {
            match monitor.check_memory_limit().await {
                Ok(crate::utils::MemoryLimitStatus::Ok) => MemoryPressureLevel::Optimal,
                Ok(crate::utils::MemoryLimitStatus::Warning) => MemoryPressureLevel::High,
                Ok(crate::utils::MemoryLimitStatus::Exceeded) => MemoryPressureLevel::Critical,
                Err(_) => MemoryPressureLevel::Emergency,
            }
        } else {
            // Base assessment on memory context growth
            let stats = memory_context.get_memory_statistics();
            let growth_ratio = (stats.peak_mb - stats.baseline_mb) / stats.baseline_mb;

            if growth_ratio > 2.0 {
                MemoryPressureLevel::Critical
            } else if growth_ratio > 1.5 {
                MemoryPressureLevel::High
            } else if growth_ratio > 1.0 {
                MemoryPressureLevel::Moderate
            } else {
                MemoryPressureLevel::Optimal
            }
        }
    }
}

/// Builder for creating adaptive pipelines
pub struct NativePipelineBuilder {
    database: Option<Database>,
    data_mapping_service: Option<DataMappingService>,
    logo_service: Option<LogoAssetService>,
    memory_limit_mb: Option<usize>,
    temp_file_manager: Option<SandboxedManager>,
    shared_memory_monitor: Option<SimpleMemoryMonitor>,
    system: Option<Arc<tokio::sync::RwLock<sysinfo::System>>>,
}

impl NativePipelineBuilder {
    pub fn new() -> Self {
        Self {
            database: None,
            data_mapping_service: None,
            logo_service: None,
            memory_limit_mb: None,
            temp_file_manager: None,
            shared_memory_monitor: None,
            system: None,
        }
    }

    pub fn with_database(mut self, database: Database) -> Self {
        self.database = Some(database);
        self
    }

    pub fn with_data_mapping_service(mut self, service: DataMappingService) -> Self {
        self.data_mapping_service = Some(service);
        self
    }

    pub fn with_logo_service(mut self, service: LogoAssetService) -> Self {
        self.logo_service = Some(service);
        self
    }

    pub fn with_memory_limit(mut self, limit_mb: usize) -> Self {
        self.memory_limit_mb = Some(limit_mb);
        self
    }

    pub fn with_temp_file_manager(mut self, manager: SandboxedManager) -> Self {
        self.temp_file_manager = Some(manager);
        self
    }

    pub fn with_shared_memory_monitor(mut self, memory_monitor: SimpleMemoryMonitor) -> Self {
        self.shared_memory_monitor = Some(memory_monitor);
        self
    }

    pub fn with_system(mut self, system: Arc<tokio::sync::RwLock<sysinfo::System>>) -> Self {
        self.system = Some(system);
        self
    }

    pub fn build(self) -> Result<NativePipeline> {
        Ok(NativePipeline::new(
            self.database.unwrap(),
            self.data_mapping_service.unwrap(),
            self.logo_service.unwrap(),
            self.memory_limit_mb,
            self.temp_file_manager.unwrap(),
            self.shared_memory_monitor,
            self.system.unwrap(),
        ))
    }
}

impl Default for NativePipelineBuilder {
    fn default() -> Self {
        Self::new()
    }
}
