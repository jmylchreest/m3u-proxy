//! URL-based source linking
//!
//! This module provides URL-based linking between stream and EPG sources,
//! eliminating the need for a separate linked_xtream_sources table.

use crate::models::*;
use anyhow::Result;
use sqlx::Row;
use tracing::{info, debug};
use uuid::Uuid;

/// URL-based linking implementation for Database
impl super::Database {
    /// Find stream sources with the same URL as the given source
    pub async fn find_linked_stream_sources(&self, epg_source: &EpgSource) -> Result<Vec<StreamSource>> {
        if epg_source.source_type != EpgSourceType::Xtream {
            return Ok(Vec::new());
        }

        let stream_sources = sqlx::query(
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
        for row in stream_sources {
            let stream_source = StreamSource {
                id: crate::utils::uuid_parser::parse_uuid_flexible(&row.get::<String, _>("id"))?,
                name: row.get("name"),
                source_type: StreamSourceType::Xtream,
                url: row.get("url"),
                max_concurrent_streams: row.get("max_concurrent_streams"),
                update_cron: row.get("update_cron"),
                username: row.get("username"),
                password: row.get("password"),
                field_map: row.get("field_map"),
                ignore_channel_numbers: row.get("ignore_channel_numbers"),
                created_at: crate::utils::datetime::DateTimeParser::parse_flexible(&row.get::<String, _>("created_at")).unwrap(),
                updated_at: crate::utils::datetime::DateTimeParser::parse_flexible(&row.get::<String, _>("updated_at")).unwrap(),
                last_ingested_at: row.get::<Option<String>, _>("last_ingested_at")
                    .map(|s| crate::utils::datetime::DateTimeParser::parse_flexible(&s).ok()).flatten(),
                is_active: row.get("is_active"),
            };
            results.push(stream_source);
        }

        Ok(results)
    }

    /// Find EPG sources with the same URL as the given source
    pub async fn find_linked_epg_sources(&self, stream_source: &StreamSource) -> Result<Vec<EpgSource>> {
        if stream_source.source_type != StreamSourceType::Xtream {
            return Ok(Vec::new());
        }

        let epg_sources = sqlx::query(
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
        for row in epg_sources {
            let epg_source = EpgSource {
                id: crate::utils::uuid_parser::parse_uuid_flexible(&row.get::<String, _>("id"))?,
                name: row.get("name"),
                source_type: EpgSourceType::Xtream,
                url: row.get("url"),
                update_cron: row.get("update_cron"),
                username: row.get("username"),
                password: row.get("password"),
                original_timezone: row.get("original_timezone"),
                time_offset: row.get("time_offset"),
                created_at: crate::utils::datetime::DateTimeParser::parse_flexible(&row.get::<String, _>("created_at")).unwrap(),
                updated_at: crate::utils::datetime::DateTimeParser::parse_flexible(&row.get::<String, _>("updated_at")).unwrap(),
                last_ingested_at: row.get::<Option<String>, _>("last_ingested_at")
                    .map(|s| crate::utils::datetime::DateTimeParser::parse_flexible(&s).ok()).flatten(),
                is_active: row.get("is_active"),
            };
            results.push(epg_source);
        }

        Ok(results)
    }

    /// Auto-populate EPG source credentials from linked stream sources (URL-based)
    pub async fn auto_populate_epg_credentials(&self, epg_source_id: Uuid) -> Result<Option<EpgSource>> {
        let epg_source = match self.get_epg_source(epg_source_id).await? {
            Some(source) => source,
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
                let now = chrono::Utc::now();
                sqlx::query(
                    "UPDATE epg_sources SET username = ?, password = ?, updated_at = ? WHERE id = ?"
                )
                .bind(username)
                .bind(password)
                .bind(now.format("%Y-%m-%d %H:%M:%S").to_string())
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
    pub async fn update_linked_sources(&self, source_id: Uuid, source_type: &str, update_request: &dyn LinkedUpdateRequest, update_linked: bool) -> Result<usize> {
        if !update_linked {
            debug!("update_linked=false, skipping linked source updates");
            return Ok(0);
        }

        let mut updated_count = 0;
        let now = chrono::Utc::now();

        match source_type {
            "stream" => {
                // Update linked EPG sources
                if let Some(stream_source) = self.get_stream_source(source_id).await? {
                    let linked_epg_sources = self.find_linked_epg_sources(&stream_source).await?;
                    
                    for epg_source in linked_epg_sources {
                        let mut needs_update = false;
                        let mut update_query = "UPDATE epg_sources SET updated_at = ?".to_string();
                        let mut bind_values: Vec<String> = vec![now.format("%Y-%m-%d %H:%M:%S").to_string()];

                        if let Some(url) = update_request.get_url() {
                            if epg_source.url != *url {
                                update_query.push_str(", url = ?");
                                bind_values.push(url.clone());
                                needs_update = true;
                            }
                        }

                        if let Some(username) = update_request.get_username() {
                            if epg_source.username.as_deref() != Some(username) {
                                update_query.push_str(", username = ?");
                                bind_values.push(username.clone());
                                needs_update = true;
                            }
                        }

                        if let Some(password) = update_request.get_password() {
                            if epg_source.password.as_deref() != Some(password) {
                                update_query.push_str(", password = ?");
                                bind_values.push(password.clone());
                                needs_update = true;
                            }
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
                // Update linked stream sources
                if let Some(epg_source) = self.get_epg_source(source_id).await? {
                    let linked_stream_sources = self.find_linked_stream_sources(&epg_source).await?;
                    
                    for stream_source in linked_stream_sources {
                        let mut needs_update = false;
                        let mut update_query = "UPDATE stream_sources SET updated_at = ?".to_string();
                        let mut bind_values: Vec<String> = vec![now.format("%Y-%m-%d %H:%M:%S").to_string()];

                        if let Some(url) = update_request.get_url() {
                            if stream_source.url != *url {
                                update_query.push_str(", url = ?");
                                bind_values.push(url.clone());
                                needs_update = true;
                            }
                        }

                        if let Some(username) = update_request.get_username() {
                            if stream_source.username.as_deref() != Some(username) {
                                update_query.push_str(", username = ?");
                                bind_values.push(username.clone());
                                needs_update = true;
                            }
                        }

                        if let Some(password) = update_request.get_password() {
                            if stream_source.password.as_deref() != Some(password) {
                                update_query.push_str(", password = ?");
                                bind_values.push(password.clone());
                                needs_update = true;
                            }
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

/// Trait for update requests that can affect linked sources
pub trait LinkedUpdateRequest: Send + Sync {
    fn get_url(&self) -> Option<&String>;
    fn get_username(&self) -> Option<&String>;
    fn get_password(&self) -> Option<&String>;
}

/// Implement for StreamSourceUpdateRequest
impl LinkedUpdateRequest for StreamSourceUpdateRequest {
    fn get_url(&self) -> Option<&String> {
        Some(&self.url)
    }

    fn get_username(&self) -> Option<&String> {
        self.username.as_ref()
    }

    fn get_password(&self) -> Option<&String> {
        self.password.as_ref()
    }
}

/// Implement for EpgSourceUpdateRequest  
impl LinkedUpdateRequest for EpgSourceUpdateRequest {
    fn get_url(&self) -> Option<&String> {
        Some(&self.url)
    }

    fn get_username(&self) -> Option<&String> {
        self.username.as_ref()
    }

    fn get_password(&self) -> Option<&String> {
        self.password.as_ref()
    }
}