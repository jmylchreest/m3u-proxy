//! SeaORM migrations for multi-database support
//!
//! This module provides database-agnostic migrations that work across SQLite, PostgreSQL, and MySQL.
//! Database-specific optimizations are applied where necessary.

use sea_orm_migration::prelude::*;

pub mod m20250829_100000_initial_schema;
pub mod m20250829_100001_insert_defaults;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20250829_100000_initial_schema::Migration),
            Box::new(m20250829_100001_insert_defaults::Migration),
        ]
    }
}
