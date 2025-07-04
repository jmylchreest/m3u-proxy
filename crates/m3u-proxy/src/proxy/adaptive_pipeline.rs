//! Adaptive pipeline that can switch processing strategies based on memory pressure
//!
//! This pipeline can dynamically switch between normal processing and alternative
//! strategies like chunked processing or temp file spill when memory limits are reached.

use anyhow::Result;
use tracing::{info, warn};
use sandboxed_file_manager::SandboxedManager;

use crate::database::Database;
use crate::data_mapping::service::DataMappingService;
use crate::logo_assets::service::LogoAssetService;
use crate::models::*;
use crate::proxy::{chunked_pipeline::ChunkedProxyPipeline, pipeline::ProxyGenerationPipeline};
use crate::utils::{SimpleMemoryMonitor, MemoryStats, MemoryLimitStatus};
use crate::utils::memory_strategy::{MemoryStrategyExecutor, MemoryAction};

/// Adaptive pipeline that can switch processing strategies dynamically
pub struct AdaptivePipeline {
    database: Database,
    data_mapping_service: DataMappingService,
    logo_service: LogoAssetService,
    memory_monitor: Option<SimpleMemoryMonitor>,
    memory_strategy: Option<MemoryStrategyExecutor>,
    temp_file_manager: SandboxedManager,
}

/// Processing mode that the adaptive pipeline can switch between
#[derive(Debug, Clone)]
enum ProcessingMode {
    /// Normal in-memory processing
    Normal,
    /// Chunked processing with specified chunk size
    Chunked { chunk_size: usize },
    /// Temporary file spill processing (using smaller chunks)
    TempFileSpill { chunk_size: usize },
}

impl AdaptivePipeline {
    pub fn new(
        database: Database,
        data_mapping_service: DataMappingService,
        logo_service: LogoAssetService,
        memory_limit_mb: Option<usize>,
        memory_strategy: Option<MemoryStrategyExecutor>,
        temp_file_manager: SandboxedManager,
    ) -> Self {
        let memory_monitor = memory_limit_mb.map(|limit| SimpleMemoryMonitor::new(Some(limit)));

        Self {
            database,
            data_mapping_service,
            logo_service,
            memory_monitor,
            memory_strategy,
            temp_file_manager,
        }
    }

    /// Generate proxy with adaptive strategy switching
    pub async fn generate_proxy_adaptive(
        &mut self,
        proxy: &StreamProxy,
        base_url: &str,
        engine_config: Option<crate::config::DataMappingEngineConfig>,
    ) -> Result<(ProxyGeneration, Option<MemoryStats>)> {
        info!("Starting adaptive proxy generation for '{}'", proxy.name);

        if let Some(ref mut monitor) = self.memory_monitor {
            monitor.initialize()?;
            monitor.observe_stage("adaptive_initialization")?;
        }

        // Start with normal processing mode
        let mut current_mode = ProcessingMode::Normal;
        
        // Try normal processing first
        match self.try_normal_processing(proxy, base_url, engine_config.clone()).await {
            Ok(result) => {
                info!("Normal processing completed successfully for '{}'", proxy.name);
                return Ok(result);
            },
            Err(e) => {
                warn!("Normal processing failed for '{}': {}. Checking memory strategy...", proxy.name, e);
                
                // Check if failure was due to memory pressure
                if let Some(ref monitor) = self.memory_monitor {
                    let status = monitor.check_memory_limit()?;
                    if let Some(ref strategy) = self.memory_strategy {
                        current_mode = self.determine_fallback_mode(status, strategy).await?;
                    }
                }
            }
        }

        // Execute fallback processing based on determined mode
        match current_mode {
            ProcessingMode::Normal => {
                // This shouldn't happen, but fallback to error
                Err(anyhow::anyhow!("Normal processing failed and no fallback strategy determined"))
            },
            ProcessingMode::Chunked { chunk_size } => {
                info!("Switching to chunked processing (chunk_size: {}) for '{}'", chunk_size, proxy.name);
                self.execute_chunked_processing(proxy, base_url, engine_config, chunk_size).await
            },
            ProcessingMode::TempFileSpill { chunk_size } => {
                info!("Switching to temp file spill processing (chunk_size: {}) for '{}'", chunk_size, proxy.name);
                self.execute_temp_file_processing(proxy, base_url, engine_config, chunk_size).await
            },
        }
    }

    /// Try normal processing and detect if memory pressure causes failure
    async fn try_normal_processing(
        &mut self,
        proxy: &StreamProxy,
        base_url: &str,
        engine_config: Option<crate::config::DataMappingEngineConfig>,
    ) -> Result<(ProxyGeneration, Option<MemoryStats>)> {
        // Create a standard pipeline
        let mut pipeline = if let Some(memory_limit) = self.memory_monitor.as_ref().and_then(|m| m.memory_limit_mb) {
            ProxyGenerationPipeline::new_with_memory_monitoring(
                self.database.clone(),
                self.data_mapping_service.clone(),
                self.logo_service.clone(),
                Some(memory_limit),
            )
        } else {
            ProxyGenerationPipeline::new(
                self.database.clone(),
                self.data_mapping_service.clone(),
                self.logo_service.clone(),
            )
        };

        // Try to generate normally
        pipeline.generate_proxy(proxy, base_url, engine_config, None).await
    }

