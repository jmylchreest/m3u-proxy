//! EPG source service
//!
//! This service provides business logic for EPG source operations,
//! including auto-linking with stream sources for Xtream providers.

use anyhow::Result;
use tokio::sync::broadcast;
use tracing::{error, info, warn};

use crate::database::Database;
use crate::models::{EpgSource, EpgSourceCreateRequest, EpgSourceUpdateRequest};
use crate::utils::DatabaseOperations;
use crate::repositories::{UrlLinkingRepository, EpgSourceRepository, Repository};

/// Service for managing EPG sources with business logic
pub struct EpgSourceService {
    database: Database,
    epg_source_repo: EpgSourceRepository,
    cache_invalidation_tx: broadcast::Sender<()>,
}

impl EpgSourceService {
    /// Create a new EPG source service
    pub fn new(database: Database, cache_invalidation_tx: broadcast::Sender<()>) -> Self {
        let epg_source_repo = EpgSourceRepository::new(database.pool());
        Self {
            database,
            epg_source_repo,
            cache_invalidation_tx,
        }
    }

    /// Create an EPG source with automatic stream linking for Xtream sources
    pub async fn create_with_auto_stream(
        &self,
        request: EpgSourceCreateRequest,
    ) -> Result<EpgSource> {
        info!("Creating EPG source: {}", request.name);

        // Create the EPG source (this includes auto-stream creation logic)
        let source = self.epg_source_repo.create(request).await
            .map_err(|e| anyhow::anyhow!("Failed to create EPG source: {}", e))?;

        // Auto-populate credentials from linked stream sources if this is an Xtream source
        let final_source = if source.source_type == crate::models::EpgSourceType::Xtream {
            match self.database.auto_populate_epg_credentials(source.id).await {
                Ok(Some(updated_source)) => {
                    if updated_source.username.is_some() && source.username.is_none() {
                        info!("Auto-populated credentials for EPG source '{}'", source.name);
                    }
                    updated_source
                }
                Ok(None) => source,
                Err(e) => {
                    error!("Failed to auto-populate EPG source '{}': {}", source.name, e);
                    source
                }
            }
        } else {
            source
        };

        // Invalidate cache since we added a new source
        let _ = self.cache_invalidation_tx.send(());

        info!(
            "Successfully created EPG source: {} ({})",
            final_source.name, final_source.id
        );

        Ok(final_source)
    }

    /// Update an EPG source with validation
    pub async fn update_with_validation(
        &self,
        id: uuid::Uuid,
        request: EpgSourceUpdateRequest,
    ) -> Result<EpgSource> {
        info!("Updating EPG source: {}", id);

        // Update linked sources first if requested
        if request.update_linked {
            let url_linking_repo = UrlLinkingRepository::new(self.database.pool());
            match url_linking_repo.update_linked_sources(
                id,
                "epg",
                Some(&request.url),
                request.username.as_ref(),
                request.password.as_ref(),
                request.update_linked,
            ).await {
                Ok(count) if count > 0 => {
                    info!("Updated {} linked sources for EPG source {}", count, id);
                }
                Ok(_) => {
                    // No linked sources to update
                }
                Err(e) => {
                    error!("Failed to update linked sources for EPG source '{}': {}", id, e);
                }
            }
        }

        // Update the EPG source
        let source = self.epg_source_repo.update(id, request).await
            .map_err(|e| anyhow::anyhow!("Failed to update EPG source: {}", e))?;

        // Invalidate cache
        let _ = self.cache_invalidation_tx.send(());

        info!(
            "Successfully updated EPG source: {} ({})",
            source.name, source.id
        );

        Ok(source)
    }

    /// Delete an EPG source with proper cleanup
    pub async fn delete_with_cleanup(&self, id: uuid::Uuid) -> Result<()> {
        info!("Deleting EPG source: {}", id);

        // Delete the EPG source (this will cascade to linked sources)
        self.epg_source_repo.delete(id).await
            .map_err(|e| anyhow::anyhow!("Failed to delete EPG source: {}", e))?;

        // Invalidate cache
        let _ = self.cache_invalidation_tx.send(());

        info!("EPG source {} deleted successfully", id);
        Ok(())
    }

