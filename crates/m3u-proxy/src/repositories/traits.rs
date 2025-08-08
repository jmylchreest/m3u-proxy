//! Repository trait definitions
//!
//! This module defines the core traits that all repositories must implement,
//! providing a consistent interface for data access operations.

use async_trait::async_trait;
use crate::errors::RepositoryResult;
use std::collections::HashMap;

/// Core repository trait providing CRUD operations
///
/// This trait defines the standard operations that all repositories should support.
/// The generic parameters allow for flexibility in entity types and ID types.
///
/// # Type Parameters
///
/// * `T` - The entity type (e.g., StreamSource, Channel)
/// * `ID` - The identifier type (usually Uuid)
///
/// # Examples
///
/// ```rust
/// use crate::repositories::Repository;
/// use uuid::Uuid;
///
/// async fn example<R: Repository<StreamSource, Uuid>>(repo: R) -> Result<(), RepositoryError> {
///     let source = repo.find_by_id(uuid).await?;
///     let updated = repo.update(uuid, update_request).await?;
///     Ok(())
/// }
/// ```
#[async_trait]
pub trait Repository<T, ID: Send + 'static>: Send + Sync {
    /// Request type for creating new entities
    type CreateRequest;
    /// Request type for updating existing entities
    type UpdateRequest;
    /// Query type for filtering and searching
    type Query;

    /// Find an entity by its ID
    ///
    /// # Arguments
    ///
    /// * `id` - The unique identifier of the entity
    ///
    /// # Returns
    ///
    /// * `Ok(Some(T))` - Entity found
    /// * `Ok(None)` - Entity not found
    /// * `Err(RepositoryError)` - Database or other error
    async fn find_by_id(&self, id: ID) -> RepositoryResult<Option<T>>;

    /// Find multiple entities based on a query
    ///
    /// # Arguments
    ///
    /// * `query` - Query parameters for filtering and pagination
    ///
    /// # Returns
    ///
    /// * `Ok(Vec<T>)` - List of matching entities (may be empty)
    /// * `Err(RepositoryError)` - Database or other error
    async fn find_all(&self, query: Self::Query) -> RepositoryResult<Vec<T>>;

    /// Create a new entity
    ///
    /// # Arguments
    ///
    /// * `request` - Data for creating the entity
    ///
    /// # Returns
    ///
    /// * `Ok(T)` - Created entity with generated ID and timestamps
    /// * `Err(RepositoryError)` - Validation, constraint, or database error
    async fn create(&self, request: Self::CreateRequest) -> RepositoryResult<T>;

    /// Update an existing entity
    ///
    /// # Arguments
    ///
    /// * `id` - The ID of the entity to update
    /// * `request` - Updated data for the entity
    ///
    /// # Returns
    ///
    /// * `Ok(T)` - Updated entity
    /// * `Err(RepositoryError)` - Entity not found, validation, or database error
    async fn update(&self, id: ID, request: Self::UpdateRequest) -> RepositoryResult<T>;

    /// Delete an entity by ID
    ///
    /// # Arguments
    ///
    /// * `id` - The ID of the entity to delete
    ///
    /// # Returns
    ///
    /// * `Ok(())` - Entity deleted successfully
    /// * `Err(RepositoryError)` - Entity not found or database error
    async fn delete(&self, id: ID) -> RepositoryResult<()>;

    /// Count entities matching a query
    ///
    /// # Arguments
    ///
    /// * `query` - Query parameters for filtering
    ///
    /// # Returns
    ///
    /// * `Ok(u64)` - Number of matching entities
    /// * `Err(RepositoryError)` - Database error
    async fn count(&self, query: Self::Query) -> RepositoryResult<u64>;

    /// Check if an entity exists by ID
    ///
    /// # Arguments
    ///
    /// * `id` - The ID to check
    ///
    /// # Returns
    ///
    /// * `Ok(true)` - Entity exists
    /// * `Ok(false)` - Entity does not exist
    /// * `Err(RepositoryError)` - Database error
    async fn exists(&self, id: ID) -> RepositoryResult<bool> {
        match self.find_by_id(id).await? {
            Some(_) => Ok(true),
            None => Ok(false),
        }
    }
}

