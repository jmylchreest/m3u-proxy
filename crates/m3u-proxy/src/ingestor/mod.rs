use crate::models::*;
use crate::repositories::{ChannelRepository, traits::RepositoryHelpers};
use crate::services::ProgressService;
use crate::sources::SourceHandlerFactory;
use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;

pub mod scheduler;
pub mod state_manager;
pub use state_manager::{IngestionStateManager, ProcessingTrigger};

#[async_trait]
pub trait SourceIngestor {
    async fn ingest(
        &self,
        source: &StreamSource,
        state_manager: &IngestionStateManager,
    ) -> Result<Vec<Channel>>;
}

/// Generic orchestrator service for source ingestion using new source handlers
pub struct IngestorService {
    progress_service: Arc<ProgressService>,
    channel_repo: ChannelRepository,
}

impl IngestorService {
    pub fn new(progress_service: Arc<ProgressService>, channel_repo: ChannelRepository) -> Self {
        Self { 
            progress_service,
            channel_repo,
        }
    }

    pub fn get_state_manager(&self) -> Arc<IngestionStateManager> {
        self.progress_service.get_ingestion_state_manager()
    }
    
    pub fn get_progress_service(&self) -> &ProgressService {
        &self.progress_service
    }

    /// Ingest stream source using new source handlers and save to database
    pub async fn ingest_source(
        &self, 
        database: crate::database::Database,
        source: &StreamSource
    ) -> Result<usize> {
        use tracing::{error, info, warn};
        
        let start_time = std::time::Instant::now();
        let source_id = source.id;
        let source_name = source.name.clone();

        // Check for duplicate processing - prevent race conditions and circular triggers
        if !self.progress_service.get_ingestion_state_manager()
            .try_start_processing(source_id, ProcessingTrigger::Scheduler).await {
            warn!("Skipping stream source ingestion for '{}' - already processing or in backoff period", source_name);
            return Ok(0);
        }

        info!("Starting stream source ingestion for '{}' ({})", source_name, source_id);

        // Create source handler
        let handler = SourceHandlerFactory::create_handler(&source.source_type)
            .map_err(|e| {
                tracing::error!("Failed to create source handler for '{}': {}", source.name, e);
                anyhow::anyhow!("Failed to create source handler: {}", e)
            })?;
        
        info!("Created {} source handler for '{}'", source.source_type, source.name);
        
        // Create one operation and pass its callback to the handler
        let _progress_callback = match self.progress_service
            .start_operation_with_id(
                source.id, // Use source.id as operation_id for consistency
                source.id, // owner_id same as operation_id
                "stream_source".to_string(),
                crate::services::progress_service::OperationType::StreamIngestion,
                format!("Stream Ingestion: {}", source.name)
            )
            .await {
                Ok(callback) => callback,
                Err(e) => {
                    warn!("Cannot start stream ingestion for '{}': {}", source.name, e);
                    return Err(crate::errors::AppError::operation_in_progress("stream ingestion", &source.name).into());
                }
            };
        
        info!("Starting channel ingestion with universal progress tracking for '{}'", source.name);

        // Use basic channel ingestion method
        let channels = handler
            .ingest_channels(source) 
            .await
            .map_err(|e| {
                tracing::error!("Source handler failed for '{}': {}", source.name, e);
                anyhow::anyhow!("New source handler failed: {}", e)
            })?;
            
        let channel_count = channels.len();
        info!("Successfully ingested {} channels for '{}'", channel_count, source.name);

        // Save channels to database
        info!("Saving {} channels to database for '{}'", channel_count, source_name);
        self.channel_repo
            .update_source_channels(source_id, &channels)
            .await
            .map_err(|e| {
                error!("Failed to save channels to database for '{}': {}", source_name, e);
                anyhow::anyhow!("Failed to update source channels: {}", e)
            })?;

        info!("Successfully saved {} channels to database for Stream source '{}'", channel_count, source_name);

        // Update last ingested timestamp using generic helper
        info!("Updating last_ingested_at timestamp for Stream source '{}'", source_name);
        if let Err(e) = RepositoryHelpers::update_last_ingested(&database.pool(), "stream_sources", source_id).await {
            error!("Failed to update last_ingested_at for stream source '{}': {}", source_name, e);
        } else {
            info!("Updated timestamp for Stream source '{}'", source_name);
        }

        // Mark ingestion as completed with final channel count
        info!("Marking ingestion as completed for Stream source '{}'", source_name);
        self.get_state_manager().complete_ingestion(source_id, channel_count).await;

        // Mark processing as completed
        self.progress_service.get_ingestion_state_manager()
            .finish_processing(source_id, true).await;

        let duration = start_time.elapsed();
        info!("Stream source ingestion completed for '{}' in {:.2}s: {} channels saved to database", 
              source_name, duration.as_secs_f64(), channel_count);

        Ok(channel_count)
    }

    /// Ingest stream source with trigger and save to database
    /// Note: Triggers are handled internally by the ProgressService
    pub async fn ingest_source_with_trigger(
        &self,
        database: crate::database::Database,
        source: &StreamSource,
        _trigger: ProcessingTrigger,
    ) -> Result<usize> {
        // New handlers use ProgressService instead of explicit triggers
        self.ingest_source(database, source).await
    }

