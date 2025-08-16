//! URL-based source linking repository implementation
//!
//! This module provides repository pattern implementation for URL-based linking
//! between stream and EPG sources, enabling automatic relationship discovery
//! and credential propagation for Xtream Codes sources.

use sqlx::{Pool, Sqlite, Row};
use uuid::Uuid;
use chrono::Utc;
use tracing::{info, debug};

use crate::errors::RepositoryResult;
use crate::models::{StreamSource, EpgSource, StreamSourceType, EpgSourceType};
use crate::utils::uuid_parser::parse_uuid_flexible;
use crate::utils::sqlite::SqliteRowExt;

/// URL-based linking repository for automatic source relationship management
pub struct UrlLinkingRepository {
    pool: Pool<Sqlite>,
}

impl UrlLinkingRepository {
    /// Create a new URL linking repository
    pub fn new(pool: Pool<Sqlite>) -> Self {
        Self { pool }
    }

    /// Find stream sources with the same URL as the given EPG source
    pub async fn find_linked_stream_sources(&self, epg_source: &EpgSource) -> RepositoryResult<Vec<StreamSource>> {
        if epg_source.source_type != EpgSourceType::Xtream {
            return Ok(Vec::new());
        }

        let rows = sqlx::query(
            "SELECT id, name, source_type, url, max_concurrent_streams, update_cron, username, password, 
             field_map, ignore_channel_numbers, created_at, updated_at, last_ingested_at, is_active 
             FROM stream_sources 
             WHERE source_type = 'xtream' AND url = ? AND id != ? AND is_active = 1"
        )
        .bind(&epg_source.url)
        .bind(epg_source.id.to_string())
        .fetch_all(&self.pool)
        .await?;

        let mut results = Vec::new();
        for row in rows {
            let stream_source = StreamSource {
                id: parse_uuid_flexible(&row.get::<String, _>("id"))?,
                name: row.get("name"),
                source_type: StreamSourceType::Xtream,
                url: row.get("url"),
                max_concurrent_streams: row.get("max_concurrent_streams"),
                update_cron: row.get("update_cron"),
                username: row.get("username"),
                password: row.get("password"),
                field_map: row.get("field_map"),
                ignore_channel_numbers: row.get("ignore_channel_numbers"),
                created_at: row.get_datetime("created_at"),
                updated_at: row.get_datetime("updated_at"),
                last_ingested_at: row.get_datetime_opt("last_ingested_at"),
                is_active: row.get("is_active"),
            };
            results.push(stream_source);
        }

        Ok(results)
    }

    /// Find EPG sources with the same URL as the given stream source
    pub async fn find_linked_epg_sources(&self, stream_source: &StreamSource) -> RepositoryResult<Vec<EpgSource>> {
        if stream_source.source_type != StreamSourceType::Xtream {
            return Ok(Vec::new());
        }

        let rows = sqlx::query(
            "SELECT id, name, source_type, url, update_cron, username, password, original_timezone, 
             time_offset, created_at, updated_at, last_ingested_at, is_active 
             FROM epg_sources 
             WHERE source_type = 'xtream' AND url = ? AND id != ? AND is_active = 1"
        )
        .bind(&stream_source.url)
        .bind(stream_source.id.to_string())
        .fetch_all(&self.pool)
        .await?;

        let mut results = Vec::new();
        for row in rows {
            let epg_source = EpgSource {
                id: parse_uuid_flexible(&row.get::<String, _>("id"))?,
                name: row.get("name"),
                source_type: EpgSourceType::Xtream,
                url: row.get("url"),
                update_cron: row.get("update_cron"),
                username: row.get("username"),
                password: row.get("password"),
                original_timezone: row.get("original_timezone"),
                time_offset: row.get("time_offset"),
                created_at: row.get_datetime("created_at"),
                updated_at: row.get_datetime("updated_at"),
                last_ingested_at: row.get_datetime_opt("last_ingested_at"),
                is_active: row.get("is_active"),
            };
            results.push(epg_source);
        }

        Ok(results)
    }

