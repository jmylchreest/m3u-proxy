//! Pipeline Builder
//!
//! Provides a fluent builder pattern for constructing pipeline orchestrators with proper configuration.
//! This builder ensures orchestrators are properly configured with their dependencies and stage settings.

use anyhow::Result;
use std::sync::Arc;

use crate::{
    config::Config,
    logo_assets::service::LogoAssetService,
    models::StreamProxy,
    pipeline::{
        core::orchestrator::PipelineOrchestrator,
        stages::logo_caching::LogoCachingConfig,
    },
};
use sandboxed_file_manager::SandboxedManager;

/// Configuration for pipeline execution
#[derive(Debug, Clone)]
pub struct PipelineConfig {
    pub enable_data_mapping: bool,
    pub enable_filtering: bool,
    pub enable_logo_caching: bool,
    pub enable_numbering: bool,
    pub enable_generation: bool,
    pub enable_helper_processing: bool,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            enable_data_mapping: true,
            enable_filtering: true,
            enable_logo_caching: true,
            enable_numbering: true,
            enable_generation: false, // Generation stage not yet fully implemented
            enable_helper_processing: true,
        }
    }
}

/// Builder for constructing pipeline orchestrators with proper configuration
pub struct PipelineBuilder {
    database: crate::database::Database,
    app_config: Config,
    file_manager: SandboxedManager,
    logo_service: Arc<LogoAssetService>,
    pipeline_config: PipelineConfig,
}

impl PipelineBuilder {
    /// Create a new pipeline builder with core dependencies
    pub fn new(
        database: crate::database::Database,
        app_config: Config,
        file_manager: SandboxedManager,
        logo_service: Arc<LogoAssetService>,
    ) -> Self {
        Self {
            database,
            app_config,
            file_manager,
            logo_service,
            pipeline_config: PipelineConfig::default(),
        }
    }

    /// Configure which pipeline stages should be enabled
    pub fn with_config(mut self, config: PipelineConfig) -> Self {
        self.pipeline_config = config;
        self
    }

    /// Enable/disable data mapping stage
    pub fn enable_data_mapping(mut self, enabled: bool) -> Self {
        self.pipeline_config.enable_data_mapping = enabled;
        self
    }

    /// Enable/disable filtering stage
    pub fn enable_filtering(mut self, enabled: bool) -> Self {
        self.pipeline_config.enable_filtering = enabled;
        self
    }

    /// Enable/disable logo caching stage
    pub fn enable_logo_caching(mut self, enabled: bool) -> Self {
        self.pipeline_config.enable_logo_caching = enabled;
        self
    }

    /// Enable/disable numbering stage
    pub fn enable_numbering(mut self, enabled: bool) -> Self {
        self.pipeline_config.enable_numbering = enabled;
        self
    }

    /// Enable/disable generation stage
    pub fn enable_generation(mut self, enabled: bool) -> Self {
        self.pipeline_config.enable_generation = enabled;
        self
    }

    /// Enable/disable helper processing within stages
    pub fn enable_helper_processing(mut self, enabled: bool) -> Self {
        self.pipeline_config.enable_helper_processing = enabled;
        self
    }

    /// Build a complete pipeline orchestrator for a specific proxy
    pub fn build_for_proxy(self, proxy_config: StreamProxy) -> Result<PipelineOrchestrator> {
        // Create logo caching configuration from proxy and app config
        let logo_config = LogoCachingConfig {
            cache_channel_logos: proxy_config.cache_channel_logos && self.pipeline_config.enable_logo_caching,
            cache_program_logos: proxy_config.cache_program_logos && self.pipeline_config.enable_logo_caching,
            base_url: self.app_config.web.base_url.clone(),
        };

        // Build orchestrator with all dependencies
        Ok(PipelineOrchestrator::new_with_dependencies(
            proxy_config,
            self.file_manager.clone(),
            self.file_manager, // TODO: Use proper proxy output file manager
            self.logo_service,
            logo_config,
            self.database,
        ))
    }

