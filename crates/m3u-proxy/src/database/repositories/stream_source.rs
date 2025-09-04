//! SeaORM-based StreamSource repository implementation
//!
//! This provides a database-agnostic repository for StreamSource operations using SeaORM.

use anyhow::Result;
use sea_orm::{ActiveModelTrait, DatabaseConnection, EntityTrait, Set, ColumnTrait, QueryFilter};
use std::sync::Arc;
use uuid::Uuid;

use crate::entities::{prelude::StreamSources, stream_sources};
use crate::models::{StreamSource, StreamSourceCreateRequest, StreamSourceType};

/// SeaORM-based repository for StreamSource operations
#[derive(Clone)]
pub struct StreamSourceSeaOrmRepository {
    connection: Arc<DatabaseConnection>,
}

impl StreamSourceSeaOrmRepository {
    /// Create a new repository instance
    pub fn new(connection: Arc<DatabaseConnection>) -> Self {
        Self { connection }
    }

    /// Get database connection for direct operations
    pub fn get_connection(&self) -> &Arc<DatabaseConnection> {
        &self.connection
    }

    /// Create a new stream source
    pub async fn create(&self, request: StreamSourceCreateRequest) -> Result<StreamSource> {
        let now = chrono::Utc::now();
        let id = Uuid::new_v4();

        let active_model = stream_sources::ActiveModel {
            id: Set(id),
            name: Set(request.name.clone()),
            source_type: Set(request.source_type),
            url: Set(request.url.clone()),
            max_concurrent_streams: Set(request.max_concurrent_streams),
            update_cron: Set(request.update_cron.clone()),
            username: Set(request.username.clone()),
            password: Set(request.password.clone()),
            field_map: Set(request.field_map.clone()),
            ignore_channel_numbers: Set(request.ignore_channel_numbers),
            created_at: Set(now),
            updated_at: Set(now),
            last_ingested_at: Set(None),
            is_active: Set(true),
        };

        // For now, repository methods work normally - circuit breaker available but not required
        // Services can choose to wrap critical operations as needed
        let model = active_model.insert(&*self.connection).await?;
        Ok(StreamSource {
            id: model.id,
            name: model.name,
            source_type: model.source_type,
            url: model.url,
            max_concurrent_streams: model.max_concurrent_streams,
            update_cron: model.update_cron,
            username: model.username,
            password: model.password,
            field_map: model.field_map,
            ignore_channel_numbers: model.ignore_channel_numbers,
            created_at: model.created_at,
            updated_at: model.updated_at,
            last_ingested_at: model.last_ingested_at.as_ref().map(|time| *time),
            is_active: model.is_active,
        })
    }

    /// Find a stream source by ID
    pub async fn find_by_id(&self, id: &Uuid) -> Result<Option<StreamSource>> {
        let model = StreamSources::find_by_id(*id)
            .one(&*self.connection)
            .await?;

        match model {
            Some(m) => Ok(Some(StreamSource {
                id: m.id,
                name: m.name,
                source_type: m.source_type,
                url: m.url,
                max_concurrent_streams: m.max_concurrent_streams,
                update_cron: m.update_cron,
                username: m.username,
                password: m.password,
                field_map: m.field_map,
                ignore_channel_numbers: m.ignore_channel_numbers,
                created_at: m.created_at,
                updated_at: m.updated_at,
                last_ingested_at: m.last_ingested_at.as_ref().map(|time| *time),
                is_active: m.is_active,
            })),
            None => Ok(None)
        }
    }

    /// Find all stream sources
    pub async fn find_all(&self) -> Result<Vec<StreamSource>> {
        let models = StreamSources::find().all(&*self.connection).await?;
        let mut results = Vec::new();
        for m in models {
            results.push(StreamSource {
                id: m.id,
                name: m.name,
                source_type: m.source_type,
                url: m.url,
                max_concurrent_streams: m.max_concurrent_streams,
                update_cron: m.update_cron,
                username: m.username,
                password: m.password,
                field_map: m.field_map,
                ignore_channel_numbers: m.ignore_channel_numbers,
                created_at: m.created_at,
                updated_at: m.updated_at,
                last_ingested_at: m.last_ingested_at.as_ref().map(|time| *time),
                is_active: m.is_active,
            });
        }
        Ok(results)
    }

