use crate::models::*;
use anyhow::Result;
use async_trait::async_trait;

pub mod ingest_epg;
pub mod ingest_m3u;
pub mod ingest_stream;
pub mod ingest_xtream;
pub mod scheduler;
pub mod state_manager;

pub use ingest_epg::EpgIngestor;
pub use ingest_stream::StreamIngestor;
pub use state_manager::{IngestionStateManager, ProcessingTrigger};

#[async_trait]
pub trait SourceIngestor {
    async fn ingest(
        &self,
        source: &StreamSource,
        state_manager: &IngestionStateManager,
    ) -> Result<Vec<Channel>>;
}

/// Generic orchestrator service that delegates to specific ingestors
pub struct IngestorService {
    state_manager: IngestionStateManager,
}

impl IngestorService {
    pub fn new(state_manager: IngestionStateManager) -> Self {
        Self { state_manager }
    }

    pub fn get_state_manager(&self) -> &IngestionStateManager {
        &self.state_manager
    }

    /// Delegate stream source ingestion to StreamIngestor
    pub async fn ingest_source(&self, source: &StreamSource) -> Result<Vec<Channel>> {
        let stream_ingestor = StreamIngestor::new(self.state_manager.clone());
        stream_ingestor.ingest_stream_source(source).await
    }

    /// Delegate stream source ingestion with trigger to StreamIngestor
    pub async fn ingest_source_with_trigger(
        &self,
        source: &StreamSource,
        trigger: ProcessingTrigger,
    ) -> Result<Vec<Channel>> {
        let stream_ingestor = StreamIngestor::new(self.state_manager.clone());
        stream_ingestor
            .ingest_stream_source_with_trigger(source, trigger)
            .await
    }

    /// Delegate EPG source ingestion to EpgIngestor
    pub async fn ingest_epg_source(
        &self,
        database: crate::database::Database,
        source: &EpgSource,
        trigger: ProcessingTrigger,
    ) -> Result<(usize, usize), Box<dyn std::error::Error + Send + Sync>> {
        EpgIngestor::refresh_epg_source(database, self.state_manager.clone(), source, trigger).await
    }

    /// Delegate stream source refresh to StreamIngestor
    pub async fn refresh_stream_source(
        &self,
        database: crate::database::Database,
        source: &StreamSource,
        trigger: ProcessingTrigger,
    ) -> Result<usize, Box<dyn std::error::Error + Send + Sync>> {
        StreamIngestor::refresh_stream_source(database, self.state_manager.clone(), source, trigger)
            .await
    }
}