    /// Auto-populate EPG source credentials from linked stream sources (URL-based)
    pub async fn auto_populate_epg_credentials(&self, epg_source_id: Uuid) -> RepositoryResult<Option<EpgSource>> {
        // First get the EPG source
        let epg_source_row = sqlx::query(
            "SELECT id, name, source_type, url, update_cron, username, password, original_timezone, 
             time_offset, created_at, updated_at, last_ingested_at, is_active 
             FROM epg_sources WHERE id = ?"
        )
        .bind(epg_source_id.to_string())
        .fetch_optional(&self.pool)
        .await?;

        let epg_source = match epg_source_row {
            Some(row) => EpgSource {
                id: parse_uuid_flexible(&row.get::<String, _>("id"))?,
                name: row.get("name"),
                source_type: EpgSourceType::Xtream,
                url: row.get("url"),
                update_cron: row.get("update_cron"),
                username: row.get("username"),
                password: row.get("password"),
                original_timezone: row.get("original_timezone"),
                time_offset: row.get("time_offset"),
                created_at: row.get_datetime("created_at"),
                updated_at: row.get_datetime("updated_at"),
                last_ingested_at: row.get_datetime_opt("last_ingested_at"),
                is_active: row.get("is_active"),
            },
            None => return Ok(None),
        };

        // Only handle Xtream sources without credentials
        if epg_source.source_type != EpgSourceType::Xtream 
            || (epg_source.username.is_some() && epg_source.password.is_some()) {
            return Ok(Some(epg_source));
        }

        // Find stream sources with the same URL
        let linked_stream_sources = self.find_linked_stream_sources(&epg_source).await?;
        
        // Look for a stream source with credentials
        for stream_source in &linked_stream_sources {
            if let (Some(username), Some(password)) = (&stream_source.username, &stream_source.password) {
                // Update EPG source with credentials from stream source
                let now = Utc::now();
                sqlx::query(
                    "UPDATE epg_sources SET username = ?, password = ?, updated_at = ? WHERE id = ?"
                )
                .bind(username)
                .bind(password)
                .bind(now.to_rfc3339())
                .bind(epg_source_id.to_string())
                .execute(&self.pool)
                .await?;

                info!(
                    "Auto-populated EPG source '{}' credentials from linked stream source '{}'",
                    epg_source.name, stream_source.name
                );

                // Return updated EPG source
                let updated_epg = EpgSource {
                    username: Some(username.clone()),
                    password: Some(password.clone()),
                    updated_at: now,
                    ..epg_source
                };

                return Ok(Some(updated_epg));
            }
        }

        Ok(Some(epg_source))
    }

