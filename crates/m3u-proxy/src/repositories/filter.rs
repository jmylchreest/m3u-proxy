//! Filter repository implementation
//!
//! This module provides the repository implementation for filter entities,
//! handling the persistence and querying of stream filters.

use async_trait::async_trait;
use sqlx::{Pool, Row, Sqlite};
use std::collections::HashMap;
use uuid::Uuid;

use super::traits::{
    BulkRepository, PaginatedRepository, PaginatedResult, QueryParams, Repository,
};
use crate::errors::{RepositoryError, RepositoryResult};
use crate::models::{Filter, FilterCreateRequest, FilterSourceType, FilterUpdateRequest};
use crate::utils::sqlite::SqliteRowExt;

/// Query parameters specific to filters
#[derive(Debug, Clone, Default)]
pub struct FilterQuery {
    /// Base query parameters
    pub base: QueryParams,
    /// Filter by source type
    pub source_type: Option<FilterSourceType>,
    /// Filter by enabled status
    pub enabled: Option<bool>,
    /// Filter by applied to sources
    pub applied_to_source: Option<Uuid>,
}

impl FilterQuery {
    /// Create new empty query
    pub fn new() -> Self {
        Self::default()
    }

    /// Filter by source type
    pub fn source_type(mut self, source_type: FilterSourceType) -> Self {
        self.source_type = Some(source_type);
        self
    }

    /// Filter by enabled status
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = Some(enabled);
        self
    }

    /// Filter by applied to source
    pub fn applied_to_source(mut self, source_id: Uuid) -> Self {
        self.applied_to_source = Some(source_id);
        self
    }

    /// Set base query parameters
    pub fn with_base(mut self, base: QueryParams) -> Self {
        self.base = base;
        self
    }
}

/// Repository implementation for filters
#[derive(Clone)]
pub struct FilterRepository {
    pool: Pool<Sqlite>,
}

impl FilterRepository {
    /// Create a new filter repository
    pub fn new(pool: Pool<Sqlite>) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl Repository<Filter, Uuid> for FilterRepository {
    type CreateRequest = FilterCreateRequest;
    type UpdateRequest = FilterUpdateRequest;
    type Query = FilterQuery;

    async fn find_by_id(&self, id: Uuid) -> RepositoryResult<Option<Filter>> {
        let id_str = id.to_string();
        let row = sqlx::query(
            r#"
            SELECT id, name, source_type, starting_channel_number, is_inverse, is_system_default, condition_tree, created_at, updated_at
            FROM filters
            WHERE id = ?
            "#
        )
        .bind(id_str)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| RepositoryError::QueryFailed {
            query: "find_filter_by_id".to_string(),
            message: e.to_string(),
        })?;

        match row {
            Some(row) => {
                let filter = Filter {
                    id: Uuid::parse_str(&row.get::<String, _>("id")).map_err(|e| {
                        RepositoryError::QueryFailed {
                            query: "parse_filter_id".to_string(),
                            message: e.to_string(),
                        }
                    })?,
                    name: row.get("name"),
                    source_type: match row.get::<String, _>("source_type").as_str() {
                        "stream" => FilterSourceType::Stream,
                        "epg" => FilterSourceType::Epg,
                        _ => FilterSourceType::Stream, // Default fallback
                    },
                    starting_channel_number: row.get("starting_channel_number"),
                    is_inverse: row.get("is_inverse"),
                    is_system_default: row.get("is_system_default"),
                    condition_tree: row.get("condition_tree"),
                    created_at: row.get_datetime("created_at"),
                    updated_at: row.get_datetime("updated_at"),
                };
                Ok(Some(filter))
            }
            None => Ok(None),
        }
    }

    async fn find_all(&self, query: Self::Query) -> RepositoryResult<Vec<Filter>> {
        let mut sql = "SELECT id, name, source_type, starting_channel_number, is_inverse, is_system_default, condition_tree, created_at, updated_at FROM filters WHERE 1=1".to_string();
        let mut params: Vec<String> = Vec::new();

        if let Some(source_type) = &query.source_type {
            sql.push_str(" AND source_type = ?");
            params.push(match source_type {
                FilterSourceType::Stream => "stream".to_string(),
                FilterSourceType::Epg => "epg".to_string(),
            });
        }

        if let Some(enabled) = query.enabled {
            // For now, we assume all filters in the database are enabled
            // This is a placeholder for future implementation
            if !enabled {
                return Ok(Vec::new());
            }
        }

        sql.push_str(" ORDER BY name");

        let mut query_builder = sqlx::query(&sql);
        for param in params {
            query_builder = query_builder.bind(param);
        }

        let rows = query_builder.fetch_all(&self.pool).await.map_err(|e| {
            RepositoryError::QueryFailed {
                query: "find_all_filters".to_string(),
                message: e.to_string(),
            }
        })?;

        let mut filters = Vec::new();
        for row in rows {
            let id_str: String = row.get("id");
            let filter = Filter {
                id: Uuid::parse_str(&id_str).map_err(|e| RepositoryError::QueryFailed {
                    query: "parse_filter_id".to_string(),
                    message: e.to_string(),
                })?,
                name: row.get("name"),
                source_type: match row.get::<String, _>("source_type").as_str() {
                    "stream" => FilterSourceType::Stream,
                    "epg" => FilterSourceType::Epg,
                    _ => FilterSourceType::Stream,
                },
                starting_channel_number: row.get("starting_channel_number"),
                is_inverse: row.get("is_inverse"),
                is_system_default: row.get("is_system_default"),
                condition_tree: row.get("condition_tree"),
                created_at: row.get_datetime("created_at"),
                updated_at: row.get_datetime("updated_at"),
            };
            filters.push(filter);
        }

        Ok(filters)
    }

