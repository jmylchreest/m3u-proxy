//! SeaORM-based Channel repository implementation
//!
//! This provides a database-agnostic repository for Channel operations using SeaORM.

use anyhow::Result;
use sea_orm::{ActiveModelTrait, DatabaseConnection, EntityTrait, Set, ColumnTrait, QueryFilter, PaginatorTrait, QueryOrder, QuerySelect};
use std::sync::Arc;
use uuid::Uuid;

use crate::entities::{prelude::Channels, channels};
use crate::models::Channel;

/// Request for channel creation
#[derive(Debug, Clone)]
pub struct ChannelCreateRequest {
    pub source_id: Uuid,
    pub tvg_id: Option<String>,
    pub tvg_name: Option<String>,
    pub tvg_chno: Option<String>,
    pub tvg_logo: Option<String>,
    pub tvg_shift: Option<String>,
    pub group_title: Option<String>,
    pub channel_name: String,
    pub stream_url: String,
}

/// SeaORM-based repository for Channel operations
#[derive(Clone)]
pub struct ChannelSeaOrmRepository {
    connection: Arc<DatabaseConnection>,
}

impl ChannelSeaOrmRepository {
    /// Create a new repository instance
    pub fn new(connection: Arc<DatabaseConnection>) -> Self {
        Self { connection }
    }

    /// Create a new channel
    pub async fn create(&self, request: ChannelCreateRequest) -> Result<Channel> {
        let now = chrono::Utc::now();
        let id = Uuid::new_v4();

        let active_model = channels::ActiveModel {
            id: Set(id),
            source_id: Set(request.source_id),
            tvg_id: Set(request.tvg_id.clone()),
            tvg_name: Set(request.tvg_name.clone()),
            tvg_chno: Set(request.tvg_chno.clone()),
            channel_name: Set(request.channel_name.clone()),
            tvg_logo: Set(request.tvg_logo.clone()),
            tvg_shift: Set(request.tvg_shift.clone()),
            group_title: Set(request.group_title.clone()),
            stream_url: Set(request.stream_url.clone()),
            created_at: Set(now),
            updated_at: Set(now),
        };

        let model = active_model.insert(&*self.connection).await?;

        Ok(Channel {
            id: model.id,
            source_id: model.source_id,
            tvg_id: model.tvg_id,
            tvg_name: model.tvg_name,
            tvg_chno: model.tvg_chno,
            channel_name: model.channel_name,
            tvg_logo: model.tvg_logo,
            tvg_shift: model.tvg_shift,
            group_title: model.group_title,
            stream_url: model.stream_url,
            created_at: model.created_at,
            updated_at: model.updated_at,
            video_codec: None,
            audio_codec: None,
            resolution: None,
            probe_method: None,
            last_probed_at: None,
        })
    }

    /// Find a channel by ID
    pub async fn find_by_id(&self, id: &Uuid) -> Result<Option<Channel>> {
        let model = Channels::find_by_id(*id)
            .one(&*self.connection)
            .await?;

        match model {
            Some(m) => Ok(Some(self.model_to_domain(m))),
            None => Ok(None),
        }
    }

    /// Find channels by source ID
    pub async fn find_by_source_id(&self, source_id: &Uuid) -> Result<Vec<Channel>> {
        let models = Channels::find()
            .filter(channels::Column::SourceId.eq(*source_id))
            .all(&*self.connection)
            .await?;
        
        Ok(models.into_iter().map(|m| self.model_to_domain(m)).collect())
    }

    /// Find all channels
    pub async fn find_all(&self) -> Result<Vec<Channel>> {
        let models = Channels::find().all(&*self.connection).await?;
        Ok(models.into_iter().map(|m| self.model_to_domain(m)).collect())
    }

    /// Find channels by group title
    pub async fn find_by_group_title(&self, group_title: &str) -> Result<Vec<Channel>> {
        let models = Channels::find()
            .filter(channels::Column::GroupTitle.eq(group_title))
            .all(&*self.connection)
            .await?;
        
        Ok(models.into_iter().map(|m| self.model_to_domain(m)).collect())
    }

