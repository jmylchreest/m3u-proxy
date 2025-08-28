//! Add last_known_codecs table for storing channel codec information
//!
//! This migration creates the last_known_codecs table to store codec information
//! obtained from probing channels.

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(LastKnownCodecs::Table)
                    .if_not_exists()
                    .col(
                        Self::create_id_column(manager, LastKnownCodecs::Id),
                    )
                    .col(
                        Self::create_uuid_fk_column(manager, LastKnownCodecs::ChannelId),
                    )
                    .col(
                        ColumnDef::new(LastKnownCodecs::VideoCodec)
                            .string()
                            .null(),
                    )
                    .col(
                        ColumnDef::new(LastKnownCodecs::AudioCodec)
                            .string()
                            .null(),
                    )
                    .col(
                        ColumnDef::new(LastKnownCodecs::ContainerFormat)
                            .string()
                            .null(),
                    )
                    .col(
                        ColumnDef::new(LastKnownCodecs::Resolution)
                            .string()
                            .null(),
                    )
                    .col(
                        ColumnDef::new(LastKnownCodecs::Framerate)
                            .string()
                            .null(),
                    )
                    .col(
                        ColumnDef::new(LastKnownCodecs::Bitrate)
                            .integer()
                            .null(),
                    )
                    .col(
                        ColumnDef::new(LastKnownCodecs::VideoBitrate)
                            .integer()
                            .null(),
                    )
                    .col(
                        ColumnDef::new(LastKnownCodecs::AudioBitrate)
                            .integer()
                            .null(),
                    )
                    .col(
                        ColumnDef::new(LastKnownCodecs::AudioChannels)
                            .string()
                            .null(),
                    )
                    .col(
                        ColumnDef::new(LastKnownCodecs::AudioSampleRate)
                            .integer()
                            .null(),
                    )
                    .col(
                        ColumnDef::new(LastKnownCodecs::ProbeMethod)
                            .string()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(LastKnownCodecs::ProbeSource)
                            .string()
                            .null(),
                    )
                    .col(
                        Self::create_timestamp_column(manager, LastKnownCodecs::DetectedAt),
                    )
                    .col(
                        Self::create_timestamp_column(manager, LastKnownCodecs::CreatedAt),
                    )
                    .col(
                        Self::create_timestamp_column(manager, LastKnownCodecs::UpdatedAt),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_last_known_codecs_channel_id")
                            .from(LastKnownCodecs::Table, LastKnownCodecs::ChannelId)
                            .to(Channels::Table, Channels::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        // Create indexes for performance
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

        manager
            .create_index(
                Index::create()
                    .name("idx_last_known_codecs_detected_at")
                    .table(LastKnownCodecs::Table)
                    .col(LastKnownCodecs::DetectedAt)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(LastKnownCodecs::Table).to_owned())
            .await
    }
}

impl Migration {
    /// Create ID column with optimal type for each database
    fn create_id_column(manager: &SchemaManager<'_>, column_name: impl sea_orm::Iden + 'static) -> ColumnDef {
        match manager.get_database_backend() {
            sea_orm::DatabaseBackend::Postgres => {
                ColumnDef::new(column_name).uuid().not_null().primary_key().to_owned()
            },
            _ => {
                ColumnDef::new(column_name).string().not_null().primary_key().to_owned()
            }
        }
    }

    /// Create UUID foreign key column with optimal type for each database
    fn create_uuid_fk_column(manager: &SchemaManager<'_>, column_name: impl sea_orm::Iden + 'static) -> ColumnDef {
        match manager.get_database_backend() {
            sea_orm::DatabaseBackend::Postgres => {
                ColumnDef::new(column_name).uuid().not_null().to_owned()
            },
            _ => {
                ColumnDef::new(column_name).string().not_null().to_owned()
            }
        }
    }

    /// Create timestamp column with database-specific types
    fn create_timestamp_column(manager: &SchemaManager<'_>, column_name: impl sea_orm::Iden + 'static) -> ColumnDef {
        match manager.get_database_backend() {
            sea_orm::DatabaseBackend::Postgres => {
                ColumnDef::new(column_name).timestamp_with_time_zone().not_null().to_owned()
            },
            sea_orm::DatabaseBackend::MySql => {
                ColumnDef::new(column_name).timestamp().not_null().to_owned()
            },
            _ => {
                // SQLite - use timestamp with SeaORM automatic conversion to DateTime<Utc>
                ColumnDef::new(column_name).timestamp().not_null().to_owned()
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