    async fn create(&self, request: Self::CreateRequest) -> RepositoryResult<Filter> {
        let id = Uuid::new_v4();
        let id_str = id.to_string();
        let now = chrono::Utc::now();
        let now_str = now.to_rfc3339();
        let source_type_str = match request.source_type {
            FilterSourceType::Stream => "stream",
            FilterSourceType::Epg => "epg",
        };

        // For now, we'll store the filter expression as-is in condition_tree
        // In a full implementation, this would be parsed into a proper JSON tree
        let condition_tree = format!(r#"{{"expression":"{}"}}"#, request.filter_expression);

        sqlx::query(
            r#"
            INSERT INTO filters (id, name, source_type, starting_channel_number, is_inverse, condition_tree, created_at, updated_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            "#
        )
        .bind(id_str)
        .bind(request.name.clone())
        .bind(source_type_str)
        .bind(request.starting_channel_number)
        .bind(request.is_inverse)
        .bind(condition_tree.clone())
        .bind(now_str.clone())
        .bind(now_str)
        .execute(&self.pool)
        .await
        .map_err(|e| RepositoryError::QueryFailed {
            query: "create_filter".to_string(),
            message: e.to_string(),
        })?;

        Ok(Filter {
            id,
            name: request.name,
            source_type: request.source_type,
            starting_channel_number: request.starting_channel_number,
            is_inverse: request.is_inverse,
            is_system_default: request.is_system_default,
            condition_tree: request.filter_expression,
            created_at: now,
            updated_at: now,
        })
    }

    async fn update(&self, id: Uuid, request: Self::UpdateRequest) -> RepositoryResult<Filter> {
        let id_str = id.to_string();
        let now = chrono::Utc::now();
        let now_str = now.to_rfc3339();
        let source_type_str = match request.source_type {
            FilterSourceType::Stream => "stream",
            FilterSourceType::Epg => "epg",
        };

        let condition_tree = format!(r#"{{"expression":"{}"}}"#, request.filter_expression);

        sqlx::query(
            r#"
            UPDATE filters
            SET name = ?, source_type = ?, starting_channel_number = ?, is_inverse = ?, condition_tree = ?, updated_at = ?
            WHERE id = ?
            "#
        )
        .bind(request.name.clone())
        .bind(source_type_str)
        .bind(request.starting_channel_number)
        .bind(request.is_inverse)
        .bind(condition_tree)
        .bind(now_str)
        .bind(id_str.clone())
        .execute(&self.pool)
        .await
        .map_err(|e| RepositoryError::QueryFailed {
            query: "update_filter".to_string(),
            message: e.to_string(),
        })?;

        // Return the updated filter
        self.find_by_id(id)
            .await?
            .ok_or_else(|| RepositoryError::QueryFailed {
                query: "find_updated_filter".to_string(),
                message: format!("Filter with id {} not found after update", id_str),
            })
    }

    async fn delete(&self, id: Uuid) -> RepositoryResult<()> {
        let id_str = id.to_string();
        sqlx::query("DELETE FROM filters WHERE id = ?")
            .bind(id_str)
            .execute(&self.pool)
            .await
            .map_err(|e| RepositoryError::QueryFailed {
                query: "delete_filter".to_string(),
                message: e.to_string(),
            })?;

        Ok(())
    }

    async fn count(&self, query: Self::Query) -> RepositoryResult<u64> {
        let mut sql = "SELECT COUNT(*) as count FROM filters WHERE 1=1".to_string();
        let mut params: Vec<String> = Vec::new();

        if let Some(source_type) = &query.source_type {
            sql.push_str(" AND source_type = ?");
            params.push(match source_type {
                FilterSourceType::Stream => "stream".to_string(),
                FilterSourceType::Epg => "epg".to_string(),
            });
        }

        let mut query_builder = sqlx::query(&sql);
        for param in params {
            query_builder = query_builder.bind(param);
        }

        let row = query_builder.fetch_one(&self.pool).await.map_err(|e| {
            RepositoryError::QueryFailed {
                query: "count_filters".to_string(),
                message: e.to_string(),
            }
        })?;

        let count: i64 = row.get("count");
        Ok(count as u64)
    }
}

#[async_trait]
impl BulkRepository<Filter, Uuid> for FilterRepository {
    async fn create_bulk(
        &self,
        _requests: Vec<Self::CreateRequest>,
    ) -> RepositoryResult<Vec<Filter>> {
        // TODO: Implement bulk filter creation
        todo!("Filter bulk repository implementation")
    }

    async fn update_bulk(
        &self,
        _updates: HashMap<Uuid, Self::UpdateRequest>,
    ) -> RepositoryResult<Vec<Filter>> {
        // TODO: Implement bulk filter updates
        todo!("Filter bulk repository implementation")
    }

    async fn delete_bulk(&self, _ids: Vec<Uuid>) -> RepositoryResult<u64> {
        // TODO: Implement bulk filter deletion
        todo!("Filter bulk repository implementation")
    }

    async fn find_by_ids(&self, _ids: Vec<Uuid>) -> RepositoryResult<Vec<Filter>> {
        // TODO: Implement finding multiple filters by IDs
        todo!("Filter bulk repository implementation")
    }
}

#[async_trait]
impl PaginatedRepository<Filter, Uuid> for FilterRepository {
    type PaginatedResult = PaginatedResult<Filter>;

    async fn find_paginated(
        &self,
        _query: Self::Query,
        _page: u32,
        _limit: u32,
    ) -> RepositoryResult<Self::PaginatedResult> {
        // TODO: Implement paginated filter queries
        todo!("Filter paginated repository implementation")
    }
}
