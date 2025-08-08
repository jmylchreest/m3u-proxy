//! EPG source repository implementation
//!
//! This module provides the concrete implementation of the repository pattern
//! for EPG sources, encapsulating all database operations related to
//! EPG source management including timezone handling and statistics.

use async_trait::async_trait;
use sqlx::{Pool, Sqlite, Row};
use uuid::Uuid;
use std::collections::HashMap;
use chrono::{DateTime, Utc};

use crate::errors::{RepositoryError, RepositoryResult};
use crate::models::{EpgSource, EpgSourceType, EpgSourceCreateRequest, EpgSourceUpdateRequest};
use crate::utils::sqlite::SqliteRowExt;
use crate::utils::uuid_parser::parse_uuid_flexible;
use super::traits::{Repository, BulkRepository, PaginatedRepository, QueryParams, PaginatedResult};

/// Query parameters specific to EPG sources
#[derive(Debug, Clone, Default)]
pub struct EpgSourceQuery {
    /// Base query parameters
    pub base: QueryParams,
    /// Filter by source type
    pub source_type: Option<EpgSourceType>,
    /// Filter by enabled status
    pub is_active: Option<bool>,
    /// Filter by recent activity (has been ingested recently)
    pub has_recent_activity: Option<bool>,
}

impl EpgSourceQuery {
    /// Create new empty query
    pub fn new() -> Self {
        Self::default()
    }

    /// Filter by source type
    pub fn source_type(mut self, source_type: EpgSourceType) -> Self {
        self.source_type = Some(source_type);
        self
    }

    /// Filter by active status
    pub fn active(mut self, is_active: bool) -> Self {
        self.is_active = Some(is_active);
        self
    }

    /// Filter by recent activity
    pub fn recent_activity(mut self, has_recent: bool) -> Self {
        self.has_recent_activity = Some(has_recent);
        self
    }
}

/// EPG source repository implementation
pub struct EpgSourceRepository {
    pool: Pool<Sqlite>,
}

impl EpgSourceRepository {
    /// Create a new EPG source repository
    pub fn new(pool: Pool<Sqlite>) -> Self {
        Self { pool }
    }

    /// Build WHERE clause from query parameters
    fn build_where_clause(&self, query: &EpgSourceQuery) -> (String, Vec<String>) {
        let mut conditions = Vec::new();
        let mut params = Vec::new();

        if let Some(source_type) = &query.source_type {
            conditions.push("source_type = ?".to_string());
            params.push(source_type.to_string());
        }

        if let Some(is_active) = query.is_active {
            conditions.push("is_active = ?".to_string());
            params.push(if is_active { "1" } else { "0" }.to_string());
        }

        if let Some(search) = &query.base.search {
            conditions.push("(name LIKE ? OR url LIKE ?)".to_string());
            let search_param = format!("%{}%", search);
            params.push(search_param.clone());
            params.push(search_param);
        }

        if let Some(has_recent) = query.has_recent_activity {
            if has_recent {
                conditions.push("last_ingested_at IS NOT NULL AND last_ingested_at > datetime('now', '-7 days')".to_string());
            } else {
                conditions.push("(last_ingested_at IS NULL OR last_ingested_at <= datetime('now', '-7 days'))".to_string());
            }
        }

        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!(" WHERE {}", conditions.join(" AND "))
        };