    /// List EPG sources with statistics
    pub async fn list_with_stats(&self) -> Result<Vec<EpgSourceWithStats>> {
        let sources_with_stats = self.epg_source_repo.list_with_stats().await
            .map_err(|e| anyhow::anyhow!("Failed to list EPG sources with stats: {}", e))?;

        let mut result = Vec::new();
        for source_with_stats in sources_with_stats {
            result.push(EpgSourceWithStats {
                source: source_with_stats.source.clone(),
                channel_count: source_with_stats.channel_count as u64,
                program_count: source_with_stats.program_count as u64,
                next_scheduled_update: source_with_stats.next_scheduled_update,
                last_ingested_at: source_with_stats.source.last_ingested_at,
                is_active: source_with_stats.source.is_active,
            });
        }

        Ok(result)
    }

    /// Get an EPG source with detailed information
    pub async fn get_with_details(&self, id: uuid::Uuid) -> Result<EpgSourceWithDetails> {
        let source = self.epg_source_repo.find_by_id(id).await
            .map_err(|e| anyhow::anyhow!("Failed to get EPG source: {}", e))?
            .ok_or_else(|| anyhow::anyhow!("EPG source not found"))?;

        let channel_count = self.epg_source_repo.get_channel_count(id).await
            .map_err(|e| anyhow::anyhow!("Failed to get channel count: {}", e))? as u64;
        
        // Find linked stream sources using URL-based matching
        let linked_stream = if source.source_type == crate::models::EpgSourceType::Xtream {
            let linked_sources = self.database.find_linked_stream_sources(&source).await
                .unwrap_or_default();
            linked_sources.into_iter().next() // Return first linked stream source if any
        } else {
            None
        };

        Ok(EpgSourceWithDetails {
            source: source.clone(),
            channel_count,
            next_scheduled_update: None, // TODO: Implement scheduling info
            last_ingested_at: source.last_ingested_at,
            is_active: source.is_active,
            linked_stream_source: linked_stream,
        })
    }

    /// Get EPG source by ID
    pub async fn get(&self, id: uuid::Uuid) -> Result<EpgSource> {
        self.epg_source_repo.find_by_id(id).await
            .map_err(|e| anyhow::anyhow!("Failed to get EPG source: {}", e))?
            .ok_or_else(|| anyhow::anyhow!("EPG source not found"))
    }

    /// List all EPG sources
    pub async fn list(&self) -> Result<Vec<EpgSource>> {
        use crate::repositories::epg_source::EpgSourceQuery;
        self.epg_source_repo.find_all(EpgSourceQuery::new()).await
            .map_err(|e| anyhow::anyhow!("Failed to list EPG sources: {}", e))
    }

    /// Check if an EPG source exists
    pub async fn exists(&self, id: uuid::Uuid) -> Result<bool> {
        Ok(self.epg_source_repo.find_by_id(id).await
            .map_err(|e| anyhow::anyhow!("Failed to check EPG source existence: {}", e))?
            .is_some())
    }

    /// Test connection to an EPG source
    pub async fn test_connection(
        &self,
        request: &EpgSourceCreateRequest,
    ) -> Result<TestConnectionResult> {
        // This would test the connection without creating the source
        // Implementation would depend on source type
        match request.source_type {
            crate::models::EpgSourceType::Xtream => {
                self.test_xtream_connection(&request.url, &request.username, &request.password)
                    .await
            }
            crate::models::EpgSourceType::Xmltv => self.test_xmltv_connection(&request.url).await,
        }
    }

