//! Common traits for SeaORM repository implementations
//!
//! This module provides shared traits and utilities to maintain DRY principles
//! across all SeaORM repository implementations.

use anyhow::Result;
use sea_orm::DatabaseConnection;
use std::sync::Arc;

/// Common conversion utilities for SeaORM entities to domain models
pub trait EntityToDomain<Entity, Domain> {
    /// Convert a SeaORM entity model to domain model
    fn to_domain(&self, entity: Entity) -> Result<Domain>;
}

/// Base repository trait providing common CRUD operations
#[async_trait::async_trait]
pub trait Repository<Entity, Domain, CreateRequest, UpdateRequest> {
    /// Create a new entity
    async fn create(&self, request: CreateRequest) -> Result<Domain>;

    /// Find entity by ID
    async fn find_by_id(&self, id: &str) -> Result<Option<Domain>>;

    /// Update entity
    async fn update(&self, id: &str, request: UpdateRequest) -> Result<Domain>;

    /// Delete entity
    async fn delete(&self, id: &str) -> Result<()>;

    /// List all entities
    async fn list_all(&self) -> Result<Vec<Domain>>;
}

/// Base SeaORM repository struct that all repositories can extend
#[derive(Clone)]
pub struct BaseSeaOrmRepository {
    pub connection: Arc<DatabaseConnection>,
}

impl BaseSeaOrmRepository {
    pub fn new(connection: Arc<DatabaseConnection>) -> Self {
        Self { connection }
    }
}
