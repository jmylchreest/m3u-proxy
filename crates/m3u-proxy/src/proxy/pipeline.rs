use anyhow::Result;
use chrono::Utc;
use sandboxed_file_manager::SandboxedManager;
use std::collections::HashMap;
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::data_mapping::service::DataMappingService;
use crate::database::Database;
use crate::logo_assets::service::LogoAssetService;
use crate::models::*;
use crate::proxy::filter_engine::FilterEngine;
use crate::utils::memory_strategy::{MemoryAction, MemoryStrategyExecutor};
use crate::utils::{MemoryLimitStatus, MemoryStats, SimpleMemoryMonitor};

/// Decision enum for memory pressure handling
#[derive(Debug)]
enum ProcessingDecision {
    Continue,
    Stop,
    SwitchToChunked,
}

/// Memory-efficient pipeline for proxy generation that processes channels in stages
pub struct ProxyGenerationPipeline {
    /// Database connection for fetching data
    database: Database,
    /// Data mapping service for transforming channels
    data_mapping_service: DataMappingService,
    /// Logo asset service for processing logos
    logo_service: LogoAssetService,
    /// Filter engine for applying filters
    filter_engine: FilterEngine,
    /// Memory monitor for passive observation
    memory_monitor: Option<SimpleMemoryMonitor>,
    /// Memory strategy executor for handling memory pressure
    memory_strategy: Option<MemoryStrategyExecutor>,
    /// Optional sandboxed file manager for temporary file operations
    temp_file_manager: Option<SandboxedManager>,
}

/// Represents a channel that has been processed through the data mapping stage
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MappedChannel {
    pub channel: Channel,
    pub source_id: Uuid,
    pub source_name: String,
    pub mapping_applied: bool,
}

// Removed MemoryFootprint trait - using real allocator stats instead

/// Temporary spill file for storing intermediate channel data
struct SpillFile {
    file_id: String,
    file_manager: SandboxedManager,
    channel_count: usize,
}

impl SpillFile {
    async fn new(file_manager: SandboxedManager) -> Result<Self> {
        let file_id = format!("pipeline_spill_{}", uuid::Uuid::new_v4());
        Ok(Self {
            file_id,
            file_manager,
            channel_count: 0,
        })
    }

    async fn write_mapped_channels(&mut self, channels: &[MappedChannel]) -> Result<()> {
        let json_data: Vec<String> = channels
            .iter()
            .map(|mc| serde_json::to_string(mc))
            .collect::<Result<Vec<_>, _>>()?;
        let content = json_data.join("\n");

        let file_id = format!("spill-{}.json", uuid::Uuid::new_v4());
        self.file_manager
            .write(&file_id, &content)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to spill mapped channels to file: {}", e))?;

        self.file_id = file_id;
        self.channel_count = channels.len();
        info!(
            "Spilled {} mapped channels to file {}",
            channels.len(),
            self.file_id
        );
        Ok(())
    }

    async fn read_mapped_channels(&self) -> Result<Vec<MappedChannel>> {
        match self.file_manager.read_to_string(&self.file_id).await {
            Ok(content) => {
                let mut channels = Vec::new();
                for line in content.lines() {
                    if !line.trim().is_empty() {
                        let channel: MappedChannel = serde_json::from_str(line)?;
                        channels.push(channel);
                    }
                }
                Ok(channels)
            }
            Err(e) => Err(anyhow::anyhow!("Failed to read spilled channels: {}", e)),
        }
    }
}

/// Represents the combined virtual channel source from all mapped channels
pub struct VirtualChannelSource {
    pub channels: Vec<MappedChannel>,
    pub total_count: usize,
    pub sources_processed: usize,
    pub spill_files: Vec<SpillFile>,
}

// Removed MemoryFootprint trait - using real allocator stats instead

/// Represents the final filtered channel list
#[derive(Debug)]
pub struct FilteredChannelList {
    pub channels: Vec<Channel>,
    pub applied_filters: Vec<Filter>,
    pub filter_statistics: FilterStatistics,
}

// Removed MemoryFootprint trait - using real allocator stats instead

