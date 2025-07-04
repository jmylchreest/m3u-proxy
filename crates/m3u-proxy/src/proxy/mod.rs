use anyhow::Result;
use tracing::info;

use crate::config::StorageConfig;
use crate::data_mapping::service::DataMappingService;
use crate::database::Database;
use crate::logo_assets::service::LogoAssetService;
use crate::models::*;

pub mod adaptive_pipeline;
pub mod chunked_pipeline;
pub mod epg_generator;
pub mod filter_engine;
pub mod generator;
pub mod pipeline;

pub struct ProxyService {
    storage_config: StorageConfig,
}

impl ProxyService {
    pub fn new(storage_config: StorageConfig) -> Self {
        Self { storage_config }
    }

    /// Generate a proxy M3U with full data mapping and filtering pipeline
    pub async fn generate_proxy(
        &self,
        proxy: &StreamProxy,
        database: &Database,
        data_mapping_service: &DataMappingService,
        logo_service: &LogoAssetService,
        base_url: &str,
        engine_config: Option<crate::config::DataMappingEngineConfig>,
    ) -> Result<ProxyGeneration> {
        let generator = generator::ProxyGenerator::new(self.storage_config.clone());
        generator
            .generate(
                proxy,
                database,
                data_mapping_service,
                logo_service,
                base_url,
                engine_config,
            )
            .await
    }

    /// Generate a proxy M3U using the new memory-efficient pipeline
    pub async fn generate_proxy_with_pipeline(
        &self,
        proxy: &StreamProxy,
        database: &Database,
        data_mapping_service: &DataMappingService,
        logo_service: &LogoAssetService,
        base_url: &str,
        engine_config: Option<crate::config::DataMappingEngineConfig>,
        pipeline_config: Option<pipeline::PipelineConfig>,
    ) -> Result<ProxyGeneration> {
        let mut pipeline = if let Some(ref config) = pipeline_config {
            if config.enable_statistics {
                // Create pipeline with memory tracking
                let _memory_config = crate::config::ProxyMemoryConfig {
                    max_memory_mb: config.memory_limit_mb,
                    batch_size: config.batch_size,
                    enable_parallel_processing: config.enable_parallel_processing,
                    memory_check_interval: 100,
                    warning_threshold: 0.8,
                    strategy_preset: Some("default".to_string()),
                    memory_strategy: Some(crate::config::MemoryStrategySettings::default()),
                };

                pipeline::ProxyGenerationPipeline::new_with_memory_monitoring(
                    database.clone(),
                    data_mapping_service.clone(),
                    logo_service.clone(),
                    config.memory_limit_mb,
                )
            } else {
                pipeline::ProxyGenerationPipeline::new(
                    database.clone(),
                    data_mapping_service.clone(),
                    logo_service.clone(),
                )
            }
        } else {
            pipeline::ProxyGenerationPipeline::new(
                database.clone(),
                data_mapping_service.clone(),
                logo_service.clone(),
            )
        };

        let (generation, memory_stats) = pipeline
            .generate_proxy(proxy, base_url, engine_config, pipeline_config)
            .await?;

        if let Some(stats) = memory_stats {
            info!("Memory tracking completed: {}", stats.summary());
        }

        Ok(generation)
    }

