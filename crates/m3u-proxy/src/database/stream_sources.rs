use anyhow::Result;
use chrono::{DateTime, Utc};
use cron::Schedule;
use reqwest::Client;
use sqlx::Row;
use std::str::FromStr;
use std::time::Duration;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use super::Database;
use crate::models::*;
use crate::utils::url::UrlUtils;

// Helper function to check if an Xtream server provides EPG data
async fn check_xtream_epg_availability(base_url: &str, username: &str, password: &str) -> bool {
    let client = Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap_or_else(|_| Client::new());

    // Ensure the base URL has a proper scheme
    let normalized_base_url = UrlUtils::normalize_scheme(base_url);

    let epg_url = format!(
        "{}/xmltv.php?username={}&password={}",
        normalized_base_url, username, password
    );

    match client.head(&epg_url).send().await {
        Ok(response) => {
            let status = response.status();
            let content_type = response
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("");

            // If HEAD request is successful, try a small GET request to verify XMLTV content
            if status.is_success() {
                info!(
                    "EPG probe HEAD request successful for '{}' - Status: {}, Content-Type: '{}'",
                    base_url, status, content_type
                );

                // Some servers don't return proper content-type in HEAD requests
                // Try a GET request with a small range to check for XMLTV content
                match client
                    .get(&epg_url)
                    .header("Range", "bytes=0-512") // Only get first 512 bytes
                    .send()
                    .await
                {
                    Ok(get_response) => {
                        let get_status = get_response.status();
                        let get_content_type = get_response
                            .headers()
                            .get("content-type")
                            .and_then(|v| v.to_str().ok())
                            .unwrap_or("")
                            .to_string(); // Convert to owned String

                        if let Ok(content) = get_response.text().await {
                            let content_preview = if content.len() > 200 {
                                format!("{}...", &content[..200])
                            } else {
                                content.clone()
                            };

                            let has_xml_declaration = content.contains("<?xml");
                            let has_tv_elements = content.contains("<tv")
                                || content.contains("<programme")
                                || content.contains("<channel");
                            let is_xmltv = has_xml_declaration && has_tv_elements;

                            info!("EPG probe GET request for '{}' - Status: {}, Content-Type: '{}', Length: {} bytes",
                                  base_url, get_status, get_content_type, content.len());
                            info!("EPG content preview: {}", content_preview);
                            info!("EPG content analysis - XML declaration: {}, TV elements: {}, Valid XMLTV: {}",
                                  has_xml_declaration, has_tv_elements, is_xmltv);

                            is_xmltv
                        } else {
                            info!("EPG probe GET request for '{}' succeeded but failed to read content - falling back to content-type check", base_url);
                            // Fallback to content-type check
                            content_type.contains("xml") || content_type.contains("text")
                        }
                    }
                    Err(e) => {
                        info!("EPG probe GET request failed for '{}': {} - falling back to HEAD content-type check", base_url, e);
                        // If GET fails, fallback to content-type check from HEAD
                        content_type.contains("xml") || content_type.contains("text")
                    }
                }
            } else {
                info!(
                    "EPG probe HEAD request failed for '{}' - Status: {}, Content-Type: '{}'",
                    base_url, status, content_type
                );
                false
            }
        }
        Err(e) => {
            info!("Failed to check EPG availability for {}: {}", epg_url, e);
            false
        }
    }
}

// Helper function to parse datetime from either RFC3339 or SQLite format
fn parse_datetime(s: &str) -> Result<DateTime<Utc>> {
    // Try RFC3339 first
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
        return Ok(dt.with_timezone(&Utc));
    }
    // Try SQLite datetime format (YYYY-MM-DD HH:MM:SS)
    if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S") {
        return Ok(dt.and_utc());
    }
    Err(anyhow::anyhow!("Failed to parse datetime: {}", s))
}

