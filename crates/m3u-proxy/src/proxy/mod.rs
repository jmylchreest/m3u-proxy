use anyhow::Result;
use tracing::{debug, info, warn};

use crate::config::StorageConfig;
use crate::data_mapping::service::DataMappingService;
use crate::database::Database;
use crate::logo_assets::service::LogoAssetService;
use crate::models::*;
use crate::utils::{CleanupStrategy, MemoryCleanupCoordinator, MemoryContext};

pub mod config_resolver;
pub mod epg_generator;
pub mod filter_engine;
pub mod generator;
pub mod simple_strategies;
pub mod stage_contracts;
pub mod stage_strategy;
pub mod streaming_pipeline;
pub mod streaming_stages;
pub mod wasm_host_interface;
// WASM plugin examples and strategies
pub mod wasm_examples;

pub struct ProxyService {
    storage_config: StorageConfig,
}

impl ProxyService {
    pub fn new(storage_config: StorageConfig) -> Self {
        Self { storage_config }
    }

    /// Generate a proxy using the sequential stage architecture (THE pipeline)
    pub async fn generate_proxy_with_config(
        &self,
        config: ResolvedProxyConfig,
        output: GenerationOutput,
        database: &Database,
        data_mapping_service: &DataMappingService,
        logo_service: &LogoAssetService,
        base_url: &str,
        engine_config: Option<crate::config::DataMappingEngineConfig>,
    ) -> Result<ProxyGeneration> {
        use crate::proxy::simple_strategies::*;
        use crate::proxy::stage_contracts::*;
        use std::time::Instant;

        info!(
            "Starting generation proxy={} pipeline=sequential sources={} filters={}",
            config.proxy.name,
            config.sources.len(),
            config.filters.len()
        );
        let overall_start = Instant::now();

        // Initialize unified memory context
        let mut memory_context = MemoryContext::new(Some(512), None);
        memory_context.initialize()?;

        // Initialize memory cleanup coordinator
        let mut cleanup_coordinator = MemoryCleanupCoordinator::new(true, Some(512.0)); // 512MB threshold

        // Create simple in-memory strategies (default behavior)
        let source_loader = SimpleSourceLoader::new(database.clone());
        let data_mapper = SimpleDataMapper::new(data_mapping_service.clone(), logo_service.clone());
        let filter = SimpleFilter;
        let numbering = SimpleChannelNumbering;
        let m3u_generator = SimpleM3uGenerator;

        // Stage 1: Source Loading
        let source_input = SourceLoadingInput {
            source_ids: config.sources.iter().map(|s| s.source.id).collect(),
            proxy_config: config.clone(),
        };

        memory_context.start_stage("source_loading")?;
        let mut source_output = source_loader
            .execute(source_input, &mut memory_context)
            .await?;
        let _stage_info = memory_context.complete_stage("source_loading")?;
        info!(
            "Stage progress stage=source_loading status=completed execution_time={}ms channels={} memory_delta=+{:.1}MB memory_peak={:.1}MB memory_pressure={:?}",
            source_output.total_stats.load_duration_ms,
            source_output.channels.len(),
            _stage_info.memory_delta_mb,
            _stage_info.memory_after_mb,
            _stage_info.pressure_level
        );

        // Cleanup memory after source loading if needed
        if memory_context.should_cleanup()? {
            let _cleanup_stats = cleanup_coordinator.cleanup_between_stages(
                "source_loading",
                &mut source_output,
                CleanupStrategy::Basic,
            )?;
        }

        // Stage 2: Data Mapping
        let mapping_input = DataMappingInput {
            channels: source_output.channels,
            source_configs: config.sources.clone(),
            engine_config: engine_config.clone(),
            base_url: base_url.to_string(),
        };

        memory_context.start_stage("data_mapping")?;
        let mut mapping_output = data_mapper
            .execute(mapping_input, &mut memory_context)
            .await?;
        let _mapping_stage_info = memory_context.complete_stage("data_mapping")?;
        info!(
            "Stage progress stage=data_mapping status=completed execution_time={}ms channels={} memory_delta=+{:.1}MB memory_peak={:.1}MB memory_pressure={:?}",
            mapping_output.mapping_stats.mapping_duration_ms,
            mapping_output.mapped_channels.len(),
            _mapping_stage_info.memory_delta_mb,
            _mapping_stage_info.memory_after_mb,
            _mapping_stage_info.pressure_level
        );

        // Cleanup memory after data mapping if needed
        if memory_context.should_cleanup()? {
            let _cleanup_stats = cleanup_coordinator.cleanup_between_stages(
                "data_mapping",
                &mut mapping_output,
                CleanupStrategy::Basic,
            )?;
        }

        // Stage 3: Filtering
        let filtering_input = FilteringInput {
            channels: mapping_output.mapped_channels,
            filters: config.filters.clone(),
        };

        memory_context.start_stage("filtering")?;
        let mut filtering_output = filter.execute(filtering_input, &mut memory_context).await?;
        let _filtering_stage_info = memory_context.complete_stage("filtering")?;
        info!(
            "Stage progress stage=filtering status=completed execution_time={}ms channels={} memory_delta=+{:.1}MB memory_peak={:.1}MB memory_pressure={:?}",
            filtering_output.filter_stats.filter_duration_ms,
            filtering_output.filtered_channels.len(),
            _filtering_stage_info.memory_delta_mb,
            _filtering_stage_info.memory_after_mb,
            _filtering_stage_info.pressure_level
        );

        // Cleanup memory after filtering if needed
        if memory_context.should_cleanup()? {
            let _cleanup_stats = cleanup_coordinator.cleanup_between_stages(
                "filtering",
                &mut filtering_output,
                CleanupStrategy::Basic,
            )?;
        }

        // Stage 4: Channel Numbering
        let numbering_input = ChannelNumberingInput {
            channels: filtering_output.filtered_channels,
            starting_number: config.proxy.starting_channel_number,
            numbering_strategy: ChannelNumberingStrategy::Sequential,
        };

        memory_context.start_stage("channel_numbering")?;
        let mut numbering_output = numbering
            .execute(numbering_input, &mut memory_context)
            .await?;
        let _numbering_stage_info = memory_context.complete_stage("channel_numbering")?;
        info!(
            "Stage progress stage=channel_numbering status=completed execution_time={}ms channels={} memory_delta={:.1}MB memory_peak={:.1}MB memory_pressure={:?}",
            numbering_output.numbering_stats.numbering_duration_ms,
            numbering_output.numbered_channels.len(),
            _numbering_stage_info.memory_delta_mb,
            _numbering_stage_info.memory_after_mb,
            _numbering_stage_info.pressure_level
        );

        // Cleanup memory after channel numbering if needed
        if memory_context.should_cleanup()? {
            let _cleanup_stats = cleanup_coordinator.cleanup_between_stages(
                "channel_numbering",
                &mut numbering_output,
                CleanupStrategy::Basic,
            )?;
        }

        // Stage 5: M3U Generation
        let m3u_input = M3uGenerationInput {
            numbered_channels: numbering_output.numbered_channels.clone(),
            proxy_ulid: config.proxy.ulid.clone(),
            base_url: base_url.to_string(),
        };

        memory_context.start_stage("m3u_generation")?;
        let mut m3u_output = m3u_generator
            .execute(m3u_input, &mut memory_context)
            .await?;
        let _m3u_stage_info = memory_context.complete_stage("m3u_generation")?;
        info!(
            "Stage progress stage=m3u_generation status=completed execution_time={}ms bytes={} memory_delta=+{:.1}MB memory_peak={:.1}MB memory_pressure={:?}",
            m3u_output.m3u_stats.generation_duration_ms,
            m3u_output.m3u_content.len(),
            _m3u_stage_info.memory_delta_mb,
            _m3u_stage_info.memory_after_mb,
            _m3u_stage_info.pressure_level
        );

        // Final cleanup after M3U generation
        let _cleanup_stats = cleanup_coordinator.cleanup_between_stages(
            "m3u_generation",
            &mut m3u_output,
            CleanupStrategy::Aggressive,
        )?;

        // Create enhanced generation statistics
        let total_duration = overall_start.elapsed().as_millis() as u64;
        let mut stats = GenerationStats::new("sequential".to_string());

        // Add stage timings from stage outputs
        stats.add_stage_timing("source_loading", source_output.total_stats.load_duration_ms);
        stats.add_stage_timing(
            "data_mapping",
            mapping_output.mapping_stats.mapping_duration_ms,
        );
        stats.add_stage_timing(
            "filtering",
            filtering_output.filter_stats.filter_duration_ms,
        );
        stats.add_stage_timing(
            "channel_numbering",
            numbering_output.numbering_stats.numbering_duration_ms,
        );
        stats.add_stage_timing(
            "m3u_generation",
            m3u_output.m3u_stats.generation_duration_ms,
        );

        // Add memory usage from context stage progression
        for stage_info in memory_context.get_stage_progression() {
            stats.add_stage_memory(
                &stage_info.stage_name,
                (stage_info.memory_after_mb * 1024.0 * 1024.0) as u64,
            );
        }

        // Get overall memory statistics from context
        let memory_stats = memory_context.get_memory_statistics();
        stats.peak_memory_usage_mb = Some(memory_stats.peak_mb);
        stats.average_memory_usage_mb =
            Some((memory_stats.baseline_mb + memory_stats.peak_mb) / 2.0);

        // Set channel processing metrics
        stats.total_channels_processed = source_output.total_stats.channels_loaded;
        stats.channels_mapped = mapping_output.mapping_stats.channels_transformed;
        stats.channels_after_filtering = filtering_output.filter_stats.channels_output;
        stats.channels_before_filtering = source_output.total_stats.channels_loaded;
        stats.channels_filtered_out = source_output.total_stats.channels_loaded
            - filtering_output.filter_stats.channels_output;
        stats.m3u_size_bytes = m3u_output.m3u_stats.m3u_size_bytes;

        // Set processing efficiency metrics
        if total_duration > 0 {
            stats.channels_per_second =
                (stats.total_channels_processed as f64 / total_duration as f64) * 1000.0;
            stats.average_channel_processing_ms =
                total_duration as f64 / stats.total_channels_processed as f64;
        }

        // Set source processing metrics
        stats.sources_processed = config.sources.len();
        stats.filters_applied = filtering_output.filter_stats.filters_applied.clone();

        // Set total duration manually before finalize
        stats.total_duration_ms = total_duration;
        stats.finalize();

        // Create generation record
        let generation = ProxyGeneration {
            id: uuid::Uuid::new_v4(),
            proxy_id: config.proxy.id,
            version: 1,
            channel_count: numbering_output.numbered_channels.len() as i32,
            total_channels: source_output.total_stats.channels_loaded,
            filtered_channels: filtering_output.filter_stats.channels_output,
            applied_filters: filtering_output.filter_stats.filters_applied,
            m3u_content: m3u_output.m3u_content,
            created_at: chrono::Utc::now(),
            stats: Some(stats.clone()),
        };

        // Handle output
        self.write_output(&generation, &output, &config).await?;

        info!(
            "Generation completed proxy={} total_time={}ms total_channels={} stream_sources={} epg_sources=0 filters={} channels_per_second={:.1} pipeline=sequential",
            config.proxy.name,
            total_duration,
            stats.total_channels_processed,
            config.sources.len(),
            config.filters.len(),
            stats.channels_per_second
        );

        // Enhanced tree-style stage summary
        let memory_stats = memory_context.get_memory_statistics();
        info!(
            "├─ stage=source_loading execution_time={}ms time_percentage={:.1} plugin=standard memory_footprint={:.1}MB memory_peak={:.1}MB",
            source_output.total_stats.load_duration_ms,
            (source_output.total_stats.load_duration_ms as f64 / total_duration as f64) * 100.0,
            memory_stats.peak_mb - memory_stats.baseline_mb,
            memory_stats.peak_mb
        );
        info!(
            "├─ stage=data_mapping execution_time={}ms time_percentage={:.1} plugin=standard memory_footprint={:.1}MB memory_peak={:.1}MB",
            mapping_output.mapping_stats.mapping_duration_ms,
            (mapping_output.mapping_stats.mapping_duration_ms as f64 / total_duration as f64)
                * 100.0,
            memory_context
                .get_stage_progression()
                .get(1)
                .map_or(0.0, |s| s.memory_delta_mb),
            memory_context
                .get_stage_progression()
                .get(1)
                .map_or(0.0, |s| s.memory_after_mb)
        );
        info!(
            "├─ stage=filtering execution_time={}ms time_percentage={:.1} plugin=standard memory_footprint={:.1}MB memory_peak={:.1}MB",
            filtering_output.filter_stats.filter_duration_ms,
            (filtering_output.filter_stats.filter_duration_ms as f64 / total_duration as f64)
                * 100.0,
            memory_context
                .get_stage_progression()
                .get(2)
                .map_or(0.0, |s| s.memory_delta_mb),
            memory_context
                .get_stage_progression()
                .get(2)
                .map_or(0.0, |s| s.memory_after_mb)
        );
        info!(
            "├─ stage=channel_numbering execution_time={}ms time_percentage={:.1} plugin=standard memory_footprint={:.1}MB memory_peak={:.1}MB",
            numbering_output.numbering_stats.numbering_duration_ms,
            (numbering_output.numbering_stats.numbering_duration_ms as f64 / total_duration as f64)
                * 100.0,
            memory_context
                .get_stage_progression()
                .get(3)
                .map_or(0.0, |s| s.memory_delta_mb),
            memory_context
                .get_stage_progression()
                .get(3)
                .map_or(0.0, |s| s.memory_after_mb)
        );
        info!(
            "└─ stage=m3u_generation execution_time={}ms time_percentage={:.1} plugin=standard memory_footprint={:.1}MB memory_peak={:.1}MB",
            m3u_output.m3u_stats.generation_duration_ms,
            (m3u_output.m3u_stats.generation_duration_ms as f64 / total_duration as f64) * 100.0,
            memory_context
                .get_stage_progression()
                .get(4)
                .map_or(0.0, |s| s.memory_delta_mb),
            memory_context
                .get_stage_progression()
                .get(4)
                .map_or(0.0, |s| s.memory_after_mb)
        );

        // Log comprehensive memory analysis
        debug!(
            "Memory cleanup summary: {}",
            cleanup_coordinator.get_cleanup_summary()
        );

        // Log detailed memory analysis
        let memory_analysis = memory_context.analyze_memory_patterns();
        info!(
            "Memory analysis total_growth={:.1}MB stages={} largest_impact={}:{:.1}MB trend={:?}",
            memory_analysis.total_memory_growth_mb,
            memory_analysis.total_stages,
            memory_analysis.largest_impact_stage,
            memory_analysis.largest_stage_impact_mb,
            memory_analysis.memory_efficiency_trend
        );

        if !memory_analysis.pressure_escalations.is_empty() {
            warn!(
                "Memory pressure escalations detected stages={}",
                memory_analysis
                    .pressure_escalations
                    .iter()
                    .map(|(stage, from, to)| format!("{}:{:?}→{:?}", stage, from, to))
                    .collect::<Vec<_>>()
                    .join(",")
            );
        }
        Ok(generation)
    }

