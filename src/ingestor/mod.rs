use crate::models::*;
use anyhow::Result;
use async_trait::async_trait;

pub mod m3u_parser;
pub mod scheduler;
pub mod state_manager;
pub mod xtream_parser;

pub use state_manager::IngestionStateManager;

#[async_trait]
pub trait SourceIngestor {
    async fn ingest(
        &self,
        source: &StreamSource,
        state_manager: &IngestionStateManager,
    ) -> Result<Vec<Channel>>;
}

pub struct IngestorService {
    state_manager: IngestionStateManager,
}

impl IngestorService {
    pub fn new(state_manager: IngestionStateManager) -> Self {
        Self { state_manager }
    }

    pub async fn ingest_source(&self, source: &StreamSource) -> Result<Vec<Channel>> {
        self.state_manager.start_ingestion(source.id).await;

        let result = match source.source_type {
            StreamSourceType::M3u => {
                let parser = m3u_parser::M3uIngestor::new();
                parser.ingest(source, &self.state_manager).await
            }
            StreamSourceType::Xtream => {
                let parser = xtream_parser::XtreamIngestor::new();
                parser.ingest(source, &self.state_manager).await
            }
        };

        match result {
            Ok(channels) => {
                self.state_manager
                    .complete_ingestion(source.id, channels.len())
                    .await;
                Ok(channels)
            }
            Err(e) => {
                self.state_manager.set_error(source.id, e.to_string()).await;
                Err(e)
            }
        }
    }

    #[allow(dead_code)]
    pub fn get_state_manager(&self) -> &IngestionStateManager {
        &self.state_manager
    }
}
