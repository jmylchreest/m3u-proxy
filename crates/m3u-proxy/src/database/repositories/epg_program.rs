//! SeaORM-based EPG Program repository implementation
//!
//! This provides a clean, database-agnostic repository for EPG Program operations using SeaORM.
//! Replaces the complex legacy repository with a simple, maintainable SeaORM implementation.

use anyhow::Result;
use chrono::{DateTime, Utc};
use sea_orm::{
    ColumnTrait, DatabaseConnection, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder,
};
use std::sync::Arc;
use uuid::Uuid;

use crate::entities::{epg_programs, prelude::EpgPrograms};
use crate::models::EpgProgram;

/// SeaORM repository for EPG programs with clean, focused interface
#[derive(Clone)]
pub struct EpgProgramSeaOrmRepository {
    connection: Arc<DatabaseConnection>,
}

impl EpgProgramSeaOrmRepository {
    pub fn new(connection: Arc<DatabaseConnection>) -> Self {
        Self { connection }
    }

    /// Find EPG programs by source ID
    pub async fn find_by_source_id(&self, source_id: &Uuid) -> Result<Vec<EpgProgram>> {
        let models = EpgPrograms::find()
            .filter(epg_programs::Column::SourceId.eq(*source_id))
            .order_by_asc(epg_programs::Column::StartTime)
            .all(&*self.connection)
            .await?;

        self.models_to_domain(models)
    }

    /// Find EPG programs by channel ID
    pub async fn find_by_channel_id(&self, channel_id: &Uuid) -> Result<Vec<EpgProgram>> {
        let models = EpgPrograms::find()
            .filter(epg_programs::Column::ChannelId.eq(*channel_id))
            .order_by_asc(epg_programs::Column::StartTime)
            .all(&*self.connection)
            .await?;

        self.models_to_domain(models)
    }

    /// Find EPG programs by time range (rationalized time-based querying)
    /// Returns programs that overlap with the given time range
    pub async fn find_by_time_range(
        &self,
        source_id: Option<&Uuid>,
        start_time: &DateTime<Utc>,
        end_time: &DateTime<Utc>,
    ) -> Result<Vec<EpgProgram>> {
        // Find programs that overlap with the time range:
        // - Program ends after the range start (still running or will run)
        // - AND program starts before the range end (not too far in the future)
        let mut query = EpgPrograms::find()
            .filter(epg_programs::Column::EndTime.gt(*start_time)) // Program ends after range start
            .filter(epg_programs::Column::StartTime.lt(*end_time)); // Program starts before range end

        if let Some(source_id) = source_id {
            query = query.filter(epg_programs::Column::SourceId.eq(*source_id));
        }

        let models = query
            .order_by_asc(epg_programs::Column::StartTime)
            .all(&*self.connection)
            .await?;

        self.models_to_domain(models)
    }

    /// Get program count for a source (for statistics)
    pub async fn count_by_source_id(&self, source_id: &Uuid) -> Result<u64> {
        let count = EpgPrograms::find()
            .filter(epg_programs::Column::SourceId.eq(*source_id))
            .count(&*self.connection)
            .await?;

        Ok(count)
    }

    /// Convert SeaORM models to domain models (private helper)
    fn models_to_domain(&self, models: Vec<epg_programs::Model>) -> Result<Vec<EpgProgram>> {
        let mut programs = Vec::new();

        for model in models {
            let program = EpgProgram {
                id: model.id,
                source_id: model.source_id,
                channel_id: model.channel_id,
                channel_name: model.channel_name,
                start_time: model.start_time,
                end_time: model.end_time,
                program_title: model.program_title,
                program_description: model.program_description,
                program_category: model.program_category,
                episode_num: model.episode_num,
                season_num: model.season_num,
                rating: model.rating,
                language: model.language,
                subtitles: model.subtitles,
                aspect_ratio: model.aspect_ratio,
                program_icon: model.program_icon,
                created_at: model.created_at,
                updated_at: model.updated_at,
            };

            programs.push(program);
        }

        Ok(programs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{config::IngestionConfig, database::Database};

    async fn create_test_db() -> Result<Database> {
        // For unit tests, we'll use the actual database structure but skip problematic migrations
        // This is acceptable for unit tests that only test repository logic
        use sea_orm::*;
        use std::sync::Arc;

        let connection = sea_orm::Database::connect("sqlite::memory:").await?;
        let arc_connection = Arc::new(connection);

        // Create minimal table structure for testing
        arc_connection
            .execute(Statement::from_string(
                DatabaseBackend::Sqlite,
                r#"
            CREATE TABLE epg_programs (
                id TEXT PRIMARY KEY,
                source_id TEXT NOT NULL,
                channel_id TEXT,
                channel_name TEXT,
                start_time TEXT NOT NULL,
                end_time TEXT NOT NULL,
                program_title TEXT,
                program_description TEXT,
                program_category TEXT,
                episode_num TEXT,
                season_num TEXT,
                rating TEXT,
                language TEXT,
                subtitles TEXT,
                aspect_ratio TEXT,
                program_icon TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            "#
                .to_string(),
            ))
            .await?;

        // Create a minimal database wrapper for testing
        let db = crate::database::Database {
            connection: arc_connection.clone(),
            read_connection: arc_connection,
            database_type: crate::database::DatabaseType::SQLite,
            backend: DatabaseBackend::Sqlite,
            ingestion_config: IngestionConfig::default(),
        };

        Ok(db)
    }

    #[tokio::test]
    async fn test_epg_program_repository_operations() -> Result<()> {
        let db = create_test_db().await?;
        let repo = EpgProgramSeaOrmRepository::new(db.connection().clone());

        let source_id = Uuid::new_v4();

        // Test find by source (should be empty initially)
        let programs = repo.find_by_source_id(&source_id).await?;
        assert!(programs.is_empty());

        // Test count by source
        let count = repo.count_by_source_id(&source_id).await?;
        assert_eq!(count, 0);

        Ok(())
    }
}