    /// Handle output writing based on destination
    async fn write_output(
        &self,
        generation: &ProxyGeneration,
        output: &GenerationOutput,
        config: &ResolvedProxyConfig,
    ) -> Result<()> {
        match output {
            GenerationOutput::Preview {
                file_manager,
                proxy_name,
            } => {
                let m3u_file_id = format!("{}-{}.m3u", proxy_name, uuid::Uuid::new_v4());
                file_manager
                    .write(&m3u_file_id, &generation.m3u_content)
                    .await
                    .map_err(|e| anyhow::anyhow!("Failed to write preview M3U: {}", e))?;

                info!("Preview content written to file manager");
            }
            GenerationOutput::Production {
                file_manager,
                update_database,
            } => {
                let m3u_file_id = format!("{}-{}.m3u", config.proxy.ulid, uuid::Uuid::new_v4());
                file_manager
                    .write(&m3u_file_id, &generation.m3u_content)
                    .await
                    .map_err(|e| anyhow::anyhow!("Failed to write production M3U: {}", e))?;

                if *update_database {
                    info!("Generation record would be saved to database");
                }

                info!("Production content written to file manager");
            }
            GenerationOutput::InMemory => {
                // Do nothing - content is just returned
            }
        }
        Ok(())
    }

    /// Generate XMLTV EPG for a proxy based on its generated channel list
    pub async fn generate_epg_for_proxy(
        &self,
        proxy: &StreamProxy,
        database: &Database,
        channel_ids: &[String],
        epg_config: Option<epg_generator::EpgGenerationConfig>,
    ) -> Result<(String, epg_generator::EpgGenerationStatistics)> {
        let epg_generator = epg_generator::EpgGenerator::new(database.clone());
        epg_generator
            .generate_xmltv_for_proxy(proxy, channel_ids, epg_config)
            .await
    }

