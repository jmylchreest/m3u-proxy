//! EPG source service
//!
//! This service provides business logic for EPG source operations,
//! including auto-linking with stream sources for Xtream providers.

use anyhow::Result;
use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::{error, info};

use crate::database::Database;
use crate::models::{EpgSource, EpgSourceCreateRequest, EpgSourceUpdateRequest};

/// Service for managing EPG sources with business logic
pub struct EpgSourceService {
    database: Database,
    cache_invalidation_tx: broadcast::Sender<()>,
}

impl EpgSourceService {
    /// Create a new EPG source service
    pub fn new(database: Database, cache_invalidation_tx: broadcast::Sender<()>) -> Self {
        Self {
            database,
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
        let source = self.database.create_epg_source(&request).await?;

        // Auto-link with existing stream sources if this is an Xtream source
        if let Err(e) = self.database.auto_link_epg_source(&source).await {
            error!("Failed to auto-link EPG source '{}': {}", source.name, e);
        }

        // Invalidate cache since we added a new source
        let _ = self.cache_invalidation_tx.send(());

        info!(
            "Successfully created EPG source: {} ({})",
            source.name, source.id
        );

        Ok(source)
    }

    /// Update an EPG source with validation
    pub async fn update_with_validation(
        &self,
        id: uuid::Uuid,
        request: EpgSourceUpdateRequest,
    ) -> Result<EpgSource> {
        info!("Updating EPG source: {}", id);

        // Update the EPG source
        let updated = self.database.update_epg_source(id, &request).await?;
        if !updated {
            return Err(anyhow::anyhow!("EPG source not found"));
        }

        // Get the updated source
        let source = self
            .database
            .get_epg_source(id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("EPG source not found after update"))?;

        // If credentials or URL changed and it's an Xtream source, re-check auto-linking
        if let Err(e) = self.database.auto_link_epg_source(&source).await {
            error!(
                "Failed to auto-link updated EPG source '{}': {}",
                source.name, e
            );
        }

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
        let deleted = self.database.delete_epg_source(id).await?;
        if !deleted {
            return Err(anyhow::anyhow!("EPG source not found"));
        }

        // Invalidate cache
        let _ = self.cache_invalidation_tx.send(());

        info!("EPG source {} deleted successfully", id);
        Ok(())
    }

    /// List EPG sources with statistics
    pub async fn list_with_stats(&self) -> Result<Vec<EpgSourceWithStats>> {
        let sources_with_stats = self.database.list_epg_sources_with_stats().await?;

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
        let source = self
            .database
            .get_epg_source(id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("EPG source not found"))?;

        let channel_count = self.database.get_epg_source_channel_count(id).await? as u64;
        let linked_stream = self.database.find_linked_stream_by_epg_id(id).await?;

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
        self.database
            .get_epg_source(id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("EPG source not found"))
    }

    /// List all EPG sources
    pub async fn list(&self) -> Result<Vec<EpgSource>> {
        self.database.list_epg_sources().await
    }

    /// Check if an EPG source exists
    pub async fn exists(&self, id: uuid::Uuid) -> Result<bool> {
        Ok(self.database.get_epg_source(id).await?.is_some())
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::EpgSourceType;

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