/// Extended repository trait for bulk operations
///
/// This trait extends the basic Repository with bulk operations that can be
/// more efficient for handling multiple entities at once.
#[async_trait]
pub trait BulkRepository<T, ID: Send + 'static>: Repository<T, ID> {
    /// Create multiple entities in a single transaction
    ///
    /// # Arguments
    ///
    /// * `requests` - Vector of create requests
    ///
    /// # Returns
    ///
    /// * `Ok(Vec<T>)` - Created entities
    /// * `Err(RepositoryError)` - Database error (all operations rolled back)
    async fn create_bulk(&self, requests: Vec<Self::CreateRequest>) -> RepositoryResult<Vec<T>>;

    /// Update multiple entities in a single transaction
    ///
    /// # Arguments
    ///
    /// * `updates` - Map of ID to update request
    ///
    /// # Returns
    ///
    /// * `Ok(Vec<T>)` - Updated entities
    /// * `Err(RepositoryError)` - Database error (all operations rolled back)
    async fn update_bulk(&self, updates: HashMap<ID, Self::UpdateRequest>) -> RepositoryResult<Vec<T>>;

    /// Delete multiple entities in a single transaction
    ///
    /// # Arguments
    ///
    /// * `ids` - Vector of IDs to delete
    ///
    /// # Returns
    ///
    /// * `Ok(u64)` - Number of entities deleted
    /// * `Err(RepositoryError)` - Database error (all operations rolled back)
    async fn delete_bulk(&self, ids: Vec<ID>) -> RepositoryResult<u64>;

    /// Find multiple entities by their IDs
    ///
    /// # Arguments
    ///
    /// * `ids` - Vector of IDs to find
    ///
    /// # Returns
    ///
    /// * `Ok(Vec<T>)` - Found entities (may be fewer than requested)
    /// * `Err(RepositoryError)` - Database error
    async fn find_by_ids(&self, ids: Vec<ID>) -> RepositoryResult<Vec<T>>;
}

/// Trait for repositories that support soft deletion
///
/// Some entities should not be permanently deleted but marked as deleted
/// for audit trails or data recovery purposes.
#[async_trait]
pub trait SoftDeleteRepository<T, ID: Send + 'static>: Repository<T, ID> {
    /// Soft delete an entity by marking it as deleted
    ///
    /// # Arguments
    ///
    /// * `id` - The ID of the entity to soft delete
    ///
    /// # Returns
    ///
    /// * `Ok(T)` - Entity marked as deleted
    /// * `Err(RepositoryError)` - Entity not found or database error
    async fn soft_delete(&self, id: ID) -> RepositoryResult<T>;

    /// Restore a soft deleted entity
    ///
    /// # Arguments
    ///
    /// * `id` - The ID of the entity to restore
    ///
    /// # Returns
    ///
    /// * `Ok(T)` - Restored entity
    /// * `Err(RepositoryError)` - Entity not found or database error
    async fn restore(&self, id: ID) -> RepositoryResult<T>;

    /// Find all entities including soft deleted ones
    ///
    /// # Arguments
    ///
    /// * `query` - Query parameters
    ///
    /// # Returns
    ///
    /// * `Ok(Vec<T>)` - All entities including soft deleted
    /// * `Err(RepositoryError)` - Database error
    async fn find_all_including_deleted(&self, query: Self::Query) -> RepositoryResult<Vec<T>>;

    /// Find only soft deleted entities
    ///
    /// # Arguments
    ///
    /// * `query` - Query parameters
    ///
    /// # Returns
    ///
    /// * `Ok(Vec<T>)` - Only soft deleted entities
    /// * `Err(RepositoryError)` - Database error
    async fn find_deleted(&self, query: Self::Query) -> RepositoryResult<Vec<T>>;
}

/// Trait for repositories that support pagination
///
/// This provides a standard interface for paginated queries across all repositories.
#[async_trait]
pub trait PaginatedRepository<T, ID: Send + 'static>: Repository<T, ID> {
    /// Paginated query result
    type PaginatedResult;

    /// Find entities with pagination
    ///
    /// # Arguments
    ///
    /// * `query` - Query parameters
    /// * `page` - Page number (1-based)
    /// * `limit` - Number of items per page
    ///
    /// # Returns
    ///
    /// * `Ok(PaginatedResult)` - Paginated results with metadata
    /// * `Err(RepositoryError)` - Database error
    async fn find_paginated(
        &self,
        query: Self::Query,
        page: u32,
        limit: u32,
    ) -> RepositoryResult<Self::PaginatedResult>;
}

