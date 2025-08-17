//! Stream Proxy Repository
//!
//! This module provides data access operations for stream proxies and their relationships.

use async_trait::async_trait;
use chrono::Utc;
use sqlx::{Row, SqlitePool};
use uuid::Uuid;

use crate::{
    errors::{RepositoryError, RepositoryResult},
    models::{
        Channel, EpgSource, ProxyEpgSource, ProxyFilter, ProxyFilterWithDetails, ProxySource, 
        StreamProxy, StreamProxyCreateRequest, StreamProxyMode, StreamProxyUpdateRequest, 
        StreamSource, Filter, FilterSourceType, EpgSourceType
    },
    repositories::traits::{QueryParams, Repository},
    utils::sqlite::SqliteRowExt,
    utils::uuid_parser::parse_uuid_flexible,
};

#[derive(Clone)]
pub struct StreamProxyRepository {
    pool: SqlitePool,
}

impl StreamProxyRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Helper function to construct a StreamProxy from a database row
    fn stream_proxy_from_row(row: &sqlx::sqlite::SqliteRow) -> RepositoryResult<StreamProxy> {
        let proxy_mode_str = row.get::<String, _>("proxy_mode");
        let proxy_mode = proxy_mode_str.parse::<StreamProxyMode>().unwrap_or_default();

        Ok(StreamProxy {
            id: row
                .get_uuid("id")
                .map_err(|e| RepositoryError::QueryFailed {
                    query: "stream_proxy_from_row".to_string(),
                    message: format!("Failed to parse id: {e}"),
                })?,
            name: row.get("name"),
            description: row.get("description"),
            proxy_mode,
            upstream_timeout: row
                .get::<Option<i64>, _>("upstream_timeout")
                .map(|v| v as i32),
            buffer_size: row.get::<Option<i64>, _>("buffer_size").map(|v| v as i32),
            max_concurrent_streams: row
                .get::<Option<i64>, _>("max_concurrent_streams")
                .map(|v| v as i32),
            starting_channel_number: row.get::<i64, _>("starting_channel_number") as i32,
            created_at: row.get_datetime("created_at"),
            updated_at: row.get_datetime("updated_at"),
            last_generated_at: row.get_datetime_opt("last_generated_at"),
            is_active: row.get("is_active"),
            auto_regenerate: row.get("auto_regenerate"),
            cache_channel_logos: row.get("cache_channel_logos"),
            cache_program_logos: row.get("cache_program_logos"),
            relay_profile_id: row.get::<Option<String>, _>("relay_profile_id")
                .and_then(|s| s.parse::<Uuid>().ok()),
        })
    }

    /// Create a new stream proxy with all its relationships
    pub async fn create_with_relationships(
        &self,
        request: StreamProxyCreateRequest,
    ) -> RepositoryResult<StreamProxy> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| RepositoryError::QueryFailed {
                query: "begin_transaction".to_string(),
                message: e.to_string(),
            })?;

        // Generate IDs
        let proxy_id = Uuid::new_v4();
        let proxy_id_str = proxy_id.to_string();
        let now = Utc::now();
        let now_str = now.to_rfc3339();

        // Create the proxy
        let proxy_mode_str = match request.proxy_mode {
            StreamProxyMode::Redirect => "redirect",
            StreamProxyMode::Proxy => "proxy",
            StreamProxyMode::Relay => "relay",
        };

        // Convert values to prevent temporary value drops
        let upstream_timeout = request.upstream_timeout.map(|v| v as i64);
        let buffer_size = request.buffer_size.map(|v| v as i64);
        let max_concurrent_streams = request.max_concurrent_streams.map(|v| v as i64);
        let starting_channel_number = request.starting_channel_number as i64;

        sqlx::query(
            r#"
            INSERT INTO stream_proxies (
                id, name, description, proxy_mode, upstream_timeout,
                buffer_size, max_concurrent_streams, starting_channel_number,
                created_at, updated_at, is_active, auto_regenerate,
                cache_channel_logos, cache_program_logos, relay_profile_id
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&proxy_id_str)
        .bind(&request.name)
        .bind(&request.description)
        .bind(proxy_mode_str)
        .bind(upstream_timeout)
        .bind(buffer_size)
        .bind(max_concurrent_streams)
        .bind(starting_channel_number)
        .bind(&now_str)
        .bind(&now_str)
        .bind(request.is_active)
        .bind(request.auto_regenerate)
        .bind(request.cache_channel_logos)
        .bind(request.cache_program_logos)
        .bind(request.relay_profile_id.map(|id| id.to_string()))
        .execute(&mut *tx)
        .await
        .map_err(|e| RepositoryError::QueryFailed {
            query: "insert_stream_proxy".to_string(),
            message: e.to_string(),
        })?;

        // Add stream sources
        for source_req in request.stream_sources {
            let source_id_str = source_req.source_id.to_string();
            let priority_order = source_req.priority_order as i64;
            sqlx::query(
                r#"
                INSERT INTO proxy_sources (proxy_id, source_id, priority_order, created_at)
                VALUES (?, ?, ?, ?)
                "#,
            )
            .bind(&proxy_id_str)
            .bind(source_id_str)
            .bind(priority_order)
            .bind(&now_str)
            .execute(&mut *tx)
            .await
            .map_err(|e| RepositoryError::QueryFailed {
                query: "insert_proxy_source".to_string(),
                message: e.to_string(),
            })?;
        }

        // Add EPG sources
        for epg_req in request.epg_sources {
            let epg_source_id_str = epg_req.epg_source_id.to_string();
            let priority_order = epg_req.priority_order as i64;
            sqlx::query(
                r#"
                INSERT INTO proxy_epg_sources (proxy_id, epg_source_id, priority_order, created_at)
                VALUES (?, ?, ?, ?)
                "#,
            )
            .bind(&proxy_id_str)
            .bind(epg_source_id_str)
            .bind(priority_order)
            .bind(&now_str)
            .execute(&mut *tx)
            .await
            .map_err(|e| RepositoryError::QueryFailed {
                query: "insert_proxy_epg_source".to_string(),
                message: e.to_string(),
            })?;
        }

        // Add filters
        for filter_req in request.filters {
            let filter_id_str = filter_req.filter_id.to_string();
            let priority_order = filter_req.priority_order as i64;

            sqlx::query!(
                r#"
                INSERT INTO proxy_filters (proxy_id, filter_id, priority_order, is_active, created_at)
                VALUES (?, ?, ?, ?, ?)
                "#,
                proxy_id_str,
                filter_id_str,
                priority_order,
                filter_req.is_active,
                now_str
            )
            .execute(&mut *tx)
            .await
            .map_err(|e| RepositoryError::QueryFailed {
                query: "insert_proxy_filter".to_string(),
                message: e.to_string(),
            })?;
        }

        tx.commit()
            .await
            .map_err(|e| RepositoryError::QueryFailed {
                query: "commit_transaction".to_string(),
                message: e.to_string(),
            })?;

        // Return the created proxy
        self.find_by_id(proxy_id)
            .await?
            .ok_or_else(|| RepositoryError::RecordNotFound {
                table: "stream_proxies".to_string(),
                field: "id".to_string(),
                value: proxy_id.to_string(),
            })
    }

    /// Update a stream proxy and its relationships
    pub async fn update_with_relationships(
        &self,
        proxy_id: Uuid,
        request: StreamProxyUpdateRequest,
    ) -> RepositoryResult<StreamProxy> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| RepositoryError::QueryFailed {
                query: "begin_transaction".to_string(),
                message: e.to_string(),
            })?;
        let now = Utc::now();
        let now_str = now.to_rfc3339();
        let proxy_id_str = proxy_id.to_string();

        let proxy_mode_str = match request.proxy_mode {
            StreamProxyMode::Redirect => "redirect",
            StreamProxyMode::Proxy => "proxy",
            StreamProxyMode::Relay => "relay",
        };

        // Convert values to prevent temporary value drops
        let upstream_timeout = request.upstream_timeout.map(|v| v as i64);
        let buffer_size = request.buffer_size.map(|v| v as i64);
        let max_concurrent_streams = request.max_concurrent_streams.map(|v| v as i64);
        let starting_channel_number = request.starting_channel_number as i64;

        // Update the proxy
        sqlx::query(
            r#"
            UPDATE stream_proxies
            SET name = ?, description = ?, proxy_mode = ?, upstream_timeout = ?,
                buffer_size = ?, max_concurrent_streams = ?, starting_channel_number = ?,
                is_active = ?, auto_regenerate = ?, cache_channel_logos = ?,
                cache_program_logos = ?, relay_profile_id = ?, updated_at = ?
            WHERE id = ?
            "#,
        )
        .bind(&request.name)
        .bind(&request.description)
        .bind(proxy_mode_str)
        .bind(upstream_timeout)
        .bind(buffer_size)
        .bind(max_concurrent_streams)
        .bind(starting_channel_number)
        .bind(request.is_active)
        .bind(request.auto_regenerate)
        .bind(request.cache_channel_logos)
        .bind(request.cache_program_logos)
        .bind(request.relay_profile_id.map(|id| id.to_string()))
        .bind(&now_str)
        .bind(&proxy_id_str)
        .execute(&mut *tx)
        .await
        .map_err(|e| RepositoryError::QueryFailed {
            query: "update_stream_proxy".to_string(),
            message: e.to_string(),
        })?;

        // Delete existing relationships
        sqlx::query!("DELETE FROM proxy_sources WHERE proxy_id = ?", proxy_id_str)
            .execute(&mut *tx)
            .await
            .map_err(|e| RepositoryError::QueryFailed {
                query: "delete_proxy_sources".to_string(),
                message: e.to_string(),
            })?;

        sqlx::query!(
            "DELETE FROM proxy_epg_sources WHERE proxy_id = ?",
            proxy_id_str
        )
        .execute(&mut *tx)
        .await
        .map_err(|e| RepositoryError::QueryFailed {
            query: "delete_proxy_epg_sources".to_string(),
            message: e.to_string(),
        })?;

        sqlx::query!("DELETE FROM proxy_filters WHERE proxy_id = ?", proxy_id_str)
            .execute(&mut *tx)
            .await
            .map_err(|e| RepositoryError::QueryFailed {
                query: "delete_proxy_filters".to_string(),
                message: e.to_string(),
            })?;

        // Re-add stream sources
        for source_req in request.stream_sources {
            let source_id_str = source_req.source_id.to_string();
            let priority_order = source_req.priority_order as i64;
            sqlx::query!(
                r#"
                INSERT INTO proxy_sources (proxy_id, source_id, priority_order, created_at)
                VALUES (?, ?, ?, ?)
                "#,
                proxy_id_str,
                source_id_str,
                priority_order,
                now_str
            )
            .execute(&mut *tx)
            .await
            .map_err(|e| RepositoryError::QueryFailed {
                query: "insert_proxy_source".to_string(),
                message: e.to_string(),
            })?;
        }

        // Re-add EPG sources
        for epg_req in request.epg_sources {
            let epg_source_id_str = epg_req.epg_source_id.to_string();
            let priority_order = epg_req.priority_order as i64;
            sqlx::query!(
                r#"
                INSERT INTO proxy_epg_sources (proxy_id, epg_source_id, priority_order, created_at)
                VALUES (?, ?, ?, ?)
                "#,
                proxy_id_str,
                epg_source_id_str,
                priority_order,
                now_str
            )
            .execute(&mut *tx)
            .await
            .map_err(|e| RepositoryError::QueryFailed {
                query: "insert_proxy_epg_source".to_string(),
                message: e.to_string(),
            })?;
        }

        // Re-add filters
        for filter_req in request.filters {
            let filter_id_str = filter_req.filter_id.to_string();
            let priority_order = filter_req.priority_order as i64;
            sqlx::query!(
                r#"
                INSERT INTO proxy_filters (proxy_id, filter_id, priority_order, is_active, created_at)
                VALUES (?, ?, ?, ?, ?)
                "#,
                proxy_id_str,
                filter_id_str,
                priority_order,
                filter_req.is_active,
                now_str
            )
            .execute(&mut *tx)
            .await
            .map_err(|e| RepositoryError::QueryFailed {
                query: "insert_proxy_filter".to_string(),
                message: e.to_string(),
            })?;
        }

        tx.commit()
            .await
            .map_err(|e| RepositoryError::QueryFailed {
                query: "commit_transaction".to_string(),
                message: e.to_string(),
            })?;

        // Return the updated proxy
        self.find_by_id(proxy_id)
            .await?
            .ok_or_else(|| RepositoryError::RecordNotFound {
                table: "stream_proxies".to_string(),
                field: "id".to_string(),
                value: proxy_id.to_string(),
            })
    }

    /// Get proxy sources with priority order
    pub async fn get_proxy_sources(&self, proxy_id: Uuid) -> RepositoryResult<Vec<ProxySource>> {
        let proxy_id_str = proxy_id.to_string();
        let rows = sqlx::query(
            r#"
            SELECT proxy_id, source_id, priority_order, created_at
            FROM proxy_sources
            WHERE proxy_id = ?
            ORDER BY priority_order ASC
            "#,
        )
        .bind(proxy_id_str)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| RepositoryError::QueryFailed {
            query: "get_proxy_sources".to_string(),
            message: e.to_string(),
        })?;

        let mut sources = Vec::new();
        for row in rows {
            let source = ProxySource {
                proxy_id: row
                    .get_uuid("proxy_id")
                    .map_err(|e| RepositoryError::QueryFailed {
                        query: "get_proxy_sources".to_string(),
                        message: format!("Failed to parse proxy_id: {e}"),
                    })?,
                source_id: row
                    .get_uuid("source_id")
                    .map_err(|e| RepositoryError::QueryFailed {
                        query: "get_proxy_sources".to_string(),
                        message: format!("Failed to parse source_id: {e}"),
                    })?,
                priority_order: row.get::<i64, _>("priority_order") as i32,
                created_at: row.get_datetime("created_at"),
            };
            sources.push(source);
        }

        Ok(sources)
    }

    /// Get proxy EPG sources with priority order (only active EPG sources)
    pub async fn get_proxy_epg_sources(
        &self,
        proxy_id: Uuid,
    ) -> RepositoryResult<Vec<ProxyEpgSource>> {
        let proxy_id_str = proxy_id.to_string();
        let rows = sqlx::query(
            r#"
            SELECT pes.proxy_id, pes.epg_source_id, pes.priority_order, pes.created_at
            FROM proxy_epg_sources pes
            JOIN epg_sources e ON e.id = pes.epg_source_id
            WHERE pes.proxy_id = ? AND e.is_active = 1
            ORDER BY pes.priority_order ASC
            "#,
        )
        .bind(proxy_id_str)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| RepositoryError::QueryFailed {
            query: "get_proxy_epg_sources".to_string(),
            message: e.to_string(),
        })?;

        let mut sources = Vec::new();
        for row in rows {
            let source = ProxyEpgSource {
                proxy_id: row
                    .get_uuid("proxy_id")
                    .map_err(|e| RepositoryError::QueryFailed {
                        query: "get_proxy_epg_sources".to_string(),
                        message: format!("Failed to parse proxy_id: {e}"),
                    })?,
                epg_source_id: row.get_uuid("epg_source_id").map_err(|e| {
                    RepositoryError::QueryFailed {
                        query: "get_proxy_epg_sources".to_string(),
                        message: format!("Failed to parse epg_source_id: {e}"),
                    }
                })?,
                priority_order: row.get::<i64, _>("priority_order") as i32,
                created_at: row.get_datetime("created_at"),
            };
            sources.push(source);
        }

        Ok(sources)
    }

    /// Get proxy filters with priority order
    pub async fn get_proxy_filters(&self, proxy_id: Uuid) -> RepositoryResult<Vec<ProxyFilter>> {
        let proxy_id_str = proxy_id.to_string();
        let rows = sqlx::query(
            r#"
            SELECT proxy_id, filter_id, priority_order, is_active, created_at
            FROM proxy_filters
            WHERE proxy_id = ?
            ORDER BY priority_order ASC
            "#,
        )
        .bind(proxy_id_str)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| RepositoryError::QueryFailed {
            query: "get_proxy_filters".to_string(),
            message: e.to_string(),
        })?;

        let mut filters = Vec::new();
        for row in rows {
            let filter = ProxyFilter {
                proxy_id: row
                    .get_uuid("proxy_id")
                    .map_err(|e| RepositoryError::QueryFailed {
                        query: "get_proxy_filters".to_string(),
                        message: format!("Failed to parse proxy_id: {e}"),
                    })?,
                filter_id: row
                    .get_uuid("filter_id")
                    .map_err(|e| RepositoryError::QueryFailed {
                        query: "get_proxy_filters".to_string(),
                        message: format!("Failed to parse filter_id: {e}"),
                    })?,
                priority_order: row.get::<i64, _>("priority_order") as i32,
                is_active: row.get("is_active"),
                created_at: row.get_datetime("created_at"),
            };
            filters.push(filter);
        }

        Ok(filters)
    }

    /// Get EPG source by ID (helper method for relationships)
    pub async fn find_epg_source_by_id(
        &self,
        epg_source_id: Uuid,
    ) -> RepositoryResult<Option<crate::models::EpgSource>> {
        let epg_source_id_str = epg_source_id.to_string();
        let row = sqlx::query(
            r#"
            SELECT id, name, source_type, url, update_cron, username, password,
                   original_timezone, time_offset, created_at, updated_at,
                   last_ingested_at, is_active
            FROM epg_sources
            WHERE id = ?
            "#,
        )
        .bind(epg_source_id_str)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| RepositoryError::QueryFailed {
            query: "find_epg_source_by_id".to_string(),
            message: e.to_string(),
        })?;

        match row {
            Some(row) => {
                let source_type = match row.get::<String, _>("source_type").as_str() {
                    "xmltv" => crate::models::EpgSourceType::Xmltv,
                    "xtream" => crate::models::EpgSourceType::Xtream,
                    _ => crate::models::EpgSourceType::Xmltv,
                };

                let epg_source = crate::models::EpgSource {
                    id: row
                        .get_uuid("id")
                        .map_err(|e| RepositoryError::QueryFailed {
                            query: "find_epg_source_by_id".to_string(),
                            message: format!("Failed to parse id: {e}"),
                        })?,
                    name: row.get("name"),
                    source_type,
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

                Ok(Some(epg_source))
            }
            None => Ok(None),
        }
    }

    /// Get proxy by ID
    pub async fn get_by_id(&self, id: &str) -> RepositoryResult<Option<StreamProxy>> {
        let row = sqlx::query(
            r#"
            SELECT id, name, description, proxy_mode,
                   upstream_timeout, buffer_size, max_concurrent_streams,
                   starting_channel_number, created_at, updated_at, last_generated_at, is_active, auto_regenerate,
                   cache_channel_logos, cache_program_logos, relay_profile_id
            FROM stream_proxies
            WHERE id = ?
            "#
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| RepositoryError::QueryFailed {
            query: "get_by_id".to_string(),
            message: e.to_string(),
        })?;

        match row {
            Some(row) => Ok(Some(Self::stream_proxy_from_row(&row)?)),
            None => Ok(None),
        }
    }

    /// Get all active stream proxies
    pub async fn get_all_active(&self) -> RepositoryResult<Vec<StreamProxy>> {
        let rows = sqlx::query(
            "SELECT id, name, created_at, updated_at, last_generated_at, is_active,
             auto_regenerate, cache_channel_logos, proxy_mode, upstream_timeout,
             buffer_size, max_concurrent_streams, starting_channel_number,
             description, cache_program_logos, relay_profile_id
             FROM stream_proxies
             WHERE is_active = 1",
        )
        .fetch_all(&self.pool)
        .await?;

        let mut proxies = Vec::new();
        for row in rows {
            let proxy = StreamProxy {
                id: parse_uuid_flexible(&row.get::<String, _>("id"))?,
                name: row.get("name"),
                description: row.get("description"),
                proxy_mode: match row.get::<String, _>("proxy_mode").as_str() {
                    "redirect" => StreamProxyMode::Redirect,
                    "proxy" => StreamProxyMode::Proxy,
                    "relay" => StreamProxyMode::Relay,
                    _ => StreamProxyMode::Redirect,
                },
                upstream_timeout: row
                    .get::<Option<i64>, _>("upstream_timeout")
                    .map(|v| v as i32),
                buffer_size: row.get::<Option<i64>, _>("buffer_size").map(|v| v as i32),
                max_concurrent_streams: row
                    .get::<Option<i64>, _>("max_concurrent_streams")
                    .map(|v| v as i32),
                starting_channel_number: row.get::<i64, _>("starting_channel_number") as i32,
                created_at: row.get_datetime("created_at"),
                updated_at: row.get_datetime("updated_at"),
                last_generated_at: row.get_datetime_opt("last_generated_at"),
                is_active: row.get("is_active"),
                auto_regenerate: row.get("auto_regenerate"),
                cache_channel_logos: row.get("cache_channel_logos"),
                cache_program_logos: row.get("cache_program_logos"),
                relay_profile_id: row
                    .get::<Option<String>, _>("relay_profile_id")
                    .and_then(|s| parse_uuid_flexible(&s).ok()),
            };
            proxies.push(proxy);
        }

        Ok(proxies)
    }

    /// Get proxy sources with full StreamSource details
    pub async fn get_proxy_sources_with_details(&self, proxy_id: Uuid) -> RepositoryResult<Vec<StreamSource>> {
        let rows = sqlx::query(
            "SELECT s.id, s.name, s.source_type, s.url, s.max_concurrent_streams, s.update_cron,
             s.username, s.password, s.field_map, s.created_at, s.updated_at, s.last_ingested_at, s.is_active
             FROM stream_sources s
             JOIN proxy_sources ps ON s.id = ps.source_id
             WHERE ps.proxy_id = ? AND s.is_active = 1
             ORDER BY s.name"
        )
        .bind(proxy_id.to_string())
        .fetch_all(&self.pool)
        .await?;

        let mut sources = Vec::new();
        for row in rows {
            let source_type_str: String = row.get("source_type");
            let source_type = match source_type_str.as_str() {
                "m3u" => crate::models::StreamSourceType::M3u,
                "xtream" => crate::models::StreamSourceType::Xtream,
                _ => continue,
            };

            let source = StreamSource {
                id: parse_uuid_flexible(&row.get::<String, _>("id"))?,
                name: row.get("name"),
                source_type,
                url: row.get("url"),
                max_concurrent_streams: row.get("max_concurrent_streams"),
                update_cron: row.get("update_cron"),
                username: row.get("username"),
                password: row.get("password"),
                field_map: row.get("field_map"),
                ignore_channel_numbers: row.get("ignore_channel_numbers"),
                created_at: row.get("created_at"),
                updated_at: row.get("updated_at"),
                last_ingested_at: row.get("last_ingested_at"),
                is_active: row.get("is_active"),
            };
            sources.push(source);
        }

        Ok(sources)
    }

    /// Update the last_generated_at timestamp for a proxy
    pub async fn update_last_generated(&self, proxy_id: Uuid) -> RepositoryResult<()> {
        let now = chrono::Utc::now().to_rfc3339();
        sqlx::query("UPDATE stream_proxies SET last_generated_at = ?, updated_at = ? WHERE id = ?")
            .bind(&now)
            .bind(&now)  
            .bind(proxy_id.to_string())
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Get proxy filters with full details (including filter information)
    pub async fn get_proxy_filters_with_details(
        &self,
        proxy_id: Uuid,
    ) -> RepositoryResult<Vec<ProxyFilterWithDetails>> {
        let rows = sqlx::query(
            "SELECT pf.proxy_id, pf.filter_id, pf.priority_order, pf.is_active, pf.created_at,
                    f.name, f.starting_channel_number, f.is_inverse, f.is_system_default, f.expression, f.updated_at as filter_updated_at
             FROM proxy_filters pf
             JOIN filters f ON pf.filter_id = f.id
             WHERE pf.proxy_id = ? AND pf.is_active = 1
             ORDER BY pf.priority_order"
        )
        .bind(proxy_id.to_string())
        .fetch_all(&self.pool)
        .await?;

        let mut result = Vec::new();
        for row in rows {
            let proxy_filter = ProxyFilter {
                proxy_id: parse_uuid_flexible(&row.get::<String, _>("proxy_id"))?,
                filter_id: parse_uuid_flexible(&row.get::<String, _>("filter_id"))?,
                priority_order: row.get("priority_order"),
                is_active: row.get("is_active"),
                created_at: row.get("created_at"),
            };

            let filter = Filter {
                id: proxy_filter.filter_id,
                name: row.get("name"),
                source_type: FilterSourceType::Stream, // Default, could be enhanced
                is_inverse: row.get("is_inverse"),
                is_system_default: row.get("is_system_default"),
                expression: row.get("expression"),
                created_at: row.get("created_at"),
                updated_at: row.get("filter_updated_at"),
            };

            result.push(ProxyFilterWithDetails {
                proxy_filter,
                filter,
            });
        }

        Ok(result)
    }

    /// Get a channel by ID within the context of a specific proxy
    /// This validates that the channel belongs to one of the proxy's sources
    pub async fn get_channel_for_proxy(
        &self,
        proxy_id: &str,
        channel_id: Uuid,
    ) -> RepositoryResult<Option<Channel>> {
        let row = sqlx::query(
            "SELECT c.id, c.source_id, c.tvg_id, c.tvg_name, c.tvg_chno, c.tvg_logo, c.tvg_shift,
             c.group_title, c.channel_name, c.stream_url, c.created_at, c.updated_at
             FROM channels c
             JOIN proxy_sources ps ON c.source_id = ps.source_id
             JOIN stream_proxies sp ON ps.proxy_id = sp.id
             WHERE sp.id = ? AND c.id = ? AND sp.is_active = 1",
        )
        .bind(proxy_id.to_string())
        .bind(channel_id.to_string())
        .fetch_optional(&self.pool)
        .await?;

        match row {
            Some(row) => Ok(Some(Channel {
                id: parse_uuid_flexible(&row.get::<String, _>("id"))?,
                source_id: parse_uuid_flexible(&row.get::<String, _>("source_id"))?,
                tvg_id: row.get("tvg_id"),
                tvg_name: row.get("tvg_name"),
                tvg_chno: row.try_get("tvg_chno").unwrap_or(None),
                tvg_logo: row.get("tvg_logo"),
                tvg_shift: row.get("tvg_shift"),
                group_title: row.get("group_title"),
                channel_name: row.get("channel_name"),
                stream_url: row.get("stream_url"),
                created_at: crate::utils::datetime::DateTimeParser::parse_flexible(
                    &row.get::<String, _>("created_at")
                ).map_err(|e| crate::errors::RepositoryError::query_failed("parse created_at", e.to_string()))?,
                updated_at: crate::utils::datetime::DateTimeParser::parse_flexible(
                    &row.get::<String, _>("updated_at")
                ).map_err(|e| crate::errors::RepositoryError::query_failed("parse updated_at", e.to_string()))?,
            })),
            None => Ok(None),
        }
    }

    /// Get EPG sources associated with a proxy (with full EpgSource details)
    pub async fn get_proxy_epg_sources_with_details(&self, proxy_id: Uuid) -> RepositoryResult<Vec<EpgSource>> {
        let rows = sqlx::query(
            "SELECT e.id, e.name, e.source_type, e.url, e.update_cron, e.username, e.password,
             e.original_timezone, e.time_offset, e.created_at, e.updated_at,
             e.last_ingested_at, e.is_active
             FROM epg_sources e
             JOIN proxy_epg_sources pes ON e.id = pes.epg_source_id
             WHERE pes.proxy_id = ? AND e.is_active = 1
             ORDER BY pes.priority_order",
        )
        .bind(proxy_id.to_string())
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

            let source = EpgSource {
                id: parse_uuid_flexible(&row.get::<String, _>("id"))?,
                name: row.get("name"),
                source_type,
                url: row.get("url"),
                update_cron: row.get("update_cron"),
                username: row.get("username"),
                password: row.get("password"),
                original_timezone: row.get("original_timezone"),
                time_offset: row.get("time_offset"),
                created_at: row.get("created_at"),
                updated_at: row.get("updated_at"),
                last_ingested_at: row.get("last_ingested_at"),
                is_active: row.get("is_active"),
            };
            sources.push(source);
        }

        Ok(sources)
    }

    /// Get all proxies that use a specific stream source (for regeneration triggers)
    pub async fn find_proxies_by_stream_source(&self, source_id: Uuid) -> RepositoryResult<Vec<Uuid>> {
        let source_id_str = source_id.to_string();
        let rows = sqlx::query(
            r#"
            SELECT DISTINCT sp.id
            FROM stream_proxies sp
            JOIN proxy_sources ps ON sp.id = ps.proxy_id
            WHERE ps.source_id = ? AND sp.is_active = 1 AND sp.auto_regenerate = 1
            "#,
        )
        .bind(&source_id_str)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| RepositoryError::QueryFailed {
            query: "find_proxies_by_stream_source".to_string(),
            message: e.to_string(),
        })?;

        let mut proxy_ids = Vec::new();
        for row in rows {
            let proxy_id = parse_uuid_flexible(&row.get::<String, _>("id")).map_err(|e| {
                RepositoryError::QueryFailed {
                    query: "find_proxies_by_stream_source".to_string(),
                    message: format!("Failed to parse proxy_id: {e}"),
                }
            })?;
            proxy_ids.push(proxy_id);
        }

        Ok(proxy_ids)
    }

    /// Get all proxies that use a specific EPG source (for regeneration triggers)
    pub async fn find_proxies_by_epg_source(&self, epg_source_id: Uuid) -> RepositoryResult<Vec<Uuid>> {
        let epg_source_id_str = epg_source_id.to_string();
        let rows = sqlx::query(
            r#"
            SELECT DISTINCT sp.id
            FROM stream_proxies sp
            JOIN proxy_epg_sources pes ON sp.id = pes.proxy_id  
            WHERE pes.epg_source_id = ? AND sp.is_active = 1 AND sp.auto_regenerate = 1
            "#,
        )
        .bind(&epg_source_id_str)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| RepositoryError::QueryFailed {
            query: "find_proxies_by_epg_source".to_string(),
            message: e.to_string(),
        })?;

        let mut proxy_ids = Vec::new();
        for row in rows {
            let proxy_id = parse_uuid_flexible(&row.get::<String, _>("id")).map_err(|e| {
                RepositoryError::QueryFailed {
                    query: "find_proxies_by_epg_source".to_string(),
                    message: format!("Failed to parse proxy_id: {e}"),
                }
            })?;
            proxy_ids.push(proxy_id);
        }

        Ok(proxy_ids)
    }

    /// Get all stream source IDs associated with a proxy (lightweight query for regeneration)
    pub async fn get_stream_source_ids(&self, proxy_id: Uuid) -> RepositoryResult<Vec<Uuid>> {
        let proxy_id_str = proxy_id.to_string();
        let rows = sqlx::query("SELECT source_id FROM proxy_sources WHERE proxy_id = ?")
            .bind(&proxy_id_str)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| RepositoryError::QueryFailed {
                query: "get_stream_source_ids".to_string(),
                message: e.to_string(),
            })?;

        let mut source_ids = Vec::new();
        for row in rows {
            let source_id = parse_uuid_flexible(&row.get::<String, _>("source_id")).map_err(|e| {
                RepositoryError::QueryFailed {
                    query: "get_stream_source_ids".to_string(),
                    message: format!("Failed to parse source_id: {e}"),
                }
            })?;
            source_ids.push(source_id);
        }

        Ok(source_ids)
    }

    /// Get all EPG source IDs associated with a proxy (lightweight query for regeneration)
    pub async fn get_epg_source_ids(&self, proxy_id: Uuid) -> RepositoryResult<Vec<Uuid>> {
        let proxy_id_str = proxy_id.to_string();
        let rows = sqlx::query("SELECT epg_source_id FROM proxy_epg_sources WHERE proxy_id = ?")
            .bind(&proxy_id_str)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| RepositoryError::QueryFailed {
                query: "get_epg_source_ids".to_string(),
                message: e.to_string(),
            })?;

        let mut source_ids = Vec::new();
        for row in rows {
            let epg_source_id = parse_uuid_flexible(&row.get::<String, _>("epg_source_id")).map_err(|e| {
                RepositoryError::QueryFailed {
                    query: "get_epg_source_ids".to_string(),
                    message: format!("Failed to parse epg_source_id: {e}"),
                }
            })?;
            source_ids.push(epg_source_id);
        }

        Ok(source_ids)
    }

    /// Get the proxy ID that contains a specific source ID
    pub async fn get_proxy_id_for_source(&self, source_id: Uuid) -> RepositoryResult<Option<Uuid>> {
        let source_id_str = source_id.to_string();
        let row = sqlx::query("SELECT proxy_id FROM proxy_sources WHERE source_id = ? LIMIT 1")
            .bind(&source_id_str)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| RepositoryError::QueryFailed {
                query: "get_proxy_id_for_source".to_string(),
                message: e.to_string(),
            })?;

        match row {
            Some(row) => {
                let proxy_id = parse_uuid_flexible(&row.get::<String, _>("proxy_id")).map_err(|e| {
                    RepositoryError::QueryFailed {
                        query: "get_proxy_id_for_source".to_string(),
                        message: format!("Failed to parse proxy_id: {e}"),
                    }
                })?;
                Ok(Some(proxy_id))
            }
            None => Ok(None),
        }
    }
}

