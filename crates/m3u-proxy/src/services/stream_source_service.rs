//! Stream source service
//!
//! This service provides business logic for stream source operations,
//! including auto-linking with EPG sources for Xtream providers.

use anyhow::Result;
use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::{debug, error, info};

use crate::database::Database;
use crate::models::{StreamSource, StreamSourceCreateRequest, StreamSourceUpdateRequest};
use crate::services::EpgSourceService;

/// Service for managing stream sources with business logic
pub struct StreamSourceService {
    database: Database,
    #[allow(dead_code)]
    epg_service: Arc<EpgSourceService>,
    cache_invalidation_tx: broadcast::Sender<()>,
}

impl StreamSourceService {
    /// Create a new stream source service
    pub fn new(
        database: Database,
        epg_service: Arc<EpgSourceService>,
        cache_invalidation_tx: broadcast::Sender<()>,
    ) -> Self {
        Self {
            database,
            epg_service,
            cache_invalidation_tx,
        }
    }

    /// Create a stream source with automatic EPG linking for Xtream sources
    pub async fn create_with_auto_epg(
        &self,
        request: StreamSourceCreateRequest,
    ) -> Result<StreamSource> {
        info!("Creating stream source: {}", request.name);

        // Create the stream source
        let source = self.database.create_stream_source(&request).await?;

        // Auto-link with existing EPG sources if this is an Xtream source
        if let Err(e) = self.database.auto_link_stream_source(&source).await {
            error!("Failed to auto-link stream source '{}': {}", source.name, e);
        }

        // Invalidate cache since we added a new source
        let _ = self.cache_invalidation_tx.send(());

        info!(
            "Successfully created stream source: {} ({})",
            source.name, source.id
        );

        Ok(source)
    }

    /// Update a stream source with validation
    pub async fn update_with_validation(
        &self,
        id: uuid::Uuid,
        request: StreamSourceUpdateRequest,
    ) -> Result<StreamSource> {
        info!("Updating stream source: {}", id);

        // Update linked sources first if requested
        if request.update_linked {
            match self.database.update_linked_sources(id, "stream", &request, request.update_linked).await {
                Ok(count) if count > 0 => {
                    info!("Updated {} linked sources for stream source {}", count, id);
                }
                Ok(_) => {
                    // No linked sources to update
                }
                Err(e) => {
                    error!("Failed to update linked sources for stream source '{}': {}", id, e);
                }
            }
        }

        // Update the stream source
        let source = self
            .database
            .update_stream_source(id, &request)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Stream source not found"))?;

        // Invalidate cache
        let _ = self.cache_invalidation_tx.send(());

        info!(
            "Successfully updated stream source: {} ({})",
            source.name, source.id
        );

        Ok(source)
    }

    /// Delete a stream source with proper cleanup
    pub async fn delete_with_cleanup(&self, id: uuid::Uuid) -> Result<()> {
        info!("Deleting stream source: {}", id);

        // Delete the stream source (this will cascade to linked sources)
        let deleted = self.database.delete_stream_source(id).await?;
        if !deleted {
            return Err(anyhow::anyhow!("Stream source not found"));
        }

        // Invalidate cache
        let _ = self.cache_invalidation_tx.send(());

        info!("Stream source {} deleted successfully", id);
        Ok(())
    }

    /// List stream sources with statistics
    pub async fn list_with_stats(&self) -> Result<Vec<StreamSourceWithStats>> {
        let sources_with_stats = self.database.list_stream_sources_with_stats().await?;

        let mut result = Vec::new();
        for source_with_stats in sources_with_stats {
            result.push(StreamSourceWithStats {
                source: source_with_stats.source.clone(),
                channel_count: source_with_stats.channel_count as u64,
                next_scheduled_update: source_with_stats.next_scheduled_update,
                last_ingested_at: source_with_stats.source.last_ingested_at,
                is_active: source_with_stats.source.is_active,
            });
        }

        Ok(result)
    }