        (where_clause, params)
    }

    /// Build ORDER BY clause from query parameters
    fn build_order_clause(&self, query: &EpgSourceQuery) -> String {
        let sort_field = query.base.sort_by.as_deref().unwrap_or("name");
        let direction = if query.base.sort_ascending { "ASC" } else { "DESC" };
        
        match sort_field {
            "name" => format!(" ORDER BY name {}", direction),
            "created_at" => format!(" ORDER BY created_at {}", direction),
            "updated_at" => format!(" ORDER BY updated_at {}", direction),
            "last_ingested_at" => format!(" ORDER BY last_ingested_at {} NULLS LAST", direction),
            "source_type" => format!(" ORDER BY source_type {}, name ASC", direction),
            _ => " ORDER BY name ASC".to_string(),
        }
    }

    /// Convert database row to EpgSource model
    fn row_to_model(&self, row: &sqlx::sqlite::SqliteRow) -> RepositoryResult<EpgSource> {
        let source_type_str: String = row.try_get("source_type")
            .map_err(|e| RepositoryError::query_failed("row_to_model", e.to_string()))?;
        
        let source_type = match source_type_str.as_str() {
            "xmltv" => EpgSourceType::Xmltv,
            "xtream" => EpgSourceType::Xtream,
            _ => return Err(RepositoryError::query_failed(
                "row_to_model", 
                format!("Invalid source type: {}", source_type_str)
            )),
        };

        Ok(EpgSource {
            id: parse_uuid_flexible(&row.try_get::<String, _>("id")?)
                .map_err(|e| RepositoryError::query_failed("parse_uuid", e.to_string()))?,
            name: row.try_get("name")?,
            source_type,
            url: row.try_get("url")?,
            update_cron: row.try_get("update_cron")?,
            username: row.try_get("username")?,
            password: row.try_get("password")?,
            original_timezone: row.try_get("original_timezone")?,
            time_offset: row.try_get("time_offset")?,
            created_at: row.get_datetime("created_at"),
            updated_at: row.get_datetime("updated_at"),
            last_ingested_at: row.get_datetime_opt("last_ingested_at"),
            is_active: row.try_get("is_active")?,
        })
    }

    /// Get EPG sources with statistics
    pub async fn find_with_stats(&self, query: EpgSourceQuery) -> RepositoryResult<Vec<EpgSourceWithStats>> {
        let (where_clause, params) = self.build_where_clause(&query);
        let order_clause = self.build_order_clause(&query);
        
        let limit_clause = if let Some(limit) = query.base.limit {
            format!(" LIMIT {}", limit)
        } else {
            String::new()
        };

        let sql = format!(
            "SELECT e.*, 
             COALESCE(ec.channel_count, 0) as channel_count,
             COALESCE(ep.program_count, 0) as program_count
             FROM epg_sources e
             LEFT JOIN (
                 SELECT source_id, COUNT(*) as channel_count
                 FROM epg_channels
                 GROUP BY source_id
             ) ec ON e.id = ec.source_id
             LEFT JOIN (
                 SELECT source_id, COUNT(*) as program_count
                 FROM epg_programs
                 GROUP BY source_id
             ) ep ON e.id = ep.source_id
             {}{}{}",
            where_clause, order_clause, limit_clause
        );

        let mut query_builder = sqlx::query(&sql);
        for param in params {
            query_builder = query_builder.bind(param);
        }

        let rows = query_builder.fetch_all(&self.pool).await
            .map_err(|e| RepositoryError::query_failed(&sql, e.to_string()))?;

        let mut results = Vec::new();
        for row in rows {
            let source = self.row_to_model(&row)?;
            let channel_count: i64 = row.try_get("channel_count")?;
            let program_count: i64 = row.try_get("program_count")?;

            results.push(EpgSourceWithStats {
                source,
                channel_count,
                program_count,
                next_scheduled_update: None, // This would require cron parsing logic
            });
        }

        Ok(results)
    }
}

/// EPG source with statistics
#[derive(Debug, Clone)]
pub struct EpgSourceWithStats {
    pub source: EpgSource,
    pub channel_count: i64,
    pub program_count: i64,
    pub next_scheduled_update: Option<DateTime<Utc>>,
}

#[async_trait]
impl Repository<EpgSource, Uuid> for EpgSourceRepository {
    type CreateRequest = EpgSourceCreateRequest;
    type UpdateRequest = EpgSourceUpdateRequest;
    type Query = EpgSourceQuery;

    async fn find_by_id(&self, id: Uuid) -> RepositoryResult<Option<EpgSource>> {
        let sql = "SELECT id, name, source_type, url, update_cron, username, password, 
                   original_timezone, time_offset, created_at, updated_at, last_ingested_at, is_active
                   FROM epg_sources WHERE id = ?";

        let row = sqlx::query(sql)
            .bind(id.to_string())
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| RepositoryError::query_failed(sql, e.to_string()))?;

