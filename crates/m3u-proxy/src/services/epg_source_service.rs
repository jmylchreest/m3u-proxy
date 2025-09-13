//! EPG source service
//!
//! This service provides business logic for EPG source operations,
//! including auto-linking with stream sources for Xtream providers.

use anyhow::Result;
use tokio::sync::broadcast;
use tracing::{debug, error, info};

use crate::database::Database;
use crate::database::repositories::{
    epg_source::EpgSourceSeaOrmRepository, stream_source::StreamSourceSeaOrmRepository,
};
use crate::models::{EpgSource, EpgSourceCreateRequest, EpgSourceUpdateRequest};
use crate::services::UrlLinkingService;

/// Service for managing EPG sources with business logic
pub struct EpgSourceService {
    database: Database,
    epg_source_repo: EpgSourceSeaOrmRepository,
    url_linking_service: UrlLinkingService,
    cache_invalidation_tx: broadcast::Sender<()>,
    http_client_factory: crate::utils::HttpClientFactory,
}

impl EpgSourceService {
    /// Create a new EPG source service with dependency injection
    pub fn new(
        database: Database,
        epg_source_repo: EpgSourceSeaOrmRepository,
        url_linking_service: UrlLinkingService,
        cache_invalidation_tx: broadcast::Sender<()>,
        http_client_factory: crate::utils::HttpClientFactory,
    ) -> Self {
        Self {
            database,
            epg_source_repo,
            url_linking_service,
            cache_invalidation_tx,
            http_client_factory,
        }
    }

    /// Legacy constructor for backward compatibility (deprecated)
    /// TODO: Remove once all callers are updated to use dependency injection
    #[deprecated(note = "Use dependency injection constructor instead")]
    pub fn new_legacy(database: Database, cache_invalidation_tx: broadcast::Sender<()>) -> Self {
        let epg_source_repo = EpgSourceSeaOrmRepository::new(database.connection().clone());
        let stream_source_repo = StreamSourceSeaOrmRepository::new(database.connection().clone());
        let url_linking_service =
            UrlLinkingService::new(stream_source_repo, epg_source_repo.clone());
        let http_client_factory =
            crate::utils::HttpClientFactory::new(None, std::time::Duration::from_secs(10));
        Self::new(
            database,
            epg_source_repo,
            url_linking_service,
            cache_invalidation_tx,
            http_client_factory,
        )
    }

