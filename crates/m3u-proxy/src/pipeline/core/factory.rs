//! Pipeline Orchestrator Factory
//!
//! This factory handles the creation of properly configured PipelineOrchestrator instances.
//! It manages dependency injection and configuration resolution, following the SOLID principles
//! by separating the concerns of dependency resolution from pipeline execution.

use crate::{
    config::{Config, StorageConfig},
    logo_assets::{service::LogoAssetService, storage::LogoAssetStorage},
    models::StreamProxy,
    pipeline::{
        core::orchestrator::PipelineOrchestrator,
        stages::logo_caching::LogoCachingConfig,
    },
};
use sandboxed_file_manager::{SandboxedManager, CleanupPolicy, TimeMatch};
use std::sync::Arc;
use std::collections::HashMap;
use tokio::sync::Mutex;
#[cfg(test)]
use std::path::PathBuf;
use tracing::{debug, error, warn};
use uuid::Uuid;

/// Factory for creating properly configured PipelineOrchestrator instances
#[derive(Clone)]
pub struct PipelineOrchestratorFactory {
    database: crate::database::Database,
    logo_service: Arc<LogoAssetService>,
    app_config: Config,
    pipeline_file_manager: SandboxedManager,
    proxy_output_file_manager: SandboxedManager,
    /// CONCURRENCY FIX: Track active orchestrators to prevent multiple instances per proxy
    active_orchestrators: Arc<Mutex<HashMap<Uuid, String>>>, // proxy_id -> pipeline_execution_id
}