impl Database {
    pub async fn list_stream_sources_with_stats(&self) -> Result<Vec<StreamSourceWithStats>> {
        // Get sources first (simple query)
        let source_rows = sqlx::query(
            "SELECT id, name, source_type, url, max_concurrent_streams, update_cron,
             username, password, field_map, created_at, updated_at, last_ingested_at, is_active
             FROM stream_sources ORDER BY name",
        )
        .fetch_all(&self.pool)
        .await?;

        let mut sources = Vec::new();
        for row in source_rows {
            let source_type_str: String = row.get("source_type");
            let source_type = match source_type_str.as_str() {
                "m3u" => StreamSourceType::M3u,
                "xtream" => StreamSourceType::Xtream,
                _ => continue,
            };

            let created_at = row.get::<String, _>("created_at");
            let updated_at = row.get::<String, _>("updated_at");
            let last_ingested_at = row.get::<Option<String>, _>("last_ingested_at");
            let source_id_str = row.get::<String, _>("id");

            let source = StreamSource {
                id: Uuid::parse_str(&source_id_str)?,
                name: row.get("name"),
                source_type,
                url: row.get("url"),
                max_concurrent_streams: row.get("max_concurrent_streams"),
                update_cron: row.get("update_cron"),
                username: row.get("username"),
                password: row.get("password"),
                field_map: row.get("field_map"),
                created_at: parse_datetime(&created_at)?,
                updated_at: parse_datetime(&updated_at)?,
                last_ingested_at: last_ingested_at.map(|s| parse_datetime(&s)).transpose()?,
                is_active: row.get("is_active"),
            };

            // Get channel count separately with a simple query
            let channel_count: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM channels WHERE source_id = ?")
                    .bind(&source_id_str)
                    .fetch_optional(&self.pool)
                    .await?
                    .unwrap_or(0);

            // Calculate next scheduled update
            let next_scheduled_update = if source.is_active {
                Schedule::from_str(&source.update_cron)
                    .ok()
                    .and_then(|schedule| schedule.upcoming(Utc).next())
            } else {
                None
            };

            sources.push(StreamSourceWithStats {
                source,
                channel_count,
                next_scheduled_update,
            });
        }

        Ok(sources)
    }

    #[allow(dead_code)]
    pub async fn list_stream_sources(&self) -> Result<Vec<StreamSource>> {
        let rows = sqlx::query(
            "SELECT id, name, source_type, url, max_concurrent_streams, update_cron,
             username, password, field_map, created_at, updated_at, last_ingested_at, is_active
             FROM stream_sources ORDER BY name",
        )
        .fetch_all(&self.pool)
        .await?;

        let mut sources = Vec::new();
        for row in rows {
            let source_type_str: String = row.get("source_type");
            let source_type = match source_type_str.as_str() {
                "m3u" => StreamSourceType::M3u,
                "xtream" => StreamSourceType::Xtream,
                _ => continue,
            };

            let created_at = row.get::<String, _>("created_at");
            let updated_at = row.get::<String, _>("updated_at");
            let last_ingested_at = row.get::<Option<String>, _>("last_ingested_at");

            sources.push(StreamSource {
                id: Uuid::parse_str(&row.get::<String, _>("id"))?,
                name: row.get("name"),
                source_type,
                url: row.get("url"),
                max_concurrent_streams: row.get("max_concurrent_streams"),
                update_cron: row.get("update_cron"),
                username: row.get("username"),
                password: row.get("password"),
                field_map: row.get("field_map"),
                created_at: parse_datetime(&created_at)?,
                updated_at: parse_datetime(&updated_at)?,
                last_ingested_at: last_ingested_at.map(|s| parse_datetime(&s)).transpose()?,
                is_active: row.get("is_active"),
            });
        }

        Ok(sources)
    }

    pub async fn get_stream_source(&self, id: Uuid) -> Result<Option<StreamSource>> {
        let row = sqlx::query(
            "SELECT id, name, source_type, url, max_concurrent_streams, update_cron,
             username, password, field_map, created_at, updated_at, last_ingested_at, is_active
             FROM stream_sources WHERE id = ?",
        )
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await?;

        let Some(row) = row else {
            return Ok(None);
        };

        let source_type_str: String = row.get("source_type");
        let source_type = match source_type_str.as_str() {
            "m3u" => StreamSourceType::M3u,
            "xtream" => StreamSourceType::Xtream,
            _ => return Ok(None),
        };

        let created_at = row.get::<String, _>("created_at");
        let updated_at = row.get::<String, _>("updated_at");
        let last_ingested_at = row.get::<Option<String>, _>("last_ingested_at");

        Ok(Some(StreamSource {
            id: Uuid::parse_str(&row.get::<String, _>("id"))?,
            name: row.get("name"),
            source_type,
            url: row.get("url"),
            max_concurrent_streams: row.get("max_concurrent_streams"),
            update_cron: row.get("update_cron"),
            username: row.get("username"),
            password: row.get("password"),
            field_map: row.get("field_map"),
            created_at: parse_datetime(&created_at)?,
            updated_at: parse_datetime(&updated_at)?,
            last_ingested_at: last_ingested_at.map(|s| parse_datetime(&s)).transpose()?,
            is_active: row.get("is_active"),
        }))
    }