    /// Create an EPG source with automatic stream linking for Xtream sources
    pub async fn create_with_auto_stream(
        &self,
        request: EpgSourceCreateRequest,
    ) -> Result<EpgSource> {
        debug!("Creating EPG source: {}", request.name);

        // Create the EPG source (this includes auto-stream creation logic)
        let source = self
            .epg_source_repo
            .create(request)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to create EPG source: {}", e))?;

        // Auto-populate credentials from linked stream sources if this is an Xtream source
        let final_source = if source.source_type == crate::models::EpgSourceType::Xtream {
            match self
                .url_linking_service
                .auto_populate_epg_credentials(source.id)
                .await
            {
                Ok(Some(updated_source)) => {
                    if updated_source.username.is_some() && source.username.is_none() {
                        debug!(
                            "Auto-populated credentials for EPG source '{}'",
                            source.name
                        );
                    }
                    updated_source
                }
                Ok(None) => source,
                Err(e) => {
                    error!(
                        "Failed to auto-populate EPG source '{}': {}",
                        source.name, e
                    );
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
        debug!("Updating EPG source: {}", id);

        // Update linked sources first if requested
        if request.update_linked {
            match self
                .url_linking_service
                .update_linked_sources(
                    id,
                    "epg",
                    Some(&request.url),
                    request.username.as_ref(),
                    request.password.as_ref(),
                    request.update_linked,
                )
                .await
            {
                Ok(count) if count > 0 => {
                    debug!("Updated {} linked sources for EPG source {}", count, id);
                }
                Ok(_) => {
                    // No linked sources to update
                }
                Err(e) => {
                    error!(
                        "Failed to update linked sources for EPG source '{}': {}",
                        id, e
                    );
                }
            }
        }

        // Update the EPG source
        let source = self
            .epg_source_repo
            .update(&id, request)
            .await
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
        debug!("Deleting EPG source: {}", id);

        // Delete the EPG source (this will cascade to linked sources)
        self.epg_source_repo
            .delete(&id)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to delete EPG source: {}", e))?;

        // Invalidate cache
        let _ = self.cache_invalidation_tx.send(());

        info!("EPG source {} deleted successfully", id);
        Ok(())
    }

    /// List EPG sources with statistics
    pub async fn list_with_stats(&self) -> Result<Vec<crate::models::EpgSourceWithStats>> {
        let sources_with_stats = self
            .epg_source_repo
            .list_with_stats()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to list EPG sources with stats: {}", e))?;

        let mut result = Vec::new();
        for source_with_stats in sources_with_stats {
            result.push(crate::models::EpgSourceWithStats {
                source: source_with_stats.source.clone(),
                program_count: source_with_stats.program_count,
                next_scheduled_update: source_with_stats.next_scheduled_update,
            });
        }

        Ok(result)
    }

    /// Get an EPG source with detailed information
    pub async fn get_with_details(&self, id: uuid::Uuid) -> Result<EpgSourceWithDetails> {
        let source = self
            .epg_source_repo
            .find_by_id(&id)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to get EPG source: {}", e))?
            .ok_or_else(|| anyhow::anyhow!("EPG source not found"))?;

        // Find linked stream sources using URL-based matching
        let linked_stream = if source.source_type == crate::models::EpgSourceType::Xtream {
            let linked_sources = self
                .url_linking_service
                .find_linked_stream_sources(&source)
                .await
                .unwrap_or_default();
            linked_sources.into_iter().next() // Return first linked stream source if any
        } else {
            None
        };

        Ok(EpgSourceWithDetails {
            source: source.clone(),
            next_scheduled_update: None, // TODO: Implement scheduling info
            last_ingested_at: source.last_ingested_at,
            is_active: source.is_active,
            linked_stream_source: linked_stream,
        })
    }

    /// Get EPG source by ID
    pub async fn get(&self, id: uuid::Uuid) -> Result<EpgSource> {
        self.epg_source_repo
            .find_by_id(&id)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to get EPG source: {}", e))?
            .ok_or_else(|| anyhow::anyhow!("EPG source not found"))
    }

    /// List all EPG sources
    pub async fn list(&self) -> Result<Vec<EpgSource>> {
        self.epg_source_repo
            .find_all()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to list EPG sources: {}", e))
    }

    /// Check if an EPG source exists
    pub async fn exists(&self, id: uuid::Uuid) -> Result<bool> {
        Ok(self
            .epg_source_repo
            .find_by_id(&id)
            .await
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
            let epg_url = crate::utils::UrlUtils::build_xtream_xmltv_url(url, username, password)
                .map_err(|e| anyhow::anyhow!("Invalid URL for Xtream test: {}", e))?;

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
                    message: format!("Connection failed: {e}"),
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
                message: format!("Connection failed: {e}"),
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
            "{url}player_api.php?username={username}&password={password}&action=get_live_categories"
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
    pub program_count: u64,
    pub next_scheduled_update: Option<chrono::DateTime<chrono::Utc>>,
    pub last_ingested_at: Option<chrono::DateTime<chrono::Utc>>,
    pub is_active: bool,
}

/// EPG source with detailed information
#[derive(Debug, Clone)]
pub struct EpgSourceWithDetails {
    pub source: EpgSource,
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
    /// Save EPG programs to database with robust batching and retry logic (atomic operation)
    async fn save_epg_programs(
        &self,
        source_id: uuid::Uuid,
        programs: Vec<crate::models::EpgProgram>,
        progress_updater: Option<&crate::services::progress_service::ProgressStageUpdater>,
    ) -> Result<usize> {
        use tracing::debug;

        if programs.is_empty() {
            debug!("No EPG programs to save for source: {}", source_id);
            return Ok(0);
        }

        // Use a single atomic transaction for delete + insert to prevent race conditions
        use sea_orm::TransactionTrait;
        let txn =
            self.database.connection().begin().await.map_err(|e| {
                anyhow::anyhow!("Failed to begin transaction for EPG programs: {}", e)
            })?;

        // Delete existing programs for this source within the transaction
        use crate::entities::{epg_programs, prelude::*};
        use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};

        let delete_result = EpgPrograms::delete_many()
            .filter(epg_programs::Column::SourceId.eq(source_id))
            .exec(&txn)
            .await?;

        debug!(
            "Deleted {} existing EPG programs for source {}",
            delete_result.rows_affected, source_id
        );

        // Convert programs to domain models for batch insertion (prepare data)
        let mut domain_programs = Vec::new();
        for program in programs {
            // Ensure each program has a unique ID
            let program_with_id = crate::models::EpgProgram {
                id: if program.id == uuid::Uuid::nil() {
                    uuid::Uuid::new_v4()
                } else {
                    program.id
                },
                source_id,
                channel_id: program.channel_id,
                channel_name: program.channel_name,
                program_title: program.program_title,
                program_description: program.program_description,
                program_category: program.program_category,
                start_time: program.start_time,
                end_time: program.end_time,
                episode_num: program.episode_num,
                season_num: program.season_num,
                rating: program.rating,
                language: program.language,
                subtitles: program.subtitles,
                aspect_ratio: program.aspect_ratio,
                program_icon: program.program_icon,
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            };
            domain_programs.push(program_with_id);
        }

        // Insert programs using our atomic batch insert function
        let total_saved = if !domain_programs.is_empty() {
            match Self::insert_epg_programs_batch_in_transaction(
                domain_programs,
                &txn,
                None, // Use default batch config for now
                progress_updater,
            )
            .await
            {
                Ok(inserted_count) => {
                    // Commit the transaction only after both delete and insert succeed
                    txn.commit().await.map_err(|e| {
                        anyhow::anyhow!("Failed to commit EPG programs transaction: {}", e)
                    })?;

                    debug!(
                        "Successfully updated {} EPG programs for source {} (atomic operation)",
                        inserted_count, source_id
                    );
                    inserted_count
                }
                Err(e) => {
                    // Transaction will be automatically rolled back when dropped
                    error!(
                        "Failed to insert EPG programs for source {}: {}",
                        source_id, e
                    );
                    return Err(anyhow::anyhow!("Failed to insert EPG programs: {}", e));
                }
            }
        } else {
            // Commit even if no programs to insert (to finalize the delete)
            txn.commit().await.map_err(|e| {
                anyhow::anyhow!("Failed to commit EPG programs delete transaction: {}", e)
            })?;
            0
        };

        debug!(
            "Successfully saved {} EPG programs for source: {}",
            total_saved, source_id
        );
        Ok(total_saved)
    }

    /// Insert EPG programs in a transaction (helper method for atomic operations)
    async fn insert_epg_programs_batch_in_transaction(
        programs: Vec<crate::models::EpgProgram>,
        txn: &sea_orm::DatabaseTransaction,
        batch_config: Option<&crate::config::DatabaseBatchConfig>,
        progress_updater: Option<&crate::services::progress_service::ProgressStageUpdater>,
    ) -> Result<usize> {
        if programs.is_empty() {
            return Ok(0);
        }

        let batch_size = programs.len();
        debug!(
            "Inserting batch of {} EPG programs using multi-value INSERT in transaction",
            batch_size
        );

        // Use configurable batch size based on database backend and user configuration
        use sea_orm::ConnectionTrait;
        let max_records_per_query = if let Some(config) = batch_config {
            config.safe_epg_program_batch_size(txn.get_database_backend())
        } else {
            // Fallback to defaults if no config provided
            let default_config = crate::config::DatabaseBatchConfig::default();
            default_config.safe_epg_program_batch_size(txn.get_database_backend())
        };

        debug!(
            "Using EPG program batch size: {} for backend: {:?}",
            max_records_per_query,
            txn.get_database_backend()
        );

        let mut total_inserted = 0;
        let total_items = programs.len();

        for (chunk_index, chunk) in programs.chunks(max_records_per_query).enumerate() {
            if chunk.is_empty() {
                continue;
            }

            // Build multi-value INSERT statement
            let mut query = String::from(
                "INSERT INTO epg_programs (id, source_id, channel_id, channel_name, program_title, program_description, program_category, start_time, end_time, language, created_at, updated_at) VALUES ",
            );

            // Generate placeholders based on database backend
            let placeholders: Vec<String> = (0..chunk.len())
                .enumerate()
                .map(|(i, _)| {
                    let base_idx = i * 12; // 12 fields per EPG program
                    match txn.get_database_backend() {
                        sea_orm::DatabaseBackend::Postgres => {
                            format!(
                                "(${}, ${}, ${}, ${}, ${}, ${}, ${}, ${}, ${}, ${}, ${}, ${})",
                                base_idx + 1,
                                base_idx + 2,
                                base_idx + 3,
                                base_idx + 4,
                                base_idx + 5,
                                base_idx + 6,
                                base_idx + 7,
                                base_idx + 8,
                                base_idx + 9,
                                base_idx + 10,
                                base_idx + 11,
                                base_idx + 12
                            )
                        }
                        _ => "(?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)".to_string(),
                    }
                })
                .collect();
            query.push_str(&placeholders.join(", "));

            let mut values = Vec::new();

            // Collect all parameters
            for program in chunk {
                values.push(program.id.into());
                values.push(program.source_id.into());
                values.push(program.channel_id.clone().into());
                values.push(program.channel_name.clone().into());
                values.push(program.program_title.clone().into());
                values.push(program.program_description.clone().into());
                values.push(program.program_category.clone().into());
                values.push(program.start_time.into());
                values.push(program.end_time.into());
                values.push(program.language.clone().into());
                values.push(program.created_at.into());
                values.push(program.updated_at.into());
            }

            use sea_orm::Statement;
            let stmt = Statement::from_sql_and_values(txn.get_database_backend(), &query, values);
            let result = txn.execute(stmt).await.map_err(|e| {
                debug!("SQL execution failed: {}", e);
                debug!("Query was: {}", query);
                anyhow::anyhow!("Failed to insert EPG programs batch: {}", e)
            })?;

            total_inserted += result.rows_affected() as usize;
            debug!(
                "Inserted {} EPG programs in multi-value query",
                result.rows_affected()
            );

            // Update progress if updater is available
            if let Some(updater) = progress_updater {
                // Calculate progress: 20% base + up to 80% for database saving
                let save_progress = (total_inserted as f64 / total_items as f64) * 80.0;
                let total_progress = 20.0 + save_progress;
                let batch_num = chunk_index + 1;
                let total_batches = total_items.div_ceil(max_records_per_query);

                let progress_message = format!(
                    "Inserting batch {}/{} ({} of {} programs)",
                    batch_num, total_batches, total_inserted, total_items
                );

                updater
                    .update_progress(total_progress, &progress_message)
                    .await;
            }
        }

        debug!(
            "Successfully prepared {} EPG programs for insertion in transaction",
            total_inserted
        );
        Ok(total_inserted)
    }

    /// Ingest EPG programs using ProgressStageUpdater (new API)
    pub async fn ingest_programs_with_progress_updater(
        &self,
        source: &EpgSource,
        progress_updater: Option<&crate::services::progress_service::ProgressStageUpdater>,
    ) -> Result<usize> {
        use crate::sources::factory::SourceHandlerFactory;

        debug!(
            "Starting EPG ingestion with ProgressStageUpdater for source: {}",
            source.name
        );

        // Wrap the entire operation in error handling to ensure progress completion
        let result = async {
            // Create EPG source handler using the factory
            let handler = SourceHandlerFactory::create_epg_handler(
                &source.source_type,
                &self.http_client_factory,
            )
            .await
            .map_err(|e| anyhow::anyhow!("Failed to create EPG source handler: {}", e))?;

            // Use the new ProgressStageUpdater API
            let programs = handler
                .ingest_epg_programs_with_progress_updater(source, progress_updater)
                .await
                .map_err(|e| anyhow::anyhow!("EPG source handler failed: {}", e))?;

            debug!(
                "EPG handler ingested {} programs from source '{}'",
                programs.len(),
                source.name
            );

            // Update progress: inserting to database (this is 80% of the total work)
            if let Some(updater) = progress_updater {
                updater
                    .update_progress(
                        20.0,
                        &format!("Inserting {} programs to database", programs.len()),
                    )
                    .await;
            }

            // Save programs to database
            debug!(
                "Saving {} EPG programs to database for '{}'",
                programs.len(),
                source.name
            );
            let programs_saved = self
                .save_epg_programs(source.id, programs, progress_updater)
                .await?;

            // Mark stage as completed
            if let Some(updater) = progress_updater {
                updater
                    .update_progress(
                        100.0,
                        &format!("Completed: {programs_saved} programs saved"),
                    )
                    .await;
                updater.complete_stage().await;
            }

            // Update the source's last_ingested_at timestamp
            info!(
                "Attempting to update last_ingested_at for EPG source '{}'",
                source.name
            );
            match self
                .epg_source_repo
                .update_last_ingested_at(&source.id)
                .await
            {
                Ok(updated_timestamp) => {
                    info!(
                        "Successfully updated last_ingested_at for EPG source '{}' to {}",
                        source.name,
                        updated_timestamp.to_rfc3339()
                    );
                }
                Err(e) => {
                    error!(
                        "Failed to update last_ingested_at for EPG source '{}': {}",
                        source.name, e
                    );
                }
            }

            // Invalidate cache since we updated EPG programs - this triggers proxy auto-regeneration
            let _ = self.cache_invalidation_tx.send(());

            info!(
                "EPG ingestion completed for source '{}': {} programs saved",
                source.name, programs_saved
            );

            Ok(programs_saved)
        }
        .await;

        // Always complete the operation, whether successful or failed
        if let Some(updater) = progress_updater {
            match &result {
                Ok(_) => {
                    updater.complete_operation().await;
                }
                Err(e) => {
                    // Mark operation as failed with error message
                    updater
                        .fail_operation(&format!("EPG ingestion failed: {e}"))
                        .await;
                }
            }
        }

        result
    }
}

#[cfg(test)]
mod tests {

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
