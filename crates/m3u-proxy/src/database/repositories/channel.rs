//! SeaORM-based Channel repository implementation
//!
//! This provides a database-agnostic repository for Channel operations using SeaORM.

use anyhow::Result;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, DatabaseConnection, EntityTrait,
    PaginatorTrait, QueryFilter, QueryOrder, QuerySelect, Set,
};
use std::sync::Arc;
use uuid::Uuid;

use crate::entities::{channels, prelude::Channels};
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
        let model = Channels::find_by_id(*id).one(&*self.connection).await?;

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

        Ok(models
            .into_iter()
            .map(|m| self.model_to_domain(m))
            .collect())
    }

    /// Find all channels
    pub async fn find_all(&self) -> Result<Vec<Channel>> {
        let models = Channels::find()
            .order_by_asc(channels::Column::ChannelName)
            .all(&*self.connection)
            .await?;
        Ok(models
            .into_iter()
            .map(|m| self.model_to_domain(m))
            .collect())
    }

    /// Find channels by group title
    pub async fn find_by_group_title(&self, group_title: &str) -> Result<Vec<Channel>> {
        let models = Channels::find()
            .filter(channels::Column::GroupTitle.eq(group_title))
            .all(&*self.connection)
            .await?;

        Ok(models
            .into_iter()
            .map(|m| self.model_to_domain(m))
            .collect())
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
    pub async fn update_source_channels(
        &self,
        source_id: Uuid,
        channels: &[Channel],
    ) -> Result<()> {
        self.update_source_channels_with_batch_config(source_id, channels, None)
            .await
    }

    /// Update all channels for a source with configurable batch size (replaces existing channels)
    pub async fn update_source_channels_with_batch_config(
        &self,
        source_id: Uuid,
        channels: &[Channel],
        batch_config: Option<&crate::config::DatabaseBatchConfig>,
    ) -> Result<()> {
        use sea_orm::TransactionTrait;

        if channels.is_empty() {
            return Ok(());
        }

        // Use a single transaction for both delete and insert operations
        let txn = self.connection.begin().await?;

        // Delete existing channels for this source
        let delete_result = Channels::delete_many()
            .filter(channels::Column::SourceId.eq(source_id))
            .exec(&txn)
            .await?;

        tracing::debug!(
            "Deleted {} existing channels for source {}",
            delete_result.rows_affected,
            source_id
        );

        // Use the batch insert function but pass the transaction instead of the connection
        match Self::insert_stream_channels_batch_in_transaction(
            channels.to_vec(),
            &txn,
            batch_config,
        )
        .await
        {
            Ok(inserted_count) => {
                // Commit the transaction only after both operations succeed
                txn.commit().await?;
                tracing::info!(
                    "Successfully updated {} channels for source {} (atomic operation)",
                    inserted_count,
                    source_id
                );
                Ok(())
            }
            Err(e) => {
                // Transaction will be automatically rolled back when dropped
                tracing::error!("Failed to insert channels for source {}: {}", source_id, e);
                Err(anyhow::anyhow!("Failed to insert channels: {}", e))
            }
        }
    }

    /// Insert stream channels in a transaction (helper method for atomic operations)
    async fn insert_stream_channels_batch_in_transaction(
        channels: Vec<Channel>,
        txn: &sea_orm::DatabaseTransaction,
        batch_config: Option<&crate::config::DatabaseBatchConfig>,
    ) -> Result<usize> {
        if channels.is_empty() {
            return Ok(0);
        }

        let batch_size = channels.len();
        tracing::debug!(
            "Inserting batch of {} stream channels using multi-value INSERT in transaction",
            batch_size
        );

        // Use configurable batch size based on database backend and user configuration
        let max_records_per_query = if let Some(config) = batch_config {
            config.safe_stream_channel_batch_size(txn.get_database_backend())
        } else {
            // Fallback to defaults if no config provided
            let default_config = crate::config::DatabaseBatchConfig::default();
            default_config.safe_stream_channel_batch_size(txn.get_database_backend())
        };

        tracing::debug!(
            "Using stream channel batch size: {} for backend: {:?}",
            max_records_per_query,
            txn.get_database_backend()
        );

        let mut total_inserted = 0;

        for chunk in channels.chunks(max_records_per_query) {
            if chunk.is_empty() {
                continue;
            }

            // Build multi-value INSERT statement with conflict resolution
            let mut query = match txn.get_database_backend() {
                sea_orm::DatabaseBackend::Postgres => String::from(
                    "INSERT INTO channels (id, source_id, tvg_id, tvg_name, tvg_chno, channel_name, tvg_logo, tvg_shift, group_title, stream_url, created_at, updated_at) VALUES ",
                ),
                sea_orm::DatabaseBackend::Sqlite => String::from(
                    "INSERT INTO channels (id, source_id, tvg_id, tvg_name, tvg_chno, channel_name, tvg_logo, tvg_shift, group_title, stream_url, created_at, updated_at) VALUES ",
                ),
                _ => String::from(
                    "INSERT INTO channels (id, source_id, tvg_id, tvg_name, tvg_chno, channel_name, tvg_logo, tvg_shift, group_title, stream_url, created_at, updated_at) VALUES ",
                ),
            };

            // Generate placeholders based on database backend
            let placeholders: Vec<String> = (0..chunk.len())
                .enumerate()
                .map(|(i, _)| {
                    let base_idx = i * 12; // 12 fields per channel
                    match txn.get_database_backend() {
                        sea_orm::DatabaseBackend::Postgres => {
                            format!(
                                "(${}, ${}, ${}, ${}, ${}, ${}, ${}, ${}, ${}, ${}, ${}, ${})",
                                base_idx + 1,
                                base_idx + 2,
                                base_idx + 3,
                                base_idx + 4,
                                base_idx + 5,
                                base_idx + 6,
                                base_idx + 7,
                                base_idx + 8,
                                base_idx + 9,
                                base_idx + 10,
                                base_idx + 11,
                                base_idx + 12
                            )
                        }
                        _ => "(?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)".to_string(),
                    }
                })
                .collect();
            query.push_str(&placeholders.join(", "));

            // Add conflict resolution clause based on database backend
            match txn.get_database_backend() {
                sea_orm::DatabaseBackend::Postgres => {
                    query.push_str(" ON CONFLICT (id) DO NOTHING");
                }
                sea_orm::DatabaseBackend::Sqlite => {
                    query.push_str(" ON CONFLICT (id) DO NOTHING");
                }
                _ => {
                    // For MySQL/MariaDB, use INSERT IGNORE or handle differently
                    // For now, we'll rely on the transaction isolation
                }
            }

            let mut values = Vec::new();

            // Collect all parameters - order must match INSERT statement
            // Use the deterministic UUIDs from the channel data
            for channel in chunk {
                values.push(channel.id.into()); // id
                values.push(channel.source_id.into()); // source_id
                values.push(channel.tvg_id.clone().into()); // tvg_id
                values.push(channel.tvg_name.clone().into()); // tvg_name
                values.push(channel.tvg_chno.clone().into()); // tvg_chno
                values.push(channel.channel_name.clone().into()); // channel_name
                values.push(channel.tvg_logo.clone().into()); // tvg_logo
                values.push(channel.tvg_shift.clone().into()); // tvg_shift
                values.push(channel.group_title.clone().into()); // group_title
                values.push(channel.stream_url.clone().into()); // stream_url
                values.push(channel.created_at.into()); // created_at
                values.push(channel.updated_at.into()); // updated_at
            }

            use sea_orm::{ConnectionTrait, Statement};
            let stmt = Statement::from_sql_and_values(txn.get_database_backend(), &query, values);
            let result = txn.execute(stmt).await.map_err(|e| {
                tracing::debug!("SQL execution failed: {}", e);
                tracing::debug!("Query was: {}", query);
                tracing::debug!("Attempted to insert {} channels in batch", chunk.len());
                anyhow::anyhow!("Failed to insert stream channels batch: {}", e)
            })?;

            let rows_affected = result.rows_affected() as usize;
            total_inserted += rows_affected;

            // Log if fewer rows were inserted than expected (due to conflicts)
            if rows_affected < chunk.len() {
                tracing::debug!(
                    "Inserted {} out of {} channels in batch (some may have been deduplicated)",
                    rows_affected,
                    chunk.len()
                );
            } else {
                tracing::trace!("Inserted {} channels in multi-value query", rows_affected);
            }
        }

        tracing::debug!(
            "Successfully prepared {} stream channels for insertion in transaction",
            total_inserted
        );
        Ok(total_inserted)
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

        let channels = models
            .into_iter()
            .map(|m| self.model_to_domain(m))
            .collect();
        Ok((channels, total_count))
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
    async fn test_channel_crud_operations() -> Result<()> {
        let db = create_test_db().await?;
        let repo = ChannelSeaOrmRepository::new(db.connection().clone());

        let source_id = Uuid::new_v4();

        // Test create
        use crate::utils::SampleDataGenerator;
        let mut generator = SampleDataGenerator::new();
        let sample_channel =
            generator.generate_sample_channels(1, Some("entertainment"))[0].clone();

        let create_request = ChannelCreateRequest {
            source_id,
            tvg_id: Some(sample_channel.tvg_id.clone()),
            tvg_name: sample_channel.tvg_name.clone(),
            tvg_chno: sample_channel.tvg_chno.clone(),
            tvg_logo: sample_channel.tvg_logo.clone(),
            tvg_shift: None,
            group_title: Some(sample_channel.group_title.clone()),
            channel_name: sample_channel.channel_name.clone(),
            stream_url: sample_channel.stream_url.clone(),
        };

        let created_channel = repo.create(create_request.clone()).await?;
        assert_eq!(created_channel.channel_name, sample_channel.channel_name);
        assert_eq!(created_channel.source_id, source_id);
        assert_eq!(created_channel.tvg_id, Some(sample_channel.tvg_id));

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

        // Create multiple channels for the same source using sample data
        use crate::utils::SampleDataGenerator;
        let mut generator = SampleDataGenerator::new();
        let sample_channels = generator.generate_sample_channels(3, Some("entertainment"));

        let channels: Vec<(String, String, String, String)> = sample_channels
            .iter()
            .enumerate()
            .map(|(i, ch)| {
                (
                    ch.channel_name.clone(),
                    ch.tvg_id.clone(),
                    format!("{}", 101 + i),
                    "Entertainment".to_string(),
                )
            })
            .collect();

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
            for j in (i + 1)..created_ids.len() {
                assert_ne!(created_ids[i], created_ids[j]);
            }
        }

        Ok(())
    }
}
