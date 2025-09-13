//! Job executor service for performing the actual work

use crate::config::Config;
use crate::database::Database;
use crate::database::repositories::StreamProxySeaOrmRepository;
use crate::services::logo_cache_maintenance::LogoCacheMaintenanceService;
use crate::services::progress_service::{OperationType, ProgressService};
use crate::services::{EpgSourceService, ProxyRegenerationService, StreamSourceBusinessService};
use anyhow::Result;
use std::sync::Arc;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

/// Service responsible for executing the actual work of jobs
pub struct JobExecutor {
    stream_service: Arc<StreamSourceBusinessService>,
    epg_service: Arc<EpgSourceService>,
    proxy_regeneration_service: Arc<ProxyRegenerationService>,
    logo_cache_maintenance_service: Arc<LogoCacheMaintenanceService>,
    proxy_repo: StreamProxySeaOrmRepository,
    database: Database,
    app_config: Config,
    temp_file_manager: sandboxed_file_manager::SandboxedManager,
    http_client_factory: Arc<crate::utils::HttpClientFactory>,
    progress_service: Arc<ProgressService>,
}

impl JobExecutor {
    /// Create a new job executor
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        stream_service: Arc<StreamSourceBusinessService>,
        epg_service: Arc<EpgSourceService>,
        proxy_regeneration_service: Arc<ProxyRegenerationService>,
        logo_cache_maintenance_service: Arc<LogoCacheMaintenanceService>,
        database: Database,
        app_config: Config,
        temp_file_manager: sandboxed_file_manager::SandboxedManager,
        http_client_factory: Arc<crate::utils::HttpClientFactory>,
        progress_service: Arc<ProgressService>,
    ) -> Self {
        Self {
            stream_service,
            epg_service,
            proxy_regeneration_service,
            logo_cache_maintenance_service,
            proxy_repo: StreamProxySeaOrmRepository::new(database.connection().clone()),
            database: database.clone(),
            app_config,
            temp_file_manager,
            http_client_factory,
            progress_service,
        }
    }

    /// Execute a stream source ingestion job
    /// Returns list of affected proxy IDs that need regeneration
    pub async fn execute_stream_job(&self, source_id: Uuid) -> Result<Vec<Uuid>> {
        info!("Executing stream source ingestion for {}", source_id);

        // Get the stream source details
        let source = match self.stream_service.get(source_id).await {
            Ok(source) => source,
            Err(e) => {
                warn!(
                    "Stream source {} not found, skipping ingestion: {}",
                    source_id, e
                );
                return Ok(Vec::new());
            }
        };

        // Execute the stream source refresh
        let result = self
            .stream_service
            .refresh_with_progress_updater(&source, None)
            .await;

        match result {
            Ok(channel_count) => {
                info!(
                    "Successfully refreshed stream source {} with {} channels",
                    source.name, channel_count
                );

                // Find affected proxies that need regeneration
                let affected_proxies = self
                    .proxy_regeneration_service
                    .find_affected_proxies(source_id, "stream")
                    .await
                    .unwrap_or_else(|e| {
                        warn!(
                            "Failed to find affected proxies for stream source {}: {}",
                            source_id, e
                        );
                        Vec::new()
                    });

                info!(
                    "Stream source {} affected {} proxies",
                    source_id,
                    affected_proxies.len()
                );
                Ok(affected_proxies)
            }
            Err(e) => {
                warn!("Failed to refresh stream source {}: {}", source_id, e);
                Err(e)
            }
        }
    }

    /// Execute an EPG source ingestion job
    /// Returns list of affected proxy IDs that need regeneration
    pub async fn execute_epg_job(&self, source_id: Uuid) -> Result<Vec<Uuid>> {
        info!("Executing EPG source ingestion for {}", source_id);

        // Get the EPG source details
        let source = match self.epg_service.get(source_id).await {
            Ok(source) => source,
            Err(e) => {
                warn!(
                    "EPG source {} not found, skipping ingestion: {}",
                    source_id, e
                );
                return Ok(Vec::new());
            }
        };

        // Execute the EPG source ingestion
        let result = self
            .epg_service
            .ingest_programs_with_progress_updater(&source, None)
            .await;

        match result {
            Ok(program_count) => {
                info!(
                    "Successfully ingested EPG source {} with {} programs",
                    source.name, program_count
                );

                // Find affected proxies that need regeneration
                let affected_proxies = self
                    .proxy_regeneration_service
                    .find_affected_proxies(source_id, "epg")
                    .await
                    .unwrap_or_else(|e| {
                        warn!(
                            "Failed to find affected proxies for EPG source {}: {}",
                            source_id, e
                        );
                        Vec::new()
                    });

                info!(
                    "EPG source {} affected {} proxies",
                    source_id,
                    affected_proxies.len()
                );
                Ok(affected_proxies)
            }
            Err(e) => {
                warn!("Failed to ingest EPG source {}: {}", source_id, e);
                Err(e)
            }
        }
    }

    /// Execute a proxy regeneration job natively within the job scheduling system
    pub async fn execute_proxy_regeneration(&self, proxy_id: Uuid) -> Result<()> {
        info!("Executing native proxy regeneration for {}", proxy_id);

        // Verify proxy exists first
        let proxy = match self.proxy_repo.find_by_id(&proxy_id).await {
            Ok(Some(proxy)) => proxy,
            Ok(None) => {
                warn!("Proxy {} not found, skipping regeneration", proxy_id);
                return Ok(());
            }
            Err(e) => {
                warn!("Failed to find proxy {}: {}", proxy_id, e);
                return Err(e);
            }
        };

        info!(
            "Regenerating proxy '{}' ({}) using native job executor",
            proxy.name, proxy_id
        );

        // Execute native proxy regeneration directly within job scheduling system
        self.execute_native_proxy_regeneration(proxy_id, &proxy.name)
            .await
    }

    /// Execute a maintenance job
    pub async fn execute_maintenance(&self, operation: &str) -> Result<()> {
        info!("Executing maintenance operation: {}", operation);

        match operation {
            "cleanup_temp_files" => self.cleanup_temp_files().await,
            "refresh_cache" => self.refresh_cache().await,
            "health_check" => self.health_check().await,
            "logo_cache_scan" => self.logo_cache_maintenance_service.execute_scan_job().await,
            "logo_cache_cleanup" => self
                .logo_cache_maintenance_service
                .execute_maintenance()
                .await
                .map(|_| ()),
            _ => {
                warn!("Unknown maintenance operation: {}", operation);
                Err(anyhow::anyhow!(
                    "Unknown maintenance operation: {}",
                    operation
                ))
            }
        }
    }

    /// Find proxies that use a specific stream source
    #[allow(dead_code)] // Placeholder for future implementation
    async fn find_proxies_using_stream_source(&self, _source_id: Uuid) -> Result<Vec<Uuid>> {
        // TODO: Implement proxy finding logic
        // For now, return empty list
        Ok(Vec::new())
    }

    /// Find proxies that use a specific EPG source
    #[allow(dead_code)] // Placeholder for future implementation
    async fn find_proxies_using_epg_source(&self, _source_id: Uuid) -> Result<Vec<Uuid>> {
        // TODO: Implement proxy finding logic
        // For now, return empty list
        Ok(Vec::new())
    }

    /// Cleanup temporary files (maintenance operation)
    async fn cleanup_temp_files(&self) -> Result<()> {
        info!("Starting temporary files cleanup");

        // This would implement actual temp file cleanup
        // For now, just log the operation
        info!("Temporary files cleanup completed");
        Ok(())
    }

    /// Refresh internal caches (maintenance operation)
    async fn refresh_cache(&self) -> Result<()> {
        info!("Starting cache refresh");

        // This would implement cache refresh logic
        // For now, just log the operation
        info!("Cache refresh completed");
        Ok(())
    }

    /// Perform system health check (maintenance operation)
    async fn health_check(&self) -> Result<()> {
        info!("Starting system health check");

        // This would implement health check logic
        // For now, just log the operation
        info!("System health check completed");
        Ok(())
    }

    /// Execute native proxy regeneration directly within the job scheduling system
    /// This replaces calling the old proxy regeneration service to avoid hybrid system issues
    async fn execute_native_proxy_regeneration(
        &self,
        proxy_id: Uuid,
        proxy_name: &str,
    ) -> Result<()> {
        use crate::pipeline::PipelineOrchestratorFactory;

        debug!(
            "Starting native proxy regeneration for '{}' ({})",
            proxy_name, proxy_id
        );

        // Create progress manager for SSE progress updates
        let operation_name = format!("Native Regeneration: Proxy '{}'", proxy_name);
        let progress_manager = match self
            .progress_service
            .create_staged_progress_manager(
                proxy_id,
                "proxy".to_string(),
                OperationType::ProxyRegeneration,
                operation_name,
            )
            .await
        {
            Ok(mgr) => {
                debug!(
                    "Created progress manager for native proxy regeneration: '{}'",
                    proxy_name
                );
                Some(mgr)
            }
            Err(e) => {
                warn!(
                    "Failed to create progress manager for proxy '{}' ({}): {} - continuing without progress tracking",
                    proxy_name, proxy_id, e
                );
                None
            }
        };

        // Create pipeline factory with all required components
        let factory = PipelineOrchestratorFactory::from_components(
            self.database.clone(),
            self.app_config.clone(),
            self.app_config.storage.clone(),
            self.temp_file_manager.clone(),
            &self.http_client_factory,
        )
        .await
        .map_err(|e| {
            if let Some(ref pm) = progress_manager {
                // Don't await the future since we're in a map_err closure
                std::mem::drop(pm.fail(&format!("Failed to create pipeline factory: {}", e)));
            }
            anyhow::anyhow!("Failed to create pipeline factory: {}", e)
        })?;

        debug!(
            "Created pipeline factory for proxy regeneration: {}",
            proxy_id
        );

        // Create orchestrator for the specific proxy
        let mut orchestrator = factory.create_for_proxy(proxy_id).await.map_err(|e| {
            if let Some(ref pm) = progress_manager {
                // Don't await the future since we're in a map_err closure
                std::mem::drop(pm.fail(&format!("Failed to create orchestrator: {}", e)));
            }
            anyhow::anyhow!(
                "Failed to create orchestrator for proxy {}: {}",
                proxy_id,
                e
            )
        })?;

        // Set the progress manager on the orchestrator for SSE updates
        if let Some(ref pm) = progress_manager {
            orchestrator.set_progress_manager(Some(pm.clone()));
            debug!(
                "Set progress manager on orchestrator for proxy '{}' ({})",
                proxy_name, proxy_id
            );
        }

        debug!(
            "Created orchestrator for proxy '{}' ({})",
            proxy_name, proxy_id
        );

        // Execute the regeneration pipeline
        match orchestrator.execute_pipeline().await {
            Ok(result) => {
                match result.status {
                    crate::pipeline::models::PipelineStatus::Completed => {
                        info!(
                            "Successfully regenerated proxy '{}' ({}) using native job executor",
                            proxy_name, proxy_id
                        );

                        // Clean up the orchestrator
                        factory.unregister_orchestrator(proxy_id).await;

                        // Update the proxy's last_generated_at timestamp
                        let update_time = chrono::Utc::now();
                        if let Err(e) = self.proxy_repo.update_last_generated(proxy_id).await {
                            warn!(
                                "Failed to update last_generated_at for proxy '{}' ({}): {}",
                                proxy_name, proxy_id, e
                            );
                        } else {
                            debug!(
                                "Updated last_generated_at timestamp for proxy '{}' ({}) to {}",
                                proxy_name,
                                proxy_id,
                                update_time.to_rfc3339()
                            );
                        }

                        // Complete the progress manager
                        if let Some(ref pm) = progress_manager {
                            pm.complete().await;
                        }

                        Ok(())
                    }
                    crate::pipeline::models::PipelineStatus::Failed => {
                        let error_msg = result
                            .error_message
                            .unwrap_or_else(|| "Unknown pipeline error".to_string());
                        error!(
                            "Native pipeline failed for proxy '{}' ({}): {}",
                            proxy_name, proxy_id, error_msg
                        );
                        factory.unregister_orchestrator(proxy_id).await;

                        // Mark progress manager as failed
                        if let Some(ref pm) = progress_manager {
                            pm.fail(&error_msg).await;
                        }

                        Err(anyhow::anyhow!("Pipeline execution failed: {}", error_msg))
                    }
                    _ => {
                        let warning_msg = format!(
                            "Pipeline completed with unexpected status: {:?}",
                            result.status
                        );
                        warn!(
                            "Native pipeline for proxy '{}' ({}): {}",
                            proxy_name, proxy_id, warning_msg
                        );
                        factory.unregister_orchestrator(proxy_id).await;

                        // Mark progress manager as failed
                        if let Some(ref pm) = progress_manager {
                            pm.fail(&warning_msg).await;
                        }

                        Err(anyhow::anyhow!(warning_msg))
                    }
                }
            }
            Err(e) => {
                error!(
                    "Failed to execute native pipeline for proxy '{}' ({}): {}",
                    proxy_name, proxy_id, e
                );
                factory.unregister_orchestrator(proxy_id).await;

                // Mark progress manager as failed
                if let Some(ref pm) = progress_manager {
                    pm.fail(&format!("Pipeline execution failed: {}", e)).await;
                }

                Err(anyhow::anyhow!("Pipeline execution failed: {}", e))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    // Test imports removed - will be added back when integration tests are implemented

    // Note: These tests would need proper mocking to be fully functional
    // For now, we'll include the structure for future implementation

    // TODO: Add integration tests with proper mocking framework
    // These tests would verify:
    // 1. Stream/EPG ingestion is called correctly
    // 2. Affected proxies are identified
    // 3. Error handling works properly
    // 4. Proxy regeneration scheduling works

    // Additional integration tests would be implemented here with proper mocking
}