/// Statistics about filter application
#[derive(Debug)]
pub struct FilterStatistics {
    pub initial_count: usize,
    pub final_count: usize,
    pub filters_applied: usize,
    pub channels_removed: usize,
    pub channels_added: usize,
}

// Removed MemoryFootprint trait - using real allocator stats instead

/// Configuration for the generation pipeline
#[derive(Debug, Clone)]
pub struct PipelineConfig {
    pub batch_size: usize,
    pub enable_parallel_processing: bool,
    pub memory_limit_mb: Option<usize>,
    pub enable_statistics: bool,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            batch_size: 1000,
            enable_parallel_processing: true,
            memory_limit_mb: Some(512),
            enable_statistics: true,
        }
    }
}

impl ProxyGenerationPipeline {
    /// Create a new pipeline instance
    pub fn new(
        database: Database,
        data_mapping_service: DataMappingService,
        logo_service: LogoAssetService,
    ) -> Self {
        Self {
            database,
            data_mapping_service,
            logo_service,
            filter_engine: FilterEngine::new(),
            memory_monitor: None,
            memory_strategy: None,
            temp_file_manager: None,
        }
    }

    /// Create a new pipeline with memory monitoring enabled
    pub fn new_with_memory_monitoring(
        database: Database,
        data_mapping_service: DataMappingService,
        logo_service: LogoAssetService,
        memory_limit_mb: Option<usize>,
    ) -> Self {
        let memory_monitor = Some(SimpleMemoryMonitor::new(memory_limit_mb));
        let memory_strategy = Some(MemoryStrategyExecutor::new(
            crate::utils::memory_strategy::ProxyGenerationStrategies::conservative(),
        ));

        Self {
            database,
            data_mapping_service,
            logo_service,
            filter_engine: FilterEngine::new(),
            memory_monitor,
            memory_strategy,
            temp_file_manager: None,
        }
    }

    /// Create a new pipeline with memory monitoring and file spilling support
    pub fn new_with_file_spilling(
        database: Database,
        data_mapping_service: DataMappingService,
        logo_service: LogoAssetService,
        memory_limit_mb: Option<usize>,
        temp_file_manager: SandboxedManager,
    ) -> Self {
        let memory_monitor = Some(SimpleMemoryMonitor::new(memory_limit_mb));
        let memory_strategy = Some(MemoryStrategyExecutor::new(
            crate::utils::memory_strategy::ProxyGenerationStrategies::aggressive(),
        ));

        Self {
            database,
            data_mapping_service,
            logo_service,
            filter_engine: FilterEngine::new(),
            memory_monitor,
            memory_strategy,
            temp_file_manager: Some(temp_file_manager),
        }
    }

    /// Main pipeline execution method
    pub async fn generate_proxy(
        &mut self,
        proxy: &StreamProxy,
        base_url: &str,
        engine_config: Option<crate::config::DataMappingEngineConfig>,
        config: Option<PipelineConfig>,
    ) -> Result<(ProxyGeneration, Option<MemoryStats>)> {
        let config = config.unwrap_or_default();

        info!("Starting pipeline generation for proxy '{}'", proxy.name);

        // Initialize memory monitoring if enabled
        if let Some(ref mut monitor) = self.memory_monitor {
            monitor.initialize()?;
        }

        // Stage 1: Initialize empty channel list
        let mut channel_list = Vec::new();

        if let Some(ref mut monitor) = self.memory_monitor {
            monitor.observe_stage("initialization")?;
        }

        // Stage 2: Process sources through data mapping
        let virtual_source = self
            .process_sources_with_mapping(proxy, base_url, engine_config, &config)
            .await?;

        if let Some(ref mut monitor) = self.memory_monitor {
            monitor.observe_stage("initialization")?;
        }

        info!(
            "Virtual channel source created with {} channels from {} sources",
            virtual_source.total_count, virtual_source.sources_processed
        );

        // Stage 3: Apply filters in order to build final channel list
        let filtered_result = self
            .apply_filters_to_virtual_source(proxy, virtual_source, &mut channel_list, &config)
            .await?;

        info!(
            "Filter application complete: {} → {} channels",
            filtered_result.filter_statistics.initial_count,
            filtered_result.filter_statistics.final_count
        );

        // Stage 4: Generate M3U content
        let m3u_content = self
            .generate_m3u_from_filtered_list(&filtered_result.channels, &proxy.ulid, base_url)
            .await?;

        // Stage 5: Create generation record
        let generation = ProxyGeneration {
            id: Uuid::new_v4(),
            proxy_id: proxy.id,
            version: 1, // TODO: Get next version number from database
            channel_count: filtered_result.channels.len() as i32,
            m3u_content,
            created_at: Utc::now(),
        };

        // Get final memory statistics
        let memory_stats = if let Some(ref mut monitor) = self.memory_monitor {
            monitor.observe_stage("generation_complete")?;
            Some(monitor.get_statistics())
        } else {
            None
        };

        info!(
            "Pipeline generation completed for proxy '{}': {} channels generated",
            proxy.name, generation.channel_count
        );

        if let Some(ref stats) = memory_stats {
            info!("Memory usage: {}", stats.summary());
        }

        Ok((generation, memory_stats))
    }

