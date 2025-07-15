use anyhow::Result;
use tracing::info;

use crate::config::StorageConfig;
use crate::data_mapping::service::DataMappingService;
use crate::database::Database;
use crate::logo_assets::service::LogoAssetService;
use crate::models::*;

pub mod config_resolver;
pub mod epg_generator;
pub mod filter_engine;
pub mod generator;
pub mod native_pipeline;
pub mod robust_streaming;
pub mod session_tracker;
pub mod simple_strategies;
pub mod stage_contracts;
pub mod stage_strategy;
pub mod streaming_pipeline;
pub mod streaming_stages;

pub struct ProxyService {
    storage_config: StorageConfig,
    shared_memory_monitor: Option<crate::utils::SimpleMemoryMonitor>,
    temp_file_manager: sandboxed_file_manager::SandboxedManager,
}

impl ProxyService {
    pub fn new(
        storage_config: StorageConfig,
        temp_file_manager: sandboxed_file_manager::SandboxedManager,
    ) -> Self {
        Self {
            storage_config,
            shared_memory_monitor: None,
            temp_file_manager,
        }
    }

    pub fn with_memory_monitor(
        storage_config: StorageConfig,
        temp_file_manager: sandboxed_file_manager::SandboxedManager,
        memory_monitor: crate::utils::SimpleMemoryMonitor,
    ) -> Self {
        Self {
            storage_config,
            shared_memory_monitor: Some(memory_monitor),
            temp_file_manager,
        }
    }

    /// Generate a proxy using the native pipeline
    pub async fn generate_proxy_with_config(
        &self,
        config: ResolvedProxyConfig,
        output: GenerationOutput,
        database: &Database,
        data_mapping_service: &DataMappingService,
        logo_service: &LogoAssetService,
        base_url: &str,
        engine_config: Option<crate::config::DataMappingEngineConfig>,
        app_config: &crate::config::Config,
    ) -> Result<ProxyGeneration> {
        use crate::proxy::native_pipeline::NativePipelineBuilder;

        info!(
            "Starting generation proxy={} pipeline=native sources={} filters={}",
            config.proxy.name,
            config.sources.len(),
            config.filters.len()
        );

        // Create native pipeline using the managed temp file manager
        let mut pipeline_builder = NativePipelineBuilder::new()
            .with_database(database.clone())
            .with_data_mapping_service(data_mapping_service.clone())
            .with_logo_service(logo_service.clone())
            .with_memory_limit(512) // 512MB limit
            .with_temp_file_manager(self.temp_file_manager.clone());

        // Add shared memory monitor if available
        if let Some(memory_monitor) = &self.shared_memory_monitor {
            pipeline_builder = pipeline_builder.with_shared_memory_monitor(memory_monitor.clone());
        }

        let mut pipeline = pipeline_builder.build()?;

        // Initialize the pipeline
        pipeline.initialize().await?;

        // Generate proxy using native pipeline
        let generation = pipeline
            .generate_with_dynamic_strategies(config.clone(), base_url, engine_config, app_config)
            .await?;

        // Handle output
        self.write_output(&generation, &output, &config).await?;

        info!(
            "Generation completed proxy={} pipeline=native channels={} total_time={}ms",
            config.proxy.name,
            generation.channel_count,
            generation.stats.as_ref().map_or(0, |s| s.total_duration_ms)
        );

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
                let m3u_file_id = format!("{}-{}.m3u", config.proxy.id, uuid::Uuid::new_v4());
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

    /// Save M3U content to storage using proxy ID and optional file manager
    pub async fn save_m3u_file_with_manager(
        &self,
        proxy_id: &str,
        content: &str,
        file_manager: Option<sandboxed_file_manager::SandboxedManager>,
    ) -> Result<std::path::PathBuf> {
        let generator = if let Some(manager) = file_manager {
            generator::ProxyGenerator::with_file_manager(self.storage_config.clone(), manager)
        } else {
            generator::ProxyGenerator::new(self.storage_config.clone())
        };
        generator.save_m3u_file_by_id(proxy_id, content).await
    }

    /// Clean up old proxy versions
    pub async fn cleanup_old_versions(&self, proxy_id: uuid::Uuid) -> Result<()> {
        let generator = generator::ProxyGenerator::new(self.storage_config.clone());
        generator.cleanup_old_versions(proxy_id).await
    }
}