    /// Test Xtream connection for EPG
    async fn test_xtream_connection(
        &self,
        url: &str,
        username: &Option<String>,
        password: &Option<String>,
    ) -> Result<TestConnectionResult> {
        if let (Some(username), Some(password)) = (username, password) {
            let client = reqwest::Client::new();
            let epg_url = format!(
                "{}xmltv.php?username={}&password={}",
                url, username, password
            );

            match client.head(&epg_url).send().await {
                Ok(response) if response.status().is_success() => {
                    // Check if it also has stream data
                    let has_streams = self
                        .check_stream_availability(url, username, password)
                        .await?;

                    Ok(TestConnectionResult {
                        success: true,
                        message: "Connection successful".to_string(),
                        has_epg: true,
                        has_streams,
                    })
                }
                Ok(response) => Ok(TestConnectionResult {
                    success: false,
                    message: format!("Server returned status: {}", response.status()),
                    has_epg: false,
                    has_streams: false,
                }),
                Err(e) => Ok(TestConnectionResult {
                    success: false,
                    message: format!("Connection failed: {}", e),
                    has_epg: false,
                    has_streams: false,
                }),
            }
        } else {
            Ok(TestConnectionResult {
                success: false,
                message: "Username and password are required for Xtream sources".to_string(),
                has_epg: false,
                has_streams: false,
            })
        }
    }

    /// Test XMLTV connection
    async fn test_xmltv_connection(&self, url: &str) -> Result<TestConnectionResult> {
        let client = reqwest::Client::new();

        match client.get(url).send().await {
            Ok(response) if response.status().is_success() => {
                let content = response.text().await?;
                let has_epg = content.contains("<tv") && content.contains("</tv>");

                Ok(TestConnectionResult {
                    success: true,
                    message: "Connection successful".to_string(),
                    has_epg,
                    has_streams: false, // XMLTV sources don't have streams
                })
            }
            Ok(response) => Ok(TestConnectionResult {
                success: false,
                message: format!("Server returned status: {}", response.status()),
                has_epg: false,
                has_streams: false,
            }),
            Err(e) => Ok(TestConnectionResult {
                success: false,
                message: format!("Connection failed: {}", e),
                has_epg: false,
                has_streams: false,
            }),
        }
    }

    /// Check if Xtream server has stream data
    async fn check_stream_availability(
        &self,
        url: &str,
        username: &str,
        password: &str,
    ) -> Result<bool> {
        let client = reqwest::Client::new();
        let stream_url = format!(
            "{}player_api.php?username={}&password={}&action=get_live_categories",
            url, username, password
        );

        match client.head(&stream_url).send().await {
            Ok(response) if response.status().is_success() => Ok(true),
            _ => Ok(false),
        }
    }
}

/// EPG source with statistics
#[derive(Debug, Clone)]
pub struct EpgSourceWithStats {
    pub source: EpgSource,
    pub channel_count: u64,
    pub program_count: u64,
    pub next_scheduled_update: Option<chrono::DateTime<chrono::Utc>>,
    pub last_ingested_at: Option<chrono::DateTime<chrono::Utc>>,
    pub is_active: bool,
}

/// EPG source with detailed information
#[derive(Debug, Clone)]
pub struct EpgSourceWithDetails {
    pub source: EpgSource,
    pub channel_count: u64,
    pub next_scheduled_update: Option<chrono::DateTime<chrono::Utc>>,
    pub last_ingested_at: Option<chrono::DateTime<chrono::Utc>>,
    pub is_active: bool,
    pub linked_stream_source: Option<crate::models::StreamSource>,
}

/// Result of testing connection to an EPG source
#[derive(Debug, Clone)]
pub struct TestConnectionResult {
    pub success: bool,
    pub message: String,
    pub has_epg: bool,
    pub has_streams: bool,
}

impl EpgSourceService {
    


