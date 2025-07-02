//! Stream Proxy Repository
//!
//! This module provides data access operations for stream proxies and their relationships.

use async_trait::async_trait;
use sqlx::SqlitePool;
use uuid::Uuid;
use chrono::{DateTime, Utc};
use ulid::Ulid;

use crate::{
    models::{
        StreamProxy, StreamProxyMode, StreamProxyCreateRequest, StreamProxyUpdateRequest,
        ProxySource, ProxyEpgSource, ProxyFilter,
    },
    repositories::traits::{Repository, QueryParams},
    errors::{RepositoryResult, RepositoryError},
};

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

        sqlx::query!(
            r#"
            INSERT INTO stream_proxies (
                id, ulid, name, description, proxy_mode, upstream_timeout, 
                buffer_size, max_concurrent_streams, starting_channel_number,
                created_at, updated_at, is_active
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
            proxy_id_str,
            ulid,
            request.name,
            request.description,
            proxy_mode_str,
            upstream_timeout,
            buffer_size,
            max_concurrent_streams,
            starting_channel_number,
            now_str,
            now_str,
            true
        )
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

        // Add EPG sources
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
                is_active = ?, updated_at = ?
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
        let rows = sqlx::query_as!(
            ProxySource,
            r#"
            SELECT proxy_id as "proxy_id: Uuid", source_id as "source_id: Uuid", 
                   priority_order as "priority_order: i32", created_at as "created_at: DateTime<Utc>"
            FROM proxy_sources 
            WHERE proxy_id = ?
            ORDER BY priority_order ASC
            "#,
            proxy_id_str
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| RepositoryError::QueryFailed {
            query: "get_proxy_sources".to_string(),
            message: e.to_string(),
        })?;

        Ok(rows)
    }

    /// Get proxy EPG sources with priority order
    pub async fn get_proxy_epg_sources(&self, proxy_id: Uuid) -> RepositoryResult<Vec<ProxyEpgSource>> {
        let proxy_id_str = proxy_id.to_string();
        let rows = sqlx::query_as!(
            ProxyEpgSource,
            r#"
            SELECT proxy_id as "proxy_id: Uuid", epg_source_id as "epg_source_id: Uuid", 
                   priority_order as "priority_order: i32", created_at as "created_at: DateTime<Utc>"
            FROM proxy_epg_sources 
            WHERE proxy_id = ?
            ORDER BY priority_order ASC
            "#,
            proxy_id_str
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| RepositoryError::QueryFailed {
            query: "get_proxy_epg_sources".to_string(),
            message: e.to_string(),
        })?;

        Ok(rows)
    }

    /// Get proxy filters with priority order
    pub async fn get_proxy_filters(&self, proxy_id: Uuid) -> RepositoryResult<Vec<ProxyFilter>> {
        let proxy_id_str = proxy_id.to_string();
        let rows = sqlx::query_as!(
            ProxyFilter,
            r#"
            SELECT proxy_id as "proxy_id: Uuid", filter_id as "filter_id: Uuid", 
                   priority_order as "priority_order: i32", is_active, created_at as "created_at: DateTime<Utc>"
            FROM proxy_filters 
            WHERE proxy_id = ?
            ORDER BY priority_order ASC
            "#,
            proxy_id_str
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| RepositoryError::QueryFailed {
            query: "get_proxy_filters".to_string(),
            message: e.to_string(),
        })?;

        Ok(rows)
    }

    /// Get proxy by ULID (public identifier)
    pub async fn get_by_ulid(&self, ulid: &str) -> RepositoryResult<Option<StreamProxy>> {
        let row = sqlx::query_as!(
            StreamProxy,
            r#"
            SELECT id as "id: Uuid", ulid, name, description,
                   proxy_mode as "proxy_mode: StreamProxyMode",
                   upstream_timeout as "upstream_timeout: i32", 
                   buffer_size as "buffer_size: i32", 
                   max_concurrent_streams as "max_concurrent_streams: i32", 
                   starting_channel_number as "starting_channel_number: i32",
                   created_at as "created_at: DateTime<Utc>", 
                   updated_at as "updated_at: DateTime<Utc>",
                   last_generated_at as "last_generated_at?: DateTime<Utc>",
                   is_active
            FROM stream_proxies 
            WHERE ulid = ?
            "#,
            ulid
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| RepositoryError::QueryFailed {
            query: "get_by_ulid".to_string(),
            message: e.to_string(),
        })?;

        Ok(row)
    }
}

#[async_trait]
impl Repository<StreamProxy, Uuid> for StreamProxyRepository {
    type CreateRequest = StreamProxyCreateRequest;
    type UpdateRequest = StreamProxyUpdateRequest;
    type Query = QueryParams;

    async fn find_by_id(&self, id: Uuid) -> RepositoryResult<Option<StreamProxy>> {
        let id_str = id.to_string();
        let row = sqlx::query_as!(
            StreamProxy,
            r#"
            SELECT id as "id: Uuid", ulid, name, description,
                   proxy_mode as "proxy_mode: StreamProxyMode",
                   upstream_timeout as "upstream_timeout: i32", 
                   buffer_size as "buffer_size: i32", 
                   max_concurrent_streams as "max_concurrent_streams: i32", 
                   starting_channel_number as "starting_channel_number: i32",
                   created_at as "created_at: DateTime<Utc>", 
                   updated_at as "updated_at: DateTime<Utc>",
                   last_generated_at as "last_generated_at?: DateTime<Utc>",
                   is_active
            FROM stream_proxies 
            WHERE id = ?
            "#,
            id_str
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| RepositoryError::QueryFailed {
            query: "find_by_id".to_string(),
            message: e.to_string(),
        })?;

        Ok(row)
    }

    async fn find_all(&self, _query: Self::Query) -> RepositoryResult<Vec<StreamProxy>> {
        let rows = sqlx::query_as!(
            StreamProxy,
            r#"
            SELECT id as "id: Uuid", ulid, name, description,
                   proxy_mode as "proxy_mode: StreamProxyMode",
                   upstream_timeout as "upstream_timeout: i32", 
                   buffer_size as "buffer_size: i32", 
                   max_concurrent_streams as "max_concurrent_streams: i32", 
                   starting_channel_number as "starting_channel_number: i32",
                   created_at as "created_at: DateTime<Utc>", 
                   updated_at as "updated_at: DateTime<Utc>",
                   last_generated_at as "last_generated_at?: DateTime<Utc>",
                   is_active
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

        Ok(rows)
    }

    async fn create(&self, request: Self::CreateRequest) -> RepositoryResult<StreamProxy> {
        self.create_with_relationships(request).await
    }

    async fn update(&self, id: Uuid, request: Self::UpdateRequest) -> RepositoryResult<StreamProxy> {
        self.update_with_relationships(id, request).await
    }

    async fn delete(&self, _id: Uuid) -> RepositoryResult<()> {
        // TODO: Implement when database schema is ready
        Err(RepositoryError::QueryFailed {
            query: "delete".to_string(),
            message: "Database schema not ready - migration needed".to_string(),
        })
    }

    async fn count(&self, _query: Self::Query) -> RepositoryResult<u64> {
        Ok(0)
    }
}