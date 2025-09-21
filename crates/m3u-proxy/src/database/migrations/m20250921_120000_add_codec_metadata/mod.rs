use crate::folder_migration_name;
use sea_orm::DatabaseBackend;
use sea_orm_migration::prelude::*;
use sea_orm_migration::sea_orm;

/// Combined migration (simplified IF NOT EXISTS version):
/// 1. Adds codec / probe metadata columns if they do not already exist:
///    framerate (varchar), bitrate (integer), probe_method (varchar), probe_source (varchar),
///    detected_at (timestamp), created_at (timestamp)
/// 2. Backfills detected_at / created_at from updated_at where NULL
/// 3. Normalizes detected_at / created_at / updated_at to TIMESTAMPTZ on PostgreSQL only
///
/// Idempotent across reruns:
/// - Uses ADD COLUMN IF NOT EXISTS
/// - Normalization tolerates already-converted columns
pub struct Migration;

folder_migration_name!();

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let backend = manager.get_database_backend();

        // 1. Add missing columns (portable IF NOT EXISTS for Pg / MySQL 8 / modern SQLite)
        raw_exec(
            manager,
            "ALTER TABLE last_known_codecs ADD COLUMN IF NOT EXISTS framerate varchar NULL",
        )
        .await?;
        raw_exec(
            manager,
            "ALTER TABLE last_known_codecs ADD COLUMN IF NOT EXISTS bitrate integer NULL",
        )
        .await?;
        raw_exec(
            manager,
            "ALTER TABLE last_known_codecs ADD COLUMN IF NOT EXISTS probe_method varchar NULL",
        )
        .await?;
        raw_exec(
            manager,
            "ALTER TABLE last_known_codecs ADD COLUMN IF NOT EXISTS probe_source varchar NULL",
        )
        .await?;
        raw_exec(
            manager,
            "ALTER TABLE last_known_codecs ADD COLUMN IF NOT EXISTS detected_at timestamp NULL",
        )
        .await?;
        raw_exec(
            manager,
            "ALTER TABLE last_known_codecs ADD COLUMN IF NOT EXISTS created_at timestamp NULL",
        )
        .await?;

        // 2. Backfill newly added timestamps
        raw_exec(
            manager,
            "UPDATE last_known_codecs SET detected_at = updated_at WHERE detected_at IS NULL",
        )
        .await?;
        raw_exec(
            manager,
            "UPDATE last_known_codecs SET created_at  = updated_at WHERE created_at  IS NULL",
        )
        .await?;

        // 3. PostgreSQL: normalize to TIMESTAMPTZ (safe to re-run)
        if backend == DatabaseBackend::Postgres {
            raw_exec(manager, "ALTER TABLE last_known_codecs ALTER COLUMN detected_at TYPE TIMESTAMPTZ USING (detected_at AT TIME ZONE 'UTC')").await?;
            raw_exec(manager, "ALTER TABLE last_known_codecs ALTER COLUMN created_at  TYPE TIMESTAMPTZ USING (created_at  AT TIME ZONE 'UTC')").await?;
            raw_exec(manager, "ALTER TABLE last_known_codecs ALTER COLUMN updated_at  TYPE TIMESTAMPTZ USING (updated_at  AT TIME ZONE 'UTC')").await?;
        }

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Best-effort rollback: drop added columns (tolerate failures); revert TIMESTAMPTZ on Postgres
        for sql in [
            "ALTER TABLE last_known_codecs DROP COLUMN framerate",
            "ALTER TABLE last_known_codecs DROP COLUMN bitrate",
            "ALTER TABLE last_known_codecs DROP COLUMN probe_method",
            "ALTER TABLE last_known_codecs DROP COLUMN probe_source",
            "ALTER TABLE last_known_codecs DROP COLUMN detected_at",
            "ALTER TABLE last_known_codecs DROP COLUMN created_at",
        ] {
            let _ = raw_exec_ignore(manager, sql).await;
        }

        if manager.get_database_backend() == DatabaseBackend::Postgres {
            // Revert to naive timestamps (lossy)
            for sql in [
                "ALTER TABLE last_known_codecs ALTER COLUMN detected_at TYPE TIMESTAMP USING (timezone('UTC', detected_at))",
                "ALTER TABLE last_known_codecs ALTER COLUMN created_at  TYPE TIMESTAMP USING (timezone('UTC', created_at))",
                "ALTER TABLE last_known_codecs ALTER COLUMN updated_at  TYPE TIMESTAMP USING (timezone('UTC', updated_at))",
            ] {
                let _ = raw_exec_ignore(manager, sql).await;
            }
        }
        Ok(())
    }
}

/// Execute raw SQL; fail on any real error.
async fn raw_exec(manager: &SchemaManager<'_>, sql: &str) -> Result<(), DbErr> {
    let backend = manager.get_database_backend();
    manager
        .get_connection()
        .execute(sea_orm::Statement::from_string(backend, sql.to_string()))
        .await
        .map(|_| ())
}

/// Execute raw SQL but ignore benign "does not exist" / "unknown" errors (used for down()).
async fn raw_exec_ignore(manager: &SchemaManager<'_>, sql: &str) -> Result<(), DbErr> {
    let backend = manager.get_database_backend();
    match manager
        .get_connection()
        .execute(sea_orm::Statement::from_string(backend, sql.to_string()))
        .await
    {
        Ok(_) => Ok(()),
        Err(e) => {
            let msg = e.to_string().to_lowercase();
            if msg.contains("does not exist")
                || msg.contains("unknown")
                || msg.contains("undefined")
                || msg.contains("not found")
            {
                Ok(())
            } else {
                Err(e)
            }
        }
    }
}