    /// Find a channel by tvg_id
    pub async fn find_by_tvg_id(&self, tvg_id: &str) -> Result<Option<Channel>> {
        let model = Channels::find()
            .filter(channels::Column::TvgId.eq(tvg_id))
            .one(&*self.connection)
            .await?;

        match model {
            Some(m) => Ok(Some(self.model_to_domain(m))),
            None => Ok(None),
        }
    }

    /// Get channel name by ID
    pub async fn get_channel_name(&self, channel_id: Uuid) -> Result<Option<String>> {
        let model = Channels::find_by_id(channel_id)
            .one(&*self.connection)
            .await?;
        
        Ok(model.map(|m| m.channel_name))
    }

    /// Update all channels for a source (replaces existing channels)
    pub async fn update_source_channels(&self, source_id: Uuid, channels: &[Channel]) -> Result<()> {
        self.update_source_channels_with_batch_config(source_id, channels, None).await
    }

    /// Update all channels for a source with configurable batch size (replaces existing channels)
    pub async fn update_source_channels_with_batch_config(
        &self, 
        source_id: Uuid, 
        channels: &[Channel],
        batch_config: Option<&crate::config::DatabaseBatchConfig>
    ) -> Result<()> {
        use sea_orm::TransactionTrait;

        if channels.is_empty() {
            return Ok(());
        }

        // Use transaction for atomicity
        let txn = self.connection.begin().await?;

        // First, delete existing channels for this source
        Channels::delete_many()
            .filter(channels::Column::SourceId.eq(source_id))
            .exec(&txn)
            .await?;

        txn.commit().await?;

        // Use the optimized batch insert function from DatabaseOperations
        match crate::utils::database_operations::DatabaseOperations::insert_stream_channels_batch(
            channels.to_vec(),
            &*self.connection,
            batch_config,
        ).await {
            Ok(inserted_count) => {
                tracing::info!("Successfully inserted {} channels for source {}", inserted_count, source_id);
                Ok(())
            },
            Err(e) => {
                tracing::error!("Failed to insert channels for source {}: {}", source_id, e);
                Err(anyhow::anyhow!("Failed to insert channels: {}", e))
            }
        }
    }

    /// Convert SeaORM model to domain model
    fn model_to_domain(&self, model: channels::Model) -> Channel {
        Channel {
            id: model.id,
            source_id: model.source_id,
            tvg_id: model.tvg_id,
            tvg_name: model.tvg_name,
            tvg_chno: model.tvg_chno,
            channel_name: model.channel_name,
            tvg_logo: model.tvg_logo,
            tvg_shift: model.tvg_shift,
            group_title: model.group_title,
            stream_url: model.stream_url,
            created_at: model.created_at,
            updated_at: model.updated_at,
            video_codec: None,
            audio_codec: None,
            resolution: None,
            probe_method: None,
            last_probed_at: None,
        }
    }

    /// Get paginated channels for a specific source
    pub async fn get_source_channels_paginated(
        &self,
        source_id: &Uuid,
        page: Option<u64>,
        page_size: Option<u64>,
    ) -> Result<(Vec<Channel>, u64)> {
        let page = page.unwrap_or(1);
        let page_size = page_size.unwrap_or(50);
        let offset = (page - 1) * page_size;

        // Get total count
        let total_count = Channels::find()
            .filter(channels::Column::SourceId.eq(*source_id))
            .count(&*self.connection)
            .await?;

        // Get paginated results
        let models = Channels::find()
            .filter(channels::Column::SourceId.eq(*source_id))
            .order_by_asc(channels::Column::ChannelName)
            .limit(page_size)
            .offset(offset)
            .all(&*self.connection)
            .await?;

        let channels = models.into_iter().map(|m| self.model_to_domain(m)).collect();
        Ok((channels, total_count))
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
        let circuit_breaker = crate::utils::create_circuit_breaker(
            crate::utils::CircuitBreakerType::Simple,
            crate::utils::CircuitBreakerConfig::default()
        );
        let db = crate::database::Database {
            connection: arc_connection.clone(),
            read_connection: arc_connection,
            database_type: crate::database::DatabaseType::SQLite,
            backend: DatabaseBackend::Sqlite,
            ingestion_config: IngestionConfig::default(),
            circuit_breaker,
        };
        
        Ok(db)
    }