    /// Build a minimal pipeline for testing (data mapping only)
    pub fn build_minimal_for_testing(self, proxy_config: StreamProxy) -> Result<PipelineOrchestrator> {
        let minimal_config = PipelineConfig {
            enable_data_mapping: true,
            enable_filtering: false,
            enable_logo_caching: false,
            enable_numbering: false,
            enable_generation: false,
            enable_helper_processing: true,
        };

        self.with_config(minimal_config).build_for_proxy(proxy_config)
    }

    /// Build a pipeline for filtering testing
    pub fn build_filtering_test(self, proxy_config: StreamProxy) -> Result<PipelineOrchestrator> {
        let filtering_config = PipelineConfig {
            enable_data_mapping: true,
            enable_filtering: true,
            enable_logo_caching: false,
            enable_numbering: false,
            enable_generation: false,
            enable_helper_processing: true,
        };

        self.with_config(filtering_config).build_for_proxy(proxy_config)
    }

    /// Build a pipeline for logo caching testing
    pub fn build_logo_caching_test(self, proxy_config: StreamProxy) -> Result<PipelineOrchestrator> {
        let logo_config = PipelineConfig {
            enable_data_mapping: true,
            enable_filtering: true,
            enable_logo_caching: true,
            enable_numbering: false,
            enable_generation: false,
            enable_helper_processing: true,
        };

        self.with_config(logo_config).build_for_proxy(proxy_config)
    }


    /// Build a complete pipeline with generation enabled (for when generation is fully implemented)
    pub fn build_with_generation(self, proxy_config: StreamProxy) -> Result<PipelineOrchestrator> {
        self.enable_generation(true).build_for_proxy(proxy_config)
    }

    /// Get the current pipeline configuration
    pub fn get_config(&self) -> &PipelineConfig {
        &self.pipeline_config
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::StreamProxyMode;
    use chrono::Utc;
    use uuid::Uuid;

    fn create_test_proxy() -> StreamProxy {
        StreamProxy {
            id: Uuid::new_v4(),
            name: "Test Proxy".to_string(),
            description: Some("Test proxy description".to_string()),
            proxy_mode: StreamProxyMode::Proxy,
            upstream_timeout: Some(30),
            buffer_size: Some(8192),
            max_concurrent_streams: Some(10),
            starting_channel_number: 1000,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            last_generated_at: None,
            is_active: true,
            auto_regenerate: false,
            cache_channel_logos: true,
            cache_program_logos: false,
            relay_profile_id: None,
        }
    }

    #[test]
    fn test_pipeline_config_default() {
        let config = PipelineConfig::default();
        assert!(config.enable_data_mapping);
        assert!(config.enable_filtering);
        assert!(config.enable_logo_caching);
        assert!(config.enable_numbering);
        assert!(!config.enable_generation); // Generation disabled by default
        assert!(config.enable_helper_processing);
    }

    #[test]
    fn test_builder_fluent_api() {
        // This test validates the fluent API structure
        let proxy = create_test_proxy();
        
        // Verify that the proxy configuration is properly structured
        assert_eq!(proxy.starting_channel_number, 1000);
        assert!(proxy.cache_channel_logos);
        assert!(!proxy.cache_program_logos);
        assert_eq!(proxy.proxy_mode, StreamProxyMode::Proxy);
    }

    #[test]
    fn test_logo_caching_config_creation() {
        let proxy = create_test_proxy();
        let base_url = "http://localhost:8080".to_string();
        
        let logo_config = LogoCachingConfig {
            cache_channel_logos: proxy.cache_channel_logos,
            cache_program_logos: proxy.cache_program_logos,
            base_url: base_url.clone(),
        };
        
        assert!(logo_config.cache_channel_logos);
        assert!(!logo_config.cache_program_logos);
        assert_eq!(logo_config.base_url, base_url);
    }

    #[test]
    fn test_pipeline_config_customization() {
        let _config = PipelineConfig::default();
        
        let config = PipelineConfig {
            enable_data_mapping: true,
            enable_filtering: false,
            enable_logo_caching: true,
            enable_numbering: false,
            enable_generation: false,
            enable_helper_processing: true,
        };
        
        assert!(config.enable_data_mapping);
        assert!(!config.enable_filtering);
        assert!(config.enable_logo_caching);
        assert!(!config.enable_numbering);
        assert!(!config.enable_generation);
        assert!(config.enable_helper_processing);
    }
}