    /// Generate a proxy using adaptive strategy switching based on memory pressure
    pub async fn generate_proxy_with_adaptive_pipeline(
        &self,
        proxy: &StreamProxy,
        database: &Database,
        data_mapping_service: &DataMappingService,
        logo_service: &LogoAssetService,
        base_url: &str,
        engine_config: Option<crate::config::DataMappingEngineConfig>,
        memory_config: Option<&crate::config::ProxyMemoryConfig>,
        temp_file_manager: Option<sandboxed_file_manager::SandboxedManager>,
    ) -> Result<ProxyGeneration> {
        use crate::proxy::adaptive_pipeline::AdaptivePipelineBuilder;
        use crate::utils::memory_strategy::MemoryStrategyExecutor;

        // Create or use provided temp file manager
        let temp_file_manager = if let Some(manager) = temp_file_manager {
            manager
        } else {
            // Create a default temp file manager using system temp directory
            use sandboxed_file_manager::{SandboxedManager, CleanupPolicy, TimeMatch};
            use std::time::Duration;
            
            let temp_dir = std::env::temp_dir().join("m3u-proxy-adaptive");
            let cleanup_policy = CleanupPolicy::new()
                .remove_after(Duration::from_secs(3600))
                .time_match(TimeMatch::LastAccess)
                .enabled(true); // 1 hour retention
            
            SandboxedManager::builder()
                .base_directory(temp_dir)
                .cleanup_policy(cleanup_policy)
                .cleanup_interval(Duration::from_secs(300)) // Check every 5 minutes
                .build()
                .await
                .map_err(|e| anyhow::anyhow!("Failed to create temp file manager: {}", e))?
        };

        let mut builder = AdaptivePipelineBuilder::new()
            .database(database.clone())
            .data_mapping_service(data_mapping_service.clone())
            .logo_service(logo_service.clone())
            .temp_file_manager(temp_file_manager);

        // Configure memory settings if provided
        if let Some(config) = memory_config {
            if let Some(limit) = config.max_memory_mb {
                builder = builder.memory_limit_mb(limit);
            }

            // Set up memory strategy based on strategy_preset or custom settings
            if let Some(preset) = &config.strategy_preset {
                match preset.to_lowercase().as_str() {
                    "default" => {
                        // Use default strategy (no additional configuration needed)
                    }
                    "conservative" => {
                        let strategy_config = crate::utils::memory_strategy::ProxyGenerationStrategies::conservative();
                        let strategy_executor = MemoryStrategyExecutor::new(strategy_config);
                        builder = builder.memory_strategy(strategy_executor);
                    }
                    "aggressive" => {
                        let strategy_config = crate::utils::memory_strategy::ProxyGenerationStrategies::aggressive();
                        let strategy_executor = MemoryStrategyExecutor::new(strategy_config);
                        builder = builder.memory_strategy(strategy_executor);
                    }
                    "temp_file_based" => {
                        let temp_dir = config.memory_strategy
                            .as_ref()
                            .and_then(|s| s.temp_dir.as_ref())
                            .cloned()
                            .unwrap_or_else(|| "/tmp".to_string());
                        let strategy_config = crate::utils::memory_strategy::ProxyGenerationStrategies::temp_file_based(&temp_dir);
                        let strategy_executor = MemoryStrategyExecutor::new(strategy_config);
                        builder = builder.memory_strategy(strategy_executor);
                    }
                    "custom" => {
                        // Use custom memory_strategy settings
                        if let Some(strategy_settings) = &config.memory_strategy {
                            let strategy_config = strategy_settings.to_memory_strategy_config()?;
                            let strategy_executor = MemoryStrategyExecutor::new(strategy_config);
                            builder = builder.memory_strategy(strategy_executor);
                        }
                    }
                    _ => {
                        return Err(anyhow::anyhow!("Unknown strategy preset: {}. Valid options: default, conservative, aggressive, temp_file_based, custom", preset));
                    }
                }
            } else {
                // No preset specified, check for custom settings (backward compatibility)
                if let Some(strategy_settings) = &config.memory_strategy {
                    let strategy_config = strategy_settings.to_memory_strategy_config()?;
                    let strategy_executor = MemoryStrategyExecutor::new(strategy_config);
                    builder = builder.memory_strategy(strategy_executor);
                }
            }
        }

        let mut adaptive_pipeline = builder.build()?;
        let (generation, memory_stats) = adaptive_pipeline
            .generate_proxy_adaptive(proxy, base_url, engine_config)
            .await?;

        if let Some(stats) = memory_stats {
            info!("Adaptive pipeline completed: {}", stats.summary());
        }

        Ok(generation)
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

    /// Generate both M3U and XMLTV for a proxy in a coordinated manner
    pub async fn generate_proxy_with_epg(
        &self,
        proxy: &StreamProxy,
        database: &Database,
        data_mapping_service: &DataMappingService,
        logo_service: &LogoAssetService,
        base_url: &str,
        engine_config: Option<crate::config::DataMappingEngineConfig>,
        pipeline_config: Option<pipeline::PipelineConfig>,
        epg_config: Option<epg_generator::EpgGenerationConfig>,
    ) -> Result<(
        ProxyGeneration,
        String,
        epg_generator::EpgGenerationStatistics,
    )> {
        // Step 1: Generate M3U using pipeline
        let mut pipeline = if let Some(ref config) = pipeline_config {
            if config.enable_statistics {
                // Create pipeline with memory tracking
                let _memory_config = crate::config::ProxyMemoryConfig {
                    max_memory_mb: config.memory_limit_mb,
                    batch_size: config.batch_size,
                    enable_parallel_processing: config.enable_parallel_processing,
                    memory_check_interval: 100,
                    warning_threshold: 0.8,
                    strategy_preset: Some("default".to_string()),
                    memory_strategy: Some(crate::config::MemoryStrategySettings::default()),
                };

                pipeline::ProxyGenerationPipeline::new_with_memory_monitoring(
                    database.clone(),
                    data_mapping_service.clone(),
                    logo_service.clone(),
                    config.memory_limit_mb,
                )
            } else {
                pipeline::ProxyGenerationPipeline::new(
                    database.clone(),
                    data_mapping_service.clone(),
                    logo_service.clone(),
                )
            }
        } else {
            pipeline::ProxyGenerationPipeline::new(
                database.clone(),
                data_mapping_service.clone(),
                logo_service.clone(),
            )
        };

        let (proxy_generation, memory_stats) = pipeline
            .generate_proxy(proxy, base_url, engine_config, pipeline_config)
            .await?;

        if let Some(stats) = memory_stats {
            info!("M3U generation: {}", stats.summary());
        }

        // Step 2: Extract channel IDs from generated M3U
        let channel_ids = pipeline.extract_channel_ids_from_generation(&proxy_generation)?;

        // Step 3: Generate XMLTV EPG filtered to those channels
        let epg_generator = epg_generator::EpgGenerator::new(database.clone());
        let (xmltv_content, epg_stats) = epg_generator
            .generate_xmltv_for_proxy(proxy, &channel_ids, epg_config)
            .await?;

        Ok((proxy_generation, xmltv_content, epg_stats))
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

    /// Clean up old proxy versions
    pub async fn cleanup_old_versions(&self, proxy_id: uuid::Uuid) -> Result<()> {
        let generator = generator::ProxyGenerator::new(self.storage_config.clone());
        generator.cleanup_old_versions(proxy_id).await
    }
}