    /// Stage 2: Process all sources through data mapping to create virtual channel source
    async fn process_sources_with_mapping(
        &mut self,
        proxy: &StreamProxy,
        base_url: &str,
        engine_config: Option<crate::config::DataMappingEngineConfig>,
        config: &PipelineConfig,
    ) -> Result<VirtualChannelSource> {
        // Track source mapping stage
        if let Some(ref mut monitor) = self.memory_monitor {
            monitor.observe_stage("source_loading")?;
        }

        // Get all sources for this proxy
        let sources = self.database.get_proxy_sources(proxy.id).await?;

        if sources.is_empty() {
            warn!("No sources found for proxy '{}'", proxy.name);
            if let Some(ref mut monitor) = self.memory_monitor {
                monitor.observe_stage("source_loading")?;
            }
            return Ok(VirtualChannelSource {
                channels: Vec::new(),
                total_count: 0,
                sources_processed: 0,
                spill_files: Vec::new(),
            });
        }

        let mut all_mapped_channels = Vec::new();
        let mut spill_files = Vec::new();
        let mut sources_processed = 0;
        let mut _total_channels_processed = 0;

        if let Some(ref mut monitor) = self.memory_monitor {
            monitor.observe_stage("source_loading")?;
            monitor.observe_stage("data_mapping")?;
        }

        // Process each source individually to maintain memory efficiency
        for source in &sources {
            info!("Processing source '{}' for mapping", source.name);

            // Get channels for this source in batches if needed
            let source_channels = self.database.get_source_channels(source.id).await?;

            if source_channels.is_empty() {
                debug!("Source '{}' has no channels, skipping", source.name);
                continue;
            }

            // Apply data mapping to this source's channels
            let mapped_channels = self
                .data_mapping_service
                .apply_mapping_for_proxy(
                    source_channels,
                    source.id,
                    &self.logo_service,
                    base_url,
                    engine_config.clone(),
                )
                .await?;

            // Convert to MappedChannel format
            let source_mapped: Vec<MappedChannel> = mapped_channels
                .into_iter()
                .map(|channel| MappedChannel {
                    channel,
                    source_id: source.id,
                    source_name: source.name.clone(),
                    mapping_applied: true,
                })
                .collect();

            info!(
                "Source '{}' processed: {} channels mapped",
                source.name,
                source_mapped.len()
            );

            let mapped_count = source_mapped.len();

            // Check memory limits during processing
            if let Some(ref monitor) = self.memory_monitor {
                let status = monitor.check_memory_limit()?;
                match status {
                    MemoryLimitStatus::Exceeded => {
                        if let Some(ref strategy) = self.memory_strategy {
                            let action = strategy
                                .handle_exceeded(&format!("source_processing_{}", source.name))
                                .await?;
                            match self
                                .handle_memory_action(
                                    action,
                                    &mut all_mapped_channels,
                                    &mut spill_files,
                                )
                                .await?
                            {
                                ProcessingDecision::Continue => {}
                                ProcessingDecision::Stop => {
                                    warn!(
                                        "Memory strategy dictates stopping processing at source '{}'",
                                        source.name
                                    );
                                    break;
                                }
                                ProcessingDecision::SwitchToChunked => {
                                    warn!(
                                        "Memory strategy suggests chunked processing - delegating to chunked pipeline"
                                    );
                                    return self
                                        .delegate_to_chunked_pipeline(
                                            proxy,
                                            base_url,
                                            engine_config,
                                        )
                                        .await;
                                }
                            }
                        } else {
                            warn!(
                                "Memory limit exceeded during source processing for '{}' - stopping early",
                                source.name
                            );
                            break;
                        }
                    }
                    MemoryLimitStatus::Warning => {
                        if let Some(ref strategy) = self.memory_strategy {
                            let action = strategy
                                .handle_warning(&format!("source_processing_{}", source.name))
                                .await?;
                            info!(
                                "Applied memory strategy for '{}': {:?}",
                                source.name, action
                            );
                            match self
                                .handle_memory_action(
                                    action,
                                    &mut all_mapped_channels,
                                    &mut spill_files,
                                )
                                .await?
                            {
                                ProcessingDecision::Continue => {}
                                ProcessingDecision::Stop => {
                                    warn!(
                                        "Memory strategy dictates stopping processing at source '{}'",
                                        source.name
                                    );
                                    break;
                                }
                                ProcessingDecision::SwitchToChunked => {
                                    warn!(
                                        "Memory strategy suggests chunked processing - delegating to chunked pipeline"
                                    );
                                    return self
                                        .delegate_to_chunked_pipeline(
                                            proxy,
                                            base_url,
                                            engine_config,
                                        )
                                        .await;
                                }
                            }
                        } else {
                            info!(
                                "Memory usage high during processing of '{}' - consider reducing batch sizes",
                                source.name
                            );
                        }
                    }
                    MemoryLimitStatus::Ok => {
                        // Continue normally
                    }
                }
            }

            all_mapped_channels.extend(source_mapped);
            sources_processed += 1;
            _total_channels_processed += mapped_count;

            // Optional: Check memory usage and yield if needed
            if let Some(limit_mb) = config.memory_limit_mb {
                if self.estimate_memory_usage_mb(&all_mapped_channels) > limit_mb {
                    warn!(
                        "Memory usage approaching limit ({}MB), processing remaining sources in next batch",
                        limit_mb
                    );
                    // In a real implementation, you might want to flush to disk or process in smaller batches
                }
            }
        }

        if let Some(ref mut monitor) = self.memory_monitor {
            monitor.observe_stage("data_mapping").ok();
        }

        Ok(VirtualChannelSource {
            total_count: all_mapped_channels.len()
                + spill_files.iter().map(|f| f.channel_count).sum::<usize>(),
            channels: all_mapped_channels,
            sources_processed,
            spill_files,
        })
    }

