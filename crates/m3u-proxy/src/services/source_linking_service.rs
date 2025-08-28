//! Source linking service
//!
//! This service handles the auto-linking logic between stream sources and EPG sources,
//! particularly for Xtream Codes providers that offer both services.

use anyhow::Result;
use std::time::Duration;
use tracing::{debug, error, info};

use crate::database::Database;
use crate::models::{EpgSource, EpgSourceType, StreamSource, StreamSourceType};
use crate::database::repositories::{
    stream_source::StreamSourceSeaOrmRepository,
    epg_source::EpgSourceSeaOrmRepository,
};
use crate::services::UrlLinkingService;

/// Service for managing links between stream and EPG sources
pub struct SourceLinkingService {
    client: reqwest::Client,
    stream_source_repo: StreamSourceSeaOrmRepository,
    epg_source_repo: EpgSourceSeaOrmRepository,
    url_linking_service: UrlLinkingService,
}

impl SourceLinkingService {
    /// Create a new source linking service with dependency injection
    pub fn new(
        stream_source_repo: StreamSourceSeaOrmRepository,
        epg_source_repo: EpgSourceSeaOrmRepository,
        url_linking_service: UrlLinkingService,
    ) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .expect("Failed to create HTTP client");

        Self {
            client,
            stream_source_repo,
            epg_source_repo,
            url_linking_service,
        }
    }

    /// Legacy constructor for backward compatibility (deprecated)
    /// TODO: Remove once all callers are updated to use dependency injection
    #[deprecated(note = "Use dependency injection constructor instead")]
    pub fn new_legacy(database: Database) -> Self {
        let stream_source_repo = StreamSourceSeaOrmRepository::new(database.connection().clone());
        let epg_source_repo = EpgSourceSeaOrmRepository::new(database.connection().clone());
        let url_linking_service = UrlLinkingService::new(
            stream_source_repo.clone(),
            epg_source_repo.clone(),
        );
        Self::new(stream_source_repo, epg_source_repo, url_linking_service)
    }

    /// Auto-link Xtream sources (both directions)
    pub async fn auto_link_xtream_sources(&self) -> Result<LinkingStats> {
        info!("Starting auto-linking process for Xtream sources");

        let mut stats = LinkingStats::default();

        // Get all Xtream sources using repositories
        let stream_sources = self.stream_source_repo.find_all().await
            .map_err(|e| anyhow::anyhow!("Failed to get stream sources: {}", e))?;
        let epg_sources = self.epg_source_repo.find_all().await
            .map_err(|e| anyhow::anyhow!("Failed to get EPG sources: {}", e))?;

        let xtream_streams: Vec<_> = stream_sources
            .into_iter()
            .filter(|s| s.source_type == StreamSourceType::Xtream)
            .collect();

        let xtream_epgs: Vec<_> = epg_sources
            .into_iter()
            .filter(|s| s.source_type == EpgSourceType::Xtream)
            .collect();

        // Try to link stream sources with EPG sources
        for stream_source in &xtream_streams {
            match self.link_stream_to_epg(stream_source, &xtream_epgs).await {
                Ok(true) => {
                    stats.streams_linked += 1;
                    info!("Successfully linked stream source: {}", stream_source.name);
                }
                Ok(false) => {
                    stats.streams_skipped += 1;
                    debug!(
                        "No matching EPG source found for stream: {}",
                        stream_source.name
                    );
                }
                Err(e) => {
                    stats.streams_failed += 1;
                    error!(
                        "Failed to link stream source '{}': {}",
                        stream_source.name, e
                    );
                }
            }
        }

        // Try to link EPG sources with stream sources
        for epg_source in &xtream_epgs {
            match self.link_epg_to_stream(epg_source, &xtream_streams).await {
                Ok(true) => {
                    stats.epgs_linked += 1;
                    info!("Successfully linked EPG source: {}", epg_source.name);
                }
                Ok(false) => {
                    stats.epgs_skipped += 1;
                    debug!(
                        "No matching stream source found for EPG: {}",
                        epg_source.name
                    );
                }
                Err(e) => {
                    stats.epgs_failed += 1;
                    error!("Failed to link EPG source '{}': {}", epg_source.name, e);
                }
            }
        }

        info!("Auto-linking completed: {:?}", stats);
        Ok(stats)
    }

    /// Link a stream source to an EPG source
    async fn link_stream_to_epg(
        &self,
        stream_source: &StreamSource,
        epg_sources: &[EpgSource],
    ) -> Result<bool> {
        // Check if already linked using service pattern
        let linked_epgs = self.url_linking_service.find_linked_epg_sources(stream_source).await.unwrap_or_default();
        if !linked_epgs.is_empty() {
            return Ok(false);
        }

        // Look for matching EPG source
        if let Some(epg_source) = self
            .find_matching_epg_source(stream_source, epg_sources)
            .await?
        {
            self.create_bidirectional_link(Some(stream_source.id), Some(epg_source.id))
                .await?;
            return Ok(true);
        }

        // No existing EPG source found, check if we can create one
        if let (Some(username), Some(password)) = (&stream_source.username, &stream_source.password)
        {
            if self
                .check_epg_availability(&stream_source.url, username, password)
                .await?
            {
                info!(
                    "Stream source '{}' provides EPG data - creating EPG source",
                    stream_source.name
                );
                let epg_source = self.create_epg_from_stream(stream_source).await?;
                self.create_bidirectional_link(Some(stream_source.id), Some(epg_source.id))
                    .await?;
                return Ok(true);
            }
        }

        Ok(false)
    }

    /// Link an EPG source to a stream source
    async fn link_epg_to_stream(
        &self,
        epg_source: &EpgSource,
        stream_sources: &[StreamSource],
    ) -> Result<bool> {
        // Check if already linked using service pattern
        let linked_streams = self.url_linking_service.find_linked_stream_sources(epg_source).await.unwrap_or_default();
        if !linked_streams.is_empty() {
            return Ok(false);
        }

        // Look for matching stream source
        if let Some(stream_source) = self
            .find_matching_stream_source(epg_source, stream_sources)
            .await?
        {
            self.create_bidirectional_link(Some(stream_source.id), Some(epg_source.id))
                .await?;
            return Ok(true);
        }

        // No existing stream source found, check if we can create one
        if let (Some(username), Some(password)) = (&epg_source.username, &epg_source.password) {
            if self
                .check_stream_availability(&epg_source.url, username, password)
                .await?
            {
                info!(
                    "EPG source '{}' provides stream data - creating stream source",
                    epg_source.name
                );
                let stream_source = self.create_stream_from_epg(epg_source).await?;
                self.create_bidirectional_link(Some(stream_source.id), Some(epg_source.id))
                    .await?;
                return Ok(true);
            }
        }

        Ok(false)
    }

    /// Find a matching EPG source for a stream source
    async fn find_matching_epg_source(
        &self,
        stream_source: &StreamSource,
        epg_sources: &[EpgSource],
    ) -> Result<Option<EpgSource>> {
        for epg_source in epg_sources {
            if self.sources_match(
                &stream_source.url,
                &stream_source.username,
                &stream_source.password,
                &epg_source.url,
                &epg_source.username,
                &epg_source.password,
            ) {
                return Ok(Some(epg_source.clone()));
            }
        }
        Ok(None)
    }

    /// Find a matching stream source for an EPG source
    async fn find_matching_stream_source(
        &self,
        epg_source: &EpgSource,
        stream_sources: &[StreamSource],
    ) -> Result<Option<StreamSource>> {
        for stream_source in stream_sources {
            if self.sources_match(
                &stream_source.url,
                &stream_source.username,
                &stream_source.password,
                &epg_source.url,
                &epg_source.username,
                &epg_source.password,
            ) {
                return Ok(Some(stream_source.clone()));
            }
        }
        Ok(None)
    }

    /// Check if two sources match (same credentials and base URL)
    fn sources_match(
        &self,
        stream_url: &str,
        stream_username: &Option<String>,
        stream_password: &Option<String>,
        epg_url: &str,
        epg_username: &Option<String>,
        epg_password: &Option<String>,
    ) -> bool {
        // Extract base URLs (remove specific endpoints)
        let stream_base = self.extract_base_url(stream_url);
        let epg_base = self.extract_base_url(epg_url);

        // Check if base URLs match and credentials match
        stream_base == epg_base
            && stream_username == epg_username
            && stream_password == epg_password
    }

    /// Extract base URL from a full URL
    fn extract_base_url(&self, url: &str) -> String {
        if let Ok(parsed) = url::Url::parse(url) {
            format!("{}://{}", parsed.scheme(), parsed.host_str().unwrap_or(""))
        } else {
            url.to_string()
        }
    }

    /// Check if Xtream server provides EPG data
    pub async fn check_epg_availability(
        &self,
        url: &str,
        username: &str,
        password: &str,
    ) -> Result<bool> {
        let epg_url = crate::utils::UrlUtils::build_xtream_xmltv_url(url, username, password)
            .map_err(|e| anyhow::anyhow!("Invalid URL for Xtream EPG validation: {}", e))?;

        match self.client.head(&epg_url).send().await {
            Ok(response) if response.status().is_success() => {
                // Additional validation - check if it actually returns XMLTV data
                match self.client.get(&epg_url).send().await {
                    Ok(response) if response.status().is_success() => {
                        let content = response.text().await?;
                        Ok(content.contains("<?xml") && content.contains("<tv"))
                    }
                    _ => Ok(false),
                }
            }
            _ => Ok(false),
        }
    }

    /// Check if Xtream server provides stream data
    pub async fn check_stream_availability(
        &self,
        url: &str,
        username: &str,
        password: &str,
    ) -> Result<bool> {
        let stream_url = format!("{url}player_api.php?username={username}&password={password}&action=get_live_categories");

        match self.client.head(&stream_url).send().await {
            Ok(response) if response.status().is_success() => Ok(true),
            _ => Ok(false),
        }
    }

    /// Create a bidirectional link between sources
    pub async fn create_bidirectional_link(
        &self,
        stream_source_id: Option<uuid::Uuid>,
        epg_source_id: Option<uuid::Uuid>,
    ) -> Result<()> {
        if stream_source_id.is_none() && epg_source_id.is_none() {
            return Err(anyhow::anyhow!("At least one source ID must be provided"));
        }

        if let (Some(stream_id), Some(epg_id)) = (stream_source_id, epg_source_id) {
            // Get the stream and EPG sources to extract URL and credentials
            let stream_source = self.stream_source_repo.find_by_id(&stream_id).await
                .map_err(|e| anyhow::anyhow!("Failed to find stream source: {}", e))?
                .ok_or_else(|| anyhow::anyhow!("Stream source not found: {}", stream_id))?;
                
            let epg_source = self.epg_source_repo.find_by_id(&epg_id).await
                .map_err(|e| anyhow::anyhow!("Failed to find EPG source: {}", e))?
                .ok_or_else(|| anyhow::anyhow!("EPG source not found: {}", epg_id))?;

            // Create a linked entry using the stream source's credentials and URL as the primary
            use crate::entities::linked_xtream_sources;
            use sea_orm::{ActiveModelTrait, Set};
            
            let link_id = format!("link_{}", uuid::Uuid::new_v4());
            let now = chrono::Utc::now().to_rfc3339();
            
            let active_model = linked_xtream_sources::ActiveModel {
                id: Set(uuid::Uuid::new_v4().to_string()),
                link_id: Set(link_id),
                name: Set(stream_source.name.clone()),
                url: Set(stream_source.url.clone()),
                username: Set(stream_source.username.unwrap_or_default()),
                password: Set(stream_source.password.unwrap_or_default()),
                stream_source_id: Set(Some(stream_id.to_string())),
                epg_source_id: Set(Some(epg_id.to_string())),
                created_at: Set(now.clone()),
                updated_at: Set(now),
            };
            
            // Insert using the database connection (access through new method)
            active_model.insert(self.stream_source_repo.get_connection().as_ref()).await
                .map_err(|e| anyhow::anyhow!("Failed to create link: {}", e))?;

            info!(
                "Created bidirectional link between stream '{}' ({}) and epg '{}' ({})",
                stream_source.name, stream_id, epg_source.name, epg_id
            );
        }
        
        Ok(())
    }

    /// Create an EPG source from a stream source
    async fn create_epg_from_stream(&self, stream_source: &StreamSource) -> Result<EpgSource> {
        let epg_request = crate::models::EpgSourceCreateRequest {
            name: stream_source.name.clone(),
            source_type: EpgSourceType::Xtream,
            url: stream_source.url.clone(),
            update_cron: stream_source.update_cron.clone(),
            username: stream_source.username.clone(),
            password: stream_source.password.clone(),
            timezone: None,
            time_offset: None,
        };

        self.epg_source_repo.create(epg_request).await
            .map_err(|e| anyhow::anyhow!("Failed to create EPG source: {}", e))
    }

    /// Create a stream source from an EPG source
    async fn create_stream_from_epg(&self, epg_source: &EpgSource) -> Result<StreamSource> {
        let stream_request = crate::models::StreamSourceCreateRequest {
            name: epg_source.name.clone(),
            source_type: StreamSourceType::Xtream,
            url: epg_source.url.clone(),
            max_concurrent_streams: 10, // Default value
            update_cron: epg_source.update_cron.clone(),
            username: epg_source.username.clone(),
            password: epg_source.password.clone(),
            field_map: None,
            ignore_channel_numbers: true, // Default to true for Xtream sources
        };

        self.stream_source_repo.create(stream_request).await
            .map_err(|e| anyhow::anyhow!("Failed to create stream source: {}", e))
    }

    /// Get linking statistics
    pub async fn get_linking_stats(&self) -> Result<LinkingStats> {
        // Get counts using SeaORM repositories directly
        let stream_sources_list = self.stream_source_repo.find_all().await
            .map_err(|e| anyhow::anyhow!("Failed to get stream sources for stats: {}", e))?;
        let epg_sources_list = self.epg_source_repo.find_all().await
            .map_err(|e| anyhow::anyhow!("Failed to get EPG sources for stats: {}", e))?;

        // Calculate total links by counting relationships
        let mut total_links = 0u64;
        for stream_source in &stream_sources_list {
            if let Ok(linked_epgs) = self.url_linking_service.find_linked_epg_sources(stream_source).await {
                total_links += linked_epgs.len() as u64;
            }
        }
        let stream_sources = stream_sources_list.len() as u64;
        let epg_sources = epg_sources_list.len() as u64;
        let xtream_streams = stream_sources_list
            .iter()
            .filter(|s| s.source_type == crate::models::StreamSourceType::Xtream)
            .count() as u64;
        let xtream_epgs = epg_sources_list
            .iter()
            .filter(|s| s.source_type == crate::models::EpgSourceType::Xtream)
            .count() as u64;

        Ok(LinkingStats {
            total_links,
            stream_sources,
            epg_sources,
            xtream_streams,
            xtream_epgs,
            ..Default::default()
        })
    }
}

