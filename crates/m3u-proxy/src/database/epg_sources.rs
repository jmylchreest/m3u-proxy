use crate::models::*;
use crate::utils::url::UrlUtils;
use anyhow::Result;
use chrono::{DateTime, Utc};
use reqwest::Client;
use sqlx::Row;
use std::time::Duration;
use tracing::{debug, info, warn};
use uuid::Uuid;

// Helper function to check if an Xtream server provides stream data
async fn check_xtream_stream_availability(base_url: &str, username: &str, password: &str) -> bool {
    let client = Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap_or_else(|_| Client::new());

    // Ensure the base URL has a proper scheme
    let normalized_base_url = UrlUtils::normalize_scheme(base_url);

    let stream_url = format!(
        "{}/player_api.php?username={}&password={}&action=get_live_streams",
        normalized_base_url, username, password
    );

    match client.head(&stream_url).send().await {
        Ok(response) => {
            let status = response.status();
            let content_type = response
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("");

            info!(
                "Stream probe HEAD request for '{}' - Status: {}, Content-Type: '{}'",
                base_url, status, content_type
            );

            let is_available = status.is_success();
            info!(
                "Stream availability result for '{}': {}",
                base_url, is_available
            );
            is_available
        }
        Err(e) => {
            info!("Stream probe HEAD request failed for '{}': {}", base_url, e);
            false
        }
    }
}

fn parse_datetime(datetime_str: &str) -> Result<DateTime<Utc>, chrono::ParseError> {
    // Try parsing as UTC first
    if let Ok(dt) = DateTime::parse_from_rfc3339(datetime_str) {
        return Ok(dt.with_timezone(&Utc));
    }

    // Try parsing as naive datetime and assume UTC
    chrono::NaiveDateTime::parse_from_str(datetime_str, "%Y-%m-%d %H:%M:%S")
        .map(|naive| naive.and_utc())
}

impl crate::database::Database {
    pub async fn list_epg_sources_with_stats(&self) -> Result<Vec<EpgSourceWithStats>> {
        // Get sources first (simple query)
        let source_rows = sqlx::query(
            "SELECT id, name, source_type, url, update_cron,
             username, password, original_timezone, time_offset, created_at, updated_at, last_ingested_at, is_active
             FROM epg_sources ORDER BY name",
        )
        .fetch_all(&self.pool)
        .await?;

        let mut sources = Vec::new();
        for row in source_rows {
            let source_type_str: String = row.get("source_type");
            let source_type = match source_type_str.as_str() {
                "xmltv" => EpgSourceType::Xmltv,
                "xtream" => EpgSourceType::Xtream,
                _ => continue,
            };

            let created_at = row.get::<String, _>("created_at");
            let updated_at = row.get::<String, _>("updated_at");
            let last_ingested_at = row.get::<Option<String>, _>("last_ingested_at");
            let source_id_str = row.get::<String, _>("id");

            let source = EpgSource {
                id: Uuid::parse_str(&source_id_str)?,
                name: row.get("name"),
                source_type,
                url: row.get("url"),
                update_cron: row.get("update_cron"),
                username: row.get("username"),
                password: row.get("password"),
                original_timezone: row.get("original_timezone"),
                time_offset: row.get("time_offset"),
                created_at: parse_datetime(&created_at)?,
                updated_at: parse_datetime(&updated_at)?,
                last_ingested_at: last_ingested_at
                    .as_ref()
                    .map(|s| parse_datetime(s))
                    .transpose()?,
                is_active: row.get("is_active"),
            };

            // Get stats for this source
            let channel_count: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM epg_channels WHERE source_id = ?")
                    .bind(&source_id_str)
                    .fetch_one(&self.pool)
                    .await?;

            let program_count: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM epg_programs WHERE source_id = ?")
                    .bind(&source_id_str)
                    .fetch_one(&self.pool)
                    .await?;

            // Calculate next scheduled update (if active)
            let next_scheduled_update = if source.is_active {
                // TODO: Implement cron calculation for EPG sources
                None
            } else {
                None
            };

            sources.push(EpgSourceWithStats {
                source,
                channel_count,
                program_count,
                next_scheduled_update,
            });
        }

        Ok(sources)
    }

