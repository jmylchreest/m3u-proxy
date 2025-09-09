//! SeaORM-based EPG Source repository implementation
//!
//! This provides a database-agnostic repository for EPG Source operations using SeaORM.

use anyhow::Result;
use sea_orm::{ActiveModelTrait, DatabaseConnection, EntityTrait, Set, ColumnTrait, QueryFilter, PaginatorTrait, QueryOrder};
use std::sync::Arc;
use uuid::Uuid;

use crate::entities::{prelude::EpgSources, epg_sources};
use crate::models::{EpgSource, EpgSourceType, EpgSourceCreateRequest};

/// SeaORM-based repository for EPG Source operations
#[derive(Clone)]
pub struct EpgSourceSeaOrmRepository {
    connection: Arc<DatabaseConnection>,
}

impl EpgSourceSeaOrmRepository {
    /// Create a new repository instance
    pub fn new(connection: Arc<DatabaseConnection>) -> Self {
        Self { connection }
    }

    /// Create a new EPG source
    pub async fn create(&self, request: EpgSourceCreateRequest) -> Result<EpgSource> {
        let now = chrono::Utc::now();
        let id = Uuid::new_v4();

        let active_model = epg_sources::ActiveModel {
            id: Set(id),
            name: Set(request.name.clone()),
            source_type: Set(request.source_type.to_string()),
            url: Set(request.url.clone()),
            update_cron: Set(request.update_cron.clone()),
            username: Set(request.username.clone()),
            password: Set(request.password.clone()),
            original_timezone: Set(request.timezone.clone()),
            time_offset: Set(Some(request.time_offset.unwrap_or_else(|| "+00:00".to_string()))),
            created_at: Set(now),
            updated_at: Set(now),
            last_ingested_at: Set(None),
            is_active: Set(true),
        };

        let model = active_model.insert(&*self.connection).await?;

        // Convert SeaORM model to domain model
        self.model_to_domain(model)
    }

    /// Find an EPG source by ID
    pub async fn find_by_id(&self, id: &Uuid) -> Result<Option<EpgSource>> {
        let model = EpgSources::find_by_id(*id)
            .one(&*self.connection)
            .await?;

        match model {
            Some(m) => Ok(Some(self.model_to_domain(m)?)),
            None => Ok(None),
        }
    }

    /// Find all EPG sources
    pub async fn find_all(&self) -> Result<Vec<EpgSource>> {
        let models = EpgSources::find()
            .order_by_asc(epg_sources::Column::Name)
            .all(&*self.connection).await?;
        
        let mut sources = Vec::new();
        for model in models {
            sources.push(self.model_to_domain(model)?);
        }
        
        Ok(sources)
    }

    /// Find EPG sources by type
    pub async fn find_by_type(&self, source_type: &EpgSourceType) -> Result<Vec<EpgSource>> {
        let models = EpgSources::find()
            .filter(epg_sources::Column::SourceType.eq(source_type.to_string()))
            .all(&*self.connection)
            .await?;
        
        let mut sources = Vec::new();
        for model in models {
            sources.push(self.model_to_domain(model)?);
        }
        
        Ok(sources)
    }

    /// Find active EPG sources
    pub async fn find_active(&self) -> Result<Vec<EpgSource>> {
        let models = EpgSources::find()
            .filter(epg_sources::Column::IsActive.eq(true))
            .order_by_asc(epg_sources::Column::Name)
            .all(&*self.connection)
            .await?;
        
        let mut sources = Vec::new();
        for model in models {
            sources.push(self.model_to_domain(model)?);
        }
        
        Ok(sources)
    }

    /// Find EPG sources by URL and source type (for URL-based linking)
    pub async fn find_by_url_and_type(&self, url: &str, source_type: EpgSourceType) -> Result<Vec<EpgSource>> {
        let models = EpgSources::find()
            .filter(epg_sources::Column::Url.eq(url))
            .filter(epg_sources::Column::SourceType.eq(source_type.to_string()))
            .filter(epg_sources::Column::IsActive.eq(true))
            .all(&*self.connection)
            .await?;
        
        let mut sources = Vec::new();
        for model in models {
            sources.push(self.model_to_domain(model)?);
        }
        
        Ok(sources)
    }

    /// Convert SeaORM model to domain model
    fn model_to_domain(&self, model: epg_sources::Model) -> Result<EpgSource> {
        let source_type = match model.source_type.as_str() {
            "xmltv" => EpgSourceType::Xmltv,
            "xtream" => EpgSourceType::Xtream,
            _ => anyhow::bail!("Unknown EPG source type: {}", model.source_type),
        };

        let created_at = model.created_at;
        let updated_at = model.updated_at;

        let last_ingested_at = model.last_ingested_at;

        let id = model.id;

        Ok(EpgSource {
            id,
            name: model.name,
            source_type,
            url: model.url,
            update_cron: model.update_cron,
            username: model.username,
            password: model.password,
            original_timezone: model.original_timezone,
            time_offset: model.time_offset.unwrap_or_else(|| "+00:00".to_string()),
            created_at,
            updated_at,
            last_ingested_at,
            is_active: model.is_active,
        })
    }

