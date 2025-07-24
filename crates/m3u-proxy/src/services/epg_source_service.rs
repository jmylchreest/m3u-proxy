//! EPG source service
//!
//! This service provides business logic for EPG source operations,
//! including auto-linking with stream sources for Xtream providers.

use anyhow::Result;
use tokio::sync::broadcast;
use tracing::{error, info, warn};

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
            match self.database.update_linked_sources(id, "epg", &request, request.update_linked).await {
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

impl EpgSourceService {
    /// Refresh an EPG source using new source handlers with progress tracking
    pub async fn refresh_with_progress(&self, 
        source: &crate::models::EpgSource,
        progress_service: &crate::services::ProgressService
    ) -> Result<(usize, usize)> {
        let start_time = std::time::Instant::now();
        
        // Execute the refresh and handle progress completion
        let result = self.execute_refresh_internal(source, progress_service).await;
        
        // Complete or fail the progress operation based on result
        match &result {
            Ok((channels_saved, programs_saved)) => {
                let elapsed = start_time.elapsed();
                info!(
                    "EPG source refresh completed for '{}' in {:.2}s: {} channels saved, {} programs saved",
                    source.name,
                    elapsed.as_secs_f64(),
                    channels_saved,
                    programs_saved
                );
                progress_service.complete_operation(source.id).await;
            }
            Err(e) => {
                let elapsed = start_time.elapsed();
                warn!(
                    "EPG source refresh failed for '{}' after {:.2}s: {}",
                    source.name,
                    elapsed.as_secs_f64(),
                    e
                );
                progress_service.fail_operation(source.id, e.to_string()).await;
            }
        }
        
        result
    }
    
    /// Internal refresh implementation without progress handling
    async fn execute_refresh_internal(&self, 
        source: &crate::models::EpgSource,
        progress_service: &crate::services::ProgressService
    ) -> Result<(usize, usize)> {
        use crate::sources::factory::SourceHandlerFactory;
        use tracing::info;
        
        // Create EPG source handler
        let handler = SourceHandlerFactory::create_epg_handler(&source.source_type)
            .map_err(|e| anyhow::anyhow!("Failed to create EPG source handler: {}", e))?;
        
        // Create universal progress callback using provided progress service
        let progress_callback = progress_service
            .start_operation(
                source.id, 
                crate::services::progress_service::OperationType::EpgIngestion,
                format!("EPG Ingestion: {}", source.name)
            )
            .await;
            
        // Use new EPG source handler with universal progress to ingest programs only
        let programs = handler
            .ingest_epg_programs_with_universal_progress(
                source, 
                Some(&Box::new(progress_callback))
            )
            .await
            .map_err(|e| anyhow::anyhow!("EPG source handler failed: {}", e))?;
        
        info!(
            "EPG handler ingested {} programs from source '{}'",
            programs.len(),
            source.name
        );
        
        // Save programs to database (programs-only mode)
        info!("Saving {} EPG programs to database for '{}'", programs.len(), source.name);
        let programs_saved = match self.save_epg_programs(source.id, programs).await {
            Ok(count) => count,
            Err(e) => {
                warn!("Failed to save EPG programs for '{}': {}", source.name, e);
                0
            }
        };
        
        info!("Completed database save for EPG source '{}': {} programs saved", 
              source.name, programs_saved);
        
        // Update last ingested timestamp
        info!("Updating last_ingested_at timestamp for EPG source '{}'", source.name);
        if let Err(e) = self.database.update_epg_source_last_ingested(source.id).await {
            tracing::error!("Failed to update last_ingested_at for EPG source '{}': {}", source.name, e);
        } else {
            info!("Updated timestamp for EPG source '{}'", source.name);
        }
        
        Ok((0, programs_saved)) // programs-only mode: no channels saved
    }


    /// Save EPG programs to database
    async fn save_epg_programs(
        &self,
        source_id: uuid::Uuid,
        programs: Vec<crate::models::EpgProgram>,
    ) -> Result<usize> {
        use tracing::debug;
        
        debug!("Saving {} EPG programs to database", programs.len());
        
        // Start a transaction for atomicity
        let mut tx = self.database.pool().begin().await?;
        
        // Delete existing programs for this source
        sqlx::query("DELETE FROM epg_programs WHERE source_id = ?")
            .bind(source_id.to_string())
            .execute(&mut *tx)
            .await?;
        
        let mut programs_saved = 0;
        
        for program in programs {
            sqlx::query(
                "INSERT INTO epg_programs (id, source_id, channel_id, channel_name, program_title, program_description, program_category, start_time, end_time, language, created_at, updated_at) 
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
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
            .bind(&program.language)
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