/// Common query parameters used across repositories
#[derive(Debug, Clone, Default)]
pub struct QueryParams {
    /// Sort field
    pub sort_by: Option<String>,
    /// Sort direction (true = ascending, false = descending)
    pub sort_ascending: bool,
    /// Search term for text fields
    pub search: Option<String>,
    /// Additional filters as key-value pairs
    pub filters: HashMap<String, String>,
    /// Limit number of results (for non-paginated queries)
    pub limit: Option<u32>,
    /// Offset for results (for non-paginated queries)
    pub offset: Option<u32>,
}

impl QueryParams {
    /// Create new empty query parameters
    pub fn new() -> Self {
        Self::default()
    }

    /// Set sort field and direction
    pub fn sort_by<S: Into<String>>(mut self, field: S, ascending: bool) -> Self {
        self.sort_by = Some(field.into());
        self.sort_ascending = ascending;
        self
    }

    /// Set search term
    pub fn search<S: Into<String>>(mut self, term: S) -> Self {
        self.search = Some(term.into());
        self
    }

    /// Add a filter
    pub fn filter<K: Into<String>, V: Into<String>>(mut self, key: K, value: V) -> Self {
        self.filters.insert(key.into(), value.into());
        self
    }

    /// Set limit and offset
    pub fn limit(mut self, limit: u32, offset: u32) -> Self {
        self.limit = Some(limit);
        self.offset = Some(offset);
        self
    }
}

/// Standard paginated result structure
#[derive(Debug, Clone, serde::Serialize)]
pub struct PaginatedResult<T> {
    /// The items for this page
    pub items: Vec<T>,
    /// Current page number (1-based)
    pub page: u32,
    /// Items per page
    pub limit: u32,
    /// Total number of items across all pages
    pub total_count: u64,
    /// Total number of pages
    pub total_pages: u32,
    /// Whether there is a next page
    pub has_next: bool,
    /// Whether there is a previous page
    pub has_previous: bool,
}

impl<T> PaginatedResult<T> {
    /// Create a new paginated result
    pub fn new(
        items: Vec<T>,
        page: u32,
        limit: u32,
        total_count: u64,
    ) -> Self {
        let total_pages = ((total_count as f64) / (limit as f64)).ceil() as u32;
        let has_next = page < total_pages;
        let has_previous = page > 1;

        Self {
            items,
            page,
            limit,
            total_count,
            total_pages,
            has_next,
            has_previous,
        }
    }
}

/// Generic repository helpers for common patterns
pub struct RepositoryHelpers;

impl RepositoryHelpers {
    /// Generic method to update last_ingested_at timestamp for any source table
    pub async fn update_last_ingested(
        pool: &sqlx::Pool<sqlx::Sqlite>,
        table_name: &str,
        source_id: uuid::Uuid,
    ) -> crate::errors::RepositoryResult<chrono::DateTime<chrono::Utc>> {
        let now = chrono::Utc::now();
        let query = format!("UPDATE {} SET last_ingested_at = ?, updated_at = ? WHERE id = ?", table_name);
        
        sqlx::query(&query)
            .bind(now.to_rfc3339())
            .bind(now.to_rfc3339())
            .bind(source_id.to_string())
            .execute(pool)
            .await?;

        Ok(now)
    }

    /// Generic method to get channel count for any source
    pub async fn get_channel_count_for_source(
        pool: &sqlx::Pool<sqlx::Sqlite>,
        channel_table: &str,
        source_id: uuid::Uuid,
    ) -> crate::errors::RepositoryResult<i64> {
        let query = format!("SELECT COUNT(*) FROM {} WHERE source_id = ?", channel_table);
        let count: i64 = sqlx::query_scalar(&query)
            .bind(source_id.to_string())
            .fetch_one(pool)
            .await?;

        Ok(count)
    }

    /// Generic method to get usage count for any entity in a relation table
    pub async fn get_usage_count(
        pool: &sqlx::Pool<sqlx::Sqlite>,
        relation_table: &str,
        entity_id_column: &str,
        entity_id: uuid::Uuid,
    ) -> crate::errors::RepositoryResult<i64> {
        let query = format!("SELECT COUNT(*) FROM {} WHERE {} = ?", relation_table, entity_id_column);
        let count: i64 = sqlx::query_scalar(&query)
            .bind(entity_id.to_string())
            .fetch_one(pool)
            .await?;

        Ok(count)
    }
}