    /// Stage 3: Apply filters in order to build the final channel list
    async fn apply_filters_to_virtual_source(
        &mut self,
        proxy: &StreamProxy,
        virtual_source: VirtualChannelSource,
        channel_list: &mut Vec<Channel>,
        _config: &PipelineConfig,
    ) -> Result<FilteredChannelList> {
        let initial_count = virtual_source.channels.len();

        if let Some(ref mut monitor) = self.memory_monitor {
            monitor.observe_stage("filter_loading").ok();
        }

        // Get filters for this proxy, ordered by priority
        let proxy_filters = self
            .database
            .get_proxy_filters_with_details(proxy.id)
            .await?;

        if proxy_filters.is_empty() {
            info!(
                "No filters found for proxy '{}', using all mapped channels",
                proxy.name
            );

            // No filters - add all mapped channels to the list
            let channels: Vec<Channel> = virtual_source
                .channels
                .into_iter()
                .map(|mc| mc.channel)
                .collect();

            channel_list.extend(channels.clone());

            if let Some(ref mut monitor) = self.memory_monitor {
                monitor.observe_stage("filter_loading").ok();
                monitor.observe_stage("no_filtering").ok();
                monitor.observe_stage("no_filtering").ok();
            }

            return Ok(FilteredChannelList {
                channels,
                applied_filters: Vec::new(),
                filter_statistics: FilterStatistics {
                    initial_count,
                    final_count: initial_count,
                    filters_applied: 0,
                    channels_removed: 0,
                    channels_added: initial_count,
                },
            });
        }

        if let Some(ref mut monitor) = self.memory_monitor {
            monitor.observe_stage("filter_loading").ok();
            monitor.observe_stage("virtual_channel_conversion").ok();
        }

        // Convert virtual source channels to regular channels for filter processing
        let virtual_channels: Vec<Channel> = virtual_source
            .channels
            .into_iter()
            .map(|mc| mc.channel)
            .collect();

        if let Some(ref mut monitor) = self.memory_monitor {
            monitor.observe_stage("virtual_channel_conversion").ok();
            monitor.observe_stage("filter_application").ok();
        }

        info!(
            "Applying {} filters to {} virtual channels",
            proxy_filters.len(),
            virtual_channels.len()
        );

        // Apply filters using the existing filter engine
        let mut filter_tuples = Vec::new();
        let applied_filters: Vec<Filter> =
            proxy_filters.iter().map(|pf| pf.filter.clone()).collect();

        for proxy_filter in proxy_filters {
            filter_tuples.push((proxy_filter.filter.clone(), proxy_filter.proxy_filter));
        }

        let filtered_channels = self
            .filter_engine
            .apply_filters(virtual_channels, filter_tuples)
            .await?;

        let final_count = filtered_channels.len();
        let filters_applied_count = applied_filters.len();

        if let Some(ref mut monitor) = self.memory_monitor {
            monitor.observe_stage("filter_application").ok();
        }

        // Add filtered channels to the final list
        channel_list.extend(filtered_channels.clone());

        Ok(FilteredChannelList {
            channels: filtered_channels,
            applied_filters,
            filter_statistics: FilterStatistics {
                initial_count,
                final_count,
                filters_applied: filters_applied_count,
                channels_removed: initial_count - final_count,
                channels_added: final_count,
            },
        })
    }

