use anyhow::Result;

use crate::models::*;

pub mod filter_engine;
pub mod generator;

#[allow(dead_code)]
pub struct ProxyService {
    // TODO: Add database and other dependencies
}

impl ProxyService {
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self {}
    }

    #[allow(dead_code)]
    pub async fn generate_proxy(&self, proxy: &StreamProxy) -> Result<ProxyGeneration> {
        let generator = generator::ProxyGenerator::new();
        generator.generate(proxy).await
    }

    #[allow(dead_code)]
    pub async fn apply_filters(
        &self,
        channels: Vec<Channel>,
        filters: Vec<(Filter, ProxyFilter)>,
    ) -> Result<Vec<Channel>> {
        let mut engine = filter_engine::FilterEngine::new();
        engine.apply_filters(channels, filters).await
    }
}
