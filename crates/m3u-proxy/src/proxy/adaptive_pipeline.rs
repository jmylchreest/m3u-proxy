//! Unified Iterator-Based Pipeline
//!
//! This pipeline implements the complete 7-stage processing pipeline using orchestrator
//! iterators, chunk management, and sophisticated buffering. It provides plugin integration
//! with fallback to native implementations and coordinates memory usage across all stages.

use anyhow::Result;
use std::sync::Arc;
use std::time::Instant;
use tracing::{debug, info, warn};

use crate::config::{create_plugin_resolver, PluginResolver};
use crate::data_mapping::service::DataMappingService;
use crate::database::Database;
use crate::logo_assets::service::LogoAssetService;
use crate::models::*;
use crate::pipeline::chunk_manager::ChunkSizeManager;
use crate::pipeline::orchestrator::OrchestratorIteratorFactory;
use crate::pipeline::rolling_buffer_iterator::BufferConfig;
use crate::pipeline::generic_iterator::{MultiSourceIterator, SingleSourceIterator};
use crate::pipeline::iterator_traits::PipelineIterator;
use crate::pipeline::{IteratorRegistry, ImmutableLogoEnrichedChannelSource, IteratorType, AccumulatorFactory, AccumulationStrategy};
use crate::services::sandboxed_file::SandboxedFileManager;
use crate::proxy::simple_strategies::*;
use crate::proxy::stage_contracts::*;
use crate::proxy::stage_strategy::{MemoryPressureLevel, StageContext};
use crate::proxy::wasm_host_interface::WasmHostInterface;
use crate::plugins::pipeline::wasm::{WasmPluginConfig, WasmPluginManager};
use crate::utils::{MemoryCleanupCoordinator, MemoryContext, SimpleMemoryMonitor};
use sandboxed_file_manager::SandboxedManager;

/// Unified iterator-based pipeline with sophisticated buffering and plugin integration
pub struct AdaptivePipeline {
    database: Database,
    data_mapping_service: DataMappingService,
    logo_service: LogoAssetService,
    memory_monitor: Option<SimpleMemoryMonitor>,
    temp_file_manager: SandboxedManager,
    plugin_manager: Arc<WasmPluginManager>,
    chunk_manager: Arc<ChunkSizeManager>,
    memory_limit_mb: Option<usize>,
    plugin_resolver: Option<PluginResolver>,
    iterator_registry: Arc<IteratorRegistry>,
}

impl AdaptivePipeline {
    pub fn new(
        database: Database,
        data_mapping_service: DataMappingService,
        logo_service: LogoAssetService,
        memory_limit_mb: Option<usize>,
        temp_file_manager: SandboxedManager,
        shared_plugin_manager: Option<Arc<WasmPluginManager>>,
        shared_memory_monitor: Option<SimpleMemoryMonitor>,
    ) -> Self {
        let memory_monitor = shared_memory_monitor.or_else(|| {
            memory_limit_mb.map(|limit| SimpleMemoryMonitor::new(Some(limit)))
        });

        // Use shared plugin manager if provided, otherwise create a minimal disabled manager
        let plugin_manager = if let Some(shared_manager) = shared_plugin_manager {
            tracing::debug!("AdaptivePipeline using shared plugin manager");
            shared_manager
        } else {
            tracing::debug!("AdaptivePipeline: No shared plugin manager provided, WASM plugins will be disabled");
            // Create a minimal disabled plugin manager to avoid duplicates
            let plugin_config = WasmPluginConfig {
                enabled: false,
                plugin_directory: String::new(),
                max_memory_per_plugin: 0,
                timeout_seconds: 1,
                enable_hot_reload: false,
                max_plugin_failures: 0,
                fallback_timeout_ms: 0,
            };
            let host_interface = WasmHostInterface::new(
                temp_file_manager.clone(),
                memory_monitor.clone(),
                false, // network disabled by default
            );
            Arc::new(WasmPluginManager::new(plugin_config, host_interface))
        };

        // Initialize chunk manager with stage dependencies
        let chunk_manager = Arc::new(ChunkSizeManager::new(
            1000, // default_chunk_size
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
            plugin_manager,
            chunk_manager,
            memory_limit_mb,
            plugin_resolver: None, // Will be set when processing with config
            iterator_registry,
        }
    }

    /// Set the plugin resolver from configuration
    pub fn set_plugin_resolver(&mut self, config: &crate::config::Config) {
        self.plugin_resolver = Some(create_plugin_resolver(config));
        info!("Plugin resolver initialized from configuration");
    }

    /// Check if a plugin should be used for a specific stage based on configuration
    /// If true, falls back to the plugin manager's existing resolution logic
    fn should_use_plugin_for_stage(&self, stage: &str) -> bool {
        // Check if we have a plugin resolver (configuration-driven)
        let Some(resolver) = self.plugin_resolver.as_ref() else {
            debug!("No plugin resolver configured, using native implementation for stage '{}'", stage);
            return false;
        };
        
        // Check if configuration specifies a plugin for this stage
        if let Some(plugin_name) = resolver.get_plugin_for_stage(stage) {
            info!("Configuration specifies plugin '{}' for stage '{}' - will attempt plugin resolution", plugin_name, stage);
            true
        } else {
            info!("No plugin configured for stage '{}', using native implementation", stage);
            false
        }
    }

    /// Check if the configured plugin has support for this stage and is available
    /// Returns the plugin instance if available and supports the stage, None otherwise
    fn get_configured_plugin_for_stage(&self, stage: &str, memory_pressure: MemoryPressureLevel) -> Option<Box<dyn crate::proxy::stage_strategy::StageStrategy + Send + Sync>> {
        // Check if we have a plugin resolver (configuration-driven)
        let Some(resolver) = self.plugin_resolver.as_ref() else {
            debug!("No plugin resolver configured for stage '{}'", stage);
            return None;
        };
        
        // Get the configured plugin name for this stage
        let Some(plugin_name) = resolver.get_plugin_for_stage(stage) else {
            debug!("No plugin configured for stage '{}'", stage);
            return None;
        };
        
        // Try to get the specific plugin by name
        let plugin = self.plugin_manager.get_plugin_by_name_for_stage(plugin_name, stage, memory_pressure);
        
        if plugin.is_some() {
            info!("Using configured plugin '{}' for stage '{}'", plugin_name, stage);
        } else {
            warn!("Configured plugin '{}' for stage '{}' is not available or doesn't support the stage", plugin_name, stage);
        }
        
        plugin
    }

    /// Initialize the pipeline with plugin loading and chunk manager setup
    pub async fn initialize(&mut self) -> Result<()> {
        info!("Initializing unified iterator-based pipeline with WASM plugin support");

        // Initialize chunk manager
        info!("Initializing chunk size manager with 6-stage dependencies");
        
        // Check if plugins are already loaded (shared manager case)
        let plugin_count = self.plugin_manager.get_loaded_plugin_count();
        if plugin_count > 0 {
            info!("Using shared plugin manager with {} already loaded plugins", plugin_count);
        } else {
            // Only load plugins if the manager appears to be enabled (has plugin directory)
            // We check if this is the disabled fallback manager by seeing if plugin directory is empty
            if self.plugin_manager.get_plugin_directory().is_empty() {
                info!("WASM plugin system is disabled (fallback manager), using native implementations only");
            } else {
                // Load WASM plugins if not already loaded
                if let Err(e) = self.plugin_manager.load_plugins().await {
                    warn!(
                        "Failed to load WASM plugins: {}, continuing with native implementations",
                        e
                    );
                } else {
                    info!("WASM plugins loaded successfully");
                }
            }
        }

        // Log pipeline status
        let health_status = self.plugin_manager.health_check();
        let healthy_plugins = health_status
            .iter()
            .filter(|(_, healthy)| **healthy)
            .count();
        info!(
            "Unified pipeline initialized: {} healthy plugins, {} total plugins, chunk manager ready",
            healthy_plugins,
            health_status.len()
        );

        Ok(())
    }

    /// Generate proxy using unified iterator-based pipeline with all 7 stages
    pub async fn generate_with_dynamic_strategies(
        &mut self,
        config: ResolvedProxyConfig,
        base_url: &str,
        engine_config: Option<crate::config::DataMappingEngineConfig>,
        app_config: &crate::config::Config,
    ) -> Result<ProxyGeneration> {
        // Initialize plugin resolver from configuration
        self.set_plugin_resolver(app_config);
        info!(
            "Starting unified pipeline generation proxy={} sources={} filters={}",
            config.proxy.name,
            config.sources.len(),
            config.filters.len()
        );

        let overall_start = Instant::now();

        // Initialize memory context for tracking
        let mut memory_context = MemoryContext::new(self.memory_limit_mb, None);
        memory_context.initialize()?;

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
            "Unified pipeline completed proxy={} total_time={}ms channels={} pipeline=iterator-based",
            config.proxy.name, total_duration, generation.channel_count
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
        info!("Loading channels using RollingBufferChannelIterator for {} sources", config.sources.len());
        
        let memory_pressure = self.get_current_memory_pressure(memory_context);
        
        // Request optimal chunk size from chunk manager first
        let requested_chunk_size = self.chunk_manager
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

        let mut channel_iterator = OrchestratorIteratorFactory::create_rolling_buffer_channel_iterator_from_configs_with_cascade(
            Arc::new(self.database.clone()),
            config.proxy.id,
            config.sources.clone(),
            buffer_config,
            Some(self.chunk_manager.clone()),
            "data_loading".to_string(),
        );

        // Load all channels using native rolling buffer iterator
        let mut channels = Vec::new();
        
        loop {
            match channel_iterator.next_chunk_with_size(requested_chunk_size).await? {
                crate::pipeline::iterator_traits::IteratorResult::Chunk(chunk) => {
                    if chunk.is_empty() {
                        break;
                    }
                    info!("Loaded chunk of {} channels from iterator", chunk.len());
                    
                    // Debug: Log first few channels to see the data structure
                    if !chunk.is_empty() && channels.is_empty() {
                        for (i, channel) in chunk.iter().take(3).enumerate() {
                            debug!(
                                "Channel #{}: id={}, name='{}', stream_url='{}', tvg_logo='{:?}', group_title='{:?}'",
                                i + 1,
                                channel.id,
                                channel.channel_name,
                                channel.stream_url,
                                channel.tvg_logo,
                                channel.group_title
                            );
                        }
                    }
                    
                    channels.extend(chunk);
                }
                crate::pipeline::iterator_traits::IteratorResult::Exhausted => {
                    info!("Channel iterator exhausted");
                    break;
                }
            }
        }

        // Close iterator and cleanup
        channel_iterator.close().await?;
        
        info!(
            "Data loading completed: {} channels loaded",
            channels.len()
        );

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
        };

        Ok(generation)
    }