    /// Update linked sources when a source's URL, username, or password changes
    pub async fn update_linked_sources(
        &self,
        source_id: Uuid,
        source_type: &str,
        url: Option<&String>,
        username: Option<&String>,
        password: Option<&String>,
        update_linked: bool,
    ) -> RepositoryResult<u64> {
        if !update_linked {
            debug!("update_linked=false, skipping linked source updates");
            return Ok(0);
        }

        let mut updated_count = 0;
        let now = Utc::now();

        match source_type {
            "stream" => {
                // Get the stream source first
                let stream_source_row = sqlx::query(
                    "SELECT id, name, source_type, url, max_concurrent_streams, update_cron, username, password, 
                     field_map, ignore_channel_numbers, created_at, updated_at, last_ingested_at, is_active 
                     FROM stream_sources WHERE id = ?"
                )
                .bind(source_id.to_string())
                .fetch_optional(&self.pool)
                .await?;

                if let Some(row) = stream_source_row {
                    let stream_source = StreamSource {
                        id: parse_uuid_flexible(&row.get::<String, _>("id"))?,
                        name: row.get("name"),
                        source_type: StreamSourceType::Xtream,
                        url: row.get("url"),
                        max_concurrent_streams: row.get("max_concurrent_streams"),
                        update_cron: row.get("update_cron"),
                        username: row.get("username"),
                        password: row.get("password"),
                        field_map: row.get("field_map"),
                        ignore_channel_numbers: row.get("ignore_channel_numbers"),
                        created_at: row.get_datetime("created_at"),
                        updated_at: row.get_datetime("updated_at"),
                        last_ingested_at: row.get_datetime_opt("last_ingested_at"),
                        is_active: row.get("is_active"),
                    };

                    // Update linked EPG sources
                    let linked_epg_sources = self.find_linked_epg_sources(&stream_source).await?;
                    
                    for epg_source in linked_epg_sources {
                        let mut needs_update = false;
                        let mut update_query = "UPDATE epg_sources SET updated_at = ?".to_string();
                        let mut bind_values: Vec<String> = vec![now.to_rfc3339()];

                        if let Some(update_url) = url {
                            if epg_source.url != **update_url {
                                update_query.push_str(", url = ?");
                                bind_values.push(update_url.clone());
                                needs_update = true;
                            }
                        }

                        if let Some(update_username) = username {
                            if update_username.is_empty() {
                                debug!("Skipping username update for linked EPG source '{}' - empty username provided", epg_source.name);
                            } else if epg_source.username.as_deref() != Some(update_username) {
                                update_query.push_str(", username = ?");
                                bind_values.push(update_username.clone());
                                needs_update = true;
                                debug!("Username will be updated for linked EPG source '{}'", epg_source.name);
                            }
                        }

                        // Only update password if explicitly provided and non-empty
                        if let Some(update_password) = password {
                            if update_password.is_empty() {
                                debug!("Skipping password update for linked EPG source '{}' - empty password provided", epg_source.name);
                            } else if epg_source.password.as_deref() != Some(update_password) {
                                update_query.push_str(", password = ?");
                                bind_values.push(update_password.clone());
                                needs_update = true;
                                debug!("Password will be updated for linked EPG source '{}'", epg_source.name);
                            }
                        } else {
                            debug!("Skipping password update for linked EPG source '{}' - no password provided", epg_source.name);
                        }

                        if needs_update {
                            update_query.push_str(" WHERE id = ?");
                            bind_values.push(epg_source.id.to_string());

                            let mut query = sqlx::query(&update_query);
                            for value in bind_values {
                                query = query.bind(value);
                            }

                            query.execute(&self.pool).await?;
                            updated_count += 1;

                            info!("Updated linked EPG source '{}' to match stream source changes", epg_source.name);
                        }
                    }
                }
            }
            "epg" => {
                // Get the EPG source first
                let epg_source_row = sqlx::query(
                    "SELECT id, name, source_type, url, update_cron, username, password, original_timezone, 
                     time_offset, created_at, updated_at, last_ingested_at, is_active 
                     FROM epg_sources WHERE id = ?"
                )
                .bind(source_id.to_string())
                .fetch_optional(&self.pool)
                .await?;

                if let Some(row) = epg_source_row {
                    let epg_source = EpgSource {
                        id: parse_uuid_flexible(&row.get::<String, _>("id"))?,
                        name: row.get("name"),
                        source_type: EpgSourceType::Xtream,
                        url: row.get("url"),
                        update_cron: row.get("update_cron"),
                        username: row.get("username"),
                        password: row.get("password"),
                        original_timezone: row.get("original_timezone"),
                        time_offset: row.get("time_offset"),
                        created_at: row.get_datetime("created_at"),
                        updated_at: row.get_datetime("updated_at"),
                        last_ingested_at: row.get_datetime_opt("last_ingested_at"),
                        is_active: row.get("is_active"),
                    };

                    // Update linked stream sources
                    let linked_stream_sources = self.find_linked_stream_sources(&epg_source).await?;
                    
                    for stream_source in linked_stream_sources {
                        let mut needs_update = false;
                        let mut update_query = "UPDATE stream_sources SET updated_at = ?".to_string();
                        let mut bind_values: Vec<String> = vec![now.to_rfc3339()];

                        if let Some(update_url) = url {
                            if stream_source.url != **update_url {
                                update_query.push_str(", url = ?");
                                bind_values.push(update_url.clone());
                                needs_update = true;
                            }
                        }

                        if let Some(update_username) = username {
                            if update_username.is_empty() {
                                debug!("Skipping username update for linked stream source '{}' - empty username provided", stream_source.name);
                            } else if stream_source.username.as_deref() != Some(update_username) {
                                update_query.push_str(", username = ?");
                                bind_values.push(update_username.clone());
                                needs_update = true;
                                debug!("Username will be updated for linked stream source '{}'", stream_source.name);
                            }
                        }

                        // Only update password if explicitly provided and non-empty
                        if let Some(update_password) = password {
                            if update_password.is_empty() {
                                debug!("Skipping password update for linked stream source '{}' - empty password provided", stream_source.name);
                            } else if stream_source.password.as_deref() != Some(update_password) {
                                update_query.push_str(", password = ?");
                                bind_values.push(update_password.clone());
                                needs_update = true;
                                debug!("Password will be updated for linked stream source '{}'", stream_source.name);
                            }
                        } else {
                            debug!("Skipping password update for linked stream source '{}' - no password provided", stream_source.name);
                        }

                        if needs_update {
                            update_query.push_str(" WHERE id = ?");
                            bind_values.push(stream_source.id.to_string());

                            let mut query = sqlx::query(&update_query);
                            for value in bind_values {
                                query = query.bind(value);
                            }

                            query.execute(&self.pool).await?;
                            updated_count += 1;

                            info!("Updated linked stream source '{}' to match EPG source changes", stream_source.name);
                        }
                    }
                }
            }
            _ => {
                debug!("Unknown source type '{}', skipping linked updates", source_type);
            }
        }

        Ok(updated_count)
    }
}