    /// Apply filters to a list of channels (utility method)
    #[allow(dead_code)]
    pub async fn apply_filters(
        &self,
        channels: Vec<Channel>,
        filters: Vec<(Filter, ProxyFilter)>,
    ) -> Result<Vec<Channel>> {
        let mut engine = filter_engine::FilterEngine::new();
        engine.apply_filters(channels, filters).await
    }

    /// Save M3U content to storage
    pub async fn save_m3u_file(
        &self,
        proxy_id: uuid::Uuid,
        content: &str,
    ) -> Result<std::path::PathBuf> {
        let generator = generator::ProxyGenerator::new(self.storage_config.clone());
        generator.save_m3u_file(proxy_id, content).await
    }

    /// Save M3U content to storage using proxy ULID and optional file manager
    pub async fn save_m3u_file_with_manager(
        &self,
        proxy_ulid: &str,
        content: &str,
        file_manager: Option<sandboxed_file_manager::SandboxedManager>,
    ) -> Result<std::path::PathBuf> {
        let generator = if let Some(manager) = file_manager {
            generator::ProxyGenerator::with_file_manager(self.storage_config.clone(), manager)
        } else {
            generator::ProxyGenerator::new(self.storage_config.clone())
        };
        generator.save_m3u_file_by_ulid(proxy_ulid, content).await
    }

    /// Clean up old proxy versions
    pub async fn cleanup_old_versions(&self, proxy_id: uuid::Uuid) -> Result<()> {
        let generator = generator::ProxyGenerator::new(self.storage_config.clone());
        generator.cleanup_old_versions(proxy_id).await
    }
}