    /// Get a stream source with detailed information
    pub async fn get_with_details(&self, id: uuid::Uuid) -> Result<StreamSourceWithDetails> {
        let source = self
            .database
            .get_stream_source(id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Stream source not found"))?;

        let channel_count = self.database.get_source_channel_count(id).await? as u64;
        let linked_epg = self.database.find_linked_epg_by_stream_id(id).await?;

        Ok(StreamSourceWithDetails {
            source: source.clone(),
            channel_count,
            next_scheduled_update: None, // TODO: Implement scheduling info
            last_ingested_at: source.last_ingested_at,
            is_active: source.is_active,
            linked_epg_source: linked_epg,
        })
    }

    /// Get stream source by ID
    pub async fn get(&self, id: uuid::Uuid) -> Result<StreamSource> {
        self.database
            .get_stream_source(id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Stream source not found"))
    }

    /// List all stream sources
    pub async fn list(&self) -> Result<Vec<StreamSource>> {
        self.database.list_stream_sources().await
    }

    /// Check if a stream source exists
    pub async fn exists(&self, id: uuid::Uuid) -> Result<bool> {
        Ok(self.database.get_stream_source(id).await?.is_some())
    }

    /// Test connection to a stream source
    pub async fn test_connection(
        &self,
        request: &StreamSourceCreateRequest,
    ) -> Result<TestConnectionResult> {
        // This would test the connection without creating the source
        // Implementation would depend on source type
        match request.source_type {
            crate::models::StreamSourceType::Xtream => {
                self.test_xtream_connection(&request.url, &request.username, &request.password)
                    .await
            }
            crate::models::StreamSourceType::M3u => self.test_m3u_connection(&request.url).await,
        }
    }

    /// Test Xtream connection
    async fn test_xtream_connection(
        &self,
        url: &str,
        username: &Option<String>,
        password: &Option<String>,
    ) -> Result<TestConnectionResult> {
        // Test connection to Xtream server
        // This would make actual API calls to validate credentials
        if let (Some(username), Some(password)) = (username, password) {
            // Make test API call
            let client = reqwest::Client::new();
            let test_url = format!(
                "{}player_api.php?username={}&password={}&action=get_live_categories",
                url, username, password
            );

            match client.get(&test_url).send().await {
                Ok(response) if response.status().is_success() => Ok(TestConnectionResult {
                    success: true,
                    message: "Connection successful".to_string(),
                    has_streams: true,
                    has_epg: self.check_epg_availability(url, username, password).await?,
                }),
                Ok(response) => Ok(TestConnectionResult {
                    success: false,
                    message: format!("Server returned status: {}", response.status()),
                    has_streams: false,
                    has_epg: false,
                }),
                Err(e) => Ok(TestConnectionResult {
                    success: false,
                    message: format!("Connection failed: {}", e),
                    has_streams: false,
                    has_epg: false,
                }),
            }
        } else {
            Ok(TestConnectionResult {
                success: false,
                message: "Username and password are required for Xtream sources".to_string(),
                has_streams: false,
                has_epg: false,
            })
        }
    }

    /// Test M3U connection
    async fn test_m3u_connection(&self, url: &str) -> Result<TestConnectionResult> {
        let client = reqwest::Client::new();

        match client.get(url).send().await {
            Ok(response) if response.status().is_success() => {
                let content = response.text().await?;
                let has_streams = content.contains("#EXTINF");

                Ok(TestConnectionResult {
                    success: true,
                    message: "Connection successful".to_string(),
                    has_streams,
                    has_epg: false, // M3U sources don't typically have EPG
                })
            }
            Ok(response) => Ok(TestConnectionResult {
                success: false,
                message: format!("Server returned status: {}", response.status()),
                has_streams: false,
                has_epg: false,
            }),
            Err(e) => Ok(TestConnectionResult {
                success: false,
                message: format!("Connection failed: {}", e),
                has_streams: false,
                has_epg: false,
            }),
        }
    }

    /// Check if Xtream server has EPG data
    async fn check_epg_availability(
        &self,
        url: &str,
        username: &str,
        password: &str,
    ) -> Result<bool> {
        let client = reqwest::Client::new();
        let epg_url = format!(
            "{}xmltv.php?username={}&password={}",
            url, username, password
        );

        match client.head(&epg_url).send().await {
            Ok(response) if response.status().is_success() => Ok(true),
            _ => Ok(false),
        }
    }

    /// Refresh a stream source using new source handlers with progress tracking
    pub async fn refresh_with_progress(
        &self, 
        source: &crate::models::StreamSource,
        progress_service: &crate::services::ProgressService,
    ) -> Result<usize> {
        use crate::sources::factory::SourceHandlerFactory;
        use tracing::info;
        
        let start_time = std::time::Instant::now();
        
        // Create stream source handler
        let handler = SourceHandlerFactory::create_handler(&source.source_type)
            .map_err(|e| anyhow::anyhow!("Failed to create stream handler: {}", e))?;
        
        // Start progress tracking
        let operation_id = source.id;
        let operation_name = format!("Stream Ingestion: {}", source.name);
        
        let _operation_callback = progress_service.start_operation(
            operation_id,
            crate::services::progress_service::OperationType::StreamIngestion,
            operation_name.clone(),
        ).await;
        
        info!(
            "Starting stream ingestion for source '{}' ({}) using new pipeline",
            source.name, source.id
        );
        
        // Create universal progress callback via ProgressService
        let progress_callback = progress_service
            .start_operation(
                source.id, 
                crate::services::progress_service::OperationType::StreamIngestion,
                format!("Stream Ingestion: {}", source.name)
            )
            .await;
        
        // Use new stream source handler with universal progress to ingest data
        let channels = handler
            .ingest_channels_with_universal_progress(
                source, 
                Some(&Box::new(progress_callback))
            )
            .await
            .map_err(|e| anyhow::anyhow!("Stream source handler failed: {}", e))?;
        
        info!("Saving {} channels to database for '{}'", channels.len(), source.name);
        
        // Save channels to database 
        let channels_saved = self.save_channels(source.id, channels).await
            .map_err(|e| {
                tracing::error!("Failed to save channels for '{}': {}", source.name, e);
                anyhow::anyhow!("Failed to save channels: {}", e)
            })?;
        
        info!("Successfully saved {} channels to database for Stream source '{}'", channels_saved, source.name);
        
        // Update last ingested timestamp
        if let Err(e) = self.database.update_source_last_ingested(source.id).await {
            tracing::warn!("Failed to update last ingested timestamp for stream source '{}': {}", source.name, e);
        }
        
        let duration = start_time.elapsed();
        info!(
            "Stream ingestion completed for '{}' in {:?}: {} channels",
            source.name, duration, channels_saved
        );
        
        progress_service.complete_operation(operation_id).await;
        
        // Invalidate cache since we updated channels
        let _ = self.cache_invalidation_tx.send(());
        
        Ok(channels_saved)
    }
    
    /// Save channels to database
    async fn save_channels(
        &self,
        source_id: uuid::Uuid,
        channels: Vec<crate::models::Channel>,
    ) -> Result<usize> {
        use tracing::debug;
        
        let mut tx = self.database.pool().begin().await?;
        
        // Delete existing channels for this source
        sqlx::query("DELETE FROM channels WHERE source_id = ?")
            .bind(source_id.to_string())
            .execute(&mut *tx)
            .await?;
        
        debug!("Deleted existing channels for source {}", source_id);
        
        let mut channels_saved = 0;
        for channel in channels {
            sqlx::query(
                "INSERT INTO channels (id, source_id, channel_name, stream_url, tvg_chno, group_title, tvg_id, tvg_name, tvg_logo, tvg_shift, created_at, updated_at) 
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
            )
            .bind(channel.id.to_string())
            .bind(source_id.to_string())
            .bind(&channel.channel_name)
            .bind(&channel.stream_url)
            .bind(channel.tvg_chno.as_deref())
            .bind(channel.group_title.as_deref())
            .bind(channel.tvg_id.as_deref())
            .bind(channel.tvg_name.as_deref())
            .bind(channel.tvg_logo.as_deref())
            .bind(channel.tvg_shift.as_deref())
            .bind(channel.created_at)
            .bind(channel.updated_at)
            .execute(&mut *tx)
            .await?;
            
            channels_saved += 1;
        }
        
        tx.commit().await?;
        debug!("Successfully saved {} channels", channels_saved);
        
        Ok(channels_saved)
    }
}

/// Stream source with statistics
#[derive(Debug, Clone)]
pub struct StreamSourceWithStats {
    pub source: StreamSource,
    pub channel_count: u64,
    pub next_scheduled_update: Option<chrono::DateTime<chrono::Utc>>,
    pub last_ingested_at: Option<chrono::DateTime<chrono::Utc>>,
    pub is_active: bool,
}

/// Stream source with detailed information
#[derive(Debug, Clone)]
pub struct StreamSourceWithDetails {
    pub source: StreamSource,
    pub channel_count: u64,
    pub next_scheduled_update: Option<chrono::DateTime<chrono::Utc>>,
    pub last_ingested_at: Option<chrono::DateTime<chrono::Utc>>,
    pub is_active: bool,
    pub linked_epg_source: Option<crate::models::EpgSource>,
}

/// Result of testing connection to a stream source
#[derive(Debug, Clone)]
pub struct TestConnectionResult {
    pub success: bool,
    pub message: String,
    pub has_streams: bool,
    pub has_epg: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_create_with_auto_epg() {
        // Test would create a service and test the create_with_auto_epg method
        // This would require setting up a test database
    }

    #[tokio::test]
    async fn test_connection_validation() {
        // Test the connection validation logic
    }
}