    /// Find stream sources by URL and source type (for URL-based linking)
    pub async fn find_by_url_and_type(&self, url: &str, source_type: StreamSourceType) -> Result<Vec<StreamSource>> {
        let models = StreamSources::find()
            .filter(stream_sources::Column::Url.eq(url))
            .filter(stream_sources::Column::SourceType.eq(source_type))
            .filter(stream_sources::Column::IsActive.eq(true))
            .all(&*self.connection)
            .await?;
        
        let mut results = Vec::new();
        for m in models {
            results.push(StreamSource {
                id: m.id,
                name: m.name,
                source_type: m.source_type,
                url: m.url,
                max_concurrent_streams: m.max_concurrent_streams,
                update_cron: m.update_cron,
                username: m.username,
                password: m.password,
                field_map: m.field_map,
                ignore_channel_numbers: m.ignore_channel_numbers,
                created_at: m.created_at,
                updated_at: m.updated_at,
                last_ingested_at: m.last_ingested_at.as_ref().map(|time| *time),
                is_active: m.is_active,
            });
        }
        Ok(results)
    }

    /// Update the last ingested timestamp for a stream source
    pub async fn update_last_ingested_at(&self, id: &Uuid) -> Result<chrono::DateTime<chrono::Utc>> {
        use sea_orm::{Set, ActiveModelTrait};
        
        let now = chrono::Utc::now();
        
        // Find the existing source
        let existing = StreamSources::find_by_id(id.to_owned())
            .one(&*self.connection)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Stream source not found"))?;

        // Update just the timestamp
        let mut active_model: stream_sources::ActiveModel = existing.into();
        active_model.last_ingested_at = Set(Some(now));
        active_model.updated_at = Set(now);

        active_model.update(&*self.connection).await?;
        Ok(now)
    }

    /// Get channel count for a stream source
    pub async fn get_channel_count_for_source(&self, source_id: &Uuid) -> Result<u64> {
        use crate::entities::{prelude::Channels, channels};
        use sea_orm::PaginatorTrait;

        let count = Channels::find()
            .filter(channels::Column::SourceId.eq(source_id.to_owned()))
            .paginate(&*self.connection, 1)
            .num_items()
            .await?;

        Ok(count)
    }

    /// Find active stream sources only
    pub async fn find_active(&self) -> Result<Vec<StreamSource>> {
        let models = StreamSources::find()
            .filter(stream_sources::Column::IsActive.eq(true))
            .all(&*self.connection)
            .await?;

        let mut results = Vec::new();
        for m in models {
            results.push(StreamSource {
                id: m.id,
                name: m.name,
                url: m.url,
                source_type: m.source_type,
                update_cron: m.update_cron,
                username: m.username,
                password: m.password,
                is_active: m.is_active,
                field_map: m.field_map,
                ignore_channel_numbers: m.ignore_channel_numbers,
                created_at: m.created_at,
                updated_at: m.updated_at,
                last_ingested_at: m.last_ingested_at.as_ref().map(|time| *time),
                max_concurrent_streams: m.max_concurrent_streams,
            });
        }
        Ok(results)
    }

    /// Update a stream source
    pub async fn update(&self, id: &Uuid, request: crate::models::StreamSourceUpdateRequest) -> Result<StreamSource> {
        use sea_orm::{Set, ActiveModelTrait};
        
        let active_model = stream_sources::ActiveModel {
            id: Set(*id),
            name: Set(request.name),
            url: Set(request.url),
            source_type: Set(request.source_type),
            max_concurrent_streams: Set(request.max_concurrent_streams),
            update_cron: Set(request.update_cron),
            username: Set(request.username),
            password: Set(request.password),
            field_map: Set(request.field_map),
            ignore_channel_numbers: Set(request.ignore_channel_numbers),
            is_active: Set(request.is_active),
            updated_at: Set(chrono::Utc::now()),
            ..Default::default()
        };

        let updated_model = active_model.update(&*self.connection).await?;
        
        Ok(StreamSource {
            id: updated_model.id,
            name: updated_model.name,
            url: updated_model.url,
            source_type: updated_model.source_type,
            max_concurrent_streams: updated_model.max_concurrent_streams,
            update_cron: updated_model.update_cron,
            username: updated_model.username,
            password: updated_model.password,
            field_map: updated_model.field_map,
            ignore_channel_numbers: updated_model.ignore_channel_numbers,
            is_active: updated_model.is_active,
            created_at: updated_model.created_at,
            updated_at: updated_model.updated_at,
            last_ingested_at: updated_model.last_ingested_at.as_ref().map(|time| *time),
        })
    }