    /// Determine which fallback mode to use based on memory status and strategy
    async fn determine_fallback_mode(
        &self,
        status: MemoryLimitStatus,
        strategy: &MemoryStrategyExecutor,
    ) -> Result<ProcessingMode> {
        let action = match status {
            MemoryLimitStatus::Warning => {
                strategy.handle_warning("adaptive_pipeline_fallback").await?
            },
            MemoryLimitStatus::Exceeded => {
                strategy.handle_exceeded("adaptive_pipeline_fallback").await?
            },
            MemoryLimitStatus::Ok => {
                // This shouldn't happen in fallback, but default to chunked
                return Ok(ProcessingMode::Chunked { chunk_size: 500 });
            }
        };

        match action {
            MemoryAction::StopProcessing => {
                return Err(anyhow::anyhow!("Memory strategy dictates stopping processing"));
            },
            MemoryAction::SwitchToChunked(chunk_size) => {
                Ok(ProcessingMode::Chunked { chunk_size })
            },
            MemoryAction::UseTemporaryStorage(_temp_dir) => {
                // Use smaller chunk size for temporary storage
                Ok(ProcessingMode::TempFileSpill { chunk_size: 250 })
            },
            MemoryAction::Continue => {
                // If strategy says continue but we're in fallback, use chunked as safe default
                Ok(ProcessingMode::Chunked { chunk_size: 1000 })
            },
        }
    }

    /// Execute chunked processing strategy
    async fn execute_chunked_processing(
        &mut self,
        proxy: &StreamProxy,
        base_url: &str,
        engine_config: Option<crate::config::DataMappingEngineConfig>,
        chunk_size: usize,
    ) -> Result<(ProxyGeneration, Option<MemoryStats>)> {
        let memory_limit = self.memory_monitor.as_ref().and_then(|m| m.memory_limit_mb);
        
        let mut chunked_pipeline = ChunkedProxyPipeline::new(
            self.database.clone(),
            self.data_mapping_service.clone(),
            self.logo_service.clone(),
            chunk_size,
            memory_limit,
            self.temp_file_manager.clone(),
        );

        chunked_pipeline.generate_proxy_chunked(proxy, base_url, engine_config).await
    }

    /// Execute temporary file spill processing strategy  
    async fn execute_temp_file_processing(
        &mut self,
        proxy: &StreamProxy,
        base_url: &str,
        engine_config: Option<crate::config::DataMappingEngineConfig>,
        chunk_size: usize,
    ) -> Result<(ProxyGeneration, Option<MemoryStats>)> {
        let memory_limit = self.memory_monitor.as_ref().and_then(|m| m.memory_limit_mb);
        
        let mut temp_file_pipeline = ChunkedProxyPipeline::new(
            self.database.clone(),
            self.data_mapping_service.clone(),
            self.logo_service.clone(),
            chunk_size,
            memory_limit,
            self.temp_file_manager.clone(),
        );

        temp_file_pipeline.generate_proxy_chunked(proxy, base_url, engine_config).await
    }

    /// Get final memory statistics
    pub fn get_memory_statistics(&self) -> Option<MemoryStats> {
        self.memory_monitor.as_ref().map(|monitor| monitor.get_statistics())
    }
}

/// Builder for creating adaptive pipelines with specific configurations
pub struct AdaptivePipelineBuilder {
    database: Option<Database>,
    data_mapping_service: Option<DataMappingService>,
    logo_service: Option<LogoAssetService>,
    memory_limit_mb: Option<usize>,
    memory_strategy: Option<MemoryStrategyExecutor>,
    temp_file_manager: Option<SandboxedManager>,
}

impl AdaptivePipelineBuilder {
    pub fn new() -> Self {
        Self {
            database: None,
            data_mapping_service: None,
            logo_service: None,
            memory_limit_mb: None,
            memory_strategy: None,
            temp_file_manager: None,
        }
    }

    pub fn database(mut self, database: Database) -> Self {
        self.database = Some(database);
        self
    }

    pub fn data_mapping_service(mut self, service: DataMappingService) -> Self {
        self.data_mapping_service = Some(service);
        self
    }

    pub fn logo_service(mut self, service: LogoAssetService) -> Self {
        self.logo_service = Some(service);
        self
    }

    pub fn memory_limit_mb(mut self, limit: usize) -> Self {
        self.memory_limit_mb = Some(limit);
        self
    }

    pub fn memory_strategy(mut self, strategy: MemoryStrategyExecutor) -> Self {
        self.memory_strategy = Some(strategy);
        self
    }

    pub fn temp_file_manager(mut self, manager: SandboxedManager) -> Self {
        self.temp_file_manager = Some(manager);
        self
    }

    pub fn build(self) -> Result<AdaptivePipeline> {
        Ok(AdaptivePipeline::new(
            self.database.ok_or_else(|| anyhow::anyhow!("Database is required"))?,
            self.data_mapping_service.ok_or_else(|| anyhow::anyhow!("Data mapping service is required"))?,
            self.logo_service.ok_or_else(|| anyhow::anyhow!("Logo service is required"))?,
            self.memory_limit_mb,
            self.memory_strategy,
            self.temp_file_manager.ok_or_else(|| anyhow::anyhow!("Temp file manager is required"))?,
        ))
    }
}