//! Update last_known_codecs to key by stream_url instead of channel_id
//!
//! This migration updates the last_known_codecs table to use stream_url
//! as the primary identifier instead of channel_id for better stability
//! across ingestion cycles.

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // First, create a backup of existing data to migrate it properly
        // We'll add the new column, populate it with data from channels table,
        // then remove the old column and foreign key
        
        // Step 1: Add stream_url column
        manager
            .alter_table(
                Table::alter()
                    .table(LastKnownCodecs::Table)
                    .add_column(
                        ColumnDef::new(LastKnownCodecs::StreamUrl)
                            .string()
                            .null() // Initially nullable to allow migration
                    )
                    .to_owned(),
            )
            .await?;

        // Step 2: Populate stream_url from channels table using raw SQL
        let db_connection = manager.get_connection();
        let update_sql = match manager.get_database_backend() {
            sea_orm::DatabaseBackend::Postgres => {
                "UPDATE last_known_codecs 
                 SET stream_url = channels.stream_url 
                 FROM channels 
                 WHERE last_known_codecs.channel_id = channels.id"
            },
            sea_orm::DatabaseBackend::MySql => {
                "UPDATE last_known_codecs 
                 INNER JOIN channels ON last_known_codecs.channel_id = channels.id 
                 SET last_known_codecs.stream_url = channels.stream_url"
            },
            _ => {
                // SQLite
                "UPDATE last_known_codecs 
                 SET stream_url = (
                     SELECT stream_url FROM channels 
                     WHERE channels.id = last_known_codecs.channel_id
                 )"
            }
        };

        db_connection.execute_unprepared(update_sql).await?;

        // Step 3: Make stream_url not nullable and add unique index
        manager
            .alter_table(
                Table::alter()
                    .table(LastKnownCodecs::Table)
                    .modify_column(
                        ColumnDef::new(LastKnownCodecs::StreamUrl)
                            .string()
                            .not_null()
                    )
                    .to_owned(),
            )
            .await?;

        // Step 4: Create unique index on stream_url
        manager
            .create_index(
                Index::create()
                    .name("idx_last_known_codecs_stream_url")
                    .table(LastKnownCodecs::Table)
                    .col(LastKnownCodecs::StreamUrl)
                    .unique()
                    .to_owned(),
            )
            .await?;

        // Step 5: Drop the old channel_id foreign key constraint
        manager
            .drop_foreign_key(
                ForeignKey::drop()
                    .name("fk_last_known_codecs_channel_id")
                    .table(LastKnownCodecs::Table)
                    .to_owned(),
            )
            .await?;

        // Step 6: Drop the old channel_id index
        manager
            .drop_index(
                Index::drop()
                    .name("idx_last_known_codecs_channel_id")
                    .table(LastKnownCodecs::Table)
                    .to_owned(),
            )
            .await?;

        // Step 7: Drop the channel_id column
        manager
            .alter_table(
                Table::alter()
                    .table(LastKnownCodecs::Table)
                    .drop_column(LastKnownCodecs::ChannelId)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // This migration is not easily reversible since we're changing the key
        // structure. In a rollback scenario, we'd need to:
        // 1. Re-add channel_id column
        // 2. Try to populate it from current channel data (may lose orphaned records)
        // 3. Remove stream_url column
        
        // For now, we'll provide a basic rollback that adds back the column structure
        // but note that data may be lost
        
        // Step 1: Add back channel_id column
        manager
            .alter_table(
                Table::alter()
                    .table(LastKnownCodecs::Table)
                    .add_column(
                        Self::create_uuid_fk_column(manager, LastKnownCodecs::ChannelId)
                    )
                    .to_owned(),
            )
            .await?;

        // Step 2: Try to populate channel_id from channels table using raw SQL
        // This may not work for all records if channels have been recreated
        let db_connection = manager.get_connection();
        let update_sql = match manager.get_database_backend() {
            sea_orm::DatabaseBackend::Postgres => {
                "UPDATE last_known_codecs 
                 SET channel_id = channels.id 
                 FROM channels 
                 WHERE last_known_codecs.stream_url = channels.stream_url"
            },
            sea_orm::DatabaseBackend::MySql => {
                "UPDATE last_known_codecs 
                 INNER JOIN channels ON last_known_codecs.stream_url = channels.stream_url 
                 SET last_known_codecs.channel_id = channels.id"
            },
            _ => {
                // SQLite
                "UPDATE last_known_codecs 
                 SET channel_id = (
                     SELECT id FROM channels 
                     WHERE channels.stream_url = last_known_codecs.stream_url
                     LIMIT 1
                 )"
            }
        };

        db_connection.execute_unprepared(update_sql).await?;

        // Step 3: Clean up records that couldn't be matched (orphaned)
        let delete_sql = "DELETE FROM last_known_codecs WHERE channel_id IS NULL";
        db_connection.execute_unprepared(delete_sql).await?;

        // Step 4: Re-create foreign key constraint
        manager
            .alter_table(
                Table::alter()
                    .table(LastKnownCodecs::Table)
                    .add_foreign_key(
                        TableForeignKey::new()
                            .name("fk_last_known_codecs_channel_id")
                            .from_tbl(LastKnownCodecs::Table)
                            .from_col(LastKnownCodecs::ChannelId)
                            .to_tbl(Channels::Table)
                            .to_col(Channels::Id)
                            .on_delete(ForeignKeyAction::Cascade)
                    )
                    .to_owned(),
            )
            .await?;

        // Step 5: Re-create channel_id index
        manager
            .create_index(
                Index::create()
                    .name("idx_last_known_codecs_channel_id")
                    .table(LastKnownCodecs::Table)
                    .col(LastKnownCodecs::ChannelId)
                    .unique()
                    .to_owned(),
            )
            .await?;

        // Step 6: Drop stream_url index
        manager
            .drop_index(
                Index::drop()
                    .name("idx_last_known_codecs_stream_url")
                    .table(LastKnownCodecs::Table)
                    .to_owned(),
            )
            .await?;

        // Step 7: Drop stream_url column
        manager
            .alter_table(
                Table::alter()
                    .table(LastKnownCodecs::Table)
                    .drop_column(LastKnownCodecs::StreamUrl)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }
}

impl Migration {
    /// Create UUID foreign key column with optimal type for each database
    fn create_uuid_fk_column(manager: &SchemaManager<'_>, column_name: impl sea_orm::Iden + 'static) -> ColumnDef {
        match manager.get_database_backend() {
            sea_orm::DatabaseBackend::Postgres => {
                ColumnDef::new(column_name).uuid().null().to_owned() // Nullable during migration
            },
            _ => {
                ColumnDef::new(column_name).string().null().to_owned()
            }
        }
    }
}

/// Entity identifiers for the last_known_codecs table
#[derive(DeriveIden)]
pub enum LastKnownCodecs {
    Table,
    Id,
    ChannelId,
    StreamUrl,
    VideoCodec,
    AudioCodec,
    ContainerFormat,
    Resolution,
    Framerate,
    Bitrate,
    VideoBitrate,
    AudioBitrate,
    AudioChannels,
    AudioSampleRate,
    ProbeMethod,
    ProbeSource,
    DetectedAt,
    CreatedAt,
    UpdatedAt,
}

/// Reference to the Channels table
#[derive(DeriveIden)]
enum Channels {
    Table,
    Id,
}