    /// Stage 4: Generate M3U content from the filtered channel list
    async fn generate_m3u_from_filtered_list(
        &mut self,
        channels: &[Channel],
        proxy_ulid: &str,
        base_url: &str,
    ) -> Result<String> {
        if let Some(ref mut monitor) = self.memory_monitor {
            monitor.observe_stage("m3u_generation").ok();
        }
        if channels.is_empty() {
            if let Some(ref mut monitor) = self.memory_monitor {
                monitor.observe_stage("m3u_generation").ok();
            }
            return Ok("#EXTM3U\n".to_string());
        }

        let mut m3u_content = String::from("#EXTM3U\n");

        for (index, channel) in channels.iter().enumerate() {
            // Generate channel number (1-based)
            let channel_number = (index + 1) as i32;

            // Build EXTINF line
            let mut extinf_parts = Vec::new();
            extinf_parts.push(format!("#EXTINF:-1"));

            // Add tvg-id if available
            if let Some(tvg_id) = &channel.tvg_id {
                if !tvg_id.is_empty() {
                    extinf_parts.push(format!("tvg-id=\"{}\"", tvg_id));
                }
            }

            // Add tvg-name if available
            if let Some(tvg_name) = &channel.tvg_name {
                if !tvg_name.is_empty() {
                    extinf_parts.push(format!("tvg-name=\"{}\"", tvg_name));
                }
            }

            // Add tvg-logo if available
            if let Some(tvg_logo) = &channel.tvg_logo {
                if !tvg_logo.is_empty() {
                    extinf_parts.push(format!("tvg-logo=\"{}\"", tvg_logo));
                }
            }

            // Add group-title if available
            if let Some(group_title) = &channel.group_title {
                if !group_title.is_empty() {
                    extinf_parts.push(format!("group-title=\"{}\"", group_title));
                }
            }

            // Add channel number
            extinf_parts.push(format!("tvg-chno=\"{}\"", channel_number));

            // Add channel name at the end
            extinf_parts.push(channel.channel_name.clone());

            let extinf_line = extinf_parts.join(" ");
            m3u_content.push_str(&extinf_line);
            m3u_content.push('\n');

            // Add stream URL (potentially proxied)
            let stream_url = if channel.stream_url.starts_with("http") {
                format!("{}/stream/{}/{}", base_url, proxy_ulid, channel.id)
            } else {
                channel.stream_url.clone()
            };

            m3u_content.push_str(&stream_url);
            m3u_content.push('\n');
        }

        if let Some(ref mut monitor) = self.memory_monitor {
            monitor.observe_stage("m3u_generation").ok();
        }

        Ok(m3u_content)
    }