        match row {
            Some(row) => Ok(Some(self.row_to_model(&row)?)),
            None => Ok(None),
        }
    }

    async fn find_all(&self, query: Self::Query) -> RepositoryResult<Vec<EpgSource>> {
        let (where_clause, params) = self.build_where_clause(&query);
        let order_clause = self.build_order_clause(&query);
        
        let limit_clause = if let Some(limit) = query.base.limit {
            let offset = query.base.offset.unwrap_or(0);
            format!(" LIMIT {} OFFSET {}", limit, offset)
        } else {
            String::new()
        };

        let sql = format!(
            "SELECT id, name, source_type, url, update_cron, username, password, 
             original_timezone, time_offset, created_at, updated_at, last_ingested_at, is_active
             FROM epg_sources{}{}{}",
            where_clause, order_clause, limit_clause
        );

        let mut query_builder = sqlx::query(&sql);
        for param in params {
            query_builder = query_builder.bind(param);
        }

        let rows = query_builder.fetch_all(&self.pool).await
            .map_err(|e| RepositoryError::query_failed(&sql, e.to_string()))?;

        let mut results = Vec::new();
        for row in rows {
            results.push(self.row_to_model(&row)?);
        }

        Ok(results)
    }

    async fn create(&self, request: Self::CreateRequest) -> RepositoryResult<EpgSource> {
        let id = Uuid::new_v4();
        let now = Utc::now();
        let time_offset = request.time_offset.unwrap_or_else(|| "+00:00".to_string());

        let sql = "INSERT INTO epg_sources 
                   (id, name, source_type, url, update_cron, username, password, 
                    original_timezone, time_offset, created_at, updated_at, is_active)
                   VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1)";

        sqlx::query(sql)
            .bind(id.to_string())
            .bind(&request.name)
            .bind(request.source_type.to_string())
            .bind(&request.url)
            .bind(&request.update_cron)
            .bind(&request.username)
            .bind(&request.password)
            .bind(&request.timezone)
            .bind(&time_offset)
            .bind(now.to_rfc3339())
            .bind(now.to_rfc3339())
            .execute(&self.pool)
            .await
            .map_err(|e| RepositoryError::query_failed(sql, e.to_string()))?;

        // Return the created entity
        self.find_by_id(id).await?
            .ok_or_else(|| RepositoryError::record_not_found("epg_sources", "id", id.to_string()))
    }

    async fn update(&self, id: Uuid, request: Self::UpdateRequest) -> RepositoryResult<EpgSource> {
        let now = Utc::now();
        let time_offset = request.time_offset.unwrap_or_else(|| "+00:00".to_string());

        let sql = "UPDATE epg_sources SET 
                   name = ?, source_type = ?, url = ?, update_cron = ?, 
                   username = ?, password = ?, original_timezone = ?, 
                   time_offset = ?, is_active = ?, updated_at = ?
                   WHERE id = ?";

        let result = sqlx::query(sql)
            .bind(&request.name)
            .bind(request.source_type.to_string())
            .bind(&request.url)
            .bind(&request.update_cron)
            .bind(&request.username)
            .bind(&request.password)
            .bind(&request.timezone)
            .bind(&time_offset)
            .bind(request.is_active)
            .bind(now.to_rfc3339())
            .bind(id.to_string())
            .execute(&self.pool)
            .await
            .map_err(|e| RepositoryError::query_failed(sql, e.to_string()))?;

        if result.rows_affected() == 0 {
            return Err(RepositoryError::record_not_found("epg_sources", "id", id.to_string()));
        }

        // Return the updated entity
        self.find_by_id(id).await?
            .ok_or_else(|| RepositoryError::record_not_found("epg_sources", "id", id.to_string()))
    }

    async fn delete(&self, id: Uuid) -> RepositoryResult<()> {
        let sql = "DELETE FROM epg_sources WHERE id = ?";

        let result = sqlx::query(sql)
            .bind(id.to_string())
            .execute(&self.pool)
            .await
            .map_err(|e| RepositoryError::query_failed(sql, e.to_string()))?;

        if result.rows_affected() == 0 {
            return Err(RepositoryError::record_not_found("epg_sources", "id", id.to_string()));
        }

        Ok(())
    }

    async fn count(&self, query: Self::Query) -> RepositoryResult<u64> {
        let (where_clause, params) = self.build_where_clause(&query);
        let sql = format!("SELECT COUNT(*) FROM epg_sources{}", where_clause);

        let mut query_builder = sqlx::query_scalar(&sql);
        for param in params {
            query_builder = query_builder.bind(param);
        }

        let count: i64 = query_builder.fetch_one(&self.pool).await
            .map_err(|e| RepositoryError::query_failed(&sql, e.to_string()))?;

        Ok(count as u64)
    }
}