    /// Update the last_ingested_at timestamp for an EPG source
    pub async fn update_last_ingested_at(&self, id: &Uuid) -> Result<chrono::DateTime<chrono::Utc>> {
        let now = chrono::Utc::now();
        
        let active_model = epg_sources::ActiveModel {
            id: Set(*id),
            last_ingested_at: Set(Some(now)),
            updated_at: Set(now),
            ..Default::default()
        };

        active_model.update(&*self.connection).await?;
        Ok(now)
    }


    /// Update an EPG source
    pub async fn update(&self, id: &Uuid, request: crate::models::EpgSourceUpdateRequest) -> Result<crate::models::EpgSource> {
        use crate::entities::{prelude::EpgSources, epg_sources};
        use sea_orm::{Set, ActiveModelTrait};
        
        // Find the existing source
        let existing = EpgSources::find_by_id(*id)
            .one(&*self.connection)
            .await?
            .ok_or_else(|| anyhow::anyhow!("EPG source not found"))?;

        // Create active model for update
        let mut active_model: epg_sources::ActiveModel = existing.into();
        active_model.name = Set(request.name);
        active_model.source_type = Set(request.source_type.to_string());
        active_model.url = Set(request.url);
        active_model.update_cron = Set(request.update_cron);
        active_model.username = Set(request.username);
        // Only update password if one is provided
        if request.password.is_some() {
            active_model.password = Set(request.password);
        }
        active_model.original_timezone = Set(request.timezone);
        active_model.time_offset = Set(Some(request.time_offset.unwrap_or_else(|| "+00:00".to_string())));
        active_model.is_active = Set(request.is_active);
        active_model.updated_at = Set(chrono::Utc::now());

        let updated_model = active_model.update(&*self.connection).await?;
        self.model_to_domain(updated_model)
    }

    /// Delete an EPG source
    pub async fn delete(&self, id: &Uuid) -> Result<()> {
        use crate::entities::{prelude::EpgSources};
        
        let result = EpgSources::delete_by_id(*id)
            .exec(&*self.connection)
            .await?;

        if result.rows_affected == 0 {
            return Err(anyhow::anyhow!("EPG source not found"));
        }

        Ok(())
    }

    /// List EPG sources with statistics (only active sources)
    pub async fn list_with_stats(&self) -> Result<Vec<crate::models::EpgSourceWithStats>> {
        let sources = self.find_active().await?;
        let mut results = Vec::new();
        
        for source in sources {
            // Get program count
            use crate::entities::{prelude::EpgPrograms, epg_programs};
            let program_count = EpgPrograms::find()
                .filter(epg_programs::Column::SourceId.eq(source.id))
                .count(&*self.connection)
                .await?;
            
            // Calculate next scheduled update from cron expression
            let next_scheduled_update = if source.is_active {
                Self::calculate_next_update_time(&source.update_cron, source.last_ingested_at)
            } else {
                None
            };
            
            results.push(crate::models::EpgSourceWithStats {
                source,
                program_count: program_count as i64,
                next_scheduled_update,
            });
        }

        Ok(results)
    }
    