    #[allow(dead_code)]
    pub async fn list_epg_sources(&self) -> Result<Vec<EpgSource>> {
        let rows = sqlx::query(
            "SELECT id, name, source_type, url, update_cron,
             username, password, original_timezone, time_offset, created_at, updated_at, last_ingested_at, is_active
             FROM epg_sources ORDER BY name",
        )
        .fetch_all(&self.pool)
        .await?;

        let mut sources = Vec::new();
        for row in rows {
            let source_type_str: String = row.get("source_type");
            let source_type = match source_type_str.as_str() {
                "xmltv" => EpgSourceType::Xmltv,
                "xtream" => EpgSourceType::Xtream,
                _ => continue,
            };

            let created_at = row.get::<String, _>("created_at");
            let updated_at = row.get::<String, _>("updated_at");
            let last_ingested_at = row.get::<Option<String>, _>("last_ingested_at");
            let source_id_str = row.get::<String, _>("id");

            sources.push(EpgSource {
                id: Uuid::parse_str(&source_id_str)?,
                name: row.get("name"),
                source_type,
                url: row.get("url"),
                update_cron: row.get("update_cron"),
                username: row.get("username"),
                password: row.get("password"),
                original_timezone: row.get("original_timezone"),
                time_offset: row.get("time_offset"),
                created_at: parse_datetime(&created_at)?,
                updated_at: parse_datetime(&updated_at)?,
                last_ingested_at: last_ingested_at
                    .as_ref()
                    .map(|s| parse_datetime(s))
                    .transpose()?,
                is_active: row.get("is_active"),
            });
        }

        Ok(sources)
    }

    pub async fn get_epg_source(&self, id: Uuid) -> Result<Option<EpgSource>> {
        let row = sqlx::query(
            "SELECT id, name, source_type, url, update_cron,
             username, password, original_timezone, time_offset, created_at, updated_at, last_ingested_at, is_active
             FROM epg_sources WHERE id = ?",
        )
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await?;

        if let Some(row) = row {
            let source_type_str: String = row.get("source_type");
            let source_type = match source_type_str.as_str() {
                "xmltv" => EpgSourceType::Xmltv,
                "xtream" => EpgSourceType::Xtream,
                _ => return Ok(None),
            };

            let created_at = row.get::<String, _>("created_at");
            let updated_at = row.get::<String, _>("updated_at");
            let last_ingested_at = row.get::<Option<String>, _>("last_ingested_at");

            Ok(Some(EpgSource {
                id,
                name: row.get("name"),
                source_type,
                url: row.get("url"),
                update_cron: row.get("update_cron"),
                username: row.get("username"),
                password: row.get("password"),
                original_timezone: row.get("original_timezone"),
                time_offset: row.get("time_offset"),
                created_at: parse_datetime(&created_at)?,
                updated_at: parse_datetime(&updated_at)?,
                last_ingested_at: last_ingested_at
                    .as_ref()
                    .map(|s| parse_datetime(s))
                    .transpose()?,
                is_active: row.get("is_active"),
            }))
        } else {
            Ok(None)
        }
    }