    /// Save EPG programs to database with robust batching and retry logic
    async fn save_epg_programs(
        &self,
        source_id: uuid::Uuid,
        programs: Vec<crate::models::EpgProgram>,
        progress_updater: Option<&crate::services::progress_service::ProgressStageUpdater>,
    ) -> Result<usize> {
        use tracing::{debug, info};
        
        if programs.is_empty() {
            debug!("No EPG programs to save for source: {}", source_id);
            return Ok(0);
        }


        // Optimize database for bulk operations
        DatabaseOperations::optimize_for_bulk_operations(&self.database.pool()).await?;

        // Delete existing programs for this source first (separate transaction)
        let deleted_count = DatabaseOperations::delete_epg_programs_for_source(
            source_id,
            &self.database.pool(),
        ).await?;

        info!("Deleted {} existing EPG programs for source: {}", deleted_count, source_id);

        // Determine optimal batch size (default to 1800, but can be configured)
        let batch_size = std::env::var("EPG_BATCH_SIZE")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(1800);

        debug!("Using batch size: {} for EPG program insertion", batch_size);

        // Insert programs in batches with retry logic
        let total_saved = DatabaseOperations::save_epg_programs_in_batches(
            &self.database.pool(),
            source_id,
            programs,
            batch_size,
            progress_updater,
        ).await?;

        // Perform WAL checkpoint after large operation
        if total_saved > 5000 {
            DatabaseOperations::checkpoint_wal(&self.database.pool()).await?;
        }

        info!("Successfully saved {} EPG programs for source: {}", total_saved, source_id);
        Ok(total_saved)
    }
    
    /// Ingest EPG programs using ProgressStageUpdater (new API)
    pub async fn ingest_programs_with_progress_updater(
        &self,
        source: &EpgSource,
        progress_updater: Option<&crate::services::progress_service::ProgressStageUpdater>,
    ) -> Result<usize> {
        use crate::sources::factory::SourceHandlerFactory;
        
        info!("Starting EPG ingestion with ProgressStageUpdater for source: {}", source.name);
        
        // Wrap the entire operation in error handling to ensure progress completion
        let result = async {
            // Create EPG source handler using the factory
            let handler = SourceHandlerFactory::create_epg_handler(&source.source_type)
                .map_err(|e| anyhow::anyhow!("Failed to create EPG source handler: {}", e))?;
                
            // Use the new ProgressStageUpdater API
            let programs = handler
                .ingest_epg_programs_with_progress_updater(source, progress_updater)
                .await
                .map_err(|e| anyhow::anyhow!("EPG source handler failed: {}", e))?;
            
            info!(
                "EPG handler ingested {} programs from source '{}'",
                programs.len(), source.name
            );
            
            // Update progress: inserting to database (this is 80% of the total work)
            if let Some(updater) = progress_updater {
                updater.update_progress(20.0, &format!("Inserting {} programs to database", programs.len())).await;
            }
            
            // Save programs to database
            info!("Saving {} EPG programs to database for '{}'", programs.len(), source.name);
            let programs_saved = self.save_epg_programs(source.id, programs, progress_updater).await?;
            
            // Mark stage as completed
            if let Some(updater) = progress_updater {
                updater.update_progress(100.0, &format!("Completed: {} programs saved", programs_saved)).await;
                updater.complete_stage().await;
            }
            
            // Update the source's last_ingested_at timestamp
            if let Err(e) = self.epg_source_repo.update_last_ingested(source.id).await {
                warn!("Failed to update last_ingested_at for EPG source '{}': {}", source.name, e);
            } else {
                info!("Updated last_ingested_at for EPG source '{}'", source.name);
            }
            
            info!(
                "EPG ingestion completed for source '{}': {} programs saved",
                source.name, programs_saved
            );
            
            Ok(programs_saved)
        }.await;
        
        // Always complete the operation, whether successful or failed
        if let Some(updater) = progress_updater {
            match &result {
                Ok(_) => {
                    updater.complete_operation().await;
                }
                Err(e) => {
                    // Mark operation as failed with error message
                    updater.fail_operation(&format!("EPG ingestion failed: {}", e)).await;
                }
            }
        }
        
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_create_with_auto_stream() {
        // Test would create a service and test the create_with_auto_stream method
        // This would require setting up a test database
    }

    #[tokio::test]
    async fn test_connection_validation() {
        // Test the connection validation logic
    }
}