    /// Calculate next scheduled update time from cron expression
    fn calculate_next_update_time(cron_expr: &str, last_ingested_at: Option<chrono::DateTime<chrono::Utc>>) -> Option<chrono::DateTime<chrono::Utc>> {
        use cron::Schedule;
        use std::str::FromStr;
        use chrono::Utc;
        
        match Schedule::from_str(cron_expr) {
            Ok(schedule) => {
                let now = Utc::now();
                if let Some(last_ingested) = last_ingested_at {
                    // Find next update after last ingestion
                    let next_after_ingestion = schedule.after(&last_ingested).next();
                    
                    // If the calculated next time is in the past, calculate from now instead
                    match next_after_ingestion {
                        Some(next_time) if next_time > now => Some(next_time),
                        _ => schedule.upcoming(Utc).next(),
                    }
                } else {
                    // Never ingested - get next update from now
                    schedule.upcoming(Utc).next()
                }
            }
            Err(_) => {
                // Invalid cron expression - return None
                None
            }
        }
    }

}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        config::IngestionConfig,
        database::Database,
    };

    async fn create_test_db() -> Result<Database> {
        // For unit tests, we'll use the actual database structure but skip problematic migrations
        // This is acceptable for unit tests that only test repository logic
        use sea_orm::*;
        use std::sync::Arc;
        
        let connection = sea_orm::Database::connect("sqlite::memory:").await?;
        let arc_connection = Arc::new(connection);
        
        // Create minimal table structure for testing
        arc_connection.execute(Statement::from_string(
            DatabaseBackend::Sqlite,
            r#"
            CREATE TABLE epg_sources (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                source_type TEXT NOT NULL,
                url TEXT NOT NULL,
                update_cron TEXT NOT NULL,
                username TEXT,
                password TEXT,
                original_timezone TEXT,
                time_offset TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                last_ingested_at TEXT,
                is_active INTEGER NOT NULL DEFAULT 1
            );
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
            "#.to_string()
        )).await?;
        
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
    async fn test_epg_source_crud_operations() -> Result<()> {
        let db = create_test_db().await?;
        let repo = EpgSourceSeaOrmRepository::new(db.connection().clone());

        // Test create
        let create_request = EpgSourceCreateRequest {
            name: "Test EPG Source".to_string(),
            source_type: EpgSourceType::Xmltv,
            url: "http://example.com/epg.xml".to_string(),
            update_cron: "0 0 */6 * * * *".to_string(),
            username: Some("testuser".to_string()),
            password: Some("testpass".to_string()),
            timezone: Some("Europe/London".to_string()),
            time_offset: Some("+01:00".to_string()),
        };

        let created_source = repo.create(create_request).await?;
        assert_eq!(created_source.name, "Test EPG Source");
        assert_eq!(created_source.source_type, EpgSourceType::Xmltv);
        assert_eq!(created_source.time_offset, "+01:00");
        assert_eq!(created_source.original_timezone, Some("Europe/London".to_string()));

        // Test find by ID
        let found_source = repo.find_by_id(&created_source.id).await?;
        assert!(found_source.is_some());
        let found_source = found_source.unwrap();
        assert_eq!(found_source.id, created_source.id);
        assert_eq!(found_source.name, created_source.name);

        // Test find by type
        let type_sources = repo.find_by_type(&EpgSourceType::Xmltv).await?;
        assert_eq!(type_sources.len(), 1);
        assert_eq!(type_sources[0].id, created_source.id);

        // Test find active
        let active_sources = repo.find_active().await?;
        assert_eq!(active_sources.len(), 1);
        assert_eq!(active_sources[0].id, created_source.id);

        // Test find all
        let all_sources = repo.find_all().await?;
        assert_eq!(all_sources.len(), 1);
        assert_eq!(all_sources[0].id, created_source.id);

        // Test find non-existent
        let non_existent = repo.find_by_id(&Uuid::new_v4()).await?;
        assert!(non_existent.is_none());

        Ok(())
    }

    #[tokio::test]
    async fn test_multiple_epg_sources_different_types() -> Result<()> {
        let db = create_test_db().await?;
        let repo = EpgSourceSeaOrmRepository::new(db.connection().clone());

        // Create EPG sources of different types
        let sources = vec![
            ("XMLTV Source", EpgSourceType::Xmltv, "http://example.com/xmltv.xml"),
            ("Xtream Source", EpgSourceType::Xtream, "http://example.com/xtream"),
        ];

        let mut created_ids = Vec::new();
        for (name, source_type, url) in sources {
            let create_request = EpgSourceCreateRequest {
                name: name.to_string(),
                source_type: source_type.clone(),
                url: url.to_string(),
                update_cron: "0 0 */6 * * * *".to_string(),
                username: None,
                password: None,
                timezone: None,
                time_offset: None,
            };

            let created_source = repo.create(create_request).await?;
            created_ids.push(created_source.id);
            assert_eq!(created_source.source_type, source_type);
            assert_eq!(created_source.time_offset, "+00:00"); // Default value
        }

        // Test that all sources were created
        let all_sources = repo.find_all().await?;
        assert_eq!(all_sources.len(), 2);

        // Test type filtering
        let xmltv_sources = repo.find_by_type(&EpgSourceType::Xmltv).await?;
        assert_eq!(xmltv_sources.len(), 1);
        assert_eq!(xmltv_sources[0].source_type, EpgSourceType::Xmltv);

        let xtream_sources = repo.find_by_type(&EpgSourceType::Xtream).await?;
        assert_eq!(xtream_sources.len(), 1);
        assert_eq!(xtream_sources[0].source_type, EpgSourceType::Xtream);

        // Test that all sources have different IDs
        for i in 0..created_ids.len() {
            for j in (i+1)..created_ids.len() {
                assert_ne!(created_ids[i], created_ids[j]);
            }
        }

        Ok(())
    }
}