    // Internal method that creates a stream source without auto-linking
    pub(crate) async fn create_stream_source_internal(
        &self,
        source: &StreamSourceCreateRequest,
    ) -> Result<StreamSource> {
        let id = Uuid::new_v4();
        let now = Utc::now();
        let source_type_str = match source.source_type {
            StreamSourceType::M3u => "m3u",
            StreamSourceType::Xtream => "xtream",
        };

        info!(
            "Creating new stream source '{}' ({}) of type {}",
            source.name, id, source_type_str
        );

        sqlx::query(
            "INSERT INTO stream_sources
             (id, name, source_type, url, max_concurrent_streams, update_cron,
              username, password, field_map, created_at, updated_at, is_active)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(id.to_string())
        .bind(&source.name)
        .bind(source_type_str)
        .bind(&source.url)
        .bind(source.max_concurrent_streams)
        .bind(&source.update_cron)
        .bind(&source.username)
        .bind(&source.password)
        .bind(&source.field_map)
        .bind(now.to_rfc3339())
        .bind(now.to_rfc3339())
        .bind(true)
        .execute(&self.pool)
        .await
        .map_err(|e| {
            error!("Failed to create stream source '{}': {}", source.name, e);
            e
        })?;

        info!(
            "Successfully created stream source '{}' ({})",
            source.name, id
        );

        Ok(StreamSource {
            id,
            name: source.name.clone(),
            source_type: source.source_type.clone(),
            url: source.url.clone(),
            max_concurrent_streams: source.max_concurrent_streams,
            update_cron: source.update_cron.clone(),
            username: source.username.clone(),
            password: source.password.clone(),
            field_map: source.field_map.clone(),
            created_at: now,
            updated_at: now,
            last_ingested_at: None,
            is_active: true,
        })
    }

    pub async fn create_stream_source(
        &self,
        source: &StreamSourceCreateRequest,
    ) -> Result<StreamSource> {
        // Create the stream source first
        let stream_source = self.create_stream_source_internal(source).await?;

        // If this is an Xtream source with credentials, check if it provides EPG data
        if source.source_type == StreamSourceType::Xtream {
            if let (Some(username), Some(password)) = (&source.username, &source.password) {
                info!(
                    "Checking if Xtream source '{}' provides EPG data",
                    source.name
                );

                let has_epg = check_xtream_epg_availability(&source.url, username, password).await;

                if has_epg {
                    info!(
                        "Xtream source '{}' provides EPG data - automatically creating EPG source",
                        source.name
                    );

                    let epg_source_request = EpgSourceCreateRequest {
                        name: source.name.clone(),
                        source_type: EpgSourceType::Xtream,
                        url: source.url.clone(),
                        update_cron: source.update_cron.clone(),
                        username: Some(username.clone()),
                        password: Some(password.clone()),
                        timezone: None,    // Will use default UTC
                        time_offset: None, // Will use default 0
                    };

                    match self.create_epg_source_internal(&epg_source_request).await {
                        Ok(epg_source) => {
                            info!("Successfully created linked EPG source '{}' ({}) for stream source '{}'",
                                  epg_source.name, epg_source.id, source.name);

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
                            .bind(stream_source.created_at.to_rfc3339())
                            .bind(stream_source.created_at.to_rfc3339())
                            .execute(&self.pool)
                            .await;

                            match link_result {
                                Ok(_) => info!("Successfully linked stream source '{}' with EPG source '{}'", source.name, epg_source.name),
                                Err(e) => warn!("Failed to create link between stream source '{}' and EPG source '{}': {}", source.name, epg_source.name, e),
                            }
                        }
                        Err(e) => {
                            warn!(
                                "Failed to create EPG source for Xtream stream source '{}': {}",
                                source.name, e
                            );
                        }
                    }
                } else {
                    info!("Xtream source '{}' does not provide EPG data - skipping EPG source creation", source.name);
                }
            } else {
                info!(
                    "Xtream source '{}' has no credentials - cannot check for EPG availability",
                    source.name
                );
            }
        }

        Ok(stream_source)
    }

    pub async fn update_stream_source(
        &self,
        id: Uuid,
        source: &StreamSourceUpdateRequest,
    ) -> Result<Option<StreamSource>> {
        let now = Utc::now();
        let source_type_str = match source.source_type {
            StreamSourceType::M3u => "m3u",
            StreamSourceType::Xtream => "xtream",
        };

        info!("Updating stream source '{}' ({})", source.name, id);

        let result = sqlx::query(
            "UPDATE stream_sources
             SET name = ?, source_type = ?, url = ?, max_concurrent_streams = ?,
                 update_cron = ?, username = ?, password = ?, field_map = ?,
                 updated_at = ?, is_active = ?
             WHERE id = ?",
        )
        .bind(&source.name)
        .bind(source_type_str)
        .bind(&source.url)
        .bind(source.max_concurrent_streams)
        .bind(&source.update_cron)
        .bind(&source.username)
        .bind(&source.password)
        .bind(&source.field_map)
        .bind(now.to_rfc3339())
        .bind(source.is_active)
        .bind(id.to_string())
        .execute(&self.pool)
        .await
        .map_err(|e| {
            error!(
                "Failed to update stream source '{}' ({}): {}",
                source.name, id, e
            );
            e
        })?;

        if result.rows_affected() == 0 {
            warn!(
                "Stream source '{}' ({}) not found for update",
                source.name, id
            );
            return Ok(None);
        }

        info!(
            "Successfully updated stream source '{}' ({})",
            source.name, id
        );
        self.get_stream_source(id).await
    }

    pub async fn delete_stream_source(&self, id: Uuid) -> Result<bool> {
        info!("Deleting stream source ({})", id);

        let result = sqlx::query("DELETE FROM stream_sources WHERE id = ?")
            .bind(id.to_string())
            .execute(&self.pool)
            .await
            .map_err(|e| {
                error!("Failed to delete stream source ({}): {}", id, e);
                e
            })?;

        let deleted = result.rows_affected() > 0;
        if deleted {
            info!("Successfully deleted stream source ({})", id);
        } else {
            warn!("Stream source ({}) not found for deletion", id);
        }

        Ok(deleted)
    }

    pub async fn update_source_channels(
        &self,
        source_id: Uuid,
        channels: &[Channel],
        state_manager: Option<&crate::ingestor::IngestionStateManager>,
    ) -> Result<()> {
        info!(
            "Updating {} channels for source ({})",
            channels.len(),
            source_id
        );

        // Update state to processing if state manager is provided
        if let Some(state_mgr) = state_manager {
            state_mgr
                .update_progress(
                    source_id,
                    crate::models::IngestionState::Processing,
                    crate::models::ProgressInfo {
                        current_step: "Processing channels into database".to_string(),
                        total_bytes: None,
                        downloaded_bytes: None,
                        channels_parsed: Some(channels.len()),
                        channels_saved: Some(0),
                        programs_parsed: None,
                        programs_saved: None,
                        percentage: Some(0.0),
                    },
                )
                .await;
        }

        // Acquire exclusive lock for channel updates to prevent concurrent modifications
        let _lock = self.acquire_channel_update_lock().await;

        // Use a simple transaction for atomic updates
        let mut tx = self.pool.begin().await?;

        // First, delete existing channels for this source
        let delete_result = sqlx::query("DELETE FROM channels WHERE source_id = ?")
            .bind(source_id.to_string())
            .execute(&mut *tx)
            .await?;

        debug!(
            "Deleted {} existing channels for source ({})",
            delete_result.rows_affected(),
            source_id
        );

        // For very large channel sets, use chunked transactions with bulk inserts
        let chunk_size = self.batch_config.stream_channels.unwrap_or(500); // Reduced chunk size for better SQLite performance
        let _progress_update_interval = self.ingestion_config.progress_update_interval;
        let mut inserted_count = 0;

        if channels.len() > chunk_size {
            debug!(
                "Processing {} channels in chunks of {}",
                channels.len(),
                chunk_size
            );

            // Commit initial transaction (just the delete)
            tx.commit().await?;

            for (chunk_idx, chunk) in channels.chunks(chunk_size).enumerate() {
                // Start new transaction for each chunk
                let mut chunk_tx = self.pool.begin().await?;

                // Prepare bulk insert statement
                let mut query_builder = sqlx::QueryBuilder::new(
                    "INSERT INTO channels (id, source_id, tvg_id, tvg_name, tvg_logo, group_title, channel_name, stream_url, created_at, updated_at) "
                );

                query_builder.push_values(chunk, |mut b, channel| {
                    b.push_bind(channel.id.to_string())
                        .push_bind(source_id.to_string())
                        .push_bind(&channel.tvg_id)
                        .push_bind(&channel.tvg_name)
                        .push_bind(&channel.tvg_logo)
                        .push_bind(&channel.group_title)
                        .push_bind(&channel.channel_name)
                        .push_bind(&channel.stream_url)
                        .push_bind(channel.created_at.to_rfc3339())
                        .push_bind(channel.updated_at.to_rfc3339());
                });

                // Execute bulk insert
                let query = query_builder.build();
                debug!(
                    "Inserting chunk {} with {} channels",
                    chunk_idx,
                    chunk.len()
                );
                query.execute(&mut *chunk_tx).await.map_err(|e| {
                    error!(
                        "Failed to bulk insert {} channels for source ({}) at chunk {}: {}",
                        chunk.len(),
                        source_id,
                        chunk_idx,
                        e
                    );
                    e
                })?;
                debug!(
                    "Successfully inserted chunk {} with {} channels",
                    chunk_idx,
                    chunk.len()
                );

                inserted_count += chunk.len();

                // Commit chunk
                debug!("Committing chunk {} transaction", chunk_idx);
                chunk_tx.commit().await.map_err(|e| {
                    error!(
                        "Failed to commit chunk {} transaction for source ({}): {}",
                        chunk_idx, source_id, e
                    );
                    e
                })?;
                debug!("Successfully committed chunk {} transaction", chunk_idx);

                // Small delay between chunks to prevent SQLite from getting overwhelmed
                if chunk_idx > 0 && chunk_idx % 10 == 0 {
                    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                }

                // Update progress after each chunk and log every 10 chunks
                if let Some(state_mgr) = state_manager {
                    let percentage = (inserted_count as f64 / channels.len() as f64) * 100.0;
                    state_mgr
                        .update_progress(
                            source_id,
                            crate::models::IngestionState::Processing,
                            crate::models::ProgressInfo {
                                current_step: format!(
                                    "Processing channels into database ({}/{})",
                                    inserted_count,
                                    channels.len()
                                ),
                                total_bytes: None,
                                downloaded_bytes: None,
                                channels_parsed: Some(channels.len()),
                                channels_saved: Some(inserted_count),
                                programs_parsed: None,
                                programs_saved: None,
                                percentage: Some(percentage),
                            },
                        )
                        .await;
                }

                if chunk_idx % 5 == 0
                    || chunk_idx == (channels.len() + chunk_size - 1) / chunk_size - 1
                {
                    info!(
                        "Progress: Inserted {}/{} channels for source ({}) - {:.1}%",
                        inserted_count,
                        channels.len(),
                        source_id,
                        (inserted_count as f64 / channels.len() as f64) * 100.0
                    );
                }
            }
        } else {
            // For smaller sets, use single transaction with bulk insert
            if !channels.is_empty() {
                // Prepare bulk insert statement for all channels
                let mut query_builder = sqlx::QueryBuilder::new(
                    "INSERT INTO channels (id, source_id, tvg_id, tvg_name, tvg_logo, group_title, channel_name, stream_url, created_at, updated_at) "
                );

                query_builder.push_values(channels, |mut b, channel| {
                    b.push_bind(channel.id.to_string())
                        .push_bind(source_id.to_string())
                        .push_bind(&channel.tvg_id)
                        .push_bind(&channel.tvg_name)
                        .push_bind(&channel.tvg_logo)
                        .push_bind(&channel.group_title)
                        .push_bind(&channel.channel_name)
                        .push_bind(&channel.stream_url)
                        .push_bind(channel.created_at.to_rfc3339())
                        .push_bind(channel.updated_at.to_rfc3339());
                });

                // Execute bulk insert
                let query = query_builder.build();
                query.execute(&mut *tx).await.map_err(|e| {
                    error!(
                        "Failed to bulk insert {} channels for source ({}): {}",
                        channels.len(),
                        source_id,
                        e
                    );
                    e
                })?;

                inserted_count = channels.len();

                // Update progress for smaller sets
                if let Some(state_mgr) = state_manager {
                    let percentage = 100.0;
                    state_mgr
                        .update_progress(
                            source_id,
                            crate::models::IngestionState::Processing,
                            crate::models::ProgressInfo {
                                current_step: format!(
                                    "Processing channels into database ({}/{})",
                                    inserted_count,
                                    channels.len()
                                ),
                                total_bytes: None,
                                downloaded_bytes: None,
                                channels_parsed: Some(channels.len()),
                                channels_saved: Some(inserted_count),
                                programs_parsed: None,
                                programs_saved: None,
                                percentage: Some(percentage),
                            },
                        )
                        .await;
                }
            }

            // Commit the transaction
            tx.commit().await?;
        }

        info!(
            "Successfully updated {} channels for source ({})",
            inserted_count, source_id
        );

        // Force checkpoint for large operations to prevent database corruption
        if inserted_count > 10000 {
            debug!("Forcing WAL checkpoint for large operation");
            let _ = sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
                .execute(&self.pool)
                .await;
        }

        Ok(())
    }

    pub async fn update_source_last_ingested(&self, source_id: Uuid) -> Result<DateTime<Utc>> {
        let now = chrono::Utc::now();
        sqlx::query("UPDATE stream_sources SET last_ingested_at = ? WHERE id = ?")
            .bind(now.to_rfc3339())
            .bind(source_id.to_string())
            .execute(&self.pool)
            .await?;

        Ok(now)
    }

    #[allow(dead_code)]
    pub async fn get_source_channel_count(&self, source_id: Uuid) -> Result<i64> {
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM channels WHERE source_id = ?")
            .bind(source_id.to_string())
            .fetch_one(&self.pool)
            .await?;

        Ok(count)
    }

    #[allow(dead_code)]
    pub async fn get_source_channels(&self, source_id: Uuid) -> Result<Vec<Channel>> {
        let rows = sqlx::query(
            "SELECT id, source_id, tvg_id, tvg_name, tvg_logo, tvg_shift, group_title, channel_name, stream_url, created_at, updated_at
             FROM channels WHERE source_id = ? ORDER BY channel_name"
        )
        .bind(source_id.to_string())
        .fetch_all(&self.pool)
        .await?;

        let mut channels = Vec::new();
        for row in rows {
            let created_at = row.get::<String, _>("created_at");
            let updated_at = row.get::<String, _>("updated_at");

            channels.push(Channel {
                id: Uuid::parse_str(&row.get::<String, _>("id"))?,
                source_id: Uuid::parse_str(&row.get::<String, _>("source_id"))?,
                tvg_id: row.get("tvg_id"),
                tvg_name: row.get("tvg_name"),
                tvg_logo: row.get("tvg_logo"),
                tvg_shift: row.get("tvg_shift"),
                group_title: row.get("group_title"),
                channel_name: row.get("channel_name"),
                stream_url: row.get("stream_url"),
                created_at: parse_datetime(&created_at)?,
                updated_at: parse_datetime(&updated_at)?,
            });
        }

        Ok(channels)
    }

    pub async fn get_source_channels_paginated(
        &self,
        source_id: Uuid,
        page: u32,
        limit: u32,
        filter: Option<&str>,
    ) -> Result<ChannelListResponse> {
        let offset = (page - 1) * limit;

        if let Some(filter_text) = filter {
            // Fuzzy filtering: split into words and match each word
            let words: Vec<&str> = filter_text.split_whitespace().collect();

            if words.is_empty() {
                // If no valid words, fall through to no-filter case
                // (avoid recursion by not calling self again)
            } else {
                // Build dynamic query with proper parameter binding
                let mut where_conditions = Vec::new();
                let mut count_bind_params = Vec::new();
                let mut select_bind_params = Vec::new();

                // For each word, create fuzzy match conditions
                for word in &words {
                    let pattern = format!("%{}%", word.to_lowercase());
                    where_conditions.push(
                        "(LOWER(channel_name) LIKE ? OR
                          LOWER(tvg_id) LIKE ? OR
                          LOWER(tvg_name) LIKE ? OR
                          LOWER(group_title) LIKE ? OR
                          LOWER(stream_url) LIKE ?)",
                    );
                    // Add 5 copies of the pattern for each field (for count query)
                    for _ in 0..5 {
                        count_bind_params.push(pattern.clone());
                    }
                    // Add 5 copies of the pattern for each field (for select query)
                    for _ in 0..5 {
                        select_bind_params.push(pattern.clone());
                    }
                }

                let where_clause = format!(
                    "WHERE source_id = ? AND {} ORDER BY
                     CASE
                       WHEN LOWER(channel_name) LIKE ? THEN 1
                       WHEN LOWER(tvg_name) LIKE ? THEN 2
                       WHEN LOWER(group_title) LIKE ? THEN 3
                       ELSE 4
                     END, channel_name",
                    where_conditions.join(" AND ")
                );

                // Add ranking parameters for select query
                let lower_filter = format!("%{}%", filter_text.to_lowercase());
                select_bind_params.push(lower_filter.clone());
                select_bind_params.push(lower_filter.clone());
                select_bind_params.push(lower_filter);

                // Get total count
                let count_query = format!(
                    "SELECT COUNT(*) FROM channels WHERE source_id = ? AND {}",
                    where_conditions.join(" AND ")
                );

                let mut count_query_builder = sqlx::query_scalar(&count_query);
                count_query_builder = count_query_builder.bind(source_id.to_string());

                // Bind parameters for count query
                for param in &count_bind_params {
                    count_query_builder = count_query_builder.bind(param);
                }

                let total_count: i64 = count_query_builder.fetch_one(&self.pool).await?;

                // Get paginated results
                let select_query = format!(
                    "SELECT id, source_id, tvg_id, tvg_name, tvg_logo, tvg_shift, group_title, channel_name, stream_url, created_at, updated_at
                     FROM channels {} LIMIT ? OFFSET ?",
                    where_clause
                );

                let mut query_builder = sqlx::query(&select_query);
                query_builder = query_builder.bind(source_id.to_string());

                // Bind all parameters in order
                for param in &select_bind_params {
                    query_builder = query_builder.bind(param);
                }
                query_builder = query_builder.bind(limit as i64);
                query_builder = query_builder.bind(offset as i64);

                let rows = query_builder.fetch_all(&self.pool).await?;

                let mut channels = Vec::new();
                for row in rows {
                    let created_at = row.get::<String, _>("created_at");
                    let updated_at = row.get::<String, _>("updated_at");

                    channels.push(Channel {
                        id: Uuid::parse_str(&row.get::<String, _>("id"))?,
                        source_id: Uuid::parse_str(&row.get::<String, _>("source_id"))?,
                        tvg_id: row.get("tvg_id"),
                        tvg_name: row.get("tvg_name"),
                        tvg_logo: row.get("tvg_logo"),
                        tvg_shift: row.get("tvg_shift"),
                        group_title: row.get("group_title"),
                        channel_name: row.get("channel_name"),
                        stream_url: row.get("stream_url"),
                        created_at: parse_datetime(&created_at)?,
                        updated_at: parse_datetime(&updated_at)?,
                    });
                }

                let total_pages = ((total_count as f64) / (limit as f64)).ceil() as u32;

                return Ok(ChannelListResponse {
                    channels,
                    total_count,
                    page,
                    limit,
                    total_pages,
                });
            }
        }

        // No filter - simple query
        let total_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM channels WHERE source_id = ?")
                .bind(source_id.to_string())
                .fetch_one(&self.pool)
                .await?;

        let rows = sqlx::query(
            "SELECT id, source_id, tvg_id, tvg_name, tvg_logo, tvg_shift, group_title, channel_name, stream_url, created_at, updated_at
             FROM channels WHERE source_id = ? ORDER BY channel_name LIMIT ? OFFSET ?"
        )
        .bind(source_id.to_string())
        .bind(limit as i64)
        .bind(offset as i64)
        .fetch_all(&self.pool)
        .await?;

        let mut channels = Vec::new();
        for row in rows {
            let created_at = row.get::<String, _>("created_at");
            let updated_at = row.get::<String, _>("updated_at");

            channels.push(Channel {
                id: Uuid::parse_str(&row.get::<String, _>("id"))?,
                source_id: Uuid::parse_str(&row.get::<String, _>("source_id"))?,
                tvg_id: row.get("tvg_id"),
                tvg_name: row.get("tvg_name"),
                tvg_logo: row.get("tvg_logo"),
                tvg_shift: row.get("tvg_shift"),
                group_title: row.get("group_title"),
                channel_name: row.get("channel_name"),
                stream_url: row.get("stream_url"),
                created_at: parse_datetime(&created_at)?,
                updated_at: parse_datetime(&updated_at)?,
            });
        }

        let total_pages = ((total_count as f64) / (limit as f64)).ceil() as u32;

        Ok(ChannelListResponse {
            channels,
            total_count,
            page,
            limit,
            total_pages,
        })
    }

    pub async fn get_channels_for_source(&self, source_id: Uuid) -> Result<Vec<Channel>> {
        let rows = sqlx::query(
            "SELECT id, source_id, tvg_id, tvg_name, tvg_logo, tvg_shift, group_title, channel_name, stream_url, created_at, updated_at
             FROM channels WHERE source_id = ? ORDER BY channel_name"
        )
        .bind(source_id.to_string())
        .fetch_all(&self.pool)
        .await?;

        let mut channels = Vec::new();
        for row in rows {
            let created_at = row.get::<String, _>("created_at");
            let updated_at = row.get::<String, _>("updated_at");

            channels.push(Channel {
                id: Uuid::parse_str(&row.get::<String, _>("id"))?,
                source_id: Uuid::parse_str(&row.get::<String, _>("source_id"))?,
                tvg_id: row.get("tvg_id"),
                tvg_name: row.get("tvg_name"),
                tvg_logo: row.get("tvg_logo"),
                tvg_shift: row.get("tvg_shift"),
                group_title: row.get("group_title"),
                channel_name: row.get("channel_name"),
                stream_url: row.get("stream_url"),
                created_at: parse_datetime(&created_at)?,
                updated_at: parse_datetime(&updated_at)?,
            });
        }

        Ok(channels)
    }

