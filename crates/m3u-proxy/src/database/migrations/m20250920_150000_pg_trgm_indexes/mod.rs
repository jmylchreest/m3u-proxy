use sea_orm::{ConnectionTrait, Statement, TransactionTrait};
use sea_orm_migration::prelude::*;

/// Migration: Enable pg_trgm (PostgreSQL only) + add trigram GIN indexes
/// AND (consolidated) normalize `filters` uniqueness (was previously split across
/// multiple 20250920_* migrations that were never applied).
///
/// Consolidated responsibilities (idempotent):
/// 1. PostgreSQL:
///    - Create pg_trgm extension (if possible)
///    - Drop legacy UNIQUE(name) on filters (if present)
///    - Create composite UNIQUE (name, source_type)
///    - Add trigram GIN indexes for epg_programs + channels
/// 2. MySQL / MariaDB:
///    - Best-effort drop legacy single-column unique index on filters.name
///    - Create composite UNIQUE (name, source_type)
/// 3. SQLite:
///    - Rebuild filters table to remove legacy UNIQUE(name)
///      and replace with composite UNIQUE(name, source_type) if legacy form detected.
///      (Safe to run early because migrations with today’s timestamp not yet applied.)
///
/// Down migration: only drops trigram indexes on Postgres (does not attempt to
/// reintroduce legacy uniqueness model; forward-only for uniqueness normalization).
///
/// Rationale:
/// Substring (contains) filters on large EPG / channel tables become costly with
/// sequential scans. The pg_trgm extension plus GIN indexes accelerates
/// `LIKE '%text%'` / ILIKE / lower(field) LIKE queries used by expression filters.
///
/// This migration is NO-OP on non-Postgres backends (SQLite / MySQL).
pub struct Migration;
folder_migration_name!();

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let backend = manager.get_database_backend();
        let conn = manager.get_connection();

        // (Postgres only) Ensure pg_trgm extension exists (idempotent)
        if backend == sea_orm::DatabaseBackend::Postgres {
            if let Err(e) = conn
                .execute_unprepared("CREATE EXTENSION IF NOT EXISTS pg_trgm")
                .await
            {
                // Do not fail entire migration if extension creation lacks privileges; log & continue.
                println!(
                    "Warning: unable to create pg_trgm extension (continuing without trigram indexes): {e}"
                );
            }
        }

        // Normalize / consolidate filters uniqueness across backends (idempotent)
        match backend {
            sea_orm::DatabaseBackend::Postgres => {
                // Drop old single-column uniqueness if still present
                let _ = conn
                    .execute_unprepared(
                        r#"ALTER TABLE filters DROP CONSTRAINT IF EXISTS filters_name_key"#,
                    )
                    .await;
                // Ensure composite unique index
                let _ = conn
                    .execute_unprepared(
                        r#"CREATE UNIQUE INDEX IF NOT EXISTS idx_filters_name_source_type_unique
                           ON filters (name, source_type)"#,
                    )
                    .await;
            }
            sea_orm::DatabaseBackend::MySql => {
                // Best-effort drops (index names vary)
                let _ = conn
                    .execute_unprepared(r#"ALTER TABLE filters DROP INDEX filters_name_key"#)
                    .await;
                let _ = conn
                    .execute_unprepared(r#"ALTER TABLE filters DROP INDEX name"#)
                    .await;
                let _ = conn
                    .execute_unprepared(
                        r#"CREATE UNIQUE INDEX idx_filters_name_source_type_unique
                           ON filters (name, source_type)"#,
                    )
                    .await;
            }
            sea_orm::DatabaseBackend::Sqlite => {
                // Ensure composite index (idempotent)
                let _ = conn
                    .execute_unprepared(
                        r#"CREATE UNIQUE INDEX IF NOT EXISTS idx_filters_name_source_type_unique
                           ON filters (name, source_type)"#,
                    )
                    .await;

                // Detect legacy single-column UNIQUE(name)
                let (legacy_unique, _have_composite) = {
                    let mut legacy = false;
                    let mut composite = false;
                    if let Ok(rows) = conn
                        .query_all(Statement::from_string(
                            sea_orm::DatabaseBackend::Sqlite,
                            "PRAGMA index_list('filters')".to_string(),
                        ))
                        .await
                    {
                        for row in rows {
                            if let Ok(uq) = row.try_get::<i64>("", "unique") {
                                if uq == 1 {
                                    if let Ok(idx_name) = row.try_get::<String>("", "name") {
                                        if let Ok(cols) = conn
                                            .query_all(Statement::from_string(
                                                sea_orm::DatabaseBackend::Sqlite,
                                                format!("PRAGMA index_info('{idx_name}')"),
                                            ))
                                            .await
                                        {
                                            let mut col_names = Vec::new();
                                            for c in cols {
                                                if let Ok(col_name) =
                                                    c.try_get::<String>("", "name")
                                                {
                                                    col_names.push(col_name);
                                                }
                                            }
                                            if col_names.len() == 1 && col_names[0] == "name" {
                                                legacy = true;
                                            }
                                            if col_names.len() == 2
                                                && ((col_names[0] == "name"
                                                    && col_names[1] == "source_type")
                                                    || (col_names[0] == "source_type"
                                                        && col_names[1] == "name"))
                                            {
                                                composite = true;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    (legacy, composite)
                };

                // Always rebuild if legacy single-column uniqueness detected, regardless of composite index presence
                if legacy_unique {
                    let txn = conn.begin().await?;
                    txn.execute(Statement::from_string(
                        sea_orm::DatabaseBackend::Sqlite,
                        r#"
CREATE TABLE IF NOT EXISTS filters_new (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    source_type TEXT NOT NULL,
    is_inverse BOOLEAN NOT NULL DEFAULT 0,
    is_system_default BOOLEAN NOT NULL DEFAULT 0,
    expression TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    CONSTRAINT filters_name_source_type_unique UNIQUE (name, source_type)
);"#
                        .to_string(),
                    ))
                    .await?;

                    txn.execute(Statement::from_string(
                        sea_orm::DatabaseBackend::Sqlite,
                        r#"
INSERT OR IGNORE INTO filters_new
(id, name, source_type, is_inverse, is_system_default, expression, created_at, updated_at)
SELECT id, name, source_type, is_inverse, is_system_default, expression, created_at, updated_at
FROM filters;
"#
                        .to_string(),
                    ))
                    .await?;

                    txn.execute(Statement::from_string(
                        sea_orm::DatabaseBackend::Sqlite,
                        "DROP TABLE filters;".to_string(),
                    ))
                    .await?;
                    txn.execute(Statement::from_string(
                        sea_orm::DatabaseBackend::Sqlite,
                        "ALTER TABLE filters_new RENAME TO filters;".to_string(),
                    ))
                    .await?;

                    // Secondary indexes
                    for idx_sql in [
                        "CREATE INDEX IF NOT EXISTS idx_filters_source_type ON filters (source_type);",
                        "CREATE INDEX IF NOT EXISTS idx_filters_is_inverse ON filters (is_inverse);",
                        "CREATE INDEX IF NOT EXISTS idx_filters_is_system_default ON filters (is_system_default);",
                    ] {
                        let _ = txn
                            .execute(Statement::from_string(
                                sea_orm::DatabaseBackend::Sqlite,
                                idx_sql.to_string(),
                            ))
                            .await;
                    }

                    txn.commit().await?;
                }
            }
        }

        // 3. Create trigram indexes (idempotent)
        // EPG programs: channel_id
        let _ = conn
            .execute_unprepared(
                "CREATE INDEX IF NOT EXISTS idx_epg_programs_channel_id_trgm \
                 ON epg_programs USING GIN (channel_id gin_trgm_ops)",
            )
            .await;

        // EPG programs: program_title
        let _ = conn
            .execute_unprepared(
                "CREATE INDEX IF NOT EXISTS idx_epg_programs_program_title_trgm \
                 ON epg_programs USING GIN (program_title gin_trgm_ops)",
            )
            .await;

        // Channels: channel_name
        let _ = conn
            .execute_unprepared(
                "CREATE INDEX IF NOT EXISTS idx_channels_channel_name_trgm \
                 ON channels USING GIN (channel_name gin_trgm_ops)",
            )
            .await;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        if manager.get_database_backend() != sea_orm::DatabaseBackend::Postgres {
            return Ok(());
        }
        let conn = manager.get_connection();

        // Drop indexes (extension left intact—safe & commonly shared).
        // Using IF EXISTS for idempotence / partial rollback resilience.
        let _ = conn
            .execute_unprepared("DROP INDEX IF EXISTS idx_epg_programs_channel_id_trgm CASCADE")
            .await;
        let _ = conn
            .execute_unprepared("DROP INDEX IF EXISTS idx_epg_programs_program_title_trgm CASCADE")
            .await;
        let _ = conn
            .execute_unprepared("DROP INDEX IF EXISTS idx_channels_channel_name_trgm CASCADE")
            .await;

        Ok(())
    }
}