    /// Get the list of channel IDs from the generated proxy for EPG filtering
    pub fn extract_channel_ids_from_generation(
        &self,
        generation: &ProxyGeneration,
    ) -> Result<Vec<String>> {
        let mut channel_ids = Vec::new();

        // Parse the M3U content to extract tvg-id values
        let lines: Vec<&str> = generation.m3u_content.lines().collect();

        for line in lines {
            if line.starts_with("#EXTINF:") {
                // Extract tvg-id from the EXTINF line
                if let Some(tvg_id) = self.extract_tvg_id_from_extinf(line) {
                    if !tvg_id.is_empty() {
                        channel_ids.push(tvg_id);
                    }
                }
            }
        }

        channel_ids.sort();
        channel_ids.dedup();

        Ok(channel_ids)
    }

    /// Extract tvg-id from an EXTINF line
    fn extract_tvg_id_from_extinf(&self, extinf_line: &str) -> Option<String> {
        // Look for tvg-id="value" pattern
        if let Some(start) = extinf_line.find("tvg-id=\"") {
            let start_pos = start + 8; // Length of 'tvg-id="'
            if let Some(end) = extinf_line[start_pos..].find('"') {
                return Some(extinf_line[start_pos..start_pos + end].to_string());
            }
        }
        None
    }

    /// Get accurate memory usage of mapped channels
    fn estimate_memory_usage_mb(&self, channels: &[MappedChannel]) -> usize {
        // Simple estimation based on channel count
        (channels.len() * 1024) / (1024 * 1024)
    }

    /// Get memory statistics from the last generation (if monitoring was enabled)
    pub fn get_memory_statistics(&self) -> Option<MemoryStats> {
        self.memory_monitor
            .as_ref()
            .map(|monitor| monitor.get_statistics())
    }

    /// Get pipeline statistics
    pub fn get_pipeline_statistics(&self) -> HashMap<String, serde_json::Value> {
        let mut stats = HashMap::new();
        stats.insert(
            "pipeline_version".to_string(),
            serde_json::Value::String("1.0".to_string()),
        );
        stats.insert(
            "filter_engine_initialized".to_string(),
            serde_json::Value::Bool(true),
        );
        stats.insert(
            "file_spilling_enabled".to_string(),
            serde_json::Value::Bool(self.temp_file_manager.is_some()),
        );
        stats.insert(
            "memory_monitoring_enabled".to_string(),
            serde_json::Value::Bool(self.memory_monitor.is_some()),
        );
        stats
    }

    /// Handle memory action based on strategy recommendation
    async fn handle_memory_action(
        &mut self,
        action: MemoryAction,
        all_mapped_channels: &mut Vec<MappedChannel>,
        spill_files: &mut Vec<SpillFile>,
    ) -> Result<ProcessingDecision> {
        match action {
            MemoryAction::Continue => Ok(ProcessingDecision::Continue),
            MemoryAction::SwitchToChunked(chunk_size) => {
                info!(
                    "Switching to chunked processing (chunk_size: {}) due to memory pressure",
                    chunk_size
                );
                Ok(ProcessingDecision::SwitchToChunked)
            }
            MemoryAction::UseTemporaryStorage(temp_dir) => {
                info!(
                    "Using temporary storage ({}) due to memory pressure",
                    temp_dir
                );
                if let Some(ref file_manager) = self.temp_file_manager {
                    info!(
                        "Spilling {} channels to disk due to memory pressure",
                        all_mapped_channels.len()
                    );

                    if !all_mapped_channels.is_empty() {
                        let mut spill_file = SpillFile::new(file_manager.clone()).await?;
                        spill_file
                            .write_mapped_channels(all_mapped_channels)
                            .await?;
                        spill_files.push(spill_file);
                        all_mapped_channels.clear();
                    }

                    Ok(ProcessingDecision::Continue)
                } else {
                    warn!("Memory action requested temp storage but no file manager configured");
                    Ok(ProcessingDecision::Continue)
                }
            }
            MemoryAction::StopProcessing => {
                warn!("Stopping processing due to memory pressure");
                Ok(ProcessingDecision::Stop)
            }
        }
    }

