//! SeaORM migrations for multi-database support
//!
//! This module provides database-agnostic migrations that work across SQLite, PostgreSQL, and MySQL.
//! Database-specific optimizations are applied where necessary.

use sea_orm_migration::prelude::*;

/// Macro to derive a migration's name from its containing folder when the migration
/// is implemented as `folder_name/mod.rs`.
///
/// This avoids the `DeriveMigrationName` pitfall that yields the non-unique "mod"
/// for every folder-based migration. The folder name must follow the
/// convention: mYYYYMMDD_HHMMSS_description
///
/// Usage inside a folder-based migration's `mod.rs`:
/// ```ignore
/// pub struct Migration;
/// folder_migration_name!();
/// ```
///
/// The macro implements `MigrationName` for the local `Migration` type by
/// parsing `file!()` at compile time, trimming the trailing `mod.rs`,
/// and extracting the last path segment (the folder name). It uses
/// a `OnceLock<String>` so the computation happens only once.
#[macro_export]
macro_rules! folder_migration_name {
    () => {
        impl sea_orm_migration::MigrationName for Migration {
            fn name(&self) -> &str {
                static NAME: ::std::sync::OnceLock<String> = ::std::sync::OnceLock::new();
                NAME.get_or_init(|| {
                    let f = file!(); // e.g. ".../m20250920_150000_pg_trgm_indexes/mod.rs"
                    let trimmed = f
                        .trim_end_matches("mod.rs")
                        .trim_end_matches(|c| c == '/' || c == '\\');
                    trimmed
                        .rsplit(|c| c == '/' || c == '\\')
                        .next()
                        .unwrap()
                        .to_string()
                })
            }
        }
    };
}

pub mod m20250829_100000_initial_schema;
pub mod m20250829_100001_insert_defaults;
pub mod m20250920_150000_pg_trgm_indexes;
// (Consolidated into m20250920_150000_pg_trgm_indexes migration)

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20250829_100000_initial_schema::Migration),
            Box::new(m20250829_100001_insert_defaults::Migration),
            Box::new(m20250920_150000_pg_trgm_indexes::Migration),
            // Consolidated uniqueness normalization migrations removed (now handled inside m20250920_150000_pg_trgm_indexes)
        ]
    }
}