impl PipelineOrchestratorFactory {
    /// Create a new factory with all required dependencies
    pub fn new(
        database: crate::database::Database,
        logo_service: Arc<LogoAssetService>,
        app_config: Config,
        pipeline_file_manager: SandboxedManager,
        proxy_output_file_manager: SandboxedManager,
    ) -> Self {
        Self {
            database,
            logo_service,
            app_config,
            pipeline_file_manager,
            proxy_output_file_manager,
            active_orchestrators: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Create a factory from basic components, creating shared services
    pub async fn from_components(
        database: crate::database::Database,
        app_config: Config,
        storage_config: StorageConfig,
        pipeline_file_manager: SandboxedManager,
        http_client_factory: &crate::utils::HttpClientFactory,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        // Create logo storage from configuration
        let logo_storage = LogoAssetStorage::new(
            storage_config.uploaded_logo_path.clone(),
            storage_config.cached_logo_path.clone(),
        );

        // Create logo service with circuit breaker protection via factory
        let logo_service = Arc::new(LogoAssetService::new(database.connection().clone(), logo_storage, http_client_factory).await);

        // Create proxy output file manager for final M3U/XMLTV files
        let proxy_output_file_manager = SandboxedManager::builder()
            .base_directory(&storage_config.m3u_path)
            .cleanup_policy(CleanupPolicy::new()
                .remove_after(humantime::parse_duration(&storage_config.m3u_retention)?)
                .time_match(TimeMatch::LastAccess))
            .cleanup_interval(humantime::parse_duration(&storage_config.m3u_cleanup_interval)?)
            .build()
            .await?;

        Ok(Self::new(database, logo_service, app_config, pipeline_file_manager, proxy_output_file_manager))
    }

    /// Create a PipelineOrchestrator for a specific proxy
    pub async fn create_for_proxy(
        &self,
        proxy_id: Uuid,
    ) -> Result<PipelineOrchestrator, Box<dyn std::error::Error>> {
        debug!("Creating pipeline orchestrator for proxy {}", proxy_id);

        // CRITICAL FIX: Check if orchestrator already exists for this proxy
        {
            let active = self.active_orchestrators.lock().await;
            if let Some(existing_pipeline_id) = active.get(&proxy_id) {
                let message = format!(
                    "Pipeline orchestrator already exists for proxy {proxy_id} (pipeline: {existing_pipeline_id}). Preventing duplicate creation."
                );
                warn!("{}", message);
                return Err(message.into());
            }
        }

        // Load proxy configuration from database
        let proxy_config = self.load_proxy_config(proxy_id).await?;

        // Create logo caching configuration from proxy and app settings
        let logo_config = LogoCachingConfig {
            cache_channel_logos: proxy_config.cache_channel_logos,
            cache_program_logos: proxy_config.cache_program_logos,
            base_url: self.app_config.web.base_url.clone(),
        };

        // Create orchestrator with all dependencies injected
        let orchestrator = PipelineOrchestrator::new_with_dependencies(
            proxy_config,
            self.pipeline_file_manager.clone(),
            self.proxy_output_file_manager.clone(),
            self.logo_service.clone(),
            logo_config,
            self.database.clone(),
        );

        // Register this orchestrator as active (get pipeline execution ID from orchestrator)
        let pipeline_id = orchestrator.get_execution_id().to_string();
        {
            let mut active = self.active_orchestrators.lock().await;
            active.insert(proxy_id, pipeline_id.clone());
        }

        debug!("Successfully created and registered pipeline orchestrator for proxy {} (pipeline: {})", proxy_id, pipeline_id);
        Ok(orchestrator)
    }

    /// Load proxy configuration from database using flexible UUID parsing
    async fn load_proxy_config(&self, proxy_id: Uuid) -> Result<StreamProxy, Box<dyn std::error::Error>> {
        
        // Use SeaORM entity query instead of raw SQL
        use crate::entities::{prelude::*, stream_proxies};
        use sea_orm::{EntityTrait, QueryFilter, ColumnTrait};
        
        let proxy_entity = StreamProxies::find()
            .filter(stream_proxies::Column::Id.eq(proxy_id))
            .filter(stream_proxies::Column::IsActive.eq(true))
            .one(&*self.database.connection())
            .await?;

        match proxy_entity {
            Some(entity) => {
                // Convert SeaORM entity to domain model with type conversions
                let config = StreamProxy {
                    id: entity.id,
                    name: entity.name,
                    description: entity.description,
                    starting_channel_number: entity.starting_channel_number,
                    is_active: entity.is_active,
                    auto_regenerate: entity.auto_regenerate,
                    cache_channel_logos: entity.cache_channel_logos,
                    cache_program_logos: entity.cache_program_logos,
                    proxy_mode: entity.proxy_mode,
                    upstream_timeout: entity.upstream_timeout,
                    buffer_size: entity.buffer_size,
                    max_concurrent_streams: entity.max_concurrent_streams,
                    created_at: entity.created_at,
                    updated_at: entity.updated_at,
                    last_generated_at: entity.last_generated_at,
                    relay_profile_id: entity.relay_profile_id,
                };
                
                debug!("Loaded configuration for proxy '{}' ({})", config.name, proxy_id);
                Ok(config)
            }
            None => {
                error!("Proxy {} not found or inactive", proxy_id);
                Err(format!("Proxy {proxy_id} not found or inactive").into())
            }
        }
    }

    /// Get the shared logo service (useful for other components)
    pub fn logo_service(&self) -> Arc<LogoAssetService> {
        self.logo_service.clone()
    }

    /// Get the app configuration
    pub fn app_config(&self) -> &Config {
        &self.app_config
    }

    /// Unregister an orchestrator when it completes (cleanup tracking)
    pub async fn unregister_orchestrator(&self, proxy_id: Uuid) {
        let mut active = self.active_orchestrators.lock().await;
        if let Some(pipeline_id) = active.remove(&proxy_id) {
            debug!("Unregistered orchestrator for proxy {} (pipeline: {})", proxy_id, pipeline_id);
        }
    }

    /// Check if an orchestrator is already active for a proxy
    pub async fn is_orchestrator_active(&self, proxy_id: Uuid) -> bool {
        let active = self.active_orchestrators.lock().await;
        active.contains_key(&proxy_id)
    }

    /// Create a factory with default storage paths (for testing/development)
    #[cfg(test)]
    pub async fn with_defaults(
        db_connection: sea_orm::DatabaseConnection,
        app_config: Config,
        pipeline_file_manager: SandboxedManager,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let storage_config = StorageConfig {
            m3u_path: PathBuf::from("./m3u"),
            m3u_retention: "30d".to_string(),
            m3u_cleanup_interval: "4h".to_string(),
            uploaded_logo_path: PathBuf::from("./uploads/logos"),
            cached_logo_path: PathBuf::from("./uploads/logos/cached"),
            cached_logo_retention: "90d".to_string(),
            cached_logo_cleanup_interval: "12h".to_string(),
            temp_path: Some("./temp".to_string()),
            temp_retention: "5m".to_string(),
            temp_cleanup_interval: "1m".to_string(),
            pipeline_path: PathBuf::from("./pipeline"),
            pipeline_retention: "10m".to_string(),
            pipeline_cleanup_interval: "5m".to_string(),
        };

        // Create Database wrapper from connection for test
        let test_database = crate::database::Database {
            connection: std::sync::Arc::new(db_connection),
            read_connection: std::sync::Arc::new(sea_orm::MockDatabase::new(sea_orm::DatabaseBackend::Sqlite).into_connection()),
            backend: sea_orm::DatabaseBackend::Sqlite,
            ingestion_config: crate::config::IngestionConfig::default(),
            database_type: crate::database::DatabaseType::SQLite,
        };
        
        // Create HTTP client factory for testing
        let http_client_factory = crate::utils::HttpClientFactory::new(None, std::time::Duration::from_secs(10));
        Self::from_components(test_database, app_config, storage_config, pipeline_file_manager, &http_client_factory).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::WebConfig;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_factory_creation() {
        // Create test configuration
        let temp_dir = TempDir::new().unwrap();
        let file_manager = SandboxedManager::builder()
            .base_directory(temp_dir.path())
            .build()
            .await
            .unwrap();
        
        let app_config = Config {
            web: WebConfig {
                base_url: "http://test:8080".to_string(),
                ..Default::default()
            },
            ..Default::default()
        };

        // Create in-memory database using SeaORM
        let db_connection = sea_orm::Database::connect("sqlite::memory:").await.unwrap();

        // Test factory creation
        let factory = PipelineOrchestratorFactory::with_defaults(
            db_connection,
            app_config,
            file_manager,
        ).await;

        assert!(factory.is_ok());
    }
}