/// Statistics for linking operations
#[derive(Debug, Clone, Default)]
pub struct LinkingStats {
    pub total_links: u64,
    pub stream_sources: u64,
    pub epg_sources: u64,
    pub xtream_streams: u64,
    pub xtream_epgs: u64,
    pub streams_linked: u64,
    pub streams_skipped: u64,
    pub streams_failed: u64,
    pub epgs_linked: u64,
    pub epgs_skipped: u64,
    pub epgs_failed: u64,
}

////#[cfg(test)]
//mod tests {
//    // use super::*; // Currently unused
//
//    #[tokio::test]
//    async fn test_sources_match() {
//        // let service = SourceLinkingService::new(Database::new_test().await);
//
//        // Test matching URLs
//        assert!(service.sources_match(
//            "http://example.com/player_api.php",
//            &Some("user".to_string()),
//            &Some("pass".to_string()),
//            "http://example.com/xmltv.php",
//            &Some("user".to_string()),
//            &Some("pass".to_string()),
//        ));
//
//        // Test non-matching URLs
//        assert!(!service.sources_match(
//            "http://example.com/player_api.php",
//            &Some("user".to_string()),
//            &Some("pass".to_string()),
//            "http://different.com/xmltv.php",
//            &Some("user".to_string()),
//            &Some("pass".to_string()),
//        ));
//    }
//
//    #[tokio::test]
//    async fn test_extract_base_url() {
//        // let service = SourceLinkingService::new(Database::new_test().await);
//
//        assert_eq!(
//            service.extract_base_url("http://example.com/player_api.php"),
//            "http://example.com"
//        );
//        assert_eq!(
//            service.extract_base_url("https://example.com:8080/xmltv.php"),
//            "https://example.com:8080"
//        );
//    }
//}