    /// Ingest EPG source using new EPG source handler architecture
    pub async fn ingest_epg_source(
        &self,
        database: crate::database::Database,
        source: &EpgSource,
        _trigger: ProcessingTrigger,
    ) -> Result<(usize, usize), Box<dyn std::error::Error + Send + Sync>> {
        use tracing::{info, warn};
        let start_time = std::time::Instant::now();
        let source_id = source.id;
        let source_name = source.name.clone();

        // Check for duplicate processing - prevent race conditions and circular triggers
        if !self.progress_service.get_ingestion_state_manager()
            .try_start_processing(source_id, ProcessingTrigger::Scheduler).await {
            warn!("Skipping EPG source ingestion for '{}' - already processing or in backoff period", source_name);
            return Ok((0, 0));
        }
        
        // Create EPG source handler
        let handler = SourceHandlerFactory::create_epg_handler(&source.source_type)
            .map_err(|e| anyhow::anyhow!("Failed to create EPG source handler: {}", e))?;
        
        // Create universal progress callback via ProgressService  
        let _progress_callback = match self.progress_service
            .start_operation(
                source.id,
                "epg_source".to_string(),
                crate::services::progress_service::OperationType::EpgIngestion,
                format!("EPG Ingestion: {}", source.name)
            )
            .await {
                Ok(callback) => callback,
                Err(e) => {
                    warn!("Cannot start EPG ingestion for '{}': {}", source.name, e);
                    // Mark processing as completed since we're not proceeding
                    self.progress_service.get_ingestion_state_manager()
                        .finish_processing(source_id, false).await;
                    return Err(anyhow::anyhow!("Operation already in progress: {}", e).into());
                }
            };
        
        // Use new EPG source handler to ingest programs only (programs-only mode)
        let programs = handler
            .ingest_epg_programs(source) 
            .await
            .map_err(|e| anyhow::anyhow!("New EPG source handler failed: {}", e))?;
        
        info!(
            "EPG handler ingested {} programs from source '{}'",
            programs.len(),
            source.name
        );
        
        // Save programs to database (programs-only mode - no channel processing)
        let programs_saved = match self.save_epg_programs(&database, source.id, programs).await {
            Ok(count) => count,
            Err(e) => {
                warn!("Failed to save EPG programs for source '{}': {}", source_name, e);
                0
            }
        };
        
        // Always update timestamp, even if some data operations failed - using generic helper
        info!("Updating last_ingested_at timestamp for EPG source '{}'", source_name);
        if let Err(e) = RepositoryHelpers::update_last_ingested(&database.pool(), "epg_sources", source_id).await {
            warn!("Failed to update last_ingested_at for EPG source '{}': {}", source_name, e);
        } else {
            info!("Updated timestamp for EPG source '{}'", source_name);
        }
        
        // Mark processing as completed
        self.progress_service.get_ingestion_state_manager()
            .finish_processing(source_id, true).await;

        let duration = start_time.elapsed();
        info!(
            "EPG ingestion completed for EPG source '{}' in {:.2}s: {} programs saved",
            source_name, duration.as_secs_f64(), programs_saved
        );
        
        // Mark operation as completed
        self.progress_service.complete_operation(source.id).await;
        
        Ok((0, programs_saved)) // programs-only mode: no channels saved
    }
    
    /// Save EPG programs to database
    async fn save_epg_programs(
        &self,
        database: &crate::database::Database,
        source_id: uuid::Uuid,
        programs: Vec<crate::models::EpgProgram>,
    ) -> Result<usize, Box<dyn std::error::Error + Send + Sync>> {
        use tracing::debug;
        
        debug!("Saving {} EPG programs to database", programs.len());
        
        // Start a transaction for atomicity
        let mut tx = database.pool().begin().await?;
        
        // First, delete existing programs for this source to avoid duplicates
        sqlx::query("DELETE FROM epg_programs WHERE source_id = ?")
            .bind(source_id.to_string())
            .execute(&mut *tx)
            .await?;
        
        let mut programs_saved = 0;
        
        for program in programs {
            sqlx::query(
                "INSERT INTO epg_programs (
                    id, source_id, channel_id, channel_name, program_title, program_description, program_category,
                    start_time, end_time, episode_num, season_num, rating, language, subtitles, aspect_ratio, program_icon,
                    created_at, updated_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
            )
            .bind(program.id.to_string())
            .bind(program.source_id.to_string())
            .bind(&program.channel_id)
            .bind(&program.channel_name)
            .bind(&program.program_title)
            .bind(&program.program_description)
            .bind(&program.program_category)
            .bind(program.start_time)
            .bind(program.end_time)
            .bind(&program.episode_num)
            .bind(&program.season_num)
            .bind(&program.rating)
            .bind(&program.language)
            .bind(&program.subtitles)
            .bind(&program.aspect_ratio)
            .bind(&program.program_icon)
            .bind(program.created_at)
            .bind(program.updated_at)
            .execute(&mut *tx)
            .await?;
            
            programs_saved += 1;
        }
        
        tx.commit().await?;
        debug!("Successfully saved {} EPG programs", programs_saved);
        
        Ok(programs_saved)
    }

}