    // Internal method that creates an EPG source without auto-linking
    pub(crate) async fn create_epg_source_internal(
        &self,
        source: &EpgSourceCreateRequest,
    ) -> Result<EpgSource> {
        let id = Uuid::new_v4();
        let now = chrono::Utc::now();

        let source_type_str = match source.source_type {
            EpgSourceType::Xmltv => "xmltv",
            EpgSourceType::Xtream => "xtream",
        };

        let timezone = source.timezone.as_deref().unwrap_or("UTC");
        let time_offset = source.time_offset.as_deref().unwrap_or("0");

        sqlx::query(
            "INSERT INTO epg_sources (id, name, source_type, url, update_cron, username, password, original_timezone, time_offset, created_at, updated_at, last_ingested_at, is_active)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(id.to_string())
        .bind(&source.name)
        .bind(source_type_str)
        .bind(&source.url)
        .bind(&source.update_cron)
        .bind(&source.username)
        .bind(&source.password)
        .bind(timezone)
        .bind(time_offset)
        .bind(now.format("%Y-%m-%d %H:%M:%S").to_string())
        .bind(now.format("%Y-%m-%d %H:%M:%S").to_string())
        .bind::<Option<String>>(None) // last_ingested_at - null initially
        .bind(true)
        .execute(&self.pool)
        .await?;

        info!("Created new EPG source: {} ({})", source.name, id);

        Ok(EpgSource {
            id,
            name: source.name.clone(),
            source_type: source.source_type.clone(),
            url: source.url.clone(),
            update_cron: source.update_cron.clone(),
            username: source.username.clone(),
            password: source.password.clone(),
            original_timezone: None,
            time_offset: time_offset.to_string(),
            created_at: now,
            updated_at: now,
            last_ingested_at: None,
            is_active: true,
        })
    }

    pub async fn create_epg_source(&self, source: &EpgSourceCreateRequest) -> Result<EpgSource> {
        // Create the EPG source first
        let epg_source = self.create_epg_source_internal(source).await?;

        // If this is an Xtream EPG source with credentials, check if it provides stream data
        if source.source_type == EpgSourceType::Xtream {
            if let (Some(username), Some(password)) = (&source.username, &source.password) {
                info!(
                    "Checking if Xtream EPG source '{}' provides stream data",
                    source.name
                );

                let has_streams =
                    check_xtream_stream_availability(&source.url, username, password).await;

                if has_streams {
                    info!(
                        "Xtream EPG source '{}' provides stream data - automatically creating stream source",
                        source.name
                    );

                    let stream_source_request = StreamSourceCreateRequest {
                        name: format!("{} (Stream)", source.name),
                        source_type: StreamSourceType::Xtream,
                        url: source.url.clone(),
                        max_concurrent_streams: 10, // Default value
                        update_cron: source.update_cron.clone(),
                        username: Some(username.clone()),
                        password: Some(password.clone()),
                        field_map: None,
                        ignore_channel_numbers: true, // Default to true for Xtream sources
                    };

                    match self
                        .create_stream_source_internal(&stream_source_request)
                        .await
                    {
                        Ok(stream_source) => {
                            info!(
                                "Successfully created linked stream source '{}' ({}) for EPG source '{}'",
                                stream_source.name, stream_source.id, source.name
                            );

                            // Create a linked entry to track the relationship
                            let link_id = Uuid::new_v4();
                            let linked_id = Uuid::new_v4();
                            let link_result = sqlx::query(
                                "INSERT INTO linked_xtream_sources (id, stream_source_id, epg_source_id, link_id, name, url, username, password, created_at, updated_at)
                                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
                            )
                            .bind(linked_id.to_string())
                            .bind(stream_source.id.to_string())
                            .bind(epg_source.id.to_string())
                            .bind(link_id.to_string())
                            .bind(format!("{} (Linked)", source.name))
                            .bind(&source.url)
                            .bind(&source.username)
                            .bind(&source.password)
                            .bind(epg_source.created_at.format("%Y-%m-%d %H:%M:%S").to_string())
                            .bind(epg_source.created_at.format("%Y-%m-%d %H:%M:%S").to_string())
                            .execute(&self.pool)
                            .await;

                            match link_result {
                                Ok(_) => info!(
                                    "Successfully linked EPG source '{}' with stream source '{}'",
                                    source.name, stream_source.name
                                ),
                                Err(e) => warn!(
                                    "Failed to create link between EPG source '{}' and stream source '{}': {}",
                                    source.name, stream_source.name, e
                                ),
                            }
                        }
                        Err(e) => {
                            warn!(
                                "Failed to create stream source for Xtream EPG source '{}': {}",
                                source.name, e
                            );
                        }
                    }
                } else {
                    info!(
                        "Xtream EPG source '{}' does not provide stream data - skipping stream source creation",
                        source.name
                    );
                }
            } else {
                info!(
                    "Xtream EPG source '{}' has no credentials - cannot check for stream availability",
                    source.name
                );
            }
        }

        // Emit scheduler event for source creation
        self.emit_scheduler_event(crate::ingestor::scheduler::SchedulerEvent::SourceCreated(epg_source.id));

        Ok(epg_source)
    }

    pub async fn update_epg_source(
        &self,
        id: Uuid,
        source: &EpgSourceUpdateRequest,
    ) -> Result<bool> {
        let source_type_str = match source.source_type {
            EpgSourceType::Xmltv => "xmltv",
            EpgSourceType::Xtream => "xtream",
        };

        // Conditionally update password - only if provided and non-empty
        let should_update_password = source.password.as_ref().map_or(false, |p| !p.is_empty());
        
        let result = if should_update_password {
            // Password provided and non-empty - update it
            info!("Updating password for EPG source '{}' ({})", source.name, id);
            sqlx::query(
                "UPDATE epg_sources
                 SET name = ?, source_type = ?, url = ?, update_cron = ?, username = ?, password = ?, original_timezone = ?, time_offset = ?, is_active = ?
                 WHERE id = ?",
            )
            .bind(&source.name)
            .bind(source_type_str)
            .bind(&source.url)
            .bind(&source.update_cron)
            .bind(&source.username)
            .bind(source.password.as_ref().unwrap())
            .bind(&source.timezone)
            .bind(&source.time_offset)
            .bind(source.is_active)
            .bind(id.to_string())
            .execute(&self.pool)
            .await
        } else {
            // Password not provided or empty - preserve existing password
            debug!("Preserving existing password for EPG source '{}' ({})", source.name, id);
            sqlx::query(
                "UPDATE epg_sources
                 SET name = ?, source_type = ?, url = ?, update_cron = ?, username = ?, original_timezone = ?, time_offset = ?, is_active = ?
                 WHERE id = ?",
            )
            .bind(&source.name)
            .bind(source_type_str)
            .bind(&source.url)
            .bind(&source.update_cron)
            .bind(&source.username)
            .bind(&source.timezone)
            .bind(&source.time_offset)
            .bind(source.is_active)
            .bind(id.to_string())
            .execute(&self.pool)
            .await
        }?;

        let updated = result.rows_affected() > 0;
        if updated {
            info!("Updated EPG source: {} ({})", source.name, id);
            // Emit scheduler event for source update
            self.emit_scheduler_event(crate::ingestor::scheduler::SchedulerEvent::SourceUpdated(id));
        } else {
            warn!("EPG source not found for update: {}", id);
        }

        Ok(updated)
    }

    pub async fn delete_epg_source(&self, id: Uuid) -> Result<bool> {
        info!("Deleting EPG source ({})", id);

        let result = sqlx::query("DELETE FROM epg_sources WHERE id = ?")
            .bind(id.to_string())
            .execute(&self.pool)
            .await?;

        let deleted = result.rows_affected() > 0;
        if deleted {
            info!("Deleted EPG source: {}", id);
            // Emit scheduler event for source deletion
            self.emit_scheduler_event(crate::ingestor::scheduler::SchedulerEvent::SourceDeleted(id));
        } else {
            warn!("EPG source not found for deletion: {}", id);
        }

        Ok(deleted)
    }

    pub async fn update_epg_source_data(
        &self,
        source_id: Uuid,
        channels: Vec<EpgChannel>,
        programs: Vec<EpgProgram>,
    ) -> Result<(usize, usize)> {
        self.update_epg_source_data_with_cancellation(source_id, channels, programs, None)
            .await
    }

    pub async fn update_epg_source_data_with_cancellation(
        &self,
        source_id: Uuid,
        channels: Vec<EpgChannel>,
        programs: Vec<EpgProgram>,
        cancellation_rx: Option<tokio::sync::broadcast::Receiver<()>>,
    ) -> Result<(usize, usize)> {
        self.update_epg_source_data_with_cancellation_and_progress(
            source_id,
            channels,
            programs,
            cancellation_rx,
            None::<fn(usize, usize)>,
        )
        .await
    }

    pub async fn update_epg_source_data_with_cancellation_and_progress<F>(
        &self,
        source_id: Uuid,
        channels: Vec<EpgChannel>,
        programs: Vec<EpgProgram>,
        mut cancellation_rx: Option<tokio::sync::broadcast::Receiver<()>>,
        progress_callback: Option<F>,
    ) -> Result<(usize, usize)>
    where
        F: Fn(usize, usize) + Send + Sync,
    {
        // Use shorter transaction timeout to reduce deadlock risk

        // Step 1: Clear existing data in a separate, quick transaction
        let clear_result = tokio::time::timeout(Duration::from_secs(30), async {
            let mut tx = self.pool.begin().await?;

            // Set shorter busy timeout for cleanup operations
            sqlx::query("PRAGMA busy_timeout = 10000") // 10 seconds
                .execute(&mut *tx)
                .await?;

            // Check for cancellation before starting
            if let Some(ref mut rx) = cancellation_rx {
                if rx.try_recv().is_ok() {
                    return Err(anyhow::anyhow!(
                        "Operation cancelled before database cleanup"
                    ));
                }
            }

            // Clear existing data for this source
            sqlx::query("DELETE FROM epg_programs WHERE source_id = ?")
                .bind(source_id.to_string())
                .execute(&mut *tx)
                .await?;

            sqlx::query("DELETE FROM epg_channels WHERE source_id = ?")
                .bind(source_id.to_string())
                .execute(&mut *tx)
                .await?;

            tx.commit().await?;
            Ok::<(), anyhow::Error>(())
        })
        .await;

        match clear_result {
            Ok(Ok(())) => {}
            Ok(Err(e)) => return Err(e),
            Err(_) => {
                return Err(anyhow::anyhow!(
                    "Database cleanup timed out after 30 seconds"
                ));
            }
        }

        // Check for cancellation after cleanup
        if let Some(ref mut rx) = cancellation_rx {
            if rx.try_recv().is_ok() {
                return Err(anyhow::anyhow!("Operation cancelled after cleanup"));
            }
        }

        // Step 2: Insert channels in smaller transactions
        let channel_batch_size = self.batch_config.safe_epg_channel_batch_size();
        let channel_chunks: Vec<_> = channels.chunks(channel_batch_size).collect();

        for (i, chunk) in channel_chunks.iter().enumerate() {
            if !chunk.is_empty() {
                let insert_result = tokio::time::timeout(Duration::from_secs(60), async {
                    let mut tx = self.pool.begin().await?;

                    // Set busy timeout for individual transactions
                    sqlx::query("PRAGMA busy_timeout = 15000") // 15 seconds
                        .execute(&mut *tx)
                        .await?;

                    // Prepare bulk insert statement
                    let mut query_builder = sqlx::QueryBuilder::new(
                        "INSERT INTO epg_channels (id, source_id, channel_id, channel_name, channel_logo, channel_group, language, created_at, updated_at) "
                    );

                    query_builder.push_values(*chunk, |mut b, channel| {
                        b.push_bind(channel.id.to_string())
                            .push_bind(source_id.to_string())
                            .push_bind(&channel.channel_id)
                            .push_bind(&channel.channel_name)
                            .push_bind(&channel.channel_logo)
                            .push_bind(&channel.channel_group)
                            .push_bind(&channel.language)
                            .push_bind(channel.created_at.format("%Y-%m-%d %H:%M:%S").to_string())
                            .push_bind(channel.updated_at.format("%Y-%m-%d %H:%M:%S").to_string());
                    });

                    // Execute bulk insert
                    let query = query_builder.build();
                    query.execute(&mut *tx).await?;
                    tx.commit().await?;
                    Ok::<(), anyhow::Error>(())
                }).await;

                match insert_result {
                    Ok(Ok(())) => {}
                    Ok(Err(e)) => return Err(e),
                    Err(_) => {
                        return Err(anyhow::anyhow!(
                            "Channel insertion timed out for batch {}",
                            i + 1
                        ));
                    }
                }

                // Check for cancellation after each chunk
                if let Some(ref mut rx) = cancellation_rx {
                    if rx.try_recv().is_ok() {
                        return Err(anyhow::anyhow!(
                            "Operation cancelled during channel insertion"
                        ));
                    }
                }
            }
        }

        // Step 3: Insert programs in smaller transactions with progress tracking
        let program_batch_size = self.batch_config.safe_epg_program_batch_size();
        let total_programs = programs.len();
        let mut programs_saved = 0;
        let program_chunks: Vec<_> = programs.chunks(program_batch_size).collect();

        for (i, chunk) in program_chunks.iter().enumerate() {
            if !chunk.is_empty() {
                let insert_result = tokio::time::timeout(Duration::from_secs(90), async {
                    let mut tx = self.pool.begin().await?;

                    // Set busy timeout for individual transactions
                    sqlx::query("PRAGMA busy_timeout = 15000") // 15 seconds
                        .execute(&mut *tx)
                        .await?;

                    // Prepare bulk insert statement
                    let mut query_builder = sqlx::QueryBuilder::new(
                        "INSERT INTO epg_programs (id, source_id, channel_id, channel_name, program_title, program_description, program_category, start_time, end_time, episode_num, season_num, rating, language, subtitles, aspect_ratio, program_icon, created_at, updated_at) "
                    );

                    query_builder.push_values(*chunk, |mut b, program| {
                        b.push_bind(program.id.to_string())
                            .push_bind(source_id.to_string())
                            .push_bind(&program.channel_id)
                            .push_bind(&program.channel_name)
                            .push_bind(&program.program_title)
                            .push_bind(&program.program_description)
                            .push_bind(&program.program_category)
                            .push_bind(program.start_time.to_rfc3339())
                            .push_bind(program.end_time.to_rfc3339())
                            .push_bind(&program.episode_num)
                            .push_bind(&program.season_num)
                            .push_bind(&program.rating)
                            .push_bind(&program.language)
                            .push_bind(&program.subtitles)
                            .push_bind(&program.aspect_ratio)
                            .push_bind(&program.program_icon)
                            .push_bind(program.created_at.format("%Y-%m-%d %H:%M:%S").to_string())
                            .push_bind(program.updated_at.format("%Y-%m-%d %H:%M:%S").to_string());
                    });

                    // Execute bulk insert
                    let query = query_builder.build();
                    query.execute(&mut *tx).await?;
                    tx.commit().await?;
                    Ok::<(), anyhow::Error>(())
                }).await;

                match insert_result {
                    Ok(Ok(())) => {}
                    Ok(Err(e)) => return Err(e),
                    Err(_) => {
                        return Err(anyhow::anyhow!(
                            "Program insertion timed out for batch {}",
                            i + 1
                        ));
                    }
                }

                programs_saved += chunk.len();

                // Report progress if callback is provided
                if let Some(ref callback) = progress_callback {
                    callback(programs_saved, total_programs);
                }

                // Check for cancellation after each batch
                if let Some(ref mut rx) = cancellation_rx {
                    if rx.try_recv().is_ok() {
                        return Err(anyhow::anyhow!(
                            "Operation cancelled during program insertion"
                        ));
                    }
                }
            }
        }

        Ok((channels.len(), programs.len()))
    }

    pub async fn update_epg_source_last_ingested(&self, source_id: Uuid) -> Result<DateTime<Utc>> {
        let now = chrono::Utc::now();
        sqlx::query("UPDATE epg_sources SET last_ingested_at = ? WHERE id = ?")
            .bind(now.format("%Y-%m-%d %H:%M:%S").to_string())
            .bind(source_id.to_string())
            .execute(&self.pool)
            .await?;

        Ok(now)
    }

    /// Update the detected timezone for an EPG source
    pub async fn update_epg_source_detected_timezone(
        &self,
        source_id: Uuid,
        detected_timezone: &str,
    ) -> Result<()> {
        sqlx::query("UPDATE epg_sources SET original_timezone = ? WHERE id = ?")
            .bind(detected_timezone)
            .bind(source_id.to_string())
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    #[allow(dead_code)]
    pub async fn get_epg_source_channel_count(&self, source_id: Uuid) -> Result<i64> {
        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM epg_channels WHERE source_id = ?")
                .bind(source_id.to_string())
                .fetch_one(&self.pool)
                .await?;

        Ok(count)
    }

    #[allow(dead_code)]
    pub async fn get_epg_source_channels(&self, source_id: Uuid) -> Result<Vec<EpgChannel>> {
        let rows = sqlx::query(
            "SELECT id, source_id, channel_id, channel_name, channel_logo, channel_group, language, created_at, updated_at
             FROM epg_channels WHERE source_id = ? ORDER BY channel_name",
        )
        .bind(source_id.to_string())
        .fetch_all(&self.pool)
        .await?;

        let mut channels = Vec::new();
        for row in rows {
            let created_at = row.get::<String, _>("created_at");
            let updated_at = row.get::<String, _>("updated_at");
            let id_str = row.get::<String, _>("id");

            channels.push(EpgChannel {
                id: Uuid::parse_str(&id_str)?,
                source_id,
                channel_id: row.get("channel_id"),
                channel_name: row.get("channel_name"),
                channel_logo: row.get("channel_logo"),
                channel_group: row.get("channel_group"),
                language: row.get("language"),
                created_at: parse_datetime(&created_at)?,
                updated_at: parse_datetime(&updated_at)?,
            });
        }

        Ok(channels)
    }
    
    /// Get EPG source channels with all their display names (multilingual support)
    pub async fn get_epg_source_channels_with_display_names(&self, source_id: Uuid) -> Result<Vec<crate::models::EpgChannelWithDisplayNames>> {
        // First get all channels for the source
        let channel_rows = sqlx::query(
            "SELECT id, source_id, channel_id, channel_name, channel_logo, channel_group, language, created_at, updated_at
             FROM epg_channels WHERE source_id = ? ORDER BY channel_name",
        )
        .bind(source_id.to_string())
        .fetch_all(&self.pool)
        .await?;

        let mut channels_with_display_names = Vec::new();
        
        for channel_row in channel_rows {
            let created_at = channel_row.get::<String, _>("created_at");
            let updated_at = channel_row.get::<String, _>("updated_at");
            let id_str = channel_row.get::<String, _>("id");
            let channel_uuid = Uuid::parse_str(&id_str)?;

            // Build the main channel
            let channel = crate::models::EpgChannel {
                id: channel_uuid,
                source_id,
                channel_id: channel_row.get("channel_id"),
                channel_name: channel_row.get("channel_name"),
                channel_logo: channel_row.get("channel_logo"),
                channel_group: channel_row.get("channel_group"),
                language: channel_row.get("language"),
                created_at: parse_datetime(&created_at)?,
                updated_at: parse_datetime(&updated_at)?,
            };

            // Get all display names for this channel
            let display_name_rows = sqlx::query(
                "SELECT id, epg_channel_id, display_name, language, is_primary, created_at, updated_at
                 FROM epg_channel_display_names WHERE epg_channel_id = ? ORDER BY is_primary DESC, display_name",
            )
            .bind(channel_uuid.to_string())
            .fetch_all(&self.pool)
            .await?;

            let mut display_names = Vec::new();
            for display_row in display_name_rows {
                let display_created_at = display_row.get::<String, _>("created_at");
                let display_updated_at = display_row.get::<String, _>("updated_at");
                let display_id_str = display_row.get::<String, _>("id");
                
                display_names.push(crate::models::EpgChannelDisplayName {
                    id: Uuid::parse_str(&display_id_str)?,
                    epg_channel_id: channel_uuid,
                    display_name: display_row.get("display_name"),
                    language: display_row.get("language"),
                    is_primary: display_row.get("is_primary"),
                    created_at: parse_datetime(&display_created_at)?,
                    updated_at: parse_datetime(&display_updated_at)?,
                });
            }

            channels_with_display_names.push(crate::models::EpgChannelWithDisplayNames {
                channel,
                display_names,
            });
        }

        Ok(channels_with_display_names)
    }

    pub async fn get_epg_data_for_viewer(
        &self,
        request: &EpgViewerRequestParsed,
    ) -> Result<EpgViewerResponse> {
        let mut query = String::from(
            "SELECT DISTINCT c.id, c.source_id, c.channel_id, c.channel_name, c.channel_logo, c.channel_group, c.language, c.created_at, c.updated_at
             FROM epg_channels c
             INNER JOIN epg_programs p ON c.source_id = p.source_id AND c.channel_id = p.channel_id
             WHERE p.start_time <= ? AND p.end_time >= ?"
        );

        let mut bind_values: Vec<String> = vec![
            request.end_time.to_rfc3339(),
            request.start_time.to_rfc3339(),
        ];

        // Add source filter if specified
        if let Some(source_ids) = &request.source_ids {
            if !source_ids.is_empty() {
                let placeholders = source_ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
                query.push_str(&format!(" AND c.source_id IN ({})", placeholders));
                bind_values.extend(source_ids.iter().map(|id| id.to_string()));
            }
        }

        // Add channel filter if specified
        if let Some(filter) = &request.channel_filter {
            if !filter.trim().is_empty() {
                query.push_str(" AND c.channel_name LIKE ?");
                bind_values.push(format!("%{}%", filter));
            }
        }

        query.push_str(" ORDER BY c.channel_name");

        // Execute channel query
        let mut query_builder = sqlx::query(&query);
        for value in &bind_values {
            query_builder = query_builder.bind(value);
        }

        let channel_rows = query_builder.fetch_all(&self.pool).await?;

        let mut channels_with_programs = Vec::new();

        for row in channel_rows {
            let created_at = row.get::<String, _>("created_at");
            let updated_at = row.get::<String, _>("updated_at");
            let id_str = row.get::<String, _>("id");
            let source_id_str = row.get::<String, _>("source_id");

            let channel = EpgChannel {
                id: Uuid::parse_str(&id_str)?,
                source_id: Uuid::parse_str(&source_id_str)?,
                channel_id: row.get("channel_id"),
                channel_name: row.get("channel_name"),
                channel_logo: row.get("channel_logo"),
                channel_group: row.get("channel_group"),
                language: row.get("language"),
                created_at: parse_datetime(&created_at)?,
                updated_at: parse_datetime(&updated_at)?,
            };

            // Get programs for this channel
            let program_rows = sqlx::query(
                "SELECT id, source_id, channel_id, channel_name, program_title, program_description, program_category, start_time, end_time, episode_num, season_num, rating, language, subtitles, aspect_ratio, program_icon, created_at, updated_at
                 FROM epg_programs
                 WHERE source_id = ? AND channel_id = ? AND start_time <= ? AND end_time >= ?
                 ORDER BY start_time",
            )
            .bind(&source_id_str)
            .bind(&channel.channel_id)
            .bind(request.end_time.to_rfc3339())
            .bind(request.start_time.to_rfc3339())
            .fetch_all(&self.pool)
            .await?;

            let mut programs = Vec::new();
            for program_row in program_rows {
                let program_created_at = program_row.get::<String, _>("created_at");
                let program_updated_at = program_row.get::<String, _>("updated_at");
                let program_id_str = program_row.get::<String, _>("id");
                let start_time_str = program_row.get::<String, _>("start_time");
                let end_time_str = program_row.get::<String, _>("end_time");

                programs.push(EpgProgram {
                    id: Uuid::parse_str(&program_id_str)?,
                    source_id: channel.source_id,
                    channel_id: program_row.get("channel_id"),
                    channel_name: program_row.get("channel_name"),
                    program_title: program_row.get("program_title"),
                    program_description: program_row.get("program_description"),
                    program_category: program_row.get("program_category"),
                    start_time: DateTime::parse_from_rfc3339(&start_time_str)?.with_timezone(&Utc),
                    end_time: DateTime::parse_from_rfc3339(&end_time_str)?.with_timezone(&Utc),
                    episode_num: program_row.get("episode_num"),
                    season_num: program_row.get("season_num"),
                    rating: program_row.get("rating"),
                    language: program_row.get("language"),
                    subtitles: program_row.get("subtitles"),
                    aspect_ratio: program_row.get("aspect_ratio"),
                    program_icon: program_row.get("program_icon"),
                    created_at: parse_datetime(&program_created_at)?,
                    updated_at: parse_datetime(&program_updated_at)?,
                });
            }

            channels_with_programs.push(EpgChannelWithPrograms { channel, programs });
        }

        Ok(EpgViewerResponse {
            channels: channels_with_programs,
            start_time: request.start_time,
            end_time: request.end_time,
        })
    }
}