#[async_trait]
impl Repository<StreamProxy, Uuid> for StreamProxyRepository {
    type CreateRequest = StreamProxyCreateRequest;
    type UpdateRequest = StreamProxyUpdateRequest;
    type Query = QueryParams;

    async fn find_by_id(&self, id: Uuid) -> RepositoryResult<Option<StreamProxy>> {
        let id_str = id.to_string();
        let row = sqlx::query(
            r#"
            SELECT id, name, description, proxy_mode,
                   upstream_timeout, buffer_size, max_concurrent_streams,
                   starting_channel_number, created_at, updated_at, last_generated_at, is_active, auto_regenerate,
                   cache_channel_logos, cache_program_logos, relay_profile_id
            FROM stream_proxies
            WHERE id = ?
            "#
        )
        .bind(id_str)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| RepositoryError::QueryFailed {
            query: "find_by_id".to_string(),
            message: e.to_string(),
        })?;

        match row {
            Some(row) => Ok(Some(Self::stream_proxy_from_row(&row)?)),
            None => Ok(None),
        }
    }

    async fn find_all(&self, _query: Self::Query) -> RepositoryResult<Vec<StreamProxy>> {
        let rows = sqlx::query(
            r#"
            SELECT id, name, description, proxy_mode,
                   upstream_timeout, buffer_size, max_concurrent_streams,
                   starting_channel_number, created_at, updated_at, last_generated_at, is_active, auto_regenerate,
                   cache_channel_logos, cache_program_logos, relay_profile_id
            FROM stream_proxies
            ORDER BY created_at DESC
            "#
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| RepositoryError::QueryFailed {
            query: "find_all".to_string(),
            message: e.to_string(),
        })?;

        let mut proxies = Vec::new();
        for row in rows {
            proxies.push(Self::stream_proxy_from_row(&row)?);
        }

        Ok(proxies)
    }

    async fn create(&self, request: Self::CreateRequest) -> RepositoryResult<StreamProxy> {
        self.create_with_relationships(request).await
    }

    async fn update(
        &self,
        id: Uuid,
        request: Self::UpdateRequest,
    ) -> RepositoryResult<StreamProxy> {
        self.update_with_relationships(id, request).await
    }

    async fn delete(&self, id: Uuid) -> RepositoryResult<()> {
        let id_str = id.to_string();

        let result = sqlx::query!("DELETE FROM stream_proxies WHERE id = ?", id_str)
            .execute(&self.pool)
            .await
            .map_err(|e| RepositoryError::QueryFailed {
                query: "delete_stream_proxy".to_string(),
                message: e.to_string(),
            })?;

        if result.rows_affected() == 0 {
            return Err(RepositoryError::RecordNotFound {
                table: "stream_proxies".to_string(),
                field: "id".to_string(),
                value: id_str,
            });
        }

        Ok(())
    }

    async fn count(&self, _query: Self::Query) -> RepositoryResult<u64> {
        Ok(0)
    }
}
