use anyhow::Result;

use tracing::info;

use crate::data_mapping::DataMappingService;
use crate::database::Database;
use crate::ingestor::IngestionStateManager;
use crate::logo_assets::service::LogoAssetService;
use crate::models::*;

pub mod config_resolver;
// Legacy filter engine removed - replaced by pipeline-based filtering
pub mod http_stream;
pub mod robust_streaming;
pub mod session_tracker;

/// Parameters for generate_proxy_with_config method
pub struct GenerateProxyParams<'a> {
    pub config: ResolvedProxyConfig,
    pub output: GenerationOutput,
    pub database: &'a Database,
    pub data_mapping_service: &'a DataMappingService,
    pub logo_service: &'a LogoAssetService,
    pub base_url: &'a str,
    pub engine_config: Option<crate::config::DataMappingEngineConfig>,
    pub app_config: &'a crate::config::Config,
}

#[derive(Clone)]
pub struct ProxyService {
    pipeline_file_manager: sandboxed_file_manager::SandboxedManager,
    proxy_output_file_manager: sandboxed_file_manager::SandboxedManager,
    ingestion_state_manager: std::sync::Arc<IngestionStateManager>,
}

impl ProxyService {
    pub fn new(
        pipeline_file_manager: sandboxed_file_manager::SandboxedManager,
        proxy_output_file_manager: sandboxed_file_manager::SandboxedManager,
        ingestion_state_manager: std::sync::Arc<IngestionStateManager>,
    ) -> Self {
        Self {
            pipeline_file_manager,
            proxy_output_file_manager,
            ingestion_state_manager,
        }
    }

    /// Generate a proxy using the new pipeline orchestrator with factory pattern
    pub async fn generate_proxy_with_config(
        &self,
        params: GenerateProxyParams<'_>,
    ) -> Result<ProxyGeneration> {
        self.generate_proxy_with_params(params).await
    }

    /// Generate a proxy using the new pipeline orchestrator with factory pattern
    pub async fn generate_proxy_with_params(
        &self,
        params: GenerateProxyParams<'_>,
    ) -> Result<ProxyGeneration> {
        use crate::pipeline::PipelineOrchestratorFactory;

        info!(
            "Starting generation proxy={} pipeline=orchestrator sources={} filters={}",
            params.config.proxy.name,
            params.config.sources.len(),
            params.config.filters.len()
        );

        let start_time = std::time::Instant::now();

        // Create factory with all dependencies
        let factory = PipelineOrchestratorFactory::new(
            params.database.clone(),
            std::sync::Arc::new(params.logo_service.clone()),
            params.app_config.clone(),
            self.pipeline_file_manager.clone(),
            self.proxy_output_file_manager.clone(),
            self.ingestion_state_manager.clone(),
        );

        // Create orchestrator using factory pattern
        let mut orchestrator = factory
            .create_for_proxy(params.config.proxy.id)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to create pipeline orchestrator: {}", e))?;
        let execution = orchestrator
            .execute_pipeline()
            .await
            .map_err(|e| anyhow::anyhow!("Pipeline execution failed: {}", e))?;

        // Convert pipeline execution to legacy format for compatibility
        let generation = ProxyGeneration {
            id: execution.id,
            proxy_id: params.config.proxy.id,
            version: 1,
            channel_count: 0,           // TODO: Extract from execution output files
            m3u_content: String::new(), // TODO: Read from generated files
            created_at: execution.started_at,
            total_channels: 0,       // TODO: Extract from execution metrics
            filtered_channels: 0,    // TODO: Extract from execution metrics
            applied_filters: vec![], // TODO: Extract from config.filters
            stats: Some({
                let mut stats = GenerationStats::new("orchestrator".to_string());
                stats.total_duration_ms = start_time.elapsed().as_millis() as u64;
                stats.started_at = execution.started_at;
                stats.completed_at = execution.completed_at.unwrap_or_else(chrono::Utc::now);
                stats
            }),
            processed_channels: None, // TODO: Load from execution output files
        };

        // Handle output
        self.write_output(&generation, &params.output, &params.config)
            .await?;

        info!(
            "Generation completed proxy={} pipeline=orchestrator status={:?} duration={}",
            params.config.proxy.name,
            execution.status,
            crate::utils::human_format::format_duration_precise(start_time.elapsed())
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

    // Legacy apply_filters method removed - replaced by pipeline-based filtering
    // All filtering now handled by crate::pipeline::stages::filtering module
}
