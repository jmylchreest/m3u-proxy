//! Stream source service
//!
//! This service provides business logic for stream source operations,
//! including auto-linking with EPG sources for Xtream providers.

use anyhow::Result;
use tokio::sync::broadcast;
use tracing::{debug, error, info, warn};

use crate::database::Database;
use crate::models::{StreamSource, StreamSourceCreateRequest, StreamSourceUpdateRequest};
use crate::database::repositories::{
    channel::ChannelSeaOrmRepository,
    stream_source::StreamSourceSeaOrmRepository,
    epg_source::EpgSourceSeaOrmRepository,
};
use crate::services::UrlLinkingService;

/// Service for managing stream sources with business logic
pub struct StreamSourceService {
    stream_source_repo: StreamSourceSeaOrmRepository,
    channel_repo: ChannelSeaOrmRepository,
    epg_source_repo: EpgSourceSeaOrmRepository,
    url_linking_service: UrlLinkingService,
    cache_invalidation_tx: broadcast::Sender<()>,
}

impl StreamSourceService {
    /// Create a new stream source service with dependency injection
    pub fn new(
        stream_source_repo: StreamSourceSeaOrmRepository,
        channel_repo: ChannelSeaOrmRepository,
        epg_source_repo: EpgSourceSeaOrmRepository,
        url_linking_service: UrlLinkingService,
        cache_invalidation_tx: broadcast::Sender<()>,
    ) -> Self {
        Self {
            stream_source_repo,
            channel_repo,
            epg_source_repo,
            url_linking_service,
            cache_invalidation_tx,
        }
    }

    /// Legacy constructor for backward compatibility (deprecated)
    /// TODO: Remove once all callers are updated to use dependency injection
    #[deprecated(note = "Use dependency injection constructor instead")]
    pub fn new_legacy(
        database: Database,
        cache_invalidation_tx: broadcast::Sender<()>,
    ) -> Self {
        let stream_source_repo = StreamSourceSeaOrmRepository::new(database.connection().clone());
        let channel_repo = ChannelSeaOrmRepository::new(database.connection().clone());
        let epg_source_repo = EpgSourceSeaOrmRepository::new(database.connection().clone());
        let url_linking_service = UrlLinkingService::new(
            stream_source_repo.clone(),
            epg_source_repo.clone(),
        );
        Self::new(stream_source_repo, channel_repo, epg_source_repo, url_linking_service, cache_invalidation_tx)
    }

    /// Normalize URL to ensure it has a proper scheme (http:// or https://)
    /// Uses smart detection based on port numbers for common HTTPS ports
    fn smart_normalize_url(url: String) -> String {
        // If URL already has a scheme, return as-is
        if url.starts_with("http://") || url.starts_with("https://") {
            return url;
        }
        
        // Check for common HTTPS ports to determine scheme
        // Common HTTPS ports: 443, 8443, 9443, 2087, 2083, 8883, etc.
        if url.contains(":443") || 
           url.contains(":8443") || 
           url.contains(":9443") || 
           url.contains(":2087") || 
           url.contains(":2083") || 
           url.contains(":8883") {
            format!("https://{}", url)
        } else {
            // Default to http:// for all other cases
            format!("http://{}", url)
        }
    }