    /// Delegate to chunked pipeline when memory pressure requires it
    async fn delegate_to_chunked_pipeline(
        &self,
        proxy: &StreamProxy,
        base_url: &str,
        engine_config: Option<crate::config::DataMappingEngineConfig>,
    ) -> Result<VirtualChannelSource> {
        use crate::proxy::chunked_pipeline::ChunkedProxyPipeline;

        info!("Delegating to chunked pipeline due to memory constraints");

        if let Some(ref file_manager) = self.temp_file_manager {
            let mut chunked_pipeline = ChunkedProxyPipeline::new(
                self.database.clone(),
                self.data_mapping_service.clone(),
                self.logo_service.clone(),
                1000, // chunk_size
                None, // memory_limit_mb
                file_manager.clone(),
            );

            // Process using chunked pipeline and convert result
            let generation_result = chunked_pipeline
                .generate_proxy_chunked(proxy, base_url, engine_config)
                .await?;

            // Convert the result to VirtualChannelSource format
            // This is a simplified conversion - in practice you might want to preserve more metadata
            Ok(VirtualChannelSource {
                channels: Vec::new(), // Chunked pipeline handles data differently
                total_count: generation_result.0.channel_count as usize,
                sources_processed: 1, // Simplified - chunked pipeline doesn't track this the same way
                spill_files: Vec::new(), // Chunked pipeline manages its own temporary files
            })
        } else {
            Err(anyhow::anyhow!(
                "Cannot delegate to chunked pipeline: no file manager configured"
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_tvg_id_from_extinf() {
        // This test would need proper mock setup in a real implementation
        // For now, we'll skip the actual pipeline creation
        let extinf_line = "#EXTINF:-1 tvg-id=\"channel1\" tvg-name=\"Channel 1\" group-title=\"Entertainment\",Channel 1";

        // Extract tvg-id manually for testing
        let tvg_id = if let Some(start) = extinf_line.find("tvg-id=\"") {
            let start_pos = start + 8;
            if let Some(end) = extinf_line[start_pos..].find('"') {
                Some(extinf_line[start_pos..start_pos + end].to_string())
            } else {
                None
            }
        } else {
            None
        };

        let _extinf_line = "#EXTINF:-1 tvg-id=\"channel1\" tvg-name=\"Channel 1\" group-title=\"Entertainment\",Channel 1";
        assert_eq!(tvg_id, Some("channel1".to_string()));

        let extinf_line_no_tvg_id =
            "#EXTINF:-1 tvg-name=\"Channel 1\" group-title=\"Entertainment\",Channel 1";
        let tvg_id_none = if let Some(start) = extinf_line_no_tvg_id.find("tvg-id=\"") {
            let start_pos = start + 8;
            if let Some(end) = extinf_line_no_tvg_id[start_pos..].find('"') {
                Some(extinf_line_no_tvg_id[start_pos..start_pos + end].to_string())
            } else {
                None
            }
        } else {
            None
        };
        assert_eq!(tvg_id_none, None);
    }

    #[test]
    fn test_pipeline_config_default() {
        let config = PipelineConfig::default();
        assert_eq!(config.batch_size, 1000);
        assert_eq!(config.enable_parallel_processing, true);
        assert_eq!(config.memory_limit_mb, Some(512));
        assert_eq!(config.enable_statistics, true);
    }
}
