use crate::models::*;
use anyhow::Result;
use sqlx::Row;
use tracing::{info, warn};
use uuid::Uuid;

impl crate::database::Database {
    pub async fn create_linked_xtream_sources(
        &self,
        request: &XtreamCodesCreateRequest,
    ) -> Result<XtreamCodesCreateResponse> {
        let mut tx = self.pool.begin().await?;
        let link_id = Uuid::new_v4();
        let now = chrono::Utc::now();

        let mut stream_source = None;
        let mut epg_source = None;

        // Create stream source if requested
        if request.create_stream_source {
            let stream_request = StreamSourceCreateRequest {
                name: format!("{} (Stream)", request.name),
                source_type: StreamSourceType::Xtream,
                url: request.url.clone(),
                max_concurrent_streams: request.max_concurrent_streams,
                update_cron: request.update_cron.clone(),
                username: Some(request.username.clone()),
                password: Some(request.password.clone()),
                field_map: None,
            };

            match self.create_stream_source_tx(&mut tx, &stream_request).await {
                Ok(source) => {
                    info!(
                        "Created linked stream source: {} ({})",
                        source.name, source.id
                    );
                    stream_source = Some(source);
                }
                Err(e) => {
                    warn!("Failed to create stream source for Xtream: {}", e);
                }
            }
        }

        // Create EPG source if requested
        if request.create_epg_source {
            let epg_request = EpgSourceCreateRequest {
                name: request.name.clone(),
                source_type: EpgSourceType::Xtream,
                url: request.url.clone(),
                update_cron: request.update_cron.clone(),
                username: Some(request.username.clone()),
                password: Some(request.password.clone()),
                timezone: request.timezone.clone(), // This will be mapped to original_timezone
                time_offset: request.time_offset.clone(),
            };

            match self.create_epg_source_tx(&mut tx, &epg_request).await {
                Ok(source) => {
                    info!("Created linked EPG source: {} ({})", source.name, source.id);
                    epg_source = Some(source);
                }
                Err(e) => {
                    warn!("Failed to create EPG source for Xtream: {}", e);
                }
            }
        }

        // Insert the linking record
        let linked_id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO linked_xtream_sources (id, link_id, name, url, username, password, stream_source_id, epg_source_id, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(linked_id.to_string())
        .bind(link_id.to_string())
        .bind(&request.name)
        .bind(&request.url)
        .bind(&request.username)
        .bind(&request.password)
        .bind(stream_source.as_ref().map(|s| s.id.to_string()))
        .bind(epg_source.as_ref().map(|s| s.id.to_string()))
        .bind(now.format("%Y-%m-%d %H:%M:%S").to_string())
        .bind(now.format("%Y-%m-%d %H:%M:%S").to_string())
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;

        let success = stream_source.is_some() || epg_source.is_some();
        let message = match (stream_source.is_some(), epg_source.is_some()) {
            (true, true) => format!(
                "Successfully created both stream and EPG sources for '{}'",
                request.name
            ),
            (true, false) => format!(
                "Successfully created stream source for '{}' (EPG source creation failed)",
                request.name
            ),
            (false, true) => format!(
                "Successfully created EPG source for '{}' (stream source creation failed)",
                request.name
            ),
            (false, false) => format!("Failed to create sources for '{}'", request.name),
        };

        Ok(XtreamCodesCreateResponse {
            success,
            message,
            stream_source,
            epg_source,
        })
    }

    pub async fn list_linked_xtream_sources(&self) -> Result<Vec<LinkedXtreamSources>> {
        let rows = sqlx::query(
            "SELECT l.id, l.link_id, l.name, l.url, l.username, l.password,
                    l.stream_source_id, l.epg_source_id, l.created_at, l.updated_at
             FROM linked_xtream_sources l
             ORDER BY l.name",
        )
        .fetch_all(&self.pool)
        .await?;

        let mut linked_sources = Vec::new();

        for row in rows {
            let created_at = row.get::<String, _>("created_at");
            let updated_at = row.get::<String, _>("updated_at");
            let link_id_str = row.get::<String, _>("link_id");
            let stream_source_id_str = row.get::<Option<String>, _>("stream_source_id");
            let epg_source_id_str = row.get::<Option<String>, _>("epg_source_id");

            // Get stream source if exists
            let stream_source = if let Some(id_str) = stream_source_id_str {
                if let Ok(id) = Uuid::parse_str(&id_str) {
                    self.get_stream_source(id).await.unwrap_or(None)
                } else {
                    None
                }
            } else {
                None
            };

            // Get EPG source if exists
            let epg_source = if let Some(id_str) = epg_source_id_str {
                if let Ok(id) = Uuid::parse_str(&id_str) {
                    self.get_epg_source(id).await.unwrap_or(None)
                } else {
                    None
                }
            } else {
                None
            };

            linked_sources.push(LinkedXtreamSources {
                stream_source,
                epg_source,
                link_id: Uuid::parse_str(&link_id_str)?,
                created_at: crate::utils::datetime::DateTimeParser::parse_flexible(&created_at)
                    .map_err(|_| anyhow::anyhow!("Failed to parse created_at"))?,
                updated_at: crate::utils::datetime::DateTimeParser::parse_flexible(&updated_at)
                    .map_err(|_| anyhow::anyhow!("Failed to parse updated_at"))?,
            });
        }

        Ok(linked_sources)
    }

    pub async fn get_linked_xtream_source(
        &self,
        link_id: &str,
    ) -> Result<Option<LinkedXtreamSources>> {
        let row = sqlx::query(
            "SELECT l.id, l.link_id, l.name, l.url, l.username, l.password,
                    l.stream_source_id, l.epg_source_id, l.created_at, l.updated_at
             FROM linked_xtream_sources l
             WHERE l.link_id = ?",
        )
        .bind(link_id)
        .fetch_optional(&self.pool)
        .await?;

        if let Some(row) = row {
            let stream_source_id: Option<Uuid> = row.try_get("stream_source_id")?;
            let epg_source_id: Option<Uuid> = row.try_get("epg_source_id")?;

            let mut stream_source = None;
            let mut epg_source = None;

            // Get stream source if linked
            if let Some(source_id) = stream_source_id {
                stream_source = self.get_stream_source(source_id).await?;
            }

            // Get EPG source if linked
            if let Some(source_id) = epg_source_id {
                epg_source = self.get_epg_source(source_id).await?;
            }

            Ok(Some(LinkedXtreamSources {
                stream_source,
                epg_source,
                link_id: row.try_get("link_id")?,
                created_at: crate::utils::datetime::DateTimeParser::parse_flexible(
                    &row.try_get::<String, _>("created_at")?,
                )
                .map_err(|_| anyhow::anyhow!("Failed to parse created_at"))?,
                updated_at: crate::utils::datetime::DateTimeParser::parse_flexible(
                    &row.try_get::<String, _>("updated_at")?,
                )
                .map_err(|_| anyhow::anyhow!("Failed to parse updated_at"))?,
            }))
        } else {
            Ok(None)
        }
    }

    pub async fn update_linked_xtream_sources(
        &self,
        link_id: &str,
        request: &XtreamCodesUpdateRequest,
    ) -> Result<bool> {
        let mut tx = self.pool.begin().await?;

        // Get the linked record
        let linked_row = sqlx::query(
            "SELECT stream_source_id, epg_source_id FROM linked_xtream_sources WHERE link_id = ?",
        )
        .bind(link_id.to_string())
        .fetch_optional(&mut *tx)
        .await?;

        if linked_row.is_none() {
            return Ok(false);
        }

        let row = linked_row.unwrap();
        let stream_source_id_str = row.get::<Option<String>, _>("stream_source_id");
        let epg_source_id_str = row.get::<Option<String>, _>("epg_source_id");

        // Update stream source if exists
        if let Some(id_str) = stream_source_id_str {
            if let Ok(id) = Uuid::parse_str(&id_str) {
                let stream_update = StreamSourceUpdateRequest {
                    name: format!("{} (Stream)", request.name),
                    source_type: StreamSourceType::Xtream,
                    url: request.url.clone(),
                    max_concurrent_streams: request.max_concurrent_streams,
                    update_cron: request.update_cron.clone(),
                    username: Some(request.username.clone()),
                    password: Some(request.password.clone()),
                    field_map: None,
                    is_active: request.is_active,
                };

                let _ = self
                    .update_stream_source_tx(&mut tx, id, &stream_update)
                    .await;
            }
        }

        // Update EPG source if exists
        if let Some(id_str) = epg_source_id_str {
            if let Ok(id) = Uuid::parse_str(&id_str) {
                let epg_update = EpgSourceUpdateRequest {
                    name: request.name.clone(),
                    source_type: EpgSourceType::Xtream,
                    url: request.url.clone(),
                    update_cron: request.update_cron.clone(),
                    username: Some(request.username.clone()),
                    password: Some(request.password.clone()),
                    timezone: Some(request.timezone.clone()), // This will be mapped to original_timezone
                    time_offset: Some(request.time_offset.clone()),
                    is_active: request.is_active,
                };

                let _ = self.update_epg_source_tx(&mut tx, id, &epg_update).await;
            }
        }

        // Update the linked record
        sqlx::query(
            "UPDATE linked_xtream_sources
             SET name = ?, url = ?, username = ?, password = ?
             WHERE link_id = ?",
        )
        .bind(&request.name)
        .bind(&request.url)
        .bind(&request.username)
        .bind(&request.password)
        .bind(link_id.to_string())
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;

        info!(
            "Updated linked Xtream sources for: {} ({})",
            request.name, link_id
        );
        Ok(true)
    }

    pub async fn delete_linked_xtream_sources(&self, link_id: &str) -> Result<bool> {
        let mut tx = self.pool.begin().await?;

        // Get the linked record
        let linked_row = sqlx::query(
            "SELECT stream_source_id, epg_source_id FROM linked_xtream_sources WHERE link_id = ?",
        )
        .bind(link_id.to_string())
        .fetch_optional(&mut *tx)
        .await?;

        if linked_row.is_none() {
            return Ok(false);
        }

        let row = linked_row.unwrap();
        let stream_source_id_str = row.get::<Option<String>, _>("stream_source_id");
        let epg_source_id_str = row.get::<Option<String>, _>("epg_source_id");

        // Delete stream source if exists
        if let Some(id_str) = stream_source_id_str {
            if let Ok(id) = Uuid::parse_str(&id_str) {
                let _ = self.delete_stream_source_tx(&mut tx, id).await;
            }
        }

        // Delete EPG source if exists
        if let Some(id_str) = epg_source_id_str {
            if let Ok(id) = Uuid::parse_str(&id_str) {
                let _ = self.delete_epg_source_tx(&mut tx, id).await;
            }
        }

        // Delete the linked record
        let result = sqlx::query("DELETE FROM linked_xtream_sources WHERE link_id = ?")
            .bind(link_id.to_string())
            .execute(&mut *tx)
            .await?;

        tx.commit().await?;

        let deleted = result.rows_affected() > 0;
        if deleted {
            info!("Deleted linked Xtream sources: {}", link_id);
        }

        Ok(deleted)
    }

    // Helper methods for transaction-based operations
    async fn create_stream_source_tx(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        source: &StreamSourceCreateRequest,
    ) -> Result<StreamSource> {
        let id = Uuid::new_v4();
        let now = chrono::Utc::now();

        let source_type_str = match source.source_type {
            StreamSourceType::M3u => "m3u",
            StreamSourceType::Xtream => "xtream",
        };

        sqlx::query(
            "INSERT INTO stream_sources (id, name, source_type, url, max_concurrent_streams, update_cron, username, password, field_map, created_at, updated_at, is_active)
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
        .bind(now.format("%Y-%m-%d %H:%M:%S").to_string())
        .bind(now.format("%Y-%m-%d %H:%M:%S").to_string())
        .bind(true)
        .execute(&mut **tx)
        .await?;

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

    async fn create_epg_source_tx(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        source: &EpgSourceCreateRequest,
    ) -> Result<EpgSource> {
        let id = Uuid::new_v4();
        let now = chrono::Utc::now();

        let source_type_str = match source.source_type {
            EpgSourceType::Xmltv => "xmltv",
            EpgSourceType::Xtream => "xtream",
        };

        let time_offset = source.time_offset.as_deref().unwrap_or("0");

        sqlx::query(
            "INSERT INTO epg_sources (id, name, source_type, url, update_cron, username, password, time_offset, created_at, updated_at, is_active)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(id.to_string())
        .bind(&source.name)
        .bind(source_type_str)
        .bind(&source.url)
        .bind(&source.update_cron)
        .bind(&source.username)
        .bind(&source.password)
        .bind(time_offset)
        .bind(now.format("%Y-%m-%d %H:%M:%S").to_string())
        .bind(now.format("%Y-%m-%d %H:%M:%S").to_string())
        .bind(true)
        .execute(&mut **tx)
        .await?;

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

    async fn update_stream_source_tx(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        id: Uuid,
        source: &StreamSourceUpdateRequest,
    ) -> Result<bool> {
        let source_type_str = match source.source_type {
            StreamSourceType::M3u => "m3u",
            StreamSourceType::Xtream => "xtream",
        };

        let result = sqlx::query(
            "UPDATE stream_sources
             SET name = ?, source_type = ?, url = ?, max_concurrent_streams = ?, update_cron = ?, username = ?, password = ?, field_map = ?, is_active = ?
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
        .bind(source.is_active)
        .bind(id.to_string())
        .execute(&mut **tx)
        .await?;

        Ok(result.rows_affected() > 0)
    }

    async fn update_epg_source_tx(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        id: Uuid,
        source: &EpgSourceUpdateRequest,
    ) -> Result<bool> {
        let source_type_str = match source.source_type {
            EpgSourceType::Xmltv => "xmltv",
            EpgSourceType::Xtream => "xtream",
        };

        let result = sqlx::query(
            "UPDATE epg_sources
             SET name = ?, source_type = ?, url = ?, update_cron = ?, username = ?, password = ?, original_timezone = ?, time_offset = ?, is_active = ?
             WHERE id = ?",
        )
        .bind(&source.name)
        .bind(source_type_str)
        .bind(&source.url)
        .bind(&source.update_cron)
        .bind(&source.username)
        .bind(&source.password)
        .bind(&source.timezone)
        .bind(&source.time_offset)
        .bind(source.is_active)
        .bind(id.to_string())
        .execute(&mut **tx)
        .await?;

        Ok(result.rows_affected() > 0)
    }

    async fn delete_stream_source_tx(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        id: Uuid,
    ) -> Result<bool> {
        let result = sqlx::query("DELETE FROM stream_sources WHERE id = ?")
            .bind(id.to_string())
            .execute(&mut **tx)
            .await?;

        Ok(result.rows_affected() > 0)
    }

    async fn delete_epg_source_tx(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        id: Uuid,
    ) -> Result<bool> {
        let result = sqlx::query("DELETE FROM epg_sources WHERE id = ?")
            .bind(id.to_string())
            .execute(&mut **tx)
            .await?;

        Ok(result.rows_affected() > 0)
    }

    /// Find linked EPG source by stream source ID
    pub async fn find_linked_epg_by_stream_id(
        &self,
        stream_source_id: Uuid,
    ) -> Result<Option<crate::models::EpgSource>> {
        let row = sqlx::query(
            "SELECT es.id, es.name, es.source_type, es.url, es.update_cron, es.username, es.password,
             es.original_timezone, es.time_offset, es.created_at, es.updated_at,
             es.last_ingested_at, es.is_active FROM epg_sources es
             JOIN linked_xtream_sources lxs ON es.id = lxs.epg_source_id
             WHERE lxs.stream_source_id = ?",
        )
        .bind(stream_source_id.to_string())
        .fetch_optional(&self.pool)
        .await?;

        if let Some(row) = row {
            let time_offset = row.get::<String, _>("time_offset");
            let created_at = crate::utils::datetime::DateTimeParser::parse_flexible(
                &row.get::<String, _>("created_at"),
            )
            .map_err(|_| anyhow::anyhow!("Failed to parse created_at"))?;
            let updated_at = crate::utils::datetime::DateTimeParser::parse_flexible(
                &row.get::<String, _>("updated_at"),
            )
            .map_err(|_| anyhow::anyhow!("Failed to parse updated_at"))?;
            let last_ingested_at = row
                .get::<Option<String>, _>("last_ingested_at")
                .map(|s| {
                    crate::utils::datetime::DateTimeParser::parse_flexible(&s)
                        .map_err(|_| anyhow::anyhow!("Failed to parse datetime"))
                })
                .transpose()?;

            Ok(Some(crate::models::EpgSource {
                id: Uuid::parse_str(&row.get::<String, _>("id"))?,
                name: row.get("name"),
                source_type: row.get::<String, _>("source_type").parse()?,
                url: row.get("url"),
                update_cron: row.get("update_cron"),
                username: row.get("username"),
                password: row.get("password"),
                original_timezone: row.get("original_timezone"),
                time_offset,
                created_at,
                updated_at,
                last_ingested_at,
                is_active: row.get("is_active"),
            }))
        } else {
            Ok(None)
        }
    }

    /// Check if a linked Xtream source already exists with the same credentials
    pub async fn find_existing_linked_xtream(
        &self,
        url: &str,
        username: &str,
        password: &str,
    ) -> Result<Option<LinkedXtreamSources>> {
        let row = sqlx::query(
            "SELECT l.id, l.link_id, l.name, l.url, l.username, l.password,
                    l.stream_source_id, l.epg_source_id, l.created_at, l.updated_at
             FROM linked_xtream_sources l
             WHERE l.url = ? AND l.username = ? AND l.password = ?",
        )
        .bind(url)
        .bind(username)
        .bind(password)
        .fetch_optional(&self.pool)
        .await?;

        if let Some(row) = row {
            let stream_source_id_str = row.get::<Option<String>, _>("stream_source_id");
            let epg_source_id_str = row.get::<Option<String>, _>("epg_source_id");

            // Get stream source if exists
            let stream_source = if let Some(id_str) = stream_source_id_str {
                if let Ok(id) = Uuid::parse_str(&id_str) {
                    self.get_stream_source(id).await.unwrap_or(None)
                } else {
                    None
                }
            } else {
                None
            };

            // Get EPG source if exists
            let epg_source = if let Some(id_str) = epg_source_id_str {
                if let Ok(id) = Uuid::parse_str(&id_str) {
                    self.get_epg_source(id).await.unwrap_or(None)
                } else {
                    None
                }
            } else {
                None
            };

            Ok(Some(LinkedXtreamSources {
                stream_source,
                epg_source,
                link_id: Uuid::parse_str(&row.get::<String, _>("link_id"))?,
                created_at: crate::utils::datetime::DateTimeParser::parse_flexible(
                    &row.get::<String, _>("created_at")
                ).map_err(|_| anyhow::anyhow!("Failed to parse created_at"))?,
                updated_at: crate::utils::datetime::DateTimeParser::parse_flexible(
                    &row.get::<String, _>("updated_at")
                ).map_err(|_| anyhow::anyhow!("Failed to parse updated_at"))?,
            }))
        } else {
            Ok(None)
        }
    }

    /// Automatically link a stream source to an existing EPG source with same credentials
    pub async fn auto_link_stream_source(&self, stream_source: &StreamSource) -> Result<bool> {
        // Only auto-link Xtream sources
        if stream_source.source_type != StreamSourceType::Xtream {
            return Ok(false);
        }

        // Check if stream source has credentials
        let username = match &stream_source.username {
            Some(u) => u,
            None => return Ok(false),
        };
        let password = match &stream_source.password {
            Some(p) => p,
            None => return Ok(false),
        };

        // Check if there's already a linked Xtream source with these credentials
        if let Some(mut linked) = self.find_existing_linked_xtream(&stream_source.url, username, password).await? {
            // If there's already a stream source linked, don't create another
            if linked.stream_source.is_some() {
                return Ok(false);
            }

            // Update the existing linked record to include this stream source
            let mut tx = self.pool.begin().await?;
            sqlx::query(
                "UPDATE linked_xtream_sources SET stream_source_id = ? WHERE link_id = ?",
            )
            .bind(stream_source.id.to_string())
            .bind(linked.link_id.to_string())
            .execute(&mut *tx)
            .await?;

            tx.commit().await?;

            info!(
                "Auto-linked stream source '{}' to existing EPG source with same credentials",
                stream_source.name
            );
            return Ok(true);
        }

        // Check if there's an EPG source with the same credentials (not yet linked)
        let epg_sources = sqlx::query(
            "SELECT id, name FROM epg_sources 
             WHERE source_type = 'xtream' AND url = ? AND username = ? AND password = ? AND is_active = 1",
        )
        .bind(&stream_source.url)
        .bind(username)
        .bind(password)
        .fetch_all(&self.pool)
        .await?;

        if let Some(epg_row) = epg_sources.first() {
            // Create a new linked record
            let link_id = Uuid::new_v4();
            let linked_id = Uuid::new_v4();
            let now = chrono::Utc::now();

            let mut tx = self.pool.begin().await?;
            sqlx::query(
                "INSERT INTO linked_xtream_sources (id, link_id, name, url, username, password, stream_source_id, epg_source_id, created_at, updated_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(linked_id.to_string())
            .bind(link_id.to_string())
            .bind(&stream_source.name.replace(" (Stream)", "")) // Remove suffix if present
            .bind(&stream_source.url)
            .bind(username)
            .bind(password)
            .bind(stream_source.id.to_string())
            .bind(epg_row.get::<String, _>("id"))
            .bind(now.format("%Y-%m-%d %H:%M:%S").to_string())
            .bind(now.format("%Y-%m-%d %H:%M:%S").to_string())
            .execute(&mut *tx)
            .await?;

            tx.commit().await?;

            info!(
                "Auto-linked stream source '{}' with EPG source '{}' (same Xtream credentials)",
                stream_source.name,
                epg_row.get::<String, _>("name")
            );
            return Ok(true);
        }

        Ok(false)
    }

    /// Automatically link an EPG source to an existing stream source with same credentials  
    pub async fn auto_link_epg_source(&self, epg_source: &EpgSource) -> Result<bool> {
        // Only auto-link Xtream sources
        if epg_source.source_type != EpgSourceType::Xtream {
            return Ok(false);
        }

        // Check if EPG source has credentials
        let username = match &epg_source.username {
            Some(u) => u,
            None => return Ok(false),
        };
        let password = match &epg_source.password {
            Some(p) => p,
            None => return Ok(false),
        };

        // Check if there's already a linked Xtream source with these credentials
        if let Some(mut linked) = self.find_existing_linked_xtream(&epg_source.url, username, password).await? {
            // If there's already an EPG source linked, don't create another
            if linked.epg_source.is_some() {
                return Ok(false);
            }

            // Update the existing linked record to include this EPG source
            let mut tx = self.pool.begin().await?;
            sqlx::query(
                "UPDATE linked_xtream_sources SET epg_source_id = ? WHERE link_id = ?",
            )
            .bind(epg_source.id.to_string())
            .bind(linked.link_id.to_string())
            .execute(&mut *tx)
            .await?;

            tx.commit().await?;

            info!(
                "Auto-linked EPG source '{}' to existing stream source with same credentials",
                epg_source.name
            );
            return Ok(true);
        }

        // Check if there's a stream source with the same credentials (not yet linked)
        let stream_sources = sqlx::query(
            "SELECT id, name FROM stream_sources 
             WHERE source_type = 'xtream' AND url = ? AND username = ? AND password = ? AND is_active = 1",
        )
        .bind(&epg_source.url)
        .bind(username)
        .bind(password)
        .fetch_all(&self.pool)
        .await?;

        if let Some(stream_row) = stream_sources.first() {
            // Create a new linked record
            let link_id = Uuid::new_v4();
            let linked_id = Uuid::new_v4();
            let now = chrono::Utc::now();

            let mut tx = self.pool.begin().await?;
            sqlx::query(
                "INSERT INTO linked_xtream_sources (id, link_id, name, url, username, password, stream_source_id, epg_source_id, created_at, updated_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(linked_id.to_string())
            .bind(link_id.to_string())
            .bind(&epg_source.name)
            .bind(&epg_source.url)
            .bind(username)
            .bind(password)
            .bind(stream_row.get::<String, _>("id"))
            .bind(epg_source.id.to_string())
            .bind(now.format("%Y-%m-%d %H:%M:%S").to_string())
            .bind(now.format("%Y-%m-%d %H:%M:%S").to_string())
            .execute(&mut *tx)
            .await?;

            tx.commit().await?;

            info!(
                "Auto-linked EPG source '{}' with stream source '{}' (same Xtream credentials)",
                epg_source.name,
                stream_row.get::<String, _>("name")
            );
            return Ok(true);
        }

        Ok(false)
    }

    /// Find linked stream source by EPG source ID
    pub async fn find_linked_stream_by_epg_id(
        &self,
        epg_source_id: Uuid,
    ) -> Result<Option<crate::models::StreamSource>> {
        let row = sqlx::query(
            "SELECT ss.* FROM stream_sources ss
             JOIN linked_xtream_sources lxs ON ss.id = lxs.stream_source_id
             WHERE lxs.epg_source_id = ?",
        )
        .bind(epg_source_id.to_string())
        .fetch_optional(&self.pool)
        .await?;

        if let Some(row) = row {
            let created_at = crate::utils::datetime::DateTimeParser::parse_flexible(
                &row.get::<String, _>("created_at"),
            )
            .map_err(|_| anyhow::anyhow!("Failed to parse created_at"))?;
            let updated_at = crate::utils::datetime::DateTimeParser::parse_flexible(
                &row.get::<String, _>("updated_at"),
            )
            .map_err(|_| anyhow::anyhow!("Failed to parse updated_at"))?;
            let last_ingested_at = row
                .get::<Option<String>, _>("last_ingested_at")
                .map(|s| {
                    crate::utils::datetime::DateTimeParser::parse_flexible(&s)
                        .map_err(|_| anyhow::anyhow!("Failed to parse datetime"))
                })
                .transpose()?;

            Ok(Some(crate::models::StreamSource {
                id: Uuid::parse_str(&row.get::<String, _>("id"))?,
                name: row.get("name"),
                source_type: row.get::<String, _>("source_type").parse()?,
                url: row.get("url"),
                max_concurrent_streams: row.get("max_concurrent_streams"),
                update_cron: row.get("update_cron"),
                username: row.get("username"),
                password: row.get("password"),
                field_map: row.get("field_map"),
                created_at,
                updated_at,
                last_ingested_at,
                is_active: row.get("is_active"),
            }))
        } else {
            Ok(None)
        }
    }
}
