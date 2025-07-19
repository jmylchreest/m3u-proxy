use crate::models::*;
use anyhow::Result;

use super::ingest_m3u::M3uIngestor;
use super::ingest_xtream::XtreamIngestor;
use super::state_manager::{IngestionStateManager, ProcessingTrigger};
use super::SourceIngestor;

pub struct StreamIngestor {
    state_manager: IngestionStateManager,
}

impl StreamIngestor {
    pub fn new(state_manager: IngestionStateManager) -> Self {
        Self { state_manager }
    }

    pub fn get_state_manager(&self) -> &IngestionStateManager {
        &self.state_manager
    }

    pub async fn ingest_stream_source(&self, source: &StreamSource) -> Result<Vec<Channel>> {
        self.ingest_stream_source_with_trigger(source, ProcessingTrigger::Manual)
            .await
    }

    pub async fn ingest_stream_source_with_trigger(
        &self,
        source: &StreamSource,
        trigger: ProcessingTrigger,
    ) -> Result<Vec<Channel>> {
        // Check if we can start processing this source
        if !self
            .state_manager
            .try_start_processing(source.id, trigger)
            .await
        {
            return Err(anyhow::anyhow!(
                "Source '{}' is already being processed or is in backoff period",
                source.name
            ));
        }

        self.state_manager.start_ingestion(source.id).await;

        let result = match source.source_type {
            StreamSourceType::M3u => {
                let parser = M3uIngestor::new();
                parser.ingest(source, &self.state_manager).await
            }
            StreamSourceType::Xtream => {
                let parser = XtreamIngestor::new();
                parser.ingest(source, &self.state_manager).await
            }
        };

        let success = result.is_ok();

        // Always finish processing to update failure state
        self.state_manager
            .finish_processing(source.id, success)
            .await;

        match result {
            Ok(channels) => Ok(channels),
            Err(e) => {
                self.state_manager.set_error(source.id, e.to_string()).await;
                Err(e)
            }
        }
    }

    /// Shared stream source refresh function used by both manual refresh and scheduler
    /// This ensures identical behavior and eliminates code duplication
    pub async fn refresh_stream_source(
        database: crate::database::Database,
        state_manager: IngestionStateManager,
        source: &StreamSource,
        trigger: ProcessingTrigger,
    ) -> Result<usize, Box<dyn std::error::Error + Send + Sync>> {
        use tracing::{error, info};

        let start_time = std::time::Instant::now();
        let source_id = source.id;
        let source_name = source.name.clone();

        info!(
            "Starting stream source refresh for '{}' ({}) - trigger: {:?}",
            source_name, source_id, trigger
        );

        let ingestor = StreamIngestor::new(state_manager.clone());

        match ingestor
            .ingest_stream_source_with_trigger(source, trigger.clone())
            .await
        {
            Ok(channels) => {
                let channel_count = channels.len();
                info!(
                    "Stream ingestion completed for '{}': {} channels",
                    source_name, channel_count
                );

                // Update the channels in database
                match database
                    .update_source_channels(source_id, &channels, Some(&state_manager))
                    .await
                {
                    Ok(_) => {
                        // Update last ingested timestamp
                        if let Err(e) = database.update_source_last_ingested(source_id).await {
                            error!(
                                "Failed to update last_ingested_at for stream source '{}': {}",
                                source_name, e
                            );
                        }

                        // Mark ingestion as completed with final channel count
                        state_manager
                            .complete_ingestion(source_id, channel_count)
                            .await;

                        let duration = start_time.elapsed();
                        info!(
                            "Stream source refresh completed source={} channels={} trigger={:?} duration={}",
                            source_name, channel_count, trigger, 
                            crate::utils::format_duration(duration.as_millis() as u64)
                        );

                        Ok(channel_count)
                    }
                    Err(e) => {
                        error!(
                            "Failed to save channels to database for stream source '{}': {}",
                            source_name, e
                        );
                        Err(e.into())
                    }
                }
            }
            Err(e) => {
                error!("Failed to refresh stream source '{}': {}", source_name, e);
                Err(e.into())
            }
        }
    }
}
