//! Stream Proxy Repository
//!
//! This module provides data access operations for stream proxies and their relationships.

use async_trait::async_trait;
use sqlx::{SqlitePool, Row};
use uuid::Uuid;
use chrono::Utc;
use ulid::Ulid;

use crate::{
    models::{
        StreamProxy, StreamProxyMode, StreamProxyCreateRequest, StreamProxyUpdateRequest,
        ProxySource, ProxyEpgSource, ProxyFilter,
    },
    repositories::traits::{Repository, QueryParams},
    errors::{RepositoryResult, RepositoryError},
    utils::sqlite::SqliteRowExt,
};

#[derive(Clone)]
pub struct StreamProxyRepository {
    pool: SqlitePool,
}

impl StreamProxyRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Create a new stream proxy with all its relationships
    pub async fn create_with_relationships(
        &self,
        request: StreamProxyCreateRequest,
    ) -> RepositoryResult<StreamProxy> {
        let mut tx = self.pool.begin().await.map_err(|e| RepositoryError::QueryFailed {
            query: "begin_transaction".to_string(),
            message: e.to_string(),
        })?;
        
        // Generate IDs
        let proxy_id = Uuid::new_v4();
        let proxy_id_str = proxy_id.to_string();
        let ulid = Ulid::new().to_string();
        let now = Utc::now();
        let now_str = now.to_rfc3339();

        // Create the proxy
        let proxy_mode_str = match request.proxy_mode {
            StreamProxyMode::Redirect => "redirect",
            StreamProxyMode::Proxy => "proxy",
        };

        // Convert values to prevent temporary value drops
        let upstream_timeout = request.upstream_timeout.map(|v| v as i64);
        let buffer_size = request.buffer_size.map(|v| v as i64);
        let max_concurrent_streams = request.max_concurrent_streams.map(|v| v as i64);
        let starting_channel_number = request.starting_channel_number as i64;

        sqlx::query(
            r#"
            INSERT INTO stream_proxies (
                id, ulid, name, description, proxy_mode, upstream_timeout, 
                buffer_size, max_concurrent_streams, starting_channel_number,
                created_at, updated_at, is_active, auto_regenerate
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#
        )
        .bind(&proxy_id_str)
        .bind(&ulid)
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
                "#
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
                "#
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

        tx.commit().await.map_err(|e| RepositoryError::QueryFailed {
            query: "commit_transaction".to_string(),
            message: e.to_string(),
        })?;

        // Return the created proxy
        self.find_by_id(proxy_id).await?.ok_or_else(|| {
            RepositoryError::RecordNotFound {
                table: "stream_proxies".to_string(),
                field: "id".to_string(),
                value: proxy_id.to_string(),
            }
        })
    }

    /// Update a stream proxy and its relationships
    pub async fn update_with_relationships(
        &self,
        proxy_id: Uuid,
        request: StreamProxyUpdateRequest,
    ) -> RepositoryResult<StreamProxy> {
        let mut tx = self.pool.begin().await.map_err(|e| RepositoryError::QueryFailed {
            query: "begin_transaction".to_string(),
            message: e.to_string(),
        })?;
        let now = Utc::now();
        let now_str = now.to_rfc3339();
        let proxy_id_str = proxy_id.to_string();

        let proxy_mode_str = match request.proxy_mode {
            StreamProxyMode::Redirect => "redirect",
            StreamProxyMode::Proxy => "proxy",
        };

        // Convert values to prevent temporary value drops
        let upstream_timeout = request.upstream_timeout.map(|v| v as i64);
        let buffer_size = request.buffer_size.map(|v| v as i64);
        let max_concurrent_streams = request.max_concurrent_streams.map(|v| v as i64);
        let starting_channel_number = request.starting_channel_number as i64;

        // Update the proxy
        sqlx::query!(
            r#"
            UPDATE stream_proxies 
            SET name = ?, description = ?, proxy_mode = ?, upstream_timeout = ?,
                buffer_size = ?, max_concurrent_streams = ?, starting_channel_number = ?,
                is_active = ?, auto_regenerate = ?, updated_at = ?
            WHERE id = ?
            "#,
            request.name,
            request.description,
            proxy_mode_str,
            upstream_timeout,
            buffer_size,
            max_concurrent_streams,
            starting_channel_number,
            request.is_active,
            request.auto_regenerate,
            now_str,
            proxy_id_str
        )
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
        
        sqlx::query!("DELETE FROM proxy_epg_sources WHERE proxy_id = ?", proxy_id_str)
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

        tx.commit().await.map_err(|e| RepositoryError::QueryFailed {
            query: "commit_transaction".to_string(),
            message: e.to_string(),
        })?;

        // Return the updated proxy
        self.find_by_id(proxy_id).await?.ok_or_else(|| {
            RepositoryError::RecordNotFound {
                table: "stream_proxies".to_string(),
                field: "id".to_string(),
                value: proxy_id.to_string(),
            }
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
            "#
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
                proxy_id: row.get_uuid("proxy_id").map_err(|e| RepositoryError::QueryFailed {
                    query: "get_proxy_sources".to_string(),
                    message: format!("Failed to parse proxy_id: {}", e),
                })?,
                source_id: row.get_uuid("source_id").map_err(|e| RepositoryError::QueryFailed {
                    query: "get_proxy_sources".to_string(),
                    message: format!("Failed to parse source_id: {}", e),
                })?,
                priority_order: row.get::<i64, _>("priority_order") as i32,
                created_at: row.get_datetime("created_at"),
            };
            sources.push(source);
        }

        Ok(sources)
    }

    /// Get proxy EPG sources with priority order
    pub async fn get_proxy_epg_sources(&self, proxy_id: Uuid) -> RepositoryResult<Vec<ProxyEpgSource>> {
        let proxy_id_str = proxy_id.to_string();
        let rows = sqlx::query(
            r#"
            SELECT proxy_id, epg_source_id, priority_order, created_at
            FROM proxy_epg_sources 
            WHERE proxy_id = ?
            ORDER BY priority_order ASC
            "#
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
                proxy_id: row.get_uuid("proxy_id").map_err(|e| RepositoryError::QueryFailed {
                    query: "get_proxy_epg_sources".to_string(),
                    message: format!("Failed to parse proxy_id: {}", e),
                })?,
                epg_source_id: row.get_uuid("epg_source_id").map_err(|e| RepositoryError::QueryFailed {
                    query: "get_proxy_epg_sources".to_string(),
                    message: format!("Failed to parse epg_source_id: {}", e),
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
            "#
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
                proxy_id: row.get_uuid("proxy_id").map_err(|e| RepositoryError::QueryFailed {
                    query: "get_proxy_filters".to_string(),
                    message: format!("Failed to parse proxy_id: {}", e),
                })?,
                filter_id: row.get_uuid("filter_id").map_err(|e| RepositoryError::QueryFailed {
                    query: "get_proxy_filters".to_string(),
                    message: format!("Failed to parse filter_id: {}", e),
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
    pub async fn find_epg_source_by_id(&self, epg_source_id: Uuid) -> RepositoryResult<Option<crate::models::EpgSource>> {
        let epg_source_id_str = epg_source_id.to_string();
        let row = sqlx::query(
            r#"
            SELECT id, name, source_type, url, update_cron, username, password,
                   timezone, timezone_detected, time_offset, created_at, updated_at,
                   last_ingested_at, is_active
            FROM epg_sources 
            WHERE id = ?
            "#
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
                    id: row.get_uuid("id").map_err(|e| RepositoryError::QueryFailed {
                        query: "find_epg_source_by_id".to_string(),
                        message: format!("Failed to parse id: {}", e),
                    })?,
                    name: row.get("name"),
                    source_type,
                    url: row.get("url"),
                    update_cron: row.get("update_cron"),
                    username: row.get("username"),
                    password: row.get("password"),
                    timezone: row.get("timezone"),
                    timezone_detected: row.get("timezone_detected"),
                    time_offset: row.get("time_offset"),
                    created_at: row.get_datetime("created_at"),
                    updated_at: row.get_datetime("updated_at"),
                    last_ingested_at: row.get_datetime_opt("last_ingested_at"),
                    is_active: row.get("is_active"),
                };
                
                Ok(Some(epg_source))
            },
            None => Ok(None),
        }
    }

    /// Get proxy by ULID (public identifier)
    pub async fn get_by_ulid(&self, ulid: &str) -> RepositoryResult<Option<StreamProxy>> {
        let row = sqlx::query(
            r#"
            SELECT id, ulid, name, description, proxy_mode,
                   upstream_timeout, buffer_size, max_concurrent_streams, 
                   starting_channel_number, created_at, updated_at, last_generated_at, is_active, auto_regenerate
            FROM stream_proxies 
            WHERE ulid = ?
            "#
        )
        .bind(ulid)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| RepositoryError::QueryFailed {
            query: "get_by_ulid".to_string(),
            message: e.to_string(),
        })?;

        match row {
            Some(row) => {
                let proxy_mode = match row.get::<String, _>("proxy_mode").as_str() {
                    "redirect" => StreamProxyMode::Redirect,
                    "proxy" => StreamProxyMode::Proxy,
                    _ => StreamProxyMode::Redirect,
                };
                
                let proxy = StreamProxy {
                    id: row.get_uuid("id").map_err(|e| RepositoryError::QueryFailed {
                        query: "get_by_ulid".to_string(),
                        message: format!("Failed to parse id: {}", e),
                    })?,
                    ulid: row.get("ulid"),
                    name: row.get("name"),
                    description: row.get("description"),
                    proxy_mode,
                    upstream_timeout: row.get::<Option<i64>, _>("upstream_timeout").map(|v| v as i32),
                    buffer_size: row.get::<Option<i64>, _>("buffer_size").map(|v| v as i32),
                    max_concurrent_streams: row.get::<Option<i64>, _>("max_concurrent_streams").map(|v| v as i32),
                    starting_channel_number: row.get::<i64, _>("starting_channel_number") as i32,
                    created_at: row.get_datetime("created_at"),
                    updated_at: row.get_datetime("updated_at"),
                    last_generated_at: row.get_datetime_opt("last_generated_at"),
                    is_active: row.get("is_active"),
                    auto_regenerate: row.get("auto_regenerate"),
                };
                
                Ok(Some(proxy))
            },
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
            SELECT id, ulid, name, description, proxy_mode,
                   upstream_timeout, buffer_size, max_concurrent_streams, 
                   starting_channel_number, created_at, updated_at, last_generated_at, is_active, auto_regenerate
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
            Some(row) => {
                let proxy_mode = match row.get::<String, _>("proxy_mode").as_str() {
                    "redirect" => StreamProxyMode::Redirect,
                    "proxy" => StreamProxyMode::Proxy,
                    _ => StreamProxyMode::Redirect,
                };
                
                let proxy = StreamProxy {
                    id: row.get_uuid("id").map_err(|e| RepositoryError::QueryFailed {
                        query: "find_by_id".to_string(),
                        message: format!("Failed to parse id: {}", e),
                    })?,
                    ulid: row.get("ulid"),
                    name: row.get("name"),
                    description: row.get("description"),
                    proxy_mode,
                    upstream_timeout: row.get::<Option<i64>, _>("upstream_timeout").map(|v| v as i32),
                    buffer_size: row.get::<Option<i64>, _>("buffer_size").map(|v| v as i32),
                    max_concurrent_streams: row.get::<Option<i64>, _>("max_concurrent_streams").map(|v| v as i32),
                    starting_channel_number: row.get::<i64, _>("starting_channel_number") as i32,
                    created_at: row.get_datetime("created_at"),
                    updated_at: row.get_datetime("updated_at"),
                    last_generated_at: row.get_datetime_opt("last_generated_at"),
                    is_active: row.get("is_active"),
                    auto_regenerate: row.get("auto_regenerate"),
                };
                
                Ok(Some(proxy))
            },
            None => Ok(None),
        }
    }

    async fn find_all(&self, _query: Self::Query) -> RepositoryResult<Vec<StreamProxy>> {
        let rows = sqlx::query(
            r#"
            SELECT id, ulid, name, description, proxy_mode,
                   upstream_timeout, buffer_size, max_concurrent_streams, 
                   starting_channel_number, created_at, updated_at, last_generated_at, is_active, auto_regenerate
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
            let proxy_mode = match row.get::<String, _>("proxy_mode").as_str() {
                "redirect" => StreamProxyMode::Redirect,
                "proxy" => StreamProxyMode::Proxy,
                _ => StreamProxyMode::Redirect,
            };
            
            let proxy = StreamProxy {
                id: row.get_uuid("id").map_err(|e| RepositoryError::QueryFailed {
                    query: "find_all".to_string(),
                    message: format!("Failed to parse id: {}", e),
                })?,
                ulid: row.get("ulid"),
                name: row.get("name"),
                description: row.get("description"),
                proxy_mode,
                upstream_timeout: row.get::<Option<i64>, _>("upstream_timeout").map(|v| v as i32),
                buffer_size: row.get::<Option<i64>, _>("buffer_size").map(|v| v as i32),
                max_concurrent_streams: row.get::<Option<i64>, _>("max_concurrent_streams").map(|v| v as i32),
                starting_channel_number: row.get::<i64, _>("starting_channel_number") as i32,
                created_at: row.get_datetime("created_at"),
                updated_at: row.get_datetime("updated_at"),
                last_generated_at: row.get_datetime_opt("last_generated_at"),
                is_active: row.get("is_active"),
                auto_regenerate: row.get("auto_regenerate"),
            };
            
            proxies.push(proxy);
        }

        Ok(proxies)
    }

    async fn create(&self, request: Self::CreateRequest) -> RepositoryResult<StreamProxy> {
        self.create_with_relationships(request).await
    }

    async fn update(&self, id: Uuid, request: Self::UpdateRequest) -> RepositoryResult<StreamProxy> {
        self.update_with_relationships(id, request).await
    }

    async fn delete(&self, id: Uuid) -> RepositoryResult<()> {
        let id_str = id.to_string();
        
        let result = sqlx::query!(
            "DELETE FROM stream_proxies WHERE id = ?",
            id_str
        )
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