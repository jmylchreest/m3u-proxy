use anyhow::Result;
use chrono::{DateTime, Utc};
use sqlx::Row;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use super::Database;
use crate::models::*;

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

            sources.push(StreamSourceWithStats {
                source,
                channel_count,
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

    pub async fn create_stream_source(
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
    ) -> Result<()> {
        info!(
            "Updating {} channels for source ({})",
            channels.len(),
            source_id
        );

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

        // For very large channel sets, use chunked transactions
        const CHUNK_SIZE: usize = 5000;
        let mut inserted_count = 0;

        if channels.len() > CHUNK_SIZE {
            info!(
                "Processing {} channels in chunks of {}",
                channels.len(),
                CHUNK_SIZE
            );

            // Commit initial transaction (just the delete)
            tx.commit().await?;

            for (chunk_idx, chunk) in channels.chunks(CHUNK_SIZE).enumerate() {
                // Start new transaction for each chunk
                let mut chunk_tx = self.pool.begin().await?;

                for channel in chunk {
                    sqlx::query(
                        "INSERT INTO channels
                         (id, source_id, tvg_id, tvg_name, tvg_logo, group_title, channel_name, stream_url, created_at, updated_at)
                         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
                    )
                    .bind(channel.id.to_string())
                    .bind(source_id.to_string())
                    .bind(&channel.tvg_id)
                    .bind(&channel.tvg_name)
                    .bind(&channel.tvg_logo)
                    .bind(&channel.group_title)
                    .bind(&channel.channel_name)
                    .bind(&channel.stream_url)
                    .bind(channel.created_at.to_rfc3339())
                    .bind(channel.updated_at.to_rfc3339())
                    .execute(&mut *chunk_tx)
                    .await
                    .map_err(|e| {
                        error!("Failed to insert channel '{}' for source ({}): {}", channel.channel_name, source_id, e);
                        e
                    })?;

                    inserted_count += 1;
                }

                // Commit chunk
                chunk_tx.commit().await?;

                // Log progress every 10 chunks
                if chunk_idx % 10 == 0
                    || chunk_idx == (channels.len() + CHUNK_SIZE - 1) / CHUNK_SIZE - 1
                {
                    info!(
                        "Inserted {}/{} channels for source ({})",
                        inserted_count,
                        channels.len(),
                        source_id
                    );
                }
            }
        } else {
            // For smaller sets, use single transaction
            for channel in channels {
                sqlx::query(
                    "INSERT INTO channels
                     (id, source_id, tvg_id, tvg_name, tvg_logo, group_title, channel_name, stream_url, created_at, updated_at)
                     VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
                )
                .bind(channel.id.to_string())
                .bind(source_id.to_string())
                .bind(&channel.tvg_id)
                .bind(&channel.tvg_name)
                .bind(&channel.tvg_logo)
                .bind(&channel.group_title)
                .bind(&channel.channel_name)
                .bind(&channel.stream_url)
                .bind(channel.created_at.to_rfc3339())
                .bind(channel.updated_at.to_rfc3339())
                .execute(&mut *tx)
                .await
                .map_err(|e| {
                    error!("Failed to insert channel '{}' for source ({}): {}", channel.channel_name, source_id, e);
                    e
                })?;

                inserted_count += 1;
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

    pub async fn update_source_last_ingested(&self, source_id: Uuid) -> Result<()> {
        let now = chrono::Utc::now();
        sqlx::query("UPDATE stream_sources SET last_ingested_at = ? WHERE id = ?")
            .bind(now.to_rfc3339())
            .bind(source_id.to_string())
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    #[allow(dead_code)]
    pub async fn get_source_channel_count(&self, source_id: Uuid) -> Result<i64> {
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM channels WHERE source_id = ?")
            .bind(source_id.to_string())
            .fetch_one(&self.pool)
            .await?;

        Ok(count)
    }

    pub async fn get_source_channels(&self, source_id: Uuid) -> Result<Vec<Channel>> {
        let rows = sqlx::query(
            "SELECT id, source_id, tvg_id, tvg_name, tvg_logo, group_title, channel_name, stream_url, created_at, updated_at
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

        // Build the WHERE clause with filtering
        let (where_clause, count_where_clause) = if let Some(filter_text) = filter {
            let filter_pattern = format!("%{}%", filter_text);
            (
                "WHERE source_id = ? AND (
                    channel_name LIKE ? OR
                    tvg_id LIKE ? OR
                    tvg_name LIKE ? OR
                    group_title LIKE ? OR
                    stream_url LIKE ?
                ) ORDER BY channel_name LIMIT ? OFFSET ?",
                "WHERE source_id = ? AND (
                    channel_name LIKE ? OR
                    tvg_id LIKE ? OR
                    tvg_name LIKE ? OR
                    group_title LIKE ? OR
                    stream_url LIKE ?
                )",
            )
        } else {
            (
                "WHERE source_id = ? ORDER BY channel_name LIMIT ? OFFSET ?",
                "WHERE source_id = ?",
            )
        };

        // Get total count first
        let total_count: i64 = if let Some(filter_text) = filter {
            let filter_pattern = format!("%{}%", filter_text);
            sqlx::query_scalar(&format!(
                "SELECT COUNT(*) FROM channels {}",
                count_where_clause
            ))
            .bind(source_id.to_string())
            .bind(&filter_pattern)
            .bind(&filter_pattern)
            .bind(&filter_pattern)
            .bind(&filter_pattern)
            .bind(&filter_pattern)
            .fetch_one(&self.pool)
            .await?
        } else {
            sqlx::query_scalar(&format!(
                "SELECT COUNT(*) FROM channels {}",
                count_where_clause
            ))
            .bind(source_id.to_string())
            .fetch_one(&self.pool)
            .await?
        };

        // Get the paginated results
        let rows = if let Some(filter_text) = filter {
            let filter_pattern = format!("%{}%", filter_text);
            sqlx::query(&format!(
                "SELECT id, source_id, tvg_id, tvg_name, tvg_logo, group_title, channel_name, stream_url, created_at, updated_at
                 FROM channels {}", where_clause
            ))
            .bind(source_id.to_string())
            .bind(&filter_pattern)
            .bind(&filter_pattern)
            .bind(&filter_pattern)
            .bind(&filter_pattern)
            .bind(&filter_pattern)
            .bind(limit as i64)
            .bind(offset as i64)
            .fetch_all(&self.pool)
            .await?
        } else {
            sqlx::query(&format!(
                "SELECT id, source_id, tvg_id, tvg_name, tvg_logo, group_title, channel_name, stream_url, created_at, updated_at
                 FROM channels {}", where_clause
            ))
            .bind(source_id.to_string())
            .bind(limit as i64)
            .bind(offset as i64)
            .fetch_all(&self.pool)
            .await?
        };

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
}