    #[tokio::test]
    async fn test_channel_crud_operations() -> Result<()> {
        let db = create_test_db().await?;
        let repo = ChannelSeaOrmRepository::new(db.connection().clone());

        let source_id = Uuid::new_v4();

        // Test create
        let create_request = ChannelCreateRequest {
            source_id,
            tvg_id: Some("bbc1hd".to_string()),
            tvg_name: Some("BBC One HD".to_string()),
            tvg_chno: Some("101".to_string()),
            tvg_logo: Some("http://example.com/logo.png".to_string()),
            tvg_shift: None,
            group_title: Some("Entertainment".to_string()),
            channel_name: "BBC One HD".to_string(),
            stream_url: "http://example.com/stream".to_string(),
        };

        let created_channel = repo.create(create_request).await?;
        assert_eq!(created_channel.channel_name, "BBC One HD");
        assert_eq!(created_channel.source_id, source_id);
        assert_eq!(created_channel.tvg_id, Some("bbc1hd".to_string()));

        // Test find by ID
        let found_channel = repo.find_by_id(&created_channel.id).await?;
        assert!(found_channel.is_some());
        let found_channel = found_channel.unwrap();
        assert_eq!(found_channel.id, created_channel.id);
        assert_eq!(found_channel.channel_name, created_channel.channel_name);

        // Test find by source ID
        let source_channels = repo.find_by_source_id(&source_id).await?;
        assert_eq!(source_channels.len(), 1);
        assert_eq!(source_channels[0].id, created_channel.id);

        // Test find by group title
        let group_channels = repo.find_by_group_title("Entertainment").await?;
        assert_eq!(group_channels.len(), 1);
        assert_eq!(group_channels[0].id, created_channel.id);

        // Test find all
        let all_channels = repo.find_all().await?;
        assert_eq!(all_channels.len(), 1);
        assert_eq!(all_channels[0].id, created_channel.id);

        // Test find non-existent
        let non_existent = repo.find_by_id(&Uuid::new_v4()).await?;
        assert!(non_existent.is_none());

        Ok(())
    }

    #[tokio::test]
    async fn test_multiple_channels_same_source() -> Result<()> {
        let db = create_test_db().await?;
        let repo = ChannelSeaOrmRepository::new(db.connection().clone());

        let source_id = Uuid::new_v4();

        // Create multiple channels for the same source
        let channels = vec![
            ("BBC One HD", "bbc1hd", "101", "Entertainment"),
            ("BBC Two HD", "bbc2hd", "102", "Entertainment"),
            ("ITV HD", "itvhd", "103", "Entertainment"),
        ];

        let mut created_ids = Vec::new();
        for (name, tvg_id, chno, group) in channels {
            let create_request = ChannelCreateRequest {
                source_id,
                tvg_id: Some(tvg_id.to_string()),
                tvg_name: Some(name.to_string()),
                tvg_chno: Some(chno.to_string()),
                tvg_logo: None,
                tvg_shift: None,
                group_title: Some(group.to_string()),
                channel_name: name.to_string(),
                stream_url: format!("http://example.com/{}", tvg_id),
            };

            let created_channel = repo.create(create_request).await?;
            created_ids.push(created_channel.id);
        }

        // Test that all channels were created for the source
        let source_channels = repo.find_by_source_id(&source_id).await?;
        assert_eq!(source_channels.len(), 3);

        // Test group filtering
        let entertainment_channels = repo.find_by_group_title("Entertainment").await?;
        assert_eq!(entertainment_channels.len(), 3);

        // Test that all channels have different IDs
        for i in 0..created_ids.len() {
            for j in (i+1)..created_ids.len() {
                assert_ne!(created_ids[i], created_ids[j]);
            }
        }

        Ok(())
    }
}