    /// Execute data mapping stage using OrderedDataMappingIterator with plugin/native fallback
    async fn execute_data_mapping_with_iterator(
        &self,
        channels: Vec<Channel>,
        config: &ResolvedProxyConfig,
        base_url: &str,
        engine_config: Option<crate::config::DataMappingEngineConfig>,
        memory_context: &mut MemoryContext,
        cleanup_coordinator: &mut MemoryCleanupCoordinator,
    ) -> Result<Vec<Channel>> {
        memory_context.start_stage("data_mapping")?;
        
        info!("Starting data mapping stage with OrderedDataMappingIterator for {} channels", channels.len());

        let memory_pressure = self.get_current_memory_pressure(memory_context);

        // Create OrderedDataMappingIterator for mapping rules
        let mut mapping_iterator = SingleSourceIterator::new(
            Arc::new(self.database.clone()),
            crate::pipeline::orchestrator::DataMappingLoader {},
            config.proxy.id,
            1000,
        );

        // Request optimal chunk size from chunk manager (coordinates with upstream)
        let default_chunk_size = self.chunk_manager.get_chunk_size("data_mapping").await;
        let requested_chunk_size = self.chunk_manager
            .request_chunk_size("data_mapping", channels.len().min(default_chunk_size))
            .await?;

        info!("Data mapping using chunk size: {}", requested_chunk_size);

        // Try WASM plugin first if available
        if self.should_use_plugin_for_stage("data_mapping") {
            if let Some(plugin) = self.get_configured_plugin_for_stage("data_mapping", memory_pressure) {
            let stage_context = self.create_temp_stage_context("data_mapping", memory_pressure);

            match plugin.execute_data_mapping(&stage_context, channels.clone()).await {
                Ok(mapped_channels) => {
                    let stage_info = memory_context.complete_stage("data_mapping")?;
                    
                    // Check if plugin returned empty results - fall back to native iterator
                    if mapped_channels.is_empty() && !channels.is_empty() {
                        warn!(
                            "WASM plugin returned empty results, falling back to native iterator:\n\
                             ├─ Plugin: {} v{}\n\
                             ├─ Channels processed: {} → {}\n\
                             ├─ Memory used: {:.1}MB\n\
                             ├─ Processing time: {}ms\n\
                             ├─ Memory pressure: {:?}\n\
                             └─ Fallback: Using OrderedDataMappingIterator",
                            plugin.get_info().name,
                            plugin.get_info().version,
                            channels.len(),
                            mapped_channels.len(),
                            stage_info.memory_delta_mb,
                            stage_info.duration_ms,
                            stage_info.pressure_level
                        );
                        
                        // Continue to native fallback below
                    } else {
                        info!(
                            "WASM plugin data mapping succeeded:\n\
                             ├─ Plugin: {} v{}\n\
                             ├─ Channels processed: {} → {}\n\
                             ├─ Memory used: {:.1}MB\n\
                             ├─ Processing time: {}ms\n\
                             └─ Memory pressure: {:?}",
                            plugin.get_info().name,
                            plugin.get_info().version,
                            channels.len(),
                            mapped_channels.len(),
                            stage_info.memory_delta_mb,
                            stage_info.duration_ms,
                            stage_info.pressure_level
                        );
                        
                        return Ok(mapped_channels);
                    }
                }
                Err(e) => {
                    warn!(
                        "WASM plugin data mapping failed, falling back to native iterator:\n\
                         ├─ Plugin: {}\n\
                         ├─ Error: {}\n\
                         ├─ Stage: data_mapping\n\
                         ├─ Fallback: Using OrderedDataMappingIterator\n\
                         └─ Reason: Plugin execution error",
                        plugin.get_info().name,
                        e
                    );
                }
            }
        } else {
            info!(
                "No WASM plugin available for data_mapping stage:\n\
                 ├─ Memory pressure: {:?}\n\
                 ├─ Available plugins: {}\n\
                 └─ Using: OrderedDataMappingIterator + DataMappingService",
                memory_pressure,
                self.plugin_manager.get_loaded_plugin_count()
            );
        }
        } // Close the should_use_plugin_for_stage("data_mapping") block

        // Native implementation using OrderedDataMappingIterator + DataMappingService
        let mut all_mapping_rules = Vec::new();
        
        // Load all mapping rules via iterator
        loop {
            match mapping_iterator.next_chunk_with_size(requested_chunk_size).await? {
                crate::pipeline::iterator_traits::IteratorResult::Chunk(chunk) => {
                    if chunk.is_empty() {
                        break;
                    }
                    info!("Loaded chunk of {} mapping rules from iterator", chunk.len());
                    all_mapping_rules.extend(chunk);
                }
                crate::pipeline::iterator_traits::IteratorResult::Exhausted => {
                    info!("Data mapping iterator exhausted");
                    break;
                }
            }
        }

        // Close mapping iterator
        mapping_iterator.close().await?;

        // Apply data mapping using the service with loaded rules
        let original_count = channels.len();
        let mapped_channels = if all_mapping_rules.is_empty() {
            info!("No mapping rules found, channels passed through unchanged");
            channels
        } else {
            info!("Applying {} mapping rules to {} channels", all_mapping_rules.len(), channels.len());
            
            // Use DataMappingService to apply the mapping rules
            self.data_mapping_service
                .apply_mapping_for_proxy(channels.clone(), config.proxy.id, &self.logo_service, base_url, engine_config)
                .await?
        };

        let stage_info = memory_context.complete_stage("data_mapping")?;
        
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

    /// Execute filtering stage using OrderedFilterIterator with plugin/native fallback
    async fn execute_filtering_with_iterator(
        &self,
        channels: Vec<Channel>,
        config: &ResolvedProxyConfig,
        memory_context: &mut MemoryContext,
        cleanup_coordinator: &mut MemoryCleanupCoordinator,
    ) -> Result<Vec<Channel>> {
        memory_context.start_stage("filtering")?;
        
        info!("Starting filtering stage with OrderedFilterIterator for {} channels", channels.len());

        let memory_pressure = self.get_current_memory_pressure(memory_context);

        // Create OrderedFilterIterator for filter rules
        let mut filter_iterator = MultiSourceIterator::new(
            Arc::new(self.database.clone()),
            config.filters.clone(),
            crate::pipeline::orchestrator::FilterLoader {},
            1000,
        );

        // Request optimal chunk size from chunk manager (coordinates with upstream)
        let default_chunk_size = self.chunk_manager.get_chunk_size("filtering").await;
        let requested_chunk_size = self.chunk_manager
            .request_chunk_size("filtering", channels.len().min(default_chunk_size))
            .await?;

        info!("Filtering using chunk size: {}", requested_chunk_size);

        // Try WASM plugin first if available
        if self.should_use_plugin_for_stage("filtering") {
            if let Some(plugin) = self
                .plugin_manager
                .get_plugin_for_stage("filtering", memory_pressure)
            {
            let stage_context = self.create_temp_stage_context("filtering", memory_pressure);

            match plugin.execute_filtering(&stage_context, channels.clone()).await {
                Ok(filtered_channels) => {
                    let stage_info = memory_context.complete_stage("filtering")?;
                    
                    // Check if plugin returned empty results - fall back to native iterator
                    if filtered_channels.is_empty() && !channels.is_empty() {
                        warn!(
                            "WASM plugin returned empty results, falling back to native iterator:\n\
                             ├─ Plugin: {} v{}\n\
                             ├─ Channels filtered: {} → {}\n\
                             ├─ Memory used: {:.1}MB\n\
                             ├─ Processing time: {}ms\n\
                             ├─ Memory pressure: {:?}\n\
                             └─ Fallback: Using OrderedFilterIterator",
                            plugin.get_info().name,
                            plugin.get_info().version,
                            channels.len(),
                            filtered_channels.len(),
                            stage_info.memory_delta_mb,
                            stage_info.duration_ms,
                            stage_info.pressure_level
                        );
                        
                        // Continue to native fallback below
                    } else {
                        info!(
                            "WASM plugin filtering succeeded:\n\
                             ├─ Plugin: {} v{}\n\
                             ├─ Channels filtered: {} → {}\n\
                             ├─ Memory used: {:.1}MB\n\
                             ├─ Processing time: {}ms\n\
                             └─ Memory pressure: {:?}",
                            plugin.get_info().name,
                            plugin.get_info().version,
                            channels.len(),
                            filtered_channels.len(),
                            stage_info.memory_delta_mb,
                            stage_info.duration_ms,
                            stage_info.pressure_level
                        );
                        
                        return Ok(filtered_channels);
                    }
                }
                Err(e) => {
                    warn!(
                        "WASM plugin filtering failed, falling back to native iterator:\n\
                         ├─ Plugin: {}\n\
                         ├─ Error: {}\n\
                         ├─ Stage: filtering\n\
                         ├─ Fallback: Using OrderedFilterIterator\n\
                         └─ Reason: Plugin execution error",
                        plugin.get_info().name,
                        e
                    );
                }
            }
        } else {
            info!(
                "No WASM plugin available for filtering stage:\n\
                 ├─ Memory pressure: {:?}\n\
                 ├─ Available plugins: {}\n\
                 └─ Using: OrderedFilterIterator + FilterEngine",
                memory_pressure,
                self.plugin_manager.get_loaded_plugin_count()
            );
        }
        } // Close the should_use_plugin_for_stage("filtering") block

        // Native implementation using direct filter configuration + FilterEngine
        // Convert filter configs to (Filter, ProxyFilter) tuples for FilterEngine
        let filter_tuples: Vec<(Filter, ProxyFilter)> = config.filters
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
            info!("Applying {} filter rules to {} channels", filter_tuples.len(), channels.len());
            
            // Use FilterEngine to apply the filter rules
            let mut filter_engine = crate::proxy::filter_engine::FilterEngine::new();
            let result = filter_engine.apply_filters(channels, filter_tuples.clone()).await?;
            
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

        let stage_info = memory_context.complete_stage("filtering")?;
        
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
            if input_channel_count == 0 { 0.0 } else { (filtered_channels.len() as f64 / input_channel_count as f64) * 100.0 },
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
        config: &ResolvedProxyConfig,
        memory_context: &mut MemoryContext,
        cleanup_coordinator: &mut MemoryCleanupCoordinator,
    ) -> Result<Vec<Channel>> {
        memory_context.start_stage("logo_prefetch")?;
        
        info!("Starting logo prefetch stage for {} channels (with EPG access)", channels.len());

        let memory_pressure = self.get_current_memory_pressure(memory_context);

        // Request optimal chunk size from chunk manager
        let default_chunk_size = self.chunk_manager.get_chunk_size("logo_prefetch").await;
        let requested_chunk_size = self.chunk_manager
            .request_chunk_size("logo_prefetch", channels.len().min(default_chunk_size))
            .await?;

        info!("Logo prefetch using chunk size: {}", requested_chunk_size);

        // Try WASM plugin first if available
        if self.should_use_plugin_for_stage("logo_prefetch") {
            if let Some(plugin) = self
                .plugin_manager
                .get_plugin_for_stage("logo_prefetch", memory_pressure)
            {
            let stage_context = self.create_temp_stage_context("logo_prefetch", memory_pressure);

            // Execute logo prefetch with the plugin
            match plugin.execute_logo_prefetch(&stage_context, channels.clone()).await {
                Ok(processed_channels) => {
                    let stage_info = memory_context.complete_stage("logo_prefetch")?;
                    
                    // Check if plugin returned empty results - fall back to native
                    if processed_channels.is_empty() && !channels.is_empty() {
                        warn!(
                            "WASM plugin returned empty results, falling back to native logo service:\n\
                             ├─ Plugin: {} v{}\n\
                             ├─ Channels processed: {} → {}\n\
                             ├─ Memory used: {:.1}MB\n\
                             ├─ Processing time: {}ms\n\
                             ├─ Memory pressure: {:?}\n\
                             └─ Fallback: Using LogoAssetService",
                            plugin.get_info().name,
                            plugin.get_info().version,
                            channels.len(),
                            processed_channels.len(),
                            stage_info.memory_delta_mb,
                            stage_info.duration_ms,
                            stage_info.pressure_level
                        );
                        
                        // Continue to native fallback below
                    } else {
                        info!(
                            "WASM plugin logo prefetch succeeded:\n\
                             ├─ Plugin: {} v{}\n\
                             ├─ Channels processed: {} → {}\n\
                             ├─ Memory used: {:.1}MB\n\
                             ├─ Processing time: {}ms\n\
                             └─ Memory pressure: {:?}",
                            plugin.get_info().name,
                            plugin.get_info().version,
                            channels.len(),
                            processed_channels.len(),
                            stage_info.memory_delta_mb,
                            stage_info.duration_ms,
                            stage_info.pressure_level
                        );
                        
                        // Register logo-enriched channels in iterator registry for multi-instance access
                        if let Err(e) = self.register_logo_enriched_channels(&processed_channels).await {
                            warn!("Failed to register logo-enriched channels in iterator registry: {}", e);
                        }
                        
                        return Ok(processed_channels);
                    }
                }
                Err(e) => {
                    warn!(
                        "WASM plugin logo prefetch failed, falling back to native service:\n\
                         ├─ Plugin: {}\n\
                         ├─ Error: {}\n\
                         ├─ Stage: logo_prefetch\n\
                         ├─ Fallback: Using LogoAssetService\n\
                         └─ Reason: Plugin execution error",
                        plugin.get_info().name,
                        e
                    );
                }
            }
        } else {
            info!(
                "No WASM plugin available for logo_prefetch stage:\n\
                 ├─ Memory pressure: {:?}\n\
                 ├─ Available plugins: {}\n\
                 └─ Using: LogoAssetService with EPG integration",
                memory_pressure,
                self.plugin_manager.get_loaded_plugin_count()
            );
        }
        } // Close the should_use_plugin_for_stage("logo_prefetch") block

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
                        // TODO: Implement logo caching when method is available
                        debug!("Would cache logo for channel: {} -> {}", channel.channel_name, logo_url);
                        total_logos_cached += 1;
                    }
                }