    /// Delete a stream source
    pub async fn delete(&self, id: &Uuid) -> Result<()> {
        let result = StreamSources::delete_by_id(*id).exec(&*self.connection).await?;
        if result.rows_affected == 0 {
            return Err(anyhow::anyhow!("Stream source not found"));
        }
        Ok(())
    }

    /// List stream sources with statistics (only active sources)
    pub async fn list_with_stats(&self) -> Result<Vec<crate::models::StreamSourceWithStats>> {
        let sources = self.find_active().await?;
        let mut results = Vec::new();
        
        for source in sources {
            let channel_count = self.get_channel_count_for_source(&source.id).await?;
            
            // Calculate next scheduled update from cron expression
            let next_scheduled_update = if source.is_active {
                Self::calculate_next_update_time(&source.update_cron, source.last_ingested_at)
            } else {
                None
            };
            
            results.push(crate::models::StreamSourceWithStats {
                source,
                channel_count: channel_count as i64,
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
                if let Some(last_ingested) = last_ingested_at {
                    // Find next update after last ingestion
                    schedule.after(&last_ingested).next()
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
            CREATE TABLE stream_sources (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                source_type INTEGER NOT NULL,
                url TEXT NOT NULL,
                max_concurrent_streams INTEGER NOT NULL,
                update_cron TEXT NOT NULL,
                username TEXT,
                password TEXT,
                field_map TEXT,
                ignore_channel_numbers INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                last_ingested_at TEXT,
                is_active INTEGER NOT NULL DEFAULT 1
            );
            CREATE TABLE channels (
                id TEXT PRIMARY KEY,
                source_id TEXT NOT NULL,
                tvg_id TEXT,
                tvg_name TEXT,
                tvg_chno TEXT,
                channel_name TEXT NOT NULL,
                tvg_logo TEXT,
                tvg_shift TEXT,
                group_title TEXT,
                stream_url TEXT NOT NULL,
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
    async fn test_stream_source_crud_operations() -> Result<()> {
        let db = create_test_db().await?;
        let repo = StreamSourceSeaOrmRepository::new(db.connection().clone());

        // Test create
        let create_request = StreamSourceCreateRequest {
            name: "Test Source".to_string(),
            source_type: StreamSourceType::M3u,
            url: "http://example.com/test.m3u".to_string(),
            max_concurrent_streams: 5,
            update_cron: "0 0 */6 * * * *".to_string(),
            username: Some("testuser".to_string()),
            password: Some("testpass".to_string()),
            field_map: None,
            ignore_channel_numbers: false,
        };

        let created_source = repo.create(create_request).await?;
        assert_eq!(created_source.name, "Test Source");
        assert_eq!(created_source.source_type, StreamSourceType::M3u);
        assert_eq!(created_source.max_concurrent_streams, 5);

        // Test find by ID
        let found_source = repo.find_by_id(&created_source.id).await?;
        assert!(found_source.is_some());
        let found_source = found_source.unwrap();
        assert_eq!(found_source.id, created_source.id);
        assert_eq!(found_source.name, created_source.name);

        // Test find all
        let all_sources = repo.find_all().await?;
        assert_eq!(all_sources.len(), 1);
        assert_eq!(all_sources[0].id, created_source.id);

        // Test find non-existent
        let non_existent = repo.find_by_id(&Uuid::new_v4()).await?;
        assert!(non_existent.is_none());

        Ok(())
    }

    // Circuit breaker integration test removed - repositories no longer have direct circuit breakers
    // Circuit breakers are now managed at the service level through HttpClientFactory
}

