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
use sqlx::SqlitePool;
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
    db_pool: SqlitePool,
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
        db_pool: SqlitePool,
        logo_service: Arc<LogoAssetService>,
        app_config: Config,
        pipeline_file_manager: SandboxedManager,
        proxy_output_file_manager: SandboxedManager,
    ) -> Self {
        Self {
            db_pool,
            logo_service,
            app_config,
            pipeline_file_manager,
            proxy_output_file_manager,
            active_orchestrators: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Create a factory from basic components, creating shared services
    pub async fn from_components(
        db_pool: SqlitePool,
        app_config: Config,
        storage_config: StorageConfig,
        pipeline_file_manager: SandboxedManager,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        // Create logo storage from configuration
        let logo_storage = LogoAssetStorage::new(
            storage_config.uploaded_logo_path.clone(),
            storage_config.cached_logo_path.clone(),
        );

        // Create shared logo service
        let logo_service = Arc::new(LogoAssetService::new(db_pool.clone(), logo_storage));

        // Create proxy output file manager for final M3U/XMLTV files
        let proxy_output_file_manager = SandboxedManager::builder()
            .base_directory(&storage_config.m3u_path)
            .cleanup_policy(CleanupPolicy::new()
                .remove_after(humantime::parse_duration(&storage_config.m3u_retention)?)
                .time_match(TimeMatch::LastAccess))
            .cleanup_interval(humantime::parse_duration(&storage_config.m3u_cleanup_interval)?)
            .build()
            .await?;

        Ok(Self::new(db_pool, logo_service, app_config, pipeline_file_manager, proxy_output_file_manager))
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
                    "Pipeline orchestrator already exists for proxy {} (pipeline: {}). Preventing duplicate creation.",
                    proxy_id, existing_pipeline_id
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
            self.db_pool.clone(),
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
        use crate::utils::uuid_parser::parse_uuid_flexible;
        use sqlx::Row;
        
        let row = sqlx::query(
            "SELECT id, name, description, starting_channel_number, is_active, auto_regenerate, 
             cache_channel_logos, cache_program_logos, proxy_mode, upstream_timeout, 
             buffer_size, max_concurrent_streams, created_at, updated_at, last_generated_at, 
             relay_profile_id FROM stream_proxies WHERE id = ? AND is_active = 1"
        )
        .bind(proxy_id.to_string())
        .fetch_optional(&self.db_pool)
        .await?;

        match row {
            Some(row) => {
                // Parse proxy mode
                let proxy_mode_str = row.get::<String, _>("proxy_mode");
                let proxy_mode = crate::models::StreamProxyMode::from_str(&proxy_mode_str);
                
                // Build StreamProxy using flexible UUID parsing
                let config = StreamProxy {
                    id: parse_uuid_flexible(&row.get::<String, _>("id"))?,
                    name: row.get("name"),
                    description: row.get("description"),
                    starting_channel_number: row.get::<i64, _>("starting_channel_number") as i32,
                    is_active: row.get("is_active"),
                    auto_regenerate: row.get("auto_regenerate"),
                    cache_channel_logos: row.get("cache_channel_logos"),
                    cache_program_logos: row.get("cache_program_logos"),
                    proxy_mode,
                    upstream_timeout: row.get::<Option<i64>, _>("upstream_timeout").map(|v| v as i32),
                    buffer_size: row.get::<Option<i64>, _>("buffer_size").map(|v| v as i32),
                    max_concurrent_streams: row.get::<Option<i64>, _>("max_concurrent_streams").map(|v| v as i32),
                    created_at: crate::utils::datetime::DateTimeParser::parse_flexible(&row.get::<String, _>("created_at"))?,
                    updated_at: crate::utils::datetime::DateTimeParser::parse_flexible(&row.get::<String, _>("updated_at"))?,
                    last_generated_at: row.get::<Option<String>, _>("last_generated_at")
                        .map(|s| crate::utils::datetime::DateTimeParser::parse_flexible(&s).ok())
                        .flatten(),
                    relay_profile_id: row.get::<Option<String>, _>("relay_profile_id")
                        .map(|s| parse_uuid_flexible(&s).ok())
                        .flatten(),
                };
                
                debug!("Loaded configuration for proxy '{}' ({})", config.name, proxy_id);
                Ok(config)
            }
            None => {
                error!("Proxy {} not found or inactive", proxy_id);
                Err(format!("Proxy {} not found or inactive", proxy_id).into())
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
        db_pool: SqlitePool,
        app_config: Config,
        pipeline_file_manager: SandboxedManager,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let storage_config = StorageConfig {
            uploaded_logo_path: PathBuf::from("./uploads/logos"),
            cached_logo_path: PathBuf::from("./uploads/logos/cached"),
            ..Default::default()
        };

        Self::from_components(db_pool, app_config, storage_config, pipeline_file_manager).await
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
        let file_manager = SandboxedManager::new(temp_dir.path()).unwrap();
        
        let app_config = Config {
            web: WebConfig {
                base_url: "http://test:8080".to_string(),
                ..Default::default()
            },
            ..Default::default()
        };

        // Create in-memory database
        let db_pool = SqlitePool::connect(":memory:").await.unwrap();

        // Test factory creation
        let factory = PipelineOrchestratorFactory::with_defaults(
            db_pool,
            app_config,
            file_manager,
        ).await;

        assert!(factory.is_ok());
    }
}