#[async_trait]
impl BulkRepository<EpgSource, Uuid> for EpgSourceRepository {
    async fn create_bulk(&self, requests: Vec<Self::CreateRequest>) -> RepositoryResult<Vec<EpgSource>> {
        let mut tx = self.pool.begin().await
            .map_err(|e| RepositoryError::query_failed("begin_transaction", e.to_string()))?;

        let mut created_sources = Vec::new();
        let now = Utc::now();

        for request in requests {
            let id = Uuid::new_v4();
            let time_offset = request.time_offset.unwrap_or_else(|| "+00:00".to_string());

            let sql = "INSERT INTO epg_sources 
                       (id, name, source_type, url, update_cron, username, password, 
                        original_timezone, time_offset, created_at, updated_at, is_active)
                       VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1)";

            sqlx::query(sql)
                .bind(id.to_string())
                .bind(&request.name)
                .bind(request.source_type.to_string())
                .bind(&request.url)
                .bind(&request.update_cron)
                .bind(&request.username)
                .bind(&request.password)
                .bind(&request.timezone)
                .bind(&time_offset)
                .bind(now.to_rfc3339())
                .bind(now.to_rfc3339())
                .execute(&mut *tx)
                .await
                .map_err(|e| RepositoryError::query_failed(sql, e.to_string()))?;

            // Fetch the created record
            let created_source = EpgSource {
                id,
                name: request.name,
                source_type: request.source_type,
                url: request.url,
                update_cron: request.update_cron,
                username: request.username,
                password: request.password,
                original_timezone: request.timezone,
                time_offset,
                created_at: now,
                updated_at: now,
                last_ingested_at: None,
                is_active: true,
            };

            created_sources.push(created_source);
        }

        tx.commit().await
            .map_err(|e| RepositoryError::query_failed("commit_transaction", e.to_string()))?;

        Ok(created_sources)
    }

    async fn update_bulk(&self, updates: HashMap<Uuid, Self::UpdateRequest>) -> RepositoryResult<Vec<EpgSource>> {
        let mut tx = self.pool.begin().await
            .map_err(|e| RepositoryError::query_failed("begin_transaction", e.to_string()))?;

        let mut updated_sources = Vec::new();
        let now = Utc::now();

        for (id, request) in updates {
            let time_offset = request.time_offset.unwrap_or_else(|| "+00:00".to_string());

            let sql = "UPDATE epg_sources SET 
                       name = ?, source_type = ?, url = ?, update_cron = ?, 
                       username = ?, password = ?, original_timezone = ?, 
                       time_offset = ?, is_active = ?, updated_at = ?
                       WHERE id = ?";

            let result = sqlx::query(sql)
                .bind(&request.name)
                .bind(request.source_type.to_string())
                .bind(&request.url)
                .bind(&request.update_cron)
                .bind(&request.username)
                .bind(&request.password)
                .bind(&request.timezone)
                .bind(&time_offset)
                .bind(request.is_active)
                .bind(now.to_rfc3339())
                .bind(id.to_string())
                .execute(&mut *tx)
                .await
                .map_err(|e| RepositoryError::query_failed(sql, e.to_string()))?;

            if result.rows_affected() == 0 {
                return Err(RepositoryError::record_not_found("epg_sources", "id", id.to_string()));
            }

            // Build the updated source (could fetch from DB, but this is more efficient)
            let updated_source = EpgSource {
                id,
                name: request.name,
                source_type: request.source_type,
                url: request.url,
                update_cron: request.update_cron,
                username: request.username,
                password: request.password,
                original_timezone: request.timezone,
                time_offset,
                created_at: now, // This would ideally be fetched, but for bulk ops we approximate
                updated_at: now,
                last_ingested_at: None, // This would need to be fetched from existing record
                is_active: request.is_active,
            };

            updated_sources.push(updated_source);
        }

        tx.commit().await
            .map_err(|e| RepositoryError::query_failed("commit_transaction", e.to_string()))?;

        Ok(updated_sources)
    }

    async fn delete_bulk(&self, ids: Vec<Uuid>) -> RepositoryResult<u64> {
        if ids.is_empty() {
            return Ok(0);
        }

        let placeholders = ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!("DELETE FROM epg_sources WHERE id IN ({})", placeholders);

        let mut query_builder = sqlx::query(&sql);
        for id in ids {
            query_builder = query_builder.bind(id.to_string());
        }

        let result = query_builder.execute(&self.pool).await
            .map_err(|e| RepositoryError::query_failed(&sql, e.to_string()))?;

        Ok(result.rows_affected())
    }

    async fn find_by_ids(&self, ids: Vec<Uuid>) -> RepositoryResult<Vec<EpgSource>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }

        let placeholders = ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!(
            "SELECT id, name, source_type, url, update_cron, username, password, 
             original_timezone, time_offset, created_at, updated_at, last_ingested_at, is_active
             FROM epg_sources WHERE id IN ({}) ORDER BY name",
            placeholders
        );

        let mut query_builder = sqlx::query(&sql);
        for id in ids {
            query_builder = query_builder.bind(id.to_string());
        }

        let rows = query_builder.fetch_all(&self.pool).await
            .map_err(|e| RepositoryError::query_failed(&sql, e.to_string()))?;

        let mut results = Vec::new();
        for row in rows {
            results.push(self.row_to_model(&row)?);
        }

        Ok(results)
    }
}

