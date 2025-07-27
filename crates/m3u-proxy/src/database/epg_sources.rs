use crate::models::*;
use crate::utils::url::UrlUtils;
use anyhow::Result;
use chrono::{DateTime, Utc};
use cron::Schedule;
use reqwest::Client;
use sqlx::Row;
use std::str::FromStr;
use std::time::Duration;
use tracing::{debug, info, warn};
use uuid::Uuid;

/// Calculate the next scheduled update time for an EPG source based on its cron expression
fn calculate_next_scheduled_update(update_cron: &str) -> Option<DateTime<Utc>> {
    match Schedule::from_str(update_cron) {
        Ok(schedule) => {
            schedule.upcoming(Utc).take(1).next()
        }
        Err(e) => {
            warn!("Failed to parse cron expression '{}': {}", update_cron, e);
            None
        }
    }
}

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
                sqlx::query_scalar("SELECT COUNT(DISTINCT channel_id) FROM epg_programs WHERE source_id = ?")
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
                calculate_next_scheduled_update(&source.update_cron)
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


    pub async fn get_epg_data_for_viewer(
        &self,
        request: &EpgViewerRequestParsed,
    ) -> Result<EpgViewerResponse> {
        let mut query = String::from(
            "SELECT id, source_id, channel_id, channel_name, program_title, program_description, program_category, start_time, end_time, episode_num, season_num, rating, language, subtitles, aspect_ratio, program_icon, created_at, updated_at
             FROM epg_programs
             WHERE start_time <= ? AND end_time >= ?"
        );

        let mut bind_values: Vec<String> = vec![
            request.end_time.to_rfc3339(),
            request.start_time.to_rfc3339(),
        ];

        // Add source filter if specified
        if let Some(source_ids) = &request.source_ids {
            if !source_ids.is_empty() {
                let placeholders = source_ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
                query.push_str(&format!(" AND source_id IN ({})", placeholders));
                bind_values.extend(source_ids.iter().map(|id| id.to_string()));
            }
        }

        // Add channel filter if specified
        if let Some(filter) = &request.channel_filter {
            if !filter.trim().is_empty() {
                query.push_str(" AND channel_name LIKE ?");
                bind_values.push(format!("%{}%", filter));
            }
        }

        query.push_str(" ORDER BY channel_name, start_time");

        // Execute program query
        let mut query_builder = sqlx::query(&query);
        for value in &bind_values {
            query_builder = query_builder.bind(value);
        }

        let program_rows = query_builder.fetch_all(&self.pool).await?;

        let mut programs = Vec::new();
        for program_row in program_rows {
            let program_created_at = program_row.get::<String, _>("created_at");
            let program_updated_at = program_row.get::<String, _>("updated_at");
            let program_id_str = program_row.get::<String, _>("id");
            let source_id_str = program_row.get::<String, _>("source_id");
            let start_time_str = program_row.get::<String, _>("start_time");
            let end_time_str = program_row.get::<String, _>("end_time");

            programs.push(EpgProgram {
                id: Uuid::parse_str(&program_id_str)?,
                source_id: Uuid::parse_str(&source_id_str)?,
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

        Ok(EpgViewerResponse {
            programs,
            start_time: request.start_time,
            end_time: request.end_time,
        })
    }
}