    /// Create a stream source with automatic EPG linking for Xtream sources
    pub async fn create_with_auto_epg(
        &self,
        mut request: StreamSourceCreateRequest,
    ) -> Result<StreamSource> {
        info!("Creating stream source: {}", request.name);
        
        // Normalize the URL to ensure it has a proper scheme
        request.url = Self::smart_normalize_url(request.url);
        debug!("Normalized URL: {}", request.url);

        // Create the stream source
        let source = self.stream_source_repo.create(request).await
            .map_err(|e| anyhow::anyhow!("Failed to create stream source: {}", e))?;

        // For Xtream sources, check if EPG is available and auto-create EPG source
        if source.source_type == crate::models::StreamSourceType::Xtream {
            if let (Some(username), Some(password)) = (&source.username, &source.password) {
                debug!("Checking EPG availability for Xtream source: {}", source.name);
                
                match self.check_epg_availability(&source.url, username, password).await {
                    Ok(true) => {
                        info!("EPG available for Xtream source '{}', creating linked EPG source", source.name);
                        
                        // Create EPG source with same credentials
                        let epg_create_request = crate::models::EpgSourceCreateRequest {
                            name: source.name.clone(),
                            source_type: crate::models::EpgSourceType::Xtream,
                            url: source.url.clone(),
                            update_cron: source.update_cron.clone(),
                            username: Some(username.clone()),
                            password: Some(password.clone()),
                            timezone: None,
                            time_offset: Some("+00:00".to_string()),
                        };

                        match self.epg_source_repo.create(epg_create_request).await {
                            Ok(epg_source) => {
                                info!(
                                    "Successfully created linked EPG source: {} ({}) for stream source: {}",
                                    epg_source.name, epg_source.id, source.name
                                );
                            }
                            Err(e) => {
                                warn!(
                                    "Failed to create linked EPG source for '{}': {} - stream source created without EPG",
                                    source.name, e
                                );
                            }
                        }
                    }
                    Ok(false) => {
                        info!("No EPG available for Xtream source '{}'", source.name);
                    }
                    Err(e) => {
                        warn!("Failed to check EPG availability for '{}': {}", source.name, e);
                    }
                }
            } else {
                debug!("Xtream source '{}' missing credentials, skipping EPG auto-creation", source.name);
            }
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
        mut request: StreamSourceUpdateRequest,
    ) -> Result<StreamSource> {
        info!("Updating stream source: {}", id);
        
        // Normalize the URL to ensure it has a proper scheme
        request.url = Self::smart_normalize_url(request.url);
        debug!("Normalized URL for update: {}", request.url);

        // Update linked sources first if requested
        if request.update_linked {
            match self.url_linking_service.update_linked_sources(
                id,
                "stream",
                Some(&request.url),
                request.username.as_ref(),
                request.password.as_ref(),
                request.update_linked,
            ).await {
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
        let source = self.stream_source_repo.update(&id, request).await
            .map_err(|e| anyhow::anyhow!("Failed to update stream source: {}", e))?;

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
        self.stream_source_repo.delete(&id).await
            .map_err(|e| anyhow::anyhow!("Failed to delete stream source: {}", e))?;

        // Invalidate cache
        let _ = self.cache_invalidation_tx.send(());

        info!("Stream source {} deleted successfully", id);
        Ok(())
    }

    /// List stream sources with statistics
    pub async fn list_with_stats(&self) -> Result<Vec<StreamSourceWithStats>> {
        let sources_with_stats = self.stream_source_repo.list_with_stats().await
            .map_err(|e| anyhow::anyhow!("Failed to list stream sources with stats: {}", e))?;

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
        let source = self.stream_source_repo.find_by_id(&id).await
            .map_err(|e| anyhow::anyhow!("Failed to get stream source: {}", e))?
            .ok_or_else(|| anyhow::anyhow!("Stream source not found"))?;

        let channel_count = self.stream_source_repo.get_channel_count_for_source(&id).await
            .map_err(|e| anyhow::anyhow!("Failed to get channel count: {}", e))? as u64;
        
        // Use service pattern for URL linking
        let linked_epgs = self.url_linking_service.find_linked_epg_sources(&source).await.unwrap_or_default();
        let linked_epg = linked_epgs.into_iter().next();

        // Calculate next scheduled update from cron expression
        let next_scheduled_update = if !source.update_cron.is_empty() {
            crate::utils::calculate_next_scheduled_time(&source.update_cron)
        } else {
            None
        };

        Ok(StreamSourceWithDetails {
            source: source.clone(),
            channel_count,
            next_scheduled_update,
            last_ingested_at: source.last_ingested_at,
            is_active: source.is_active,
            linked_epg_source: linked_epg,
        })
    }

    /// Get stream source by ID
    pub async fn get(&self, id: uuid::Uuid) -> Result<StreamSource> {
        self.stream_source_repo.find_by_id(&id).await
            .map_err(|e| anyhow::anyhow!("Failed to get stream source: {}", e))?
            .ok_or_else(|| anyhow::anyhow!("Stream source not found"))
    }

    /// List all stream sources
    pub async fn list(&self) -> Result<Vec<StreamSource>> {
        self.stream_source_repo.find_all().await
            .map_err(|e| anyhow::anyhow!("Failed to list stream sources: {}", e))
    }

    /// Check if a stream source exists
    pub async fn exists(&self, id: uuid::Uuid) -> Result<bool> {
        Ok(self.stream_source_repo.find_by_id(&id).await
            .map_err(|e| anyhow::anyhow!("Failed to check stream source existence: {}", e))?
            .is_some())
    }

    /// Test connection to a stream source
    pub async fn test_connection(
        &self,
        request: &StreamSourceCreateRequest,
    ) -> Result<TestConnectionResult> {
        // Normalize the URL before testing
        let normalized_url = Self::smart_normalize_url(request.url.clone());
        
        // This would test the connection without creating the source
        // Implementation would depend on source type
        match request.source_type {
            crate::models::StreamSourceType::Xtream => {
                self.test_xtream_connection(&normalized_url, &request.username, &request.password)
                    .await
            }
            crate::models::StreamSourceType::M3u => self.test_m3u_connection(&normalized_url).await,
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
                "{url}player_api.php?username={username}&password={password}&action=get_live_categories"
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
                    message: format!("Connection failed: {e}"),
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
                message: format!("Connection failed: {e}"),
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
        let epg_url = crate::utils::UrlUtils::build_xtream_xmltv_url(url, username, password)
            .map_err(|e| anyhow::anyhow!("Invalid URL for Xtream EPG check: {}", e))?;

        match client.head(&epg_url).send().await {
            Ok(response) if response.status().is_success() => Ok(true),
            _ => Ok(false),
        }
    }


    
    /// Save channels to database using ChannelRepository
    async fn save_channels(
        &self,
        source_id: uuid::Uuid,
        channels: Vec<crate::models::Channel>,
    ) -> Result<usize> {
        use tracing::debug;
        
        debug!("Saving {} channels to database using ChannelRepository", channels.len());
        
        // Use ChannelRepository to replace channels for this source
        let channels_count = channels.len();
        self.channel_repo
            .update_source_channels(source_id, &channels)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to update source channels: {}", e))?;
        
        debug!("Successfully saved {} channels using ChannelRepository", channels_count);
        
        Ok(channels_count)
    }
    
    /// Refresh stream source using ProgressStageUpdater (new API)
    pub async fn refresh_with_progress_updater(
        &self,
        source: &crate::models::StreamSource,
        progress_updater: Option<&crate::services::progress_service::ProgressStageUpdater>,
    ) -> Result<usize> {
        use crate::sources::factory::SourceHandlerFactory;
        
        info!("Starting stream source refresh with ProgressStageUpdater for source: {}", source.name);
        
        // Create stream source handler using the factory
        let handler = SourceHandlerFactory::create_handler(&source.source_type)
            .map_err(|e| anyhow::anyhow!("Failed to create stream source handler: {}", e))?;
        
        // Update progress: starting ingestion
        if let Some(updater) = progress_updater {
            updater.update_progress(0.0, &format!("Starting stream ingestion for {}", source.name)).await;
            
            // Check for cancellation before starting the operation
            if updater.is_cancellation_requested().await {
                return Err(anyhow::anyhow!("Stream ingestion cancelled for source: {}", source.name));
            }
        }
        
        // Ingest channels using the handler
        let channels = handler
            .ingest_channels(source)
            .await
            .map_err(|e| anyhow::anyhow!("Stream source handler failed: {}", e))?;
        
        info!(
            "Stream handler ingested {} channels from source '{}'",
            channels.len(), source.name
        );
        
        // Update progress: saving to database
        if let Some(updater) = progress_updater {
            updater.update_progress(80.0, &format!("Saving {} channels to database", channels.len())).await;
            
            // DO NOT check for cancellation here - we must complete the database transaction
            // to avoid partial state corruption (deleting old data without inserting new data)
        }
        
        // Save channels to database
        info!("Saving {} channels to database for '{}'", channels.len(), source.name);
        let channels_saved = match self.save_channels(source.id, channels).await {
            Ok(count) => count,
            Err(e) => {
                warn!("Failed to save channels for '{}': {}", source.name, e);
                return Err(e);
            }
        };
        
        // Final progress update
        if let Some(updater) = progress_updater {
            updater.update_progress(100.0, &format!("Completed: {channels_saved} channels saved")).await;
            updater.complete_stage().await;
        }
        
        // Update the source's last_ingested_at timestamp
        if let Err(e) = self.stream_source_repo.update_last_ingested_at(&source.id).await {
            warn!("Failed to update last_ingested_at for stream source '{}': {}", source.name, e);
        } else {
            info!("Updated last_ingested_at for stream source '{}'", source.name);
        }
        
        // Invalidate cache since we updated channels
        let _ = self.cache_invalidation_tx.send(());
        
        info!(
            "Stream source refresh completed for '{}': {} channels saved",
            source.name, channels_saved
        );
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