#[async_trait]
impl PaginatedRepository<EpgSource, Uuid> for EpgSourceRepository {
    type PaginatedResult = PaginatedResult<EpgSource>;

    async fn find_paginated(
        &self,
        query: Self::Query,
        page: u32,
        limit: u32,
    ) -> RepositoryResult<Self::PaginatedResult> {
        let offset = (page.saturating_sub(1)) * limit;
        
        // Get total count
        let total_count = self.count(query.clone()).await?;
        
        // Get items for this page
        let mut page_query = query;
        page_query.base.limit = Some(limit);
        page_query.base.offset = Some(offset);
        
        let items = self.find_all(page_query).await?;
        
        Ok(PaginatedResult::new(items, page, limit, total_count))
    }
}

impl EpgSourceRepository {
    /// Additional domain-specific methods for EPG source operations
    
    /// Get EPG sources with statistics (channel counts, program counts, etc.)
    pub async fn list_with_stats(&self) -> RepositoryResult<Vec<crate::models::EpgSourceWithStats>> {
        let rows = sqlx::query(
            "SELECT es.id, es.name, es.source_type, es.url, es.update_cron, es.username, es.password,
             es.original_timezone, es.time_offset, es.created_at, es.updated_at, es.last_ingested_at, es.is_active,
             COUNT(DISTINCT ec.id) as channel_count,
             COUNT(DISTINCT ep.id) as program_count
             FROM epg_sources es
             LEFT JOIN epg_channels ec ON es.id = ec.source_id
             LEFT JOIN epg_programs ep ON es.id = ep.source_id
             GROUP BY es.id
             ORDER BY es.name"
        )
        .fetch_all(&self.pool)
        .await?;

        let mut results = Vec::new();
        for row in rows {
            let source = EpgSource {
                id: parse_uuid_flexible(&row.try_get::<String, _>("id")?)?,
                name: row.try_get("name")?,
                source_type: match row.try_get::<String, _>("source_type")?.as_str() {
                    "xmltv" => EpgSourceType::Xmltv,
                    "xtream" => EpgSourceType::Xtream,
                    _ => return Err(RepositoryError::query_failed("invalid_source_type", "Unknown EPG source type")),
                },
                url: row.try_get("url")?,
                update_cron: row.try_get("update_cron")?,
                username: row.try_get("username")?,
                password: row.try_get("password")?,
                original_timezone: row.try_get("original_timezone")?,
                time_offset: row.try_get("time_offset")?,
                created_at: row.get_datetime("created_at"),
                updated_at: row.get_datetime("updated_at"),
                last_ingested_at: row.get_datetime_opt("last_ingested_at"),
                is_active: row.try_get("is_active")?,
            };

            let channel_count: i64 = row.try_get("channel_count")?;
            let program_count: i64 = row.try_get("program_count")?;
            
            results.push(crate::models::EpgSourceWithStats {
                source,
                channel_count,
                program_count,
                next_scheduled_update: None, // TODO: Implement scheduling info
            });
        }

        Ok(results)
    }

    /// Get channel count for a specific EPG source
    pub async fn get_channel_count(&self, source_id: Uuid) -> RepositoryResult<i64> {
        use crate::repositories::traits::RepositoryHelpers;
        RepositoryHelpers::get_channel_count_for_source(&self.pool, "epg_channels", source_id).await
    }

    /// Update the last ingested timestamp for an EPG source
    pub async fn update_last_ingested(&self, source_id: Uuid) -> RepositoryResult<chrono::DateTime<chrono::Utc>> {
        use crate::repositories::traits::RepositoryHelpers;
        RepositoryHelpers::update_last_ingested(&self.pool, "epg_sources", source_id).await
    }

    /// Update detected timezone for an EPG source
    pub async fn update_detected_timezone(&self, source_id: Uuid, timezone: &str, time_offset: &str) -> RepositoryResult<()> {
        sqlx::query("UPDATE epg_sources SET original_timezone = ?, time_offset = ?, updated_at = ? WHERE id = ?")
            .bind(timezone)
            .bind(time_offset)
            .bind(chrono::Utc::now().to_rfc3339())
            .bind(source_id.to_string())
            .execute(&self.pool)
            .await?;

        Ok(())
    }
}