                // TODO: In future, also access EPG data to cache program-specific logos
                // This would require integrating with OrderedEpgAggregateIterator
                // to get program information and cache logos for specific shows/movies
            }

            processed_channels.extend(chunk_channels);
            
            info!("Processed logo prefetch chunk: {} channels", chunk.len());
        }

        let stage_info = memory_context.complete_stage("logo_prefetch")?;
        
        info!(
            "Native logo prefetch completed:\n\
             ├─ Implementation: LogoAssetService\n\
             ├─ Channels processed: {}\n\
             ├─ Logos cached: {} ({:.1}% success rate)\n\
             ├─ Chunk size used: {}\n\
             ├─ Memory used: {:.1}MB\n\
             ├─ Processing time: {}ms\n\
             ├─ Memory pressure: {:?}\n\
             └─ Note: EPG logo integration pending",
            processed_channels.len(),
            total_logos_cached,
            if processed_channels.is_empty() { 0.0 } else { (total_logos_cached as f64 / processed_channels.len() as f64) * 100.0 },
            requested_chunk_size,
            stage_info.memory_delta_mb,
            stage_info.duration_ms,
            stage_info.pressure_level
        );

        // Register logo-enriched channels in iterator registry for multi-instance access
        if let Err(e) = self.register_logo_enriched_channels(&processed_channels).await {
            warn!("Failed to register logo-enriched channels in iterator registry: {}", e);
        }

        Ok(processed_channels)
    }

    /// Execute channel numbering stage with iterator-based processing
    async fn execute_channel_numbering_with_iterator(
        &self,
        channels: Vec<Channel>,
        config: &ResolvedProxyConfig,
        memory_context: &mut MemoryContext,
        cleanup_coordinator: &mut MemoryCleanupCoordinator,
    ) -> Result<Vec<NumberedChannel>> {
        memory_context.start_stage("channel_numbering")?;
        
        info!("Starting channel numbering stage for {} channels", channels.len());

        let memory_pressure = self.get_current_memory_pressure(memory_context);

        // Request optimal chunk size from chunk manager
        let requested_chunk_size = self.chunk_manager
            .request_chunk_size("channel_numbering", channels.len().min(2000))
            .await?;

        info!("Channel numbering using chunk size: {}", requested_chunk_size);

        // Try WASM plugin first if available
        if self.should_use_plugin_for_stage("channel_numbering") {
            if let Some(plugin) = self
                .plugin_manager
                .get_plugin_for_stage("channel_numbering", memory_pressure)
            {
            let stage_context = self.create_temp_stage_context("channel_numbering", memory_pressure);

            match plugin.execute_channel_numbering(&stage_context, channels.clone()).await {
                Ok(numbered_channels) => {
                    let stage_info = memory_context.complete_stage("channel_numbering")?;
                    
                    // Check if plugin returned empty results - fall back to native
                    if numbered_channels.is_empty() && !channels.is_empty() {
                        warn!(
                            "WASM plugin returned empty results, falling back to native numbering:\n\
                             ├─ Plugin: {} v{}\n\
                             ├─ Channels numbered: {}\n\
                             ├─ Memory used: {:.1}MB\n\
                             ├─ Processing time: {}ms\n\
                             ├─ Memory pressure: {:?}\n\
                             └─ Fallback: Using sequential numbering",
                            plugin.get_info().name,
                            plugin.get_info().version,
                            numbered_channels.len(),
                            stage_info.memory_delta_mb,
                            stage_info.duration_ms,
                            stage_info.pressure_level
                        );
                        
                        // Continue to native fallback below
                    } else {
                        info!(
                            "WASM plugin channel numbering succeeded:\n\
                             ├─ Plugin: {} v{}\n\
                             ├─ Channels numbered: {}\n\
                             ├─ Memory used: {:.1}MB\n\
                             ├─ Processing time: {}ms\n\
                             └─ Memory pressure: {:?}",
                            plugin.get_info().name,
                            plugin.get_info().version,
                            numbered_channels.len(),
                            stage_info.memory_delta_mb,
                            stage_info.duration_ms,
                            stage_info.pressure_level
                        );
                        
                        return Ok(numbered_channels);
                    }
                }
                Err(e) => {
                    warn!(
                        "WASM plugin channel numbering failed, falling back to native:\n\
                         ├─ Plugin: {}\n\
                         ├─ Error: {}\n\
                         ├─ Stage: channel_numbering\n\
                         ├─ Fallback: Using sequential numbering\n\
                         └─ Reason: Plugin execution error",
                        plugin.get_info().name,
                        e
                    );
                }
            }
        } else {
            info!(
                "No WASM plugin available for channel_numbering stage:\n\
                 ├─ Memory pressure: {:?}\n\
                 ├─ Available plugins: {}\n\
                 └─ Using: Sequential numbering from proxy config",
                memory_pressure,
                self.plugin_manager.get_loaded_plugin_count()
            );
        }
        } // Close the should_use_plugin_for_stage("channel_numbering") block

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
            info!("Numbered chunk of {} channels (numbers {} to {})", 
                  chunk.len(), 
                  current_number - chunk.len() as i32, 
                  current_number - 1);
        }

        let stage_info = memory_context.complete_stage("channel_numbering")?;
        
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
        cleanup_coordinator: &mut MemoryCleanupCoordinator,
    ) -> Result<String> {
        memory_context.start_stage("m3u_generation")?;
        
        info!("Starting M3U generation stage for {} numbered channels", numbered_channels.len());

        let memory_pressure = self.get_current_memory_pressure(memory_context);

        // Request optimal chunk size from chunk manager
        let default_chunk_size = self.chunk_manager.get_chunk_size("m3u_generation").await;
        let requested_chunk_size = self.chunk_manager
            .request_chunk_size("m3u_generation", numbered_channels.len().min(default_chunk_size))
            .await?;

        info!("M3U generation using chunk size: {}", requested_chunk_size);

        // Try WASM plugin first if available
        if self.should_use_plugin_for_stage("m3u_generation") {
            if let Some(plugin) = self
                .plugin_manager
                .get_plugin_for_stage("m3u_generation", memory_pressure)
            {
            let stage_context = self.create_temp_stage_context("m3u_generation", memory_pressure);

            match plugin.execute_m3u_generation(&stage_context, numbered_channels.clone()).await {
                Ok(m3u_content) => {
                    let stage_info = memory_context.complete_stage("m3u_generation")?;
                    
                    // Check if plugin returned empty content - fall back to native
                    if m3u_content.is_empty() && !numbered_channels.is_empty() {
                        warn!(
                            "WASM plugin returned empty content, falling back to native generation:\n\
                             ├─ Plugin: {} v{}\n\
                             ├─ M3U content size: {} bytes\n\
                             ├─ Memory used: {:.1}MB\n\
                             ├─ Processing time: {}ms\n\
                             ├─ Memory pressure: {:?}\n\
                             └─ Fallback: Using native M3U generator",
                            plugin.get_info().name,
                            plugin.get_info().version,
                            m3u_content.len(),
                            stage_info.memory_delta_mb,
                            stage_info.duration_ms,
                            stage_info.pressure_level
                        );
                        
                        // Continue to native fallback below
                    } else {
                        info!(
                            "WASM plugin M3U generation succeeded:\n\
                             ├─ Plugin: {} v{}\n\
                             ├─ M3U content size: {} bytes\n\
                             ├─ Memory used: {:.1}MB\n\
                             ├─ Processing time: {}ms\n\
                             └─ Memory pressure: {:?}",
                            plugin.get_info().name,
                            plugin.get_info().version,
                            m3u_content.len(),
                            stage_info.memory_delta_mb,
                            stage_info.duration_ms,
                            stage_info.pressure_level
                        );
                        
                        return Ok(m3u_content);
                    }
                }
                Err(e) => {
                    warn!(
                        "WASM plugin M3U generation failed, falling back to native:\n\
                         ├─ Plugin: {}\n\
                         ├─ Error: {}\n\
                         ├─ Stage: m3u_generation\n\
                         ├─ Fallback: Using native M3U generator\n\
                         └─ Reason: Plugin execution error",
                        plugin.get_info().name,
                        e
                    );
                }
            }
        } else {
            info!(
                "No WASM plugin available for m3u_generation stage:\n\
                 ├─ Memory pressure: {:?}\n\
                 ├─ Available plugins: {}\n\
                 └─ Using: Native M3U generator",
                memory_pressure,
                self.plugin_manager.get_loaded_plugin_count()
            );
        }
        } // Close the should_use_plugin_for_stage("m3u_generation") block

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
                    numbered_channel.assigned_number, channel.id, channel.channel_name, channel.tvg_name, channel.stream_url
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
                
                if let Some(group_title) = &channel.group_title {
                    if !group_title.is_empty() {
                        extinf_parts.push(format!("group-title=\"{}\"", group_title));
                    }
                }
                
                // Add channel number
                extinf_parts.push(format!("tvg-chno=\"{}\"", numbered_channel.assigned_number));
                
                // Add channel name at the end
                extinf_parts.push(channel.channel_name.clone());
                
                // Build complete EXTINF line
                let complete_extinf = extinf_parts.join(" ");
                debug!("M3U Generation (Adaptive Native) - Generated EXTINF: '{}'", complete_extinf);
                chunk_content.push_str(&complete_extinf);
                chunk_content.push('\n');
                
                // Add stream URL (potentially proxied through our system)
                let stream_url = if channel.stream_url.starts_with("http") {
                    // Use original URL for now, could be proxied through base_url in future
                    channel.stream_url.clone()
                } else {
                    format!("{}/stream/{}/{}", base_url, config.proxy.ulid, channel.id)
                };
                chunk_content.push_str(&stream_url);
                chunk_content.push('\n');
                
                total_channels_processed += 1;
            }
            
            m3u_content.push_str(&chunk_content);
            info!("Generated M3U chunk: {} channels, {} bytes", chunk.len(), chunk_content.len());
        }

        let stage_info = memory_context.complete_stage("m3u_generation")?;
        
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
            if total_channels_processed > 0 { m3u_content.len() as f64 / total_channels_processed as f64 } else { 0.0 },
            requested_chunk_size,
            stage_info.memory_delta_mb,
            stage_info.duration_ms,
            stage_info.pressure_level
        );

        Ok(m3u_content)
    }

    /// Execute EPG processing stage using OrderedEpgAggregateIterator with plugin/native fallback
    async fn execute_epg_processing_with_iterator(
        &self,
        numbered_channels: Vec<NumberedChannel>,
        config: &ResolvedProxyConfig,
        base_url: &str,
        memory_context: &mut MemoryContext,
        cleanup_coordinator: &mut MemoryCleanupCoordinator,
    ) -> Result<String> {
        memory_context.start_stage("epg_processing")?;
        
        info!("Starting EPG processing stage for {} channels", numbered_channels.len());

        let memory_pressure = self.get_current_memory_pressure(memory_context);

        // Create OrderedEpgAggregateIterator for EPG data
        let mut epg_iterator = MultiSourceIterator::new(
            Arc::new(self.database.clone()),
            config.epg_sources.clone(),
            crate::pipeline::orchestrator::EpgLoader {},
            1000,
        );

        // Request optimal chunk size from chunk manager
        let default_chunk_size = self.chunk_manager.get_chunk_size("epg_processing").await;
        let requested_chunk_size = self.chunk_manager
            .request_chunk_size("epg_processing", default_chunk_size)
            .await?;

        info!("EPG processing using chunk size: {}", requested_chunk_size);

        // Try WASM plugin first if available
        if self.should_use_plugin_for_stage("epg_processing") {
            if let Some(plugin) = self
                .plugin_manager
                .get_plugin_for_stage("epg_processing", memory_pressure)
            {
            // For WASM plugin, we would need to provide EPG iterator context
            // This is a more complex integration that would require PipelineIteratorContext
            let stage_context = self.create_stage_context("epg_processing", config, base_url, memory_pressure, None);

            // Note: This would require a specialized EPG processing plugin method
            // For now, we'll fall back to native implementation
            warn!(
                "WASM plugin EPG processing not yet fully integrated:\n\
                 ├─ Plugin: {} v{}\n\
                 ├─ Stage: epg_processing\n\
                 ├─ Fallback: Using native EPG generator\n\
                 └─ Reason: EPG iterator context integration pending",
                plugin.get_info().name,
                plugin.get_info().version
            );
        } else {
            info!(
                "No WASM plugin available for epg_processing stage:\n\
                 ├─ Memory pressure: {:?}\n\
                 ├─ Available plugins: {}\n\
                 └─ Using: Native EPG generator with OrderedEpgAggregateIterator",
                memory_pressure,
                self.plugin_manager.get_loaded_plugin_count()
            );
        }
        } // Close the should_use_plugin_for_stage("epg_processing") block

        // Native implementation using OrderedEpgAggregateIterator + EPG generator
        let mut all_epg_entries = Vec::new();
        
        // Load all EPG entries via iterator
        loop {
            match epg_iterator.next_chunk_with_size(requested_chunk_size).await? {
                crate::pipeline::iterator_traits::IteratorResult::Chunk(chunk) => {
                    if chunk.is_empty() {
                        break;
                    }
                    info!("Loaded chunk of {} EPG entries from iterator", chunk.len());
                    all_epg_entries.extend(chunk);
                }
                crate::pipeline::iterator_traits::IteratorResult::Exhausted => {
                    info!("EPG iterator exhausted");
                    break;
                }
            }
        }

        // Close EPG iterator
        epg_iterator.close().await?;

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
            info!("Processing {} EPG entries for {} channels", all_epg_entries.len(), numbered_channels.len());
            
            // Use EPG generator service to create XMLTV
            let epg_generator = crate::proxy::epg_generator::EpgGenerator::new(self.database.clone());
            
            // Extract channel IDs for EPG filtering
            let channel_ids: Vec<String> = numbered_channels
                .iter()
                .filter_map(|nc| nc.channel.tvg_id.clone())
                .filter(|id| !id.is_empty())
                .collect();

            match epg_generator.generate_xmltv_for_proxy(&config.proxy, &channel_ids, None).await {
                Ok((xmltv_content, stats)) => {
                    info!(
                        "EPG generation succeeded:\n\
                         ├─ Channels processed: {}\n\
                         ├─ Programs generated: {}\n\
                         ├─ XMLTV size: {} bytes\n\
                         └─ Time span: {} hours",
                        stats.matched_epg_channels,
                        stats.total_programs_after_filter,
                        xmltv_content.len(),
                        (stats.generation_time_ms as f64 / 3600000.0)
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

        let stage_info = memory_context.complete_stage("epg_processing")?;
        
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

    /// Generic plugin execution wrapper with consistent fallback handling
    async fn execute_with_plugin_fallback<T, F, Fut>(
        &self,
        stage_name: &str,
        memory_context: &mut MemoryContext,
        plugin_execution: F,
        native_fallback: Fut,
        result_validator: impl Fn(&T) -> bool,
    ) -> Result<T>
    where
        F: FnOnce() -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<T>> + Send>>,
        Fut: std::future::Future<Output = Result<T>>,
    {
        let memory_pressure = self.get_current_memory_pressure(memory_context);

        // Try WASM plugin first if available
        if self.should_use_plugin_for_stage(stage_name) {
            if let Some(plugin) = self
                .plugin_manager
                .get_plugin_for_stage(stage_name, memory_pressure)
            {
            match plugin_execution().await {
                Ok(result) => {
                    let stage_info = memory_context.complete_stage(stage_name)?;
                    
                    // Validate result using provided validator
                    if !result_validator(&result) {
                        warn!(
                            "WASM plugin returned invalid result, falling back to native:\n\
                             ├─ Plugin: {} v{}\n\
                             ├─ Stage: {}\n\
                             ├─ Memory used: {:.1}MB\n\
                             ├─ Processing time: {}ms\n\
                             └─ Fallback: Using native implementation",
                            plugin.get_info().name,
                            plugin.get_info().version,
                            stage_name,
                            stage_info.memory_delta_mb,
                            stage_info.duration_ms,
                        );
                        
                        // Re-start stage for native fallback
                        memory_context.start_stage(stage_name)?;
                    } else {
                        info!(
                            "WASM plugin {} execution succeeded:\n\
                             ├─ Plugin: {} v{}\n\
                             ├─ Memory used: {:.1}MB\n\
                             ├─ Processing time: {}ms\n\
                             └─ Memory pressure: {:?}",
                            stage_name,
                            plugin.get_info().name,
                            plugin.get_info().version,
                            stage_info.memory_delta_mb,
                            stage_info.duration_ms,
                            stage_info.pressure_level
                        );
                        
                        return Ok(result);
                    }
                }
                Err(e) => {
                    warn!(
                        "WASM plugin {} failed, falling back to native:\n\
                         ├─ Plugin: {}\n\
                         ├─ Error: {}\n\
                         ├─ Stage: {}\n\
                         └─ Fallback: Using native implementation",
                        stage_name,
                        plugin.get_info().name,
                        e,
                        stage_name,
                    );
                    
                    // Re-start stage for native fallback
                    memory_context.start_stage(stage_name)?;
                }
            }
        } else {
            info!(
                "No WASM plugin available for {} stage:\n\
                 ├─ Memory pressure: {:?}\n\
                 ├─ Available plugins: {}\n\
                 └─ Using: Native implementation",
                stage_name,
                memory_pressure,
                self.plugin_manager.get_loaded_plugin_count()
            );
        }
        } // Close the should_use_plugin_for_stage(stage_name) block

        // Execute native fallback
        native_fallback.await
    }

    /// Generic iterator processing with chunk management
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

        info!("{} using iterator with chunk size: {}", stage_name, chunk_size);

        loop {
            match iterator.next_chunk_with_size(chunk_size).await? {
                crate::pipeline::iterator_traits::IteratorResult::Chunk(chunk) => {
                    if chunk.is_empty() {
                        break;
                    }
                    info!("Loaded chunk of {} items from {} iterator", chunk.len(), stage_name);
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

    /// Create stage context for plugin execution
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
            stats: GenerationStats::new("unified_iterator_wasm".to_string()),
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
                    ulid: "temp".to_string(),
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
            stats: GenerationStats::new("unified_iterator_wasm".to_string()),
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
        memory_context.start_stage("source_loading")?;

        let memory_pressure = self.get_current_memory_pressure(memory_context);

        // Try WASM plugin first if available
        if self.should_use_plugin_for_stage("source_loading") {
            if let Some(plugin) = self
                .plugin_manager
                .get_plugin_for_stage("source_loading", memory_pressure)
            {
            info!(
                "Selected WASM plugin for source_loading stage:\n\
                 ├─ Plugin: {} v{}\n\
                 ├─ Memory pressure: {:?}\n\
                 └─ Expected processing: {} channels",
                plugin.get_info().name,
                plugin.get_info().version,
                memory_pressure,
                source_ids.len()
            );

            // Try to execute the WASM plugin
            // Create a minimal stage context for WASM plugin execution
            let stage_context = StageContext {
                proxy_config: ResolvedProxyConfig {
                    proxy: StreamProxy {
                        id: uuid::Uuid::new_v4(),
                        ulid: "temp_proxy".to_string(),
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
                    },
                    sources: Vec::new(),
                    filters: Vec::new(),
                    epg_sources: Vec::new(),
                },
                output: GenerationOutput::InMemory,
                base_url: "http://localhost:8080".to_string(),
                engine_config: None,
                memory_pressure,
                available_memory_mb: self.memory_limit_mb,
                current_stage: "source_loading".to_string(),
                stats: GenerationStats::new("adaptive_wasm".to_string()),
                database: Some(Arc::new(self.database.clone())),
                logo_service: Some(Arc::new(self.logo_service.clone())),
                iterator_registry: None,
            };

            match plugin.execute_source_loading(&stage_context, source_ids.clone()).await {
                Ok(channels) => {
                    let stage_info = memory_context.complete_stage("source_loading")?;
                    
                    // Check if plugin returned empty results - fall back to native strategy
                    if channels.is_empty() {
                        warn!(
                            "WASM plugin returned empty results, falling back to built-in strategy:\n\
                             ├─ Plugin: {} v{}\n\
                             ├─ Channels loaded: {}\n\
                             ├─ Memory used: {:.1}MB\n\
                             ├─ Processing time: {}ms\n\
                             ├─ Memory pressure: {:?}\n\
                             └─ Fallback: Using built-in inmemory source loading strategy",
                            plugin.get_info().name,
                            plugin.get_info().version,
                            channels.len(),
                            stage_info.memory_delta_mb,
                            stage_info.duration_ms,
                            stage_info.pressure_level
                        );
                        
                        // Continue to fallback strategy below
                    } else {
                        info!(
                            "WASM plugin execution succeeded:\n\
                             ├─ Plugin: {} v{}\n\
                             ├─ Channels loaded: {}\n\
                             ├─ Memory used: {:.1}MB\n\
                             ├─ Processing time: {}ms\n\
                             └─ Memory pressure: {:?}",
                            plugin.get_info().name,
                            plugin.get_info().version,
                            channels.len(),
                            stage_info.memory_delta_mb,
                            stage_info.duration_ms,
                            stage_info.pressure_level
                        );
                        
                        return Ok(channels);
                    }
                }
                Err(e) => {
                    warn!(
                        "WASM plugin execution failed, falling back to built-in strategy:\n\
                         ├─ Plugin: {} \n\
                         ├─ Error: {}\n\
                         ├─ Stage: source_loading\n\
                         ├─ Fallback: Using built-in inmemory source loading strategy\n\
                         └─ Reason: Plugin execution error",
                        plugin.get_info().name,
                        e
                    );
                }
            }
        } else {
            warn!(
                "No WASM plugin available for source_loading stage:\n\
                 ├─ Memory pressure: {:?}\n\
                 ├─ Available plugins: {}\n\
                 └─ Fallback: Using built-in inmemory source loading strategy",
                memory_pressure,
                self.plugin_manager.get_loaded_plugin_count()
            );
        }
        } // Close the should_use_plugin_for_stage("source_loading") block

        // Use simple strategy (default)
        let strategy = SimpleSourceLoader::new(self.database.clone());
        let input = SourceLoadingInput {
            source_ids,
            proxy_config: ResolvedProxyConfig {
                proxy: StreamProxy {
                    id: uuid::Uuid::new_v4(),
                    ulid: "temp_proxy".to_string(),
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
                },
                sources: Vec::new(),
                filters: Vec::new(),
                epg_sources: Vec::new(),
            },
        };

        let mut output = strategy.execute(input, memory_context).await?;
        let stage_info = memory_context.complete_stage("source_loading")?;

        info!(
            "Source loading completed: {} channels, {:.1}MB memory used, {:?} pressure",
            output.channels.len(),
            stage_info.memory_delta_mb,
            stage_info.pressure_level
        );

        // Cleanup if needed
        if memory_context.should_cleanup()? {
            cleanup_coordinator.cleanup_between_stages(
                "source_loading",
                &mut output,
                crate::utils::CleanupStrategy::Basic,
            )?;
        }

        Ok(output.channels)
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
        memory_context.start_stage("data_mapping")?;

        let memory_pressure = self.get_current_memory_pressure(memory_context);

        // Try WASM plugin first if available
        if self.should_use_plugin_for_stage("data_mapping") {
            if let Some(plugin) = self.get_configured_plugin_for_stage("data_mapping", memory_pressure) {
            info!(
                "Selected WASM plugin for data_mapping stage:\n\
                 ├─ Plugin: {} v{}\n\
                 ├─ Memory pressure: {:?}\n\
                 └─ Expected processing: {} channels",
                plugin.get_info().name,
                plugin.get_info().version,
                memory_pressure,
                channels.len()
            );

            // Try to execute the WASM plugin
            // Create a minimal stage context for WASM plugin execution
            let stage_context = StageContext {
                proxy_config: ResolvedProxyConfig {
                    proxy: StreamProxy {
                        id: uuid::Uuid::new_v4(),
                        ulid: "temp_proxy".to_string(),
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
                    },
                    sources: Vec::new(),
                    filters: Vec::new(),
                    epg_sources: Vec::new(),
                },
                output: GenerationOutput::InMemory,
                base_url: "http://localhost:8080".to_string(),
                engine_config: None,
                memory_pressure,
                available_memory_mb: self.memory_limit_mb,
                current_stage: "source_loading".to_string(),
                stats: GenerationStats::new("adaptive_wasm".to_string()),
                database: Some(Arc::new(self.database.clone())),
                logo_service: Some(Arc::new(self.logo_service.clone())),
                iterator_registry: None,
            };

            match plugin.execute_data_mapping(&stage_context, channels.clone()).await {
                Ok(mapped_channels) => {
                    let stage_info = memory_context.complete_stage("data_mapping")?;
                    
                    // Check if plugin returned empty results - fall back to native strategy
                    if mapped_channels.is_empty() && !channels.is_empty() {
                        warn!(
                            "WASM plugin returned empty results, falling back to built-in strategy:\n\
                             ├─ Plugin: {} v{}\n\
                             ├─ Channels processed: {} → {}\n\
                             ├─ Memory used: {:.1}MB\n\
                             ├─ Processing time: {}ms\n\
                             ├─ Memory pressure: {:?}\n\
                             └─ Fallback: Using built-in data mapping strategy",
                            plugin.get_info().name,
                            plugin.get_info().version,
                            channels.len(),
                            mapped_channels.len(),
                            stage_info.memory_delta_mb,
                            stage_info.duration_ms,
                            stage_info.pressure_level
                        );
                        
                        // Continue to fallback strategy below
                    } else {
                        info!(
                            "WASM plugin execution succeeded:\n\
                             ├─ Plugin: {} v{}\n\
                             ├─ Channels processed: {} → {}\n\
                             ├─ Memory used: {:.1}MB\n\
                             ├─ Processing time: {}ms\n\
                             └─ Memory pressure: {:?}",
                            plugin.get_info().name,
                            plugin.get_info().version,
                            channels.len(),
                            mapped_channels.len(),
                            stage_info.memory_delta_mb,
                            stage_info.duration_ms,
                            stage_info.pressure_level
                        );
                        
                        return Ok(mapped_channels);
                    }
                }
                Err(e) => {
                    warn!(
                        "WASM plugin execution failed, falling back to built-in strategy:\n\
                         ├─ Plugin: {} \n\
                         ├─ Error: {}\n\
                         ├─ Stage: data_mapping\n\
                         ├─ Fallback: Using built-in data mapping service\n\
                         └─ Reason: Plugin execution error",
                        plugin.get_info().name,
                        e
                    );
                }
            }
        } else {
            warn!(
                "No WASM plugin available for data_mapping stage:\n\
                 ├─ Memory pressure: {:?}\n\
                 ├─ Available plugins: {}\n\
                 └─ Fallback: Using built-in data mapping service",
                memory_pressure,
                self.plugin_manager.get_loaded_plugin_count()
            );
        }
        } // Close the should_use_plugin_for_stage("data_mapping") block

        // Use simple strategy (default)
        let strategy =
            SimpleDataMapper::new(self.data_mapping_service.clone(), self.logo_service.clone());
        let input = DataMappingInput {
            channels,
            source_configs: config.sources.clone(),
            engine_config,
            base_url: base_url.to_string(),
        };

        let mut output = strategy.execute(input, memory_context).await?;
        let stage_info = memory_context.complete_stage("data_mapping")?;

        info!(
            "Data mapping completed: {} channels, {:.1}MB memory used",
            output.mapped_channels.len(),
            stage_info.memory_delta_mb
        );

        // Cleanup if needed
        if memory_context.should_cleanup()? {
            cleanup_coordinator.cleanup_between_stages(
                "data_mapping",
                &mut output,
                crate::utils::CleanupStrategy::Basic,
            )?;
        }

        Ok(output.mapped_channels)
    }

    /// Execute filtering stage with strategy selection
    async fn execute_filtering_stage(
        &self,
        channels: Vec<Channel>,
        config: &ResolvedProxyConfig,
        memory_context: &mut MemoryContext,
        cleanup_coordinator: &mut MemoryCleanupCoordinator,
    ) -> Result<Vec<Channel>> {
        memory_context.start_stage("filtering")?;

        let memory_pressure = self.get_current_memory_pressure(memory_context);

        // Try WASM plugin first if available
        if self.should_use_plugin_for_stage("filtering") {
            if let Some(plugin) = self
                .plugin_manager
                .get_plugin_for_stage("filtering", memory_pressure)
            {
            info!(
                "Selected WASM plugin for filtering stage:\n\
                 ├─ Plugin: {} v{}\n\
                 ├─ Memory pressure: {:?}\n\
                 └─ Expected processing: {} channels",
                plugin.get_info().name,
                plugin.get_info().version,
                memory_pressure,
                channels.len()
            );

            // Try to execute the WASM plugin
            // Create a minimal stage context for WASM plugin execution
            let stage_context = StageContext {
                proxy_config: ResolvedProxyConfig {
                    proxy: StreamProxy {
                        id: uuid::Uuid::new_v4(),
                        ulid: "temp_proxy".to_string(),
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
                    },
                    sources: Vec::new(),
                    filters: Vec::new(),
                    epg_sources: Vec::new(),
                },
                output: GenerationOutput::InMemory,
                base_url: "http://localhost:8080".to_string(),
                engine_config: None,
                memory_pressure,
                available_memory_mb: self.memory_limit_mb,
                current_stage: "source_loading".to_string(),
                stats: GenerationStats::new("adaptive_wasm".to_string()),
                database: Some(Arc::new(self.database.clone())),
                logo_service: Some(Arc::new(self.logo_service.clone())),
                iterator_registry: None,
            };

            match plugin.execute_filtering(&stage_context, channels.clone()).await {
                Ok(filtered_channels) => {
                    let stage_info = memory_context.complete_stage("filtering")?;
                    
                    // Check if plugin returned empty results - fall back to native strategy
                    if filtered_channels.is_empty() && !channels.is_empty() {
                        warn!(
                            "WASM plugin returned empty results, falling back to built-in strategy:\n\
                             ├─ Plugin: {} v{}\n\
                             ├─ Channels filtered: {} → {}\n\
                             ├─ Memory used: {:.1}MB\n\
                             ├─ Processing time: {}ms\n\
                             ├─ Memory pressure: {:?}\n\
                             └─ Fallback: Using built-in filtering strategy",
                            plugin.get_info().name,
                            plugin.get_info().version,
                            channels.len(),
                            filtered_channels.len(),
                            stage_info.memory_delta_mb,
                            stage_info.duration_ms,
                            stage_info.pressure_level
                        );
                        
                        // Continue to fallback strategy below
                    } else {
                        info!(
                            "WASM plugin execution succeeded:\n\
                             ├─ Plugin: {} v{}\n\
                             ├─ Channels filtered: {} → {}\n\
                             ├─ Memory used: {:.1}MB\n\
                             ├─ Processing time: {}ms\n\
                             └─ Memory pressure: {:?}",
                            plugin.get_info().name,
                            plugin.get_info().version,
                            channels.len(),
                            filtered_channels.len(),
                            stage_info.memory_delta_mb,
                            stage_info.duration_ms,
                            stage_info.pressure_level
                        );
                        
                        return Ok(filtered_channels);
                    }
                }
                Err(e) => {
                    warn!(
                        "WASM plugin execution failed, falling back to built-in strategy:\n\
                         ├─ Plugin: {} \n\
                         ├─ Error: {}\n\
                         ├─ Stage: filtering\n\
                         ├─ Fallback: Using built-in filtering strategy\n\
                         └─ Reason: Plugin execution error",
                        plugin.get_info().name,
                        e
                    );
                }
            }
        }
        } // Close the should_use_plugin_for_stage("filtering") block

        // Use simple strategy (default)
        let strategy = SimpleFilter;
        let input = FilteringInput {
            channels,
            filters: config.filters.clone(),
        };

        let mut output = strategy.execute(input, memory_context).await?;
        let stage_info = memory_context.complete_stage("filtering")?;

        info!(
            "Filtering completed: {} channels, {:.1}MB memory used",
            output.filtered_channels.len(),
            stage_info.memory_delta_mb
        );

        // Cleanup if needed
        if memory_context.should_cleanup()? {
            cleanup_coordinator.cleanup_between_stages(
                "filtering",
                &mut output,
                crate::utils::CleanupStrategy::Basic,
            )?;
        }

        Ok(output.filtered_channels)
    }

    /// Execute channel numbering stage with strategy selection
    async fn execute_channel_numbering_stage(
        &self,
        channels: Vec<Channel>,
        config: &ResolvedProxyConfig,
        memory_context: &mut MemoryContext,
        cleanup_coordinator: &mut MemoryCleanupCoordinator,
    ) -> Result<Vec<NumberedChannel>> {
        memory_context.start_stage("channel_numbering")?;

        let memory_pressure = self.get_current_memory_pressure(memory_context);

        // Try WASM plugin first if available
        if self.should_use_plugin_for_stage("channel_numbering") {
            if let Some(plugin) = self
                .plugin_manager
                .get_plugin_for_stage("channel_numbering", memory_pressure)
            {
            info!(
                "Selected WASM plugin for channel_numbering stage:\n\
                 ├─ Plugin: {} v{}\n\
                 ├─ Memory pressure: {:?}\n\
                 └─ Expected processing: {} channels",
                plugin.get_info().name,
                plugin.get_info().version,
                memory_pressure,
                channels.len()
            );

            // Try to execute the WASM plugin
            // Create a minimal stage context for WASM plugin execution
            let stage_context = StageContext {
                proxy_config: ResolvedProxyConfig {
                    proxy: StreamProxy {
                        id: uuid::Uuid::new_v4(),
                        ulid: "temp_proxy".to_string(),
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
                    },
                    sources: Vec::new(),
                    filters: Vec::new(),
                    epg_sources: Vec::new(),
                },
                output: GenerationOutput::InMemory,
                base_url: "http://localhost:8080".to_string(),
                engine_config: None,
                memory_pressure,
                available_memory_mb: self.memory_limit_mb,
                current_stage: "source_loading".to_string(),
                stats: GenerationStats::new("adaptive_wasm".to_string()),
                database: Some(Arc::new(self.database.clone())),
                logo_service: Some(Arc::new(self.logo_service.clone())),
                iterator_registry: None,
            };

            match plugin.execute_channel_numbering(&stage_context, channels.clone()).await {
                Ok(numbered_channels) => {
                    let stage_info = memory_context.complete_stage("channel_numbering")?;
                    
                    // Check if plugin returned empty results - fall back to native strategy
                    if numbered_channels.is_empty() && !channels.is_empty() {
                        warn!(
                            "WASM plugin returned empty results, falling back to built-in strategy:\n\
                             ├─ Plugin: {} v{}\n\
                             ├─ Channels numbered: {}\n\
                             ├─ Memory used: {:.1}MB\n\
                             ├─ Processing time: {}ms\n\
                             ├─ Memory pressure: {:?}\n\
                             └─ Fallback: Using built-in channel numbering strategy",
                            plugin.get_info().name,
                            plugin.get_info().version,
                            numbered_channels.len(),
                            stage_info.memory_delta_mb,
                            stage_info.duration_ms,
                            stage_info.pressure_level
                        );
                        
                        // Continue to fallback strategy below
                    } else {
                        info!(
                            "WASM plugin execution succeeded:\n\
                             ├─ Plugin: {} v{}\n\
                             ├─ Channels numbered: {}\n\
                             ├─ Memory used: {:.1}MB\n\
                             ├─ Processing time: {}ms\n\
                             └─ Memory pressure: {:?}",
                            plugin.get_info().name,
                            plugin.get_info().version,
                            numbered_channels.len(),
                            stage_info.memory_delta_mb,
                            stage_info.duration_ms,
                            stage_info.pressure_level
                        );
                        
                        return Ok(numbered_channels);
                    }
                }
                Err(e) => {
                    warn!(
                        "WASM plugin execution failed, falling back to built-in strategy:\n\
                         ├─ Plugin: {} \n\
                         ├─ Error: {}\n\
                         ├─ Stage: channel_numbering\n\
                         ├─ Fallback: Using built-in channel numbering strategy\n\
                         └─ Reason: Plugin execution error",
                        plugin.get_info().name,
                        e
                    );
                }
            }
        }
        } // Close the should_use_plugin_for_stage("channel_numbering") block

        // Use simple strategy (default)
        let strategy = SimpleChannelNumbering;
        let input = ChannelNumberingInput {
            channels,
            starting_number: config.proxy.starting_channel_number,
            numbering_strategy: ChannelNumberingStrategy::Sequential,
        };

        let mut output = strategy.execute(input, memory_context).await?;
        let stage_info = memory_context.complete_stage("channel_numbering")?;

        info!(
            "Channel numbering completed: {} channels, {:.1}MB memory used",
            output.numbered_channels.len(),
            stage_info.memory_delta_mb
        );

        // Cleanup if needed
        if memory_context.should_cleanup()? {
            cleanup_coordinator.cleanup_between_stages(
                "channel_numbering",
                &mut output,
                crate::utils::CleanupStrategy::Basic,
            )?;
        }

        Ok(output.numbered_channels)
    }

    /// Execute M3U generation stage with strategy selection
    async fn execute_m3u_generation_stage(
        &self,
        numbered_channels: Vec<NumberedChannel>,
        config: &ResolvedProxyConfig,
        base_url: &str,
        memory_context: &mut MemoryContext,
        cleanup_coordinator: &mut MemoryCleanupCoordinator,
    ) -> Result<String> {
        memory_context.start_stage("m3u_generation")?;

        let memory_pressure = self.get_current_memory_pressure(memory_context);

        // Try WASM plugin first if available
        if self.should_use_plugin_for_stage("m3u_generation") {
            if let Some(plugin) = self
                .plugin_manager
                .get_plugin_for_stage("m3u_generation", memory_pressure)
            {
            info!(
                "Selected WASM plugin for m3u_generation stage:\n\
                 ├─ Plugin: {} v{}\n\
                 ├─ Memory pressure: {:?}\n\
                 └─ Expected processing: {} channels",
                plugin.get_info().name,
                plugin.get_info().version,
                memory_pressure,
                numbered_channels.len()
            );

            // Try to execute the WASM plugin
            // Create a minimal stage context for WASM plugin execution
            let stage_context = StageContext {
                proxy_config: ResolvedProxyConfig {
                    proxy: StreamProxy {
                        id: uuid::Uuid::new_v4(),
                        ulid: "temp_proxy".to_string(),
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
                    },
                    sources: Vec::new(),
                    filters: Vec::new(),
                    epg_sources: Vec::new(),
                },
                output: GenerationOutput::InMemory,
                base_url: "http://localhost:8080".to_string(),
                engine_config: None,
                memory_pressure,
                available_memory_mb: self.memory_limit_mb,
                current_stage: "source_loading".to_string(),
                stats: GenerationStats::new("adaptive_wasm".to_string()),
                database: Some(Arc::new(self.database.clone())),
                logo_service: Some(Arc::new(self.logo_service.clone())),
                iterator_registry: None,
            };

            match plugin.execute_m3u_generation(&stage_context, numbered_channels.clone()).await {
                Ok(m3u_content) => {
                    let stage_info = memory_context.complete_stage("m3u_generation")?;
                    
                    // Check if plugin returned empty content - fall back to native strategy
                    if m3u_content.is_empty() && !numbered_channels.is_empty() {
                        warn!(
                            "WASM plugin returned empty content, falling back to built-in strategy:\n\
                             ├─ Plugin: {} v{}\n\
                             ├─ M3U content size: {} bytes\n\
                             ├─ Memory used: {:.1}MB\n\
                             ├─ Processing time: {}ms\n\
                             ├─ Memory pressure: {:?}\n\
                             └─ Fallback: Using built-in M3U generation strategy",
                            plugin.get_info().name,
                            plugin.get_info().version,
                            m3u_content.len(),
                            stage_info.memory_delta_mb,
                            stage_info.duration_ms,
                            stage_info.pressure_level
                        );
                        
                        // Continue to fallback strategy below
                    } else {
                        info!(
                            "WASM plugin execution succeeded:\n\
                             ├─ Plugin: {} v{}\n\
                             ├─ M3U content size: {} bytes\n\
                             ├─ Memory used: {:.1}MB\n\
                             ├─ Processing time: {}ms\n\
                             └─ Memory pressure: {:?}",
                            plugin.get_info().name,
                            plugin.get_info().version,
                            m3u_content.len(),
                            stage_info.memory_delta_mb,
                            stage_info.duration_ms,
                            stage_info.pressure_level
                        );
                        
                        return Ok(m3u_content);
                    }
                }
                Err(e) => {
                    warn!(
                        "WASM plugin execution failed, falling back to built-in strategy:\n\
                         ├─ Plugin: {} \n\
                         ├─ Error: {}\n\
                         ├─ Stage: m3u_generation\n\
                         ├─ Fallback: Using built-in M3U generation strategy\n\
                         └─ Reason: Plugin execution error",
                        plugin.get_info().name,
                        e
                    );
                }
            }
        }
        } // Close the should_use_plugin_for_stage("m3u_generation") block

        // Use simple strategy (default)
        let strategy = SimpleM3uGenerator;
        let input = M3uGenerationInput {
            numbered_channels: numbered_channels.clone(),
            proxy_ulid: config.proxy.ulid.clone(),
            base_url: base_url.to_string(),
        };

        let mut output = strategy.execute(input, memory_context).await?;
        let stage_info = memory_context.complete_stage("m3u_generation")?;

        info!(
            "M3U generation completed: {} bytes, {:.1}MB memory used",
            output.m3u_content.len(),
            stage_info.memory_delta_mb
        );

        // Final cleanup
        cleanup_coordinator.cleanup_between_stages(
            "m3u_generation",
            &mut output,
            crate::utils::CleanupStrategy::Aggressive,
        )?;

        Ok(output.m3u_content)
    }

    /// Get current memory pressure level
    fn get_current_memory_pressure(&self, memory_context: &MemoryContext) -> MemoryPressureLevel {
        if let Some(ref monitor) = self.memory_monitor {
            match monitor.check_memory_limit() {
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

    /// Get plugin manager statistics
    pub async fn get_plugin_statistics(
        &self,
    ) -> Result<std::collections::HashMap<String, serde_json::Value>> {
        Ok(self.plugin_manager.get_statistics())
    }

    /// Reload plugins if hot reload is enabled
    pub async fn reload_plugins(&self) -> Result<()> {
        self.plugin_manager.reload_plugins().await
    }

    /// Register logo-enriched channels as an immutable source in the iterator registry
    async fn register_logo_enriched_channels(&self, channels: &[Channel]) -> Result<()> {
        if channels.is_empty() {
            debug!("No channels to register in iterator registry");
            return Ok(());
        }

        // Create immutable source directly from channels
        let immutable_source = Arc::new(ImmutableLogoEnrichedChannelSource::new(
            channels.to_vec(),
            IteratorType::LogoChannels,
        ));

        // Register in the iterator registry
        self.iterator_registry.register_channel_source(
            "logo_channels".to_string(),
            immutable_source,
        )?;

        // Calculate logo enrichment statistics
        let logo_enriched_count = channels.iter()
            .filter(|ch| ch.tvg_logo.is_some())
            .count();

        info!(
            "Registered {} logo-enriched channels in iterator registry ({} with logos, {:.1}% enriched)",
            channels.len(),
            logo_enriched_count,
            if channels.len() > 0 { (logo_enriched_count as f64 / channels.len() as f64) * 100.0 } else { 0.0 }
        );

        Ok(())
    }
}

/// Builder for creating adaptive pipelines
pub struct AdaptivePipelineBuilder {
    database: Option<Database>,
    data_mapping_service: Option<DataMappingService>,
    logo_service: Option<LogoAssetService>,
    memory_limit_mb: Option<usize>,
    temp_file_manager: Option<SandboxedManager>,
    shared_plugin_manager: Option<std::sync::Arc<crate::plugins::pipeline::wasm::WasmPluginManager>>,
    shared_memory_monitor: Option<SimpleMemoryMonitor>,
}

impl AdaptivePipelineBuilder {
    pub fn new() -> Self {
        Self {
            database: None,
            data_mapping_service: None,
            logo_service: None,
            memory_limit_mb: None,
            temp_file_manager: None,
            shared_plugin_manager: None,
            shared_memory_monitor: None,
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

    pub fn with_shared_plugin_manager(mut self, plugin_manager: std::sync::Arc<crate::plugins::pipeline::wasm::WasmPluginManager>) -> Self {
        self.shared_plugin_manager = Some(plugin_manager);
        self
    }

    pub fn with_shared_memory_monitor(mut self, memory_monitor: SimpleMemoryMonitor) -> Self {
        self.shared_memory_monitor = Some(memory_monitor);
        self
    }

    pub fn build(self) -> Result<AdaptivePipeline> {
        Ok(AdaptivePipeline::new(
            self.database
                .ok_or_else(|| anyhow::anyhow!("Database is required"))?,
            self.data_mapping_service
                .ok_or_else(|| anyhow::anyhow!("Data mapping service is required"))?,
            self.logo_service
                .ok_or_else(|| anyhow::anyhow!("Logo service is required"))?,
            self.memory_limit_mb,
            self.temp_file_manager
                .ok_or_else(|| anyhow::anyhow!("Temp file manager is required"))?,
            self.shared_plugin_manager,
            self.shared_memory_monitor,
        ))
    }
}

impl Default for AdaptivePipelineBuilder {
    fn default() -> Self {
        Self::new()
    }
}
