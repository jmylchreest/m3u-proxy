use anyhow::Result;
use tracing::info;

use crate::config::StorageConfig;
use crate::data_mapping::service::DataMappingService;
use crate::database::Database;
use crate::logo_assets::service::LogoAssetService;
use crate::models::*;

pub mod adaptive_pipeline;
pub mod config_resolver;
pub mod epg_generator;
pub mod filter_engine;
pub mod generator;
pub mod simple_strategies;
pub mod stage_contracts;
pub mod stage_strategy;
pub mod streaming_pipeline;
pub mod streaming_stages;
pub mod wasm_examples;
pub mod wasm_host_interface; // Temporary compatibility stub
pub mod wasm_plugin; // Temporary compatibility stub  
pub mod wasm_plugin_test;

pub struct ProxyService {
    storage_config: StorageConfig,
    shared_plugin_manager: Option<std::sync::Arc<wasm_plugin::WasmPluginManager>>,
}

impl ProxyService {
    pub fn new(storage_config: StorageConfig) -> Self {
        Self { 
            storage_config,
            shared_plugin_manager: None,
        }
    }

    pub fn with_plugin_manager(storage_config: StorageConfig, plugin_manager: std::sync::Arc<wasm_plugin::WasmPluginManager>) -> Self {
        Self {
            storage_config,
            shared_plugin_manager: Some(plugin_manager),
        }
    }

    /// Generate a proxy using the adaptive pipeline with WASM plugin support
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
        use crate::proxy::adaptive_pipeline::AdaptivePipelineBuilder;
        use sandboxed_file_manager::SandboxedManager;
        use std::path::PathBuf;

        info!(
            "Starting generation proxy={} pipeline=adaptive sources={} filters={}",
            config.proxy.name,
            config.sources.len(),
            config.filters.len()
        );

        // Create temp file manager for the pipeline
        let temp_file_manager = SandboxedManager::builder()
            .base_directory(PathBuf::from(
                &self
                    .storage_config
                    .temp_path
                    .clone()
                    .unwrap_or_else(|| "/tmp/m3u-proxy".to_string()),
            ))
            .build()
            .await?;

        // Create adaptive pipeline with WASM plugin support
        let mut pipeline_builder = AdaptivePipelineBuilder::new()
            .with_database(database.clone())
            .with_data_mapping_service(data_mapping_service.clone())
            .with_logo_service(logo_service.clone())
            .with_memory_limit(512) // 512MB limit
            .with_temp_file_manager(temp_file_manager);

        // Add shared plugin manager if available
        if let Some(plugin_manager) = &self.shared_plugin_manager {
            pipeline_builder = pipeline_builder.with_shared_plugin_manager(plugin_manager.clone());
        }

        let mut pipeline = pipeline_builder.build()?;

        // Initialize the pipeline (loads WASM plugins)
        pipeline.initialize().await?;

        // Generate proxy using adaptive pipeline
        let generation = pipeline
            .generate_with_dynamic_strategies(config.clone(), base_url, engine_config)
            .await?;

        // Handle output
        self.write_output(&generation, &output, &config).await?;

        info!(
            "Generation completed proxy={} pipeline=adaptive channels={} total_time={}ms",
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
