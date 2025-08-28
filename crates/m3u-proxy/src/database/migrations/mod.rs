//! SeaORM migrations for multi-database support
//!
//! This module provides database-agnostic migrations that work across SQLite, PostgreSQL, and MySQL.
//! Database-specific optimizations are applied where necessary.

use sea_orm_migration::prelude::*;

pub mod m20250817_000001_initial_schema;
pub mod m20250817_000009_insert_defaults;
pub mod m20250817_000010_add_last_known_codecs;
pub mod m20250825_000001_update_last_known_codecs_to_stream_url;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20250817_000001_initial_schema::Migration),
            Box::new(m20250817_000009_insert_defaults::Migration),
            Box::new(m20250817_000010_add_last_known_codecs::Migration),
            Box::new(m20250825_000001_update_last_known_codecs_to_stream_url::Migration),
        ]
    }
}