    /// Get channels for a source with pagination support for streaming
    pub async fn get_channels_for_source_paginated(&self, source_id: Uuid, offset: usize, limit: usize) -> Result<Vec<Channel>> {
        let rows = sqlx::query(
            "SELECT id, source_id, tvg_id, tvg_name, tvg_logo, tvg_shift, group_title, channel_name, stream_url, created_at, updated_at
             FROM channels WHERE source_id = ? ORDER BY channel_name LIMIT ? OFFSET ?"
        )
        .bind(source_id.to_string())
        .bind(limit as i64)
        .bind(offset as i64)
        .fetch_all(&self.pool)
        .await?;

        let mut channels = Vec::new();
        for row in rows {
            let created_at = row.get::<String, _>("created_at");
            let updated_at = row.get::<String, _>("updated_at");

            channels.push(Channel {
                id: Uuid::parse_str(&row.get::<String, _>("id"))?,
                source_id: Uuid::parse_str(&row.get::<String, _>("source_id"))?,
                tvg_id: row.get("tvg_id"),
                tvg_name: row.get("tvg_name"),
                tvg_logo: row.get("tvg_logo"),
                tvg_shift: row.get("tvg_shift"),
                group_title: row.get("group_title"),
                channel_name: row.get("channel_name"),
                stream_url: row.get("stream_url"),
                created_at: parse_datetime(&created_at)?,
                updated_at: parse_datetime(&updated_at)?,
            });
        }

        Ok(channels)
    }
}
