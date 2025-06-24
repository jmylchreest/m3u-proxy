use anyhow::Result;

use crate::config::StorageConfig;
use crate::data_mapping::service::DataMappingService;
use crate::database::Database;
use crate::logo_assets::service::LogoAssetService;
use crate::models::*;

pub mod filter_engine;
pub mod generator;

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
    ) -> Result<ProxyGeneration> {
        let generator = generator::ProxyGenerator::new(self.storage_config.clone());
        generator
            .generate(
                proxy,
                database,
                data_mapping_service,
                logo_service,
                base_url,
            )
            .await
    }

    /// Apply filters to a list of channels (utility method)
    #[allow(dead_code)]
    pub async fn apply_filters(
        &self,
        channels: Vec<Channel>,
        filters: Vec<(Filter, ProxyFilter, Vec<FilterCondition>)>,
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
