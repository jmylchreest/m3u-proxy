use anyhow::Result;

use crate::config::StorageConfig;
use crate::models::*;

pub mod filter_engine;
pub mod generator;

#[allow(dead_code)]
pub struct ProxyService {
    storage_config: StorageConfig,
}

impl ProxyService {
    #[allow(dead_code)]
    pub fn new(storage_config: StorageConfig) -> Self {
        Self { storage_config }
    }

    #[allow(dead_code)]
    pub async fn generate_proxy(&self, proxy: &StreamProxy) -> Result<ProxyGeneration> {
        let generator = generator::ProxyGenerator::new(self.storage_config.clone());
        generator.generate(proxy).await
    }

    #[allow(dead_code)]
    pub async fn apply_filters(
        &self,
        channels: Vec<Channel>,
        filters: Vec<(Filter, ProxyFilter, Vec<FilterCondition>)>,
    ) -> Result<Vec<Channel>> {
        let mut engine = filter_engine::FilterEngine::new();
        engine.apply_filters(channels, filters).await
    }
}
