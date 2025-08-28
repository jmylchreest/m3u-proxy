//! Initial schema migration that works across SQLite, PostgreSQL, and MySQL
//!
//! This migration creates the complete database schema for the M3U Proxy application.
//! It includes database-specific optimizations where appropriate.

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Create all tables
        self.create_stream_sources_table(manager).await?;
        self.create_epg_sources_table(manager).await?;
        self.create_stream_proxies_table(manager).await?;
        self.create_proxy_sources_table(manager).await?;
        self.create_proxy_epg_sources_table(manager).await?;
        self.create_filters_table(manager).await?;
        self.create_proxy_filters_table(manager).await?;
        self.create_channels_table(manager).await?;
        self.create_epg_channels_table(manager).await?;
        self.create_epg_programs_table(manager).await?;
        self.create_channel_epg_mapping_table(manager).await?;
        self.create_proxy_generations_table(manager).await?;
        self.create_data_mapping_rules_table(manager).await?;
        self.create_logo_assets_table(manager).await?;
        self.create_relay_profiles_table(manager).await?;
        self.create_channel_relay_configs_table(manager).await?;
        self.create_relay_runtime_status_table(manager).await?;
        self.create_relay_events_table(manager).await?;
        self.create_migration_notes_table(manager).await?;
        self.create_linked_xtream_sources_table(manager).await?;

        // Create indexes for performance
        self.create_indexes(manager).await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Drop tables in reverse order to handle foreign key constraints
        manager.drop_table(Table::drop().table(LinkedXtreamSources::Table).to_owned()).await?;
        manager.drop_table(Table::drop().table(MigrationNotes::Table).to_owned()).await?;
        manager.drop_table(Table::drop().table(RelayEvents::Table).to_owned()).await?;
        manager.drop_table(Table::drop().table(RelayRuntimeStatus::Table).to_owned()).await?;
        manager.drop_table(Table::drop().table(ChannelRelayConfigs::Table).to_owned()).await?;
        manager.drop_table(Table::drop().table(RelayProfiles::Table).to_owned()).await?;
        manager.drop_table(Table::drop().table(LogoAssets::Table).to_owned()).await?;
        manager.drop_table(Table::drop().table(DataMappingRules::Table).to_owned()).await?;
        manager.drop_table(Table::drop().table(ProxyGenerations::Table).to_owned()).await?;
        manager.drop_table(Table::drop().table(ChannelEpgMapping::Table).to_owned()).await?;
        manager.drop_table(Table::drop().table(EpgPrograms::Table).to_owned()).await?;
        manager.drop_table(Table::drop().table(EpgChannels::Table).to_owned()).await?;
        manager.drop_table(Table::drop().table(Channels::Table).to_owned()).await?;
        manager.drop_table(Table::drop().table(ProxyFilters::Table).to_owned()).await?;
        manager.drop_table(Table::drop().table(Filters::Table).to_owned()).await?;
        manager.drop_table(Table::drop().table(ProxyEpgSources::Table).to_owned()).await?;
        manager.drop_table(Table::drop().table(ProxySources::Table).to_owned()).await?;
        manager.drop_table(Table::drop().table(StreamProxies::Table).to_owned()).await?;
        manager.drop_table(Table::drop().table(EpgSources::Table).to_owned()).await?;
        manager.drop_table(Table::drop().table(StreamSources::Table).to_owned()).await?;

        Ok(())
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
    /// For entities using DateTime<Utc>, creates proper timestamp columns for all databases
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

    /// Create nullable timestamp column with database-specific types
    /// For entities using Option<DateTime<Utc>>, creates proper nullable timestamp columns
    fn create_nullable_timestamp_column(manager: &SchemaManager<'_>, column_name: impl sea_orm::Iden + 'static) -> ColumnDef {
        match manager.get_database_backend() {
            sea_orm::DatabaseBackend::Postgres => {
                ColumnDef::new(column_name).timestamp_with_time_zone().to_owned()
            },
            sea_orm::DatabaseBackend::MySql => {
                ColumnDef::new(column_name).timestamp().to_owned()
            },
            _ => {
                // SQLite - use timestamp with SeaORM automatic conversion to DateTime<Utc>
                ColumnDef::new(column_name).timestamp().to_owned()
            }
        }
    }

    /// Create nullable UUID foreign key column with optimal type for each database  
    fn create_nullable_uuid_fk_column(manager: &SchemaManager<'_>, column_name: impl sea_orm::Iden + 'static) -> ColumnDef {
        match manager.get_database_backend() {
            sea_orm::DatabaseBackend::Postgres => {
                ColumnDef::new(column_name).uuid().to_owned()
            },
            _ => {
                ColumnDef::new(column_name).string().to_owned()
            }
        }
    }

    async fn create_stream_sources_table(&self, manager: &SchemaManager<'_>) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(StreamSources::Table)
                    .if_not_exists()
                    .col(Self::create_id_column(manager, StreamSources::Id))
                    .col(ColumnDef::new(StreamSources::Name).string().not_null())
                    .col(ColumnDef::new(StreamSources::SourceType).string().not_null())
                    .col(ColumnDef::new(StreamSources::Url).string().not_null())
                    .col(ColumnDef::new(StreamSources::MaxConcurrentStreams).integer().not_null().default(1))
                    .col(ColumnDef::new(StreamSources::UpdateCron).string().not_null().default("0 0 0 */6 * * * * *"))
                    .col(ColumnDef::new(StreamSources::Username).string())
                    .col(ColumnDef::new(StreamSources::Password).string())
                    .col(ColumnDef::new(StreamSources::FieldMap).string())
                    .col(ColumnDef::new(StreamSources::IgnoreChannelNumbers).boolean().not_null().default(true))
                    .col(Self::create_timestamp_column(manager, StreamSources::CreatedAt))
                    .col(Self::create_timestamp_column(manager, StreamSources::UpdatedAt))
                    .col(Self::create_nullable_timestamp_column(manager, StreamSources::LastIngestedAt))
                    .col(ColumnDef::new(StreamSources::IsActive).boolean().not_null().default(true))
                    .to_owned(),
            )
            .await
    }

    async fn create_epg_sources_table(&self, manager: &SchemaManager<'_>) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(EpgSources::Table)
                    .if_not_exists()
                    .col(Self::create_id_column(manager, EpgSources::Id))
                    .col(ColumnDef::new(EpgSources::Name).string().not_null())
                    .col(ColumnDef::new(EpgSources::SourceType).string().not_null())
                    .col(ColumnDef::new(EpgSources::Url).string().not_null())
                    .col(ColumnDef::new(EpgSources::UpdateCron).string().not_null().default("0 0 0 */6 * * * * *"))
                    .col(ColumnDef::new(EpgSources::Username).string())
                    .col(ColumnDef::new(EpgSources::Password).string())
                    .col(ColumnDef::new(EpgSources::OriginalTimezone).string())
                    .col(ColumnDef::new(EpgSources::TimeOffset).string().default("0"))
                    .col(Self::create_timestamp_column(manager, EpgSources::CreatedAt))
                    .col(Self::create_timestamp_column(manager, EpgSources::UpdatedAt))
                    .col(Self::create_nullable_timestamp_column(manager, EpgSources::LastIngestedAt))
                    .col(ColumnDef::new(EpgSources::IsActive).boolean().not_null().default(true))
                    .to_owned(),
            )
            .await
    }

    async fn create_stream_proxies_table(&self, manager: &SchemaManager<'_>) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(StreamProxies::Table)
                    .if_not_exists()
                    .col(Self::create_id_column(manager, StreamProxies::Id))
                    .col(ColumnDef::new(StreamProxies::Name).string().not_null())
                    .col(ColumnDef::new(StreamProxies::Description).string())
                    .col(ColumnDef::new(StreamProxies::ProxyMode).string().not_null().default("redirect"))
                    .col(ColumnDef::new(StreamProxies::UpstreamTimeout).integer().default(30))
                    .col(ColumnDef::new(StreamProxies::BufferSize).integer().default(8192))
                    .col(ColumnDef::new(StreamProxies::MaxConcurrentStreams).integer().default(1))
                    .col(ColumnDef::new(StreamProxies::StartingChannelNumber).integer().not_null().default(1))
                    .col(Self::create_timestamp_column(manager, StreamProxies::CreatedAt))
                    .col(Self::create_timestamp_column(manager, StreamProxies::UpdatedAt))
                    .col(Self::create_nullable_timestamp_column(manager, StreamProxies::LastGeneratedAt))
                    .col(ColumnDef::new(StreamProxies::IsActive).boolean().not_null().default(true))
                    .col(ColumnDef::new(StreamProxies::AutoRegenerate).boolean().not_null().default(false))
                    .col(ColumnDef::new(StreamProxies::CacheChannelLogos).boolean().not_null().default(true))
                    .col(ColumnDef::new(StreamProxies::CacheProgramLogos).boolean().not_null().default(false))
                    .col(Self::create_nullable_uuid_fk_column(manager, StreamProxies::RelayProfileId))
                    .to_owned(),
            )
            .await
    }

    async fn create_proxy_sources_table(&self, manager: &SchemaManager<'_>) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(ProxySources::Table)
                    .if_not_exists()
                    .col(Self::create_uuid_fk_column(manager, ProxySources::ProxyId))
                    .col(Self::create_uuid_fk_column(manager, ProxySources::SourceId))
                    .col(ColumnDef::new(ProxySources::PriorityOrder).integer().not_null().default(0))
                    .col(Self::create_timestamp_column(manager, ProxySources::CreatedAt))
                    .primary_key(Index::create().col(ProxySources::ProxyId).col(ProxySources::SourceId))
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_proxy_sources_proxy_id")
                            .from(ProxySources::Table, ProxySources::ProxyId)
                            .to(StreamProxies::Table, StreamProxies::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_proxy_sources_source_id")
                            .from(ProxySources::Table, ProxySources::SourceId)
                            .to(StreamSources::Table, StreamSources::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn create_proxy_epg_sources_table(&self, manager: &SchemaManager<'_>) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(ProxyEpgSources::Table)
                    .if_not_exists()
                    .col(Self::create_uuid_fk_column(manager, ProxyEpgSources::ProxyId))
                    .col(Self::create_uuid_fk_column(manager, ProxyEpgSources::EpgSourceId))
                    .col(ColumnDef::new(ProxyEpgSources::PriorityOrder).integer().not_null().default(0))
                    .col(Self::create_timestamp_column(manager, ProxyEpgSources::CreatedAt))
                    .primary_key(Index::create().col(ProxyEpgSources::ProxyId).col(ProxyEpgSources::EpgSourceId))
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_proxy_epg_sources_proxy_id")
                            .from(ProxyEpgSources::Table, ProxyEpgSources::ProxyId)
                            .to(StreamProxies::Table, StreamProxies::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_proxy_epg_sources_epg_source_id")
                            .from(ProxyEpgSources::Table, ProxyEpgSources::EpgSourceId)
                            .to(EpgSources::Table, EpgSources::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn create_filters_table(&self, manager: &SchemaManager<'_>) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Filters::Table)
                    .if_not_exists()
                    .col(Self::create_id_column(manager, Filters::Id))
                    .col(ColumnDef::new(Filters::Name).string().not_null())
                    .col(ColumnDef::new(Filters::SourceType).string().not_null().default("stream"))
                    .col(ColumnDef::new(Filters::IsInverse).boolean().not_null().default(false))
                    .col(ColumnDef::new(Filters::IsSystemDefault).boolean().not_null().default(false))
                    .col(ColumnDef::new(Filters::Expression).string().not_null())
                    .col(Self::create_timestamp_column(manager, Filters::CreatedAt))
                    .col(Self::create_timestamp_column(manager, Filters::UpdatedAt))
                    .to_owned(),
            )
            .await
    }

    async fn create_proxy_filters_table(&self, manager: &SchemaManager<'_>) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(ProxyFilters::Table)
                    .if_not_exists()
                    .col(Self::create_uuid_fk_column(manager, ProxyFilters::ProxyId))
                    .col(Self::create_uuid_fk_column(manager, ProxyFilters::FilterId))
                    .col(ColumnDef::new(ProxyFilters::PriorityOrder).integer().not_null().default(0))
                    .col(ColumnDef::new(ProxyFilters::IsActive).boolean().not_null().default(true))
                    .col(Self::create_timestamp_column(manager, ProxyFilters::CreatedAt))
                    .primary_key(Index::create().col(ProxyFilters::ProxyId).col(ProxyFilters::FilterId))
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_proxy_filters_proxy_id")
                            .from(ProxyFilters::Table, ProxyFilters::ProxyId)
                            .to(StreamProxies::Table, StreamProxies::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_proxy_filters_filter_id")
                            .from(ProxyFilters::Table, ProxyFilters::FilterId)
                            .to(Filters::Table, Filters::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn create_channels_table(&self, manager: &SchemaManager<'_>) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Channels::Table)
                    .if_not_exists()
                    .col(Self::create_id_column(manager, Channels::Id))
                    .col(Self::create_uuid_fk_column(manager, Channels::SourceId))
                    .col(ColumnDef::new(Channels::TvgId).string())
                    .col(ColumnDef::new(Channels::TvgName).string())
                    .col(ColumnDef::new(Channels::TvgChno).string())
                    .col(ColumnDef::new(Channels::ChannelName).string().not_null())
                    .col(ColumnDef::new(Channels::TvgLogo).string())
                    .col(ColumnDef::new(Channels::TvgShift).string())
                    .col(ColumnDef::new(Channels::GroupTitle).string())
                    .col(ColumnDef::new(Channels::StreamUrl).string().not_null())
                    .col(Self::create_timestamp_column(manager, Channels::CreatedAt))
                    .col(Self::create_timestamp_column(manager, Channels::UpdatedAt))
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_channels_source_id")
                            .from(Channels::Table, Channels::SourceId)
                            .to(StreamSources::Table, StreamSources::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn create_epg_channels_table(&self, manager: &SchemaManager<'_>) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(EpgChannels::Table)
                    .if_not_exists()
                    .col(Self::create_id_column(manager, EpgChannels::Id))
                    .col(Self::create_uuid_fk_column(manager, EpgChannels::SourceId))
                    .col(ColumnDef::new(EpgChannels::ChannelId).string().not_null())
                    .col(ColumnDef::new(EpgChannels::ChannelName).string().not_null())
                    .col(ColumnDef::new(EpgChannels::ChannelLogo).string())
                    .col(ColumnDef::new(EpgChannels::ChannelGroup).string())
                    .col(ColumnDef::new(EpgChannels::Language).string())
                    .col(Self::create_timestamp_column(manager, EpgChannels::CreatedAt))
                    .col(Self::create_timestamp_column(manager, EpgChannels::UpdatedAt))
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_epg_channels_source_id")
                            .from(EpgChannels::Table, EpgChannels::SourceId)
                            .to(EpgSources::Table, EpgSources::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn create_epg_programs_table(&self, manager: &SchemaManager<'_>) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(EpgPrograms::Table)
                    .if_not_exists()
                    .col(Self::create_id_column(manager, EpgPrograms::Id))
                    .col(Self::create_uuid_fk_column(manager, EpgPrograms::SourceId))
                    .col(ColumnDef::new(EpgPrograms::ChannelId).string().not_null())
                    .col(ColumnDef::new(EpgPrograms::ChannelName).string().not_null())
                    .col(Self::create_timestamp_column(manager, EpgPrograms::StartTime))
                    .col(Self::create_timestamp_column(manager, EpgPrograms::EndTime))
                    .col(ColumnDef::new(EpgPrograms::ProgramTitle).string().not_null())
                    .col(ColumnDef::new(EpgPrograms::ProgramDescription).string())
                    .col(ColumnDef::new(EpgPrograms::ProgramCategory).string())
                    .col(ColumnDef::new(EpgPrograms::EpisodeNum).string())
                    .col(ColumnDef::new(EpgPrograms::SeasonNum).string())
                    .col(ColumnDef::new(EpgPrograms::Rating).string())
                    .col(ColumnDef::new(EpgPrograms::Language).string())
                    .col(ColumnDef::new(EpgPrograms::Subtitles).string())
                    .col(ColumnDef::new(EpgPrograms::AspectRatio).string())
                    .col(ColumnDef::new(EpgPrograms::ProgramIcon).string())
                    .col(Self::create_timestamp_column(manager, EpgPrograms::CreatedAt))
                    .col(Self::create_timestamp_column(manager, EpgPrograms::UpdatedAt))
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_epg_programs_source_id")
                            .from(EpgPrograms::Table, EpgPrograms::SourceId)
                            .to(EpgSources::Table, EpgSources::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn create_channel_epg_mapping_table(&self, manager: &SchemaManager<'_>) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(ChannelEpgMapping::Table)
                    .if_not_exists()
                    .col(Self::create_id_column(manager, ChannelEpgMapping::Id))
                    .col(Self::create_uuid_fk_column(manager, ChannelEpgMapping::StreamChannelId))
                    .col(Self::create_uuid_fk_column(manager, ChannelEpgMapping::EpgChannelId))
                    .col(ColumnDef::new(ChannelEpgMapping::MappingType).string().not_null())
                    .col(Self::create_timestamp_column(manager, ChannelEpgMapping::CreatedAt))
                    .col(Self::create_timestamp_column(manager, ChannelEpgMapping::UpdatedAt))
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_channel_epg_mapping_stream_channel_id")
                            .from(ChannelEpgMapping::Table, ChannelEpgMapping::StreamChannelId)
                            .to(Channels::Table, Channels::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_channel_epg_mapping_epg_channel_id")
                            .from(ChannelEpgMapping::Table, ChannelEpgMapping::EpgChannelId)
                            .to(EpgChannels::Table, EpgChannels::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn create_proxy_generations_table(&self, manager: &SchemaManager<'_>) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(ProxyGenerations::Table)
                    .if_not_exists()
                    .col(Self::create_id_column(manager, ProxyGenerations::Id))
                    .col(Self::create_uuid_fk_column(manager, ProxyGenerations::ProxyId))
                    .col(ColumnDef::new(ProxyGenerations::Version).integer().not_null())
                    .col(ColumnDef::new(ProxyGenerations::M3uContent).string())
                    .col(ColumnDef::new(ProxyGenerations::ChannelCount).integer().not_null().default(0))
                    .col(Self::create_timestamp_column(manager, ProxyGenerations::GeneratedAt))
                    .col(ColumnDef::new(ProxyGenerations::GenerationTimeMs).integer())
                    .col(ColumnDef::new(ProxyGenerations::IsCurrent).boolean().not_null().default(false))
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_proxy_generations_proxy_id")
                            .from(ProxyGenerations::Table, ProxyGenerations::ProxyId)
                            .to(StreamProxies::Table, StreamProxies::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn create_data_mapping_rules_table(&self, manager: &SchemaManager<'_>) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(DataMappingRules::Table)
                    .if_not_exists()
                    .col(Self::create_id_column(manager, DataMappingRules::Id))
                    .col(ColumnDef::new(DataMappingRules::Name).string().not_null())
                    .col(ColumnDef::new(DataMappingRules::Description).string())
                    .col(ColumnDef::new(DataMappingRules::SourceType).string())
                    .col(ColumnDef::new(DataMappingRules::Expression).string())
                    .col(ColumnDef::new(DataMappingRules::SortOrder).integer().not_null().default(0))
                    .col(ColumnDef::new(DataMappingRules::IsActive).boolean().not_null().default(true))
                    .col(Self::create_timestamp_column(manager, DataMappingRules::CreatedAt))
                    .col(Self::create_timestamp_column(manager, DataMappingRules::UpdatedAt))
                    .to_owned(),
            )
            .await
    }

    async fn create_logo_assets_table(&self, manager: &SchemaManager<'_>) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(LogoAssets::Table)
                    .if_not_exists()
                    .col(Self::create_id_column(manager, LogoAssets::Id))
                    .col(ColumnDef::new(LogoAssets::Name).string().not_null())
                    .col(ColumnDef::new(LogoAssets::Description).string())
                    .col(ColumnDef::new(LogoAssets::FileName).string().not_null())
                    .col(ColumnDef::new(LogoAssets::FilePath).string().not_null())
                    .col(ColumnDef::new(LogoAssets::FileSize).integer().not_null())
                    .col(ColumnDef::new(LogoAssets::MimeType).string().not_null())
                    .col(ColumnDef::new(LogoAssets::AssetType).string().not_null())
                    .col(ColumnDef::new(LogoAssets::SourceUrl).string())
                    .col(ColumnDef::new(LogoAssets::Width).integer())
                    .col(ColumnDef::new(LogoAssets::Height).integer())
                    .col(Self::create_nullable_uuid_fk_column(manager, LogoAssets::ParentAssetId))
                    .col(ColumnDef::new(LogoAssets::FormatType).string().not_null().default("original"))
                    .col(Self::create_timestamp_column(manager, LogoAssets::CreatedAt))
                    .col(Self::create_timestamp_column(manager, LogoAssets::UpdatedAt))
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_logo_assets_parent_asset_id")
                            .from(LogoAssets::Table, LogoAssets::ParentAssetId)
                            .to(LogoAssets::Table, LogoAssets::Id)
                            .on_delete(ForeignKeyAction::SetNull),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn create_relay_profiles_table(&self, manager: &SchemaManager<'_>) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(RelayProfiles::Table)
                    .if_not_exists()
                    .col(Self::create_id_column(manager, RelayProfiles::Id))
                    .col(ColumnDef::new(RelayProfiles::Name).string().not_null().unique_key())
                    .col(ColumnDef::new(RelayProfiles::Description).string())
                    .col(ColumnDef::new(RelayProfiles::VideoCodec).string().not_null().default("h264"))
                    .col(ColumnDef::new(RelayProfiles::AudioCodec).string().not_null().default("aac"))
                    .col(ColumnDef::new(RelayProfiles::VideoProfile).string())
                    .col(ColumnDef::new(RelayProfiles::VideoPreset).string())
                    .col(ColumnDef::new(RelayProfiles::VideoBitrate).integer())
                    .col(ColumnDef::new(RelayProfiles::AudioBitrate).integer())
                    .col(ColumnDef::new(RelayProfiles::AudioSampleRate).integer())
                    .col(ColumnDef::new(RelayProfiles::AudioChannels).integer())
                    .col(ColumnDef::new(RelayProfiles::EnableHardwareAcceleration).boolean().not_null().default(false))
                    .col(ColumnDef::new(RelayProfiles::PreferredHwaccel).string())
                    .col(ColumnDef::new(RelayProfiles::ManualArgs).string())
                    .col(ColumnDef::new(RelayProfiles::OutputFormat).string().not_null().default("transport_stream"))
                    .col(ColumnDef::new(RelayProfiles::SegmentDuration).integer())
                    .col(ColumnDef::new(RelayProfiles::MaxSegments).integer())
                    .col(ColumnDef::new(RelayProfiles::InputTimeout).integer().not_null().default(30))
                    .col(ColumnDef::new(RelayProfiles::IsSystemDefault).boolean().not_null().default(false))
                    .col(ColumnDef::new(RelayProfiles::IsActive).boolean().not_null().default(true))
                    .col(Self::create_timestamp_column(manager, RelayProfiles::CreatedAt))
                    .col(Self::create_timestamp_column(manager, RelayProfiles::UpdatedAt))
                    .to_owned(),
            )
            .await
    }

    async fn create_channel_relay_configs_table(&self, manager: &SchemaManager<'_>) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(ChannelRelayConfigs::Table)
                    .if_not_exists()
                    .col(Self::create_id_column(manager, ChannelRelayConfigs::Id))
                    .col(Self::create_uuid_fk_column(manager, ChannelRelayConfigs::ProxyId))
                    .col(ColumnDef::new(ChannelRelayConfigs::ChannelId).string().not_null())
                    .col(Self::create_uuid_fk_column(manager, ChannelRelayConfigs::ProfileId))
                    .col(ColumnDef::new(ChannelRelayConfigs::Name).string().not_null())
                    .col(ColumnDef::new(ChannelRelayConfigs::Description).string())
                    .col(ColumnDef::new(ChannelRelayConfigs::CustomArgs).string())
                    .col(ColumnDef::new(ChannelRelayConfigs::IsActive).boolean().not_null().default(true))
                    .col(Self::create_timestamp_column(manager, ChannelRelayConfigs::CreatedAt))
                    .col(Self::create_timestamp_column(manager, ChannelRelayConfigs::UpdatedAt))
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_channel_relay_configs_proxy_id")
                            .from(ChannelRelayConfigs::Table, ChannelRelayConfigs::ProxyId)
                            .to(StreamProxies::Table, StreamProxies::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_channel_relay_configs_profile_id")
                            .from(ChannelRelayConfigs::Table, ChannelRelayConfigs::ProfileId)
                            .to(RelayProfiles::Table, RelayProfiles::Id),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn create_relay_runtime_status_table(&self, manager: &SchemaManager<'_>) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(RelayRuntimeStatus::Table)
                    .if_not_exists()
                    .col(Self::create_uuid_fk_column(manager, RelayRuntimeStatus::ChannelRelayConfigId).primary_key())
                    .col(ColumnDef::new(RelayRuntimeStatus::ProcessId).string())
                    .col(ColumnDef::new(RelayRuntimeStatus::SandboxPath).string())
                    .col(ColumnDef::new(RelayRuntimeStatus::IsRunning).boolean().not_null().default(false))
                    .col(Self::create_nullable_timestamp_column(manager, RelayRuntimeStatus::StartedAt))
                    .col(ColumnDef::new(RelayRuntimeStatus::ClientCount).integer().not_null().default(0))
                    .col(ColumnDef::new(RelayRuntimeStatus::BytesServed).integer().not_null().default(0))
                    .col(ColumnDef::new(RelayRuntimeStatus::ErrorMessage).string())
                    .col(Self::create_nullable_timestamp_column(manager, RelayRuntimeStatus::LastHeartbeat))
                    .col(Self::create_timestamp_column(manager, RelayRuntimeStatus::UpdatedAt))
                    .to_owned(),
            )
            .await
    }

    async fn create_relay_events_table(&self, manager: &SchemaManager<'_>) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(RelayEvents::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(RelayEvents::Id).integer().not_null().auto_increment().primary_key())
                    .col(Self::create_uuid_fk_column(manager, RelayEvents::ConfigId))
                    .col(ColumnDef::new(RelayEvents::EventType).string().not_null())
                    .col(ColumnDef::new(RelayEvents::Details).string())
                    .col(ColumnDef::new(RelayEvents::Timestamp).string().not_null())
                    .col(Self::create_timestamp_column(manager, RelayEvents::CreatedAt))
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_relay_events_config_id")
                            .from(RelayEvents::Table, RelayEvents::ConfigId)
                            .to(ChannelRelayConfigs::Table, ChannelRelayConfigs::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn create_migration_notes_table(&self, manager: &SchemaManager<'_>) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(MigrationNotes::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(MigrationNotes::Id).integer().not_null().auto_increment().primary_key())
                    .col(ColumnDef::new(MigrationNotes::Version).string().not_null())
                    .col(ColumnDef::new(MigrationNotes::Note).string().not_null())
                    .col(Self::create_timestamp_column(manager, MigrationNotes::CreatedAt))
                    .to_owned(),
            )
            .await
    }

    async fn create_linked_xtream_sources_table(&self, manager: &SchemaManager<'_>) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(LinkedXtreamSources::Table)
                    .if_not_exists()
                    .col(Self::create_id_column(manager, LinkedXtreamSources::Id))
                    .col(ColumnDef::new(LinkedXtreamSources::LinkId).string().not_null().unique_key())
                    .col(ColumnDef::new(LinkedXtreamSources::Name).string().not_null())
                    .col(ColumnDef::new(LinkedXtreamSources::Url).string().not_null())
                    .col(ColumnDef::new(LinkedXtreamSources::Username).string().not_null())
                    .col(ColumnDef::new(LinkedXtreamSources::Password).string().not_null())
                    .col(Self::create_nullable_uuid_fk_column(manager, LinkedXtreamSources::StreamSourceId))
                    .col(Self::create_nullable_uuid_fk_column(manager, LinkedXtreamSources::EpgSourceId))
                    .col(Self::create_timestamp_column(manager, LinkedXtreamSources::CreatedAt))
                    .col(Self::create_timestamp_column(manager, LinkedXtreamSources::UpdatedAt))
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_linked_xtream_sources_stream_source_id")
                            .from(LinkedXtreamSources::Table, LinkedXtreamSources::StreamSourceId)
                            .to(StreamSources::Table, StreamSources::Id)
                            .on_delete(ForeignKeyAction::SetNull),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_linked_xtream_sources_epg_source_id")
                            .from(LinkedXtreamSources::Table, LinkedXtreamSources::EpgSourceId)
                            .to(EpgSources::Table, EpgSources::Id)
                            .on_delete(ForeignKeyAction::SetNull),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn create_indexes(&self, manager: &SchemaManager<'_>) -> Result<(), DbErr> {
        // Stream Sources indexes
        manager.create_index(Index::create().name("idx_stream_sources_active").table(StreamSources::Table).col(StreamSources::IsActive).to_owned()).await?;
        manager.create_index(Index::create().name("idx_stream_sources_type").table(StreamSources::Table).col(StreamSources::SourceType).to_owned()).await?;
        manager.create_index(Index::create().name("idx_stream_sources_last_ingested").table(StreamSources::Table).col(StreamSources::LastIngestedAt).to_owned()).await?;

        // EPG Sources indexes
        manager.create_index(Index::create().name("idx_epg_sources_active").table(EpgSources::Table).col(EpgSources::IsActive).to_owned()).await?;
        manager.create_index(Index::create().name("idx_epg_sources_type").table(EpgSources::Table).col(EpgSources::SourceType).to_owned()).await?;
        manager.create_index(Index::create().name("idx_epg_sources_last_ingested").table(EpgSources::Table).col(EpgSources::LastIngestedAt).to_owned()).await?;

        // Stream Proxies indexes
        manager.create_index(Index::create().name("idx_stream_proxies_active").table(StreamProxies::Table).col(StreamProxies::IsActive).to_owned()).await?;
        manager.create_index(Index::create().name("idx_stream_proxies_proxy_mode").table(StreamProxies::Table).col(StreamProxies::ProxyMode).to_owned()).await?;
        manager.create_index(Index::create().name("idx_stream_proxies_auto_regenerate").table(StreamProxies::Table).col(StreamProxies::AutoRegenerate).to_owned()).await?;
        manager.create_index(Index::create().name("idx_stream_proxies_last_generated").table(StreamProxies::Table).col(StreamProxies::LastGeneratedAt).to_owned()).await?;
        manager.create_index(Index::create().name("idx_stream_proxies_relay_profile_id").table(StreamProxies::Table).col(StreamProxies::RelayProfileId).to_owned()).await?;

        // Proxy Sources indexes
        manager.create_index(Index::create().name("idx_proxy_sources_source_id").table(ProxySources::Table).col(ProxySources::SourceId).to_owned()).await?;
        manager.create_index(Index::create().name("idx_proxy_sources_priority").table(ProxySources::Table).col(ProxySources::ProxyId).col(ProxySources::PriorityOrder).to_owned()).await?;

        // Proxy EPG Sources indexes
        manager.create_index(Index::create().name("idx_proxy_epg_sources_epg_source_id").table(ProxyEpgSources::Table).col(ProxyEpgSources::EpgSourceId).to_owned()).await?;
        manager.create_index(Index::create().name("idx_proxy_epg_sources_priority").table(ProxyEpgSources::Table).col(ProxyEpgSources::ProxyId).col(ProxyEpgSources::PriorityOrder).to_owned()).await?;

        // Filters indexes
        manager.create_index(Index::create().name("idx_filters_source_type").table(Filters::Table).col(Filters::SourceType).to_owned()).await?;

        // Proxy Filters indexes
        manager.create_index(Index::create().name("idx_proxy_filters_filter_id").table(ProxyFilters::Table).col(ProxyFilters::FilterId).to_owned()).await?;
        manager.create_index(Index::create().name("idx_proxy_filters_active").table(ProxyFilters::Table).col(ProxyFilters::IsActive).to_owned()).await?;
        manager.create_index(Index::create().name("idx_proxy_filters_priority").table(ProxyFilters::Table).col(ProxyFilters::ProxyId).col(ProxyFilters::PriorityOrder).to_owned()).await?;

        // Channels indexes
        manager.create_index(Index::create().name("idx_channels_source_id").table(Channels::Table).col(Channels::SourceId).to_owned()).await?;
        manager.create_index(Index::create().name("idx_channels_tvg_id").table(Channels::Table).col(Channels::TvgId).to_owned()).await?;
        manager.create_index(Index::create().name("idx_channels_tvg_name").table(Channels::Table).col(Channels::TvgName).to_owned()).await?;
        manager.create_index(Index::create().name("idx_channels_channel_name").table(Channels::Table).col(Channels::ChannelName).to_owned()).await?;
        manager.create_index(Index::create().name("idx_channels_group_title").table(Channels::Table).col(Channels::GroupTitle).to_owned()).await?;

        // EPG Channels indexes
        manager.create_index(Index::create().name("idx_epg_channels_source_id").table(EpgChannels::Table).col(EpgChannels::SourceId).to_owned()).await?;
        manager.create_index(Index::create().name("idx_epg_channels_channel_id").table(EpgChannels::Table).col(EpgChannels::ChannelId).to_owned()).await?;
        manager.create_index(Index::create().name("idx_epg_channels_channel_name").table(EpgChannels::Table).col(EpgChannels::ChannelName).to_owned()).await?;

        // EPG Programs indexes
        manager.create_index(Index::create().name("idx_epg_programs_source_id").table(EpgPrograms::Table).col(EpgPrograms::SourceId).to_owned()).await?;
        manager.create_index(Index::create().name("idx_epg_programs_channel_id").table(EpgPrograms::Table).col(EpgPrograms::ChannelId).to_owned()).await?;
        manager.create_index(Index::create().name("idx_epg_programs_start_time").table(EpgPrograms::Table).col(EpgPrograms::StartTime).to_owned()).await?;
        manager.create_index(Index::create().name("idx_epg_programs_end_time").table(EpgPrograms::Table).col(EpgPrograms::EndTime).to_owned()).await?;
        manager.create_index(Index::create().name("idx_epg_programs_program_title").table(EpgPrograms::Table).col(EpgPrograms::ProgramTitle).to_owned()).await?;

        // Channel EPG Mapping indexes
        manager.create_index(Index::create().name("idx_channel_epg_mapping_stream_channel_id").table(ChannelEpgMapping::Table).col(ChannelEpgMapping::StreamChannelId).to_owned()).await?;
        manager.create_index(Index::create().name("idx_channel_epg_mapping_epg_channel_id").table(ChannelEpgMapping::Table).col(ChannelEpgMapping::EpgChannelId).to_owned()).await?;
        manager.create_index(Index::create().name("idx_channel_epg_mapping_mapping_type").table(ChannelEpgMapping::Table).col(ChannelEpgMapping::MappingType).to_owned()).await?;

        // Proxy Generations indexes
        manager.create_index(Index::create().name("idx_proxy_generations_proxy_id").table(ProxyGenerations::Table).col(ProxyGenerations::ProxyId).to_owned()).await?;
        manager.create_index(Index::create().name("idx_proxy_generations_current").table(ProxyGenerations::Table).col(ProxyGenerations::IsCurrent).to_owned()).await?;
        manager.create_index(Index::create().name("idx_proxy_generations_generated_at").table(ProxyGenerations::Table).col(ProxyGenerations::GeneratedAt).to_owned()).await?;

        // Data Mapping Rules indexes
        manager.create_index(Index::create().name("idx_data_mapping_rules_active").table(DataMappingRules::Table).col(DataMappingRules::IsActive).to_owned()).await?;
        manager.create_index(Index::create().name("idx_data_mapping_rules_source_type").table(DataMappingRules::Table).col(DataMappingRules::SourceType).to_owned()).await?;
        manager.create_index(Index::create().name("idx_data_mapping_rules_sort_order").table(DataMappingRules::Table).col(DataMappingRules::SortOrder).col(DataMappingRules::IsActive).to_owned()).await?;

        // Logo Assets indexes
        manager.create_index(Index::create().name("idx_logo_assets_asset_type").table(LogoAssets::Table).col(LogoAssets::AssetType).to_owned()).await?;
        manager.create_index(Index::create().name("idx_logo_assets_format_type").table(LogoAssets::Table).col(LogoAssets::FormatType).to_owned()).await?;
        manager.create_index(Index::create().name("idx_logo_assets_parent_asset_id").table(LogoAssets::Table).col(LogoAssets::ParentAssetId).to_owned()).await?;

        // Relay System indexes
        manager.create_index(Index::create().name("idx_channel_relay_configs_proxy_id").table(ChannelRelayConfigs::Table).col(ChannelRelayConfigs::ProxyId).to_owned()).await?;
        manager.create_index(Index::create().name("idx_channel_relay_configs_channel_id").table(ChannelRelayConfigs::Table).col(ChannelRelayConfigs::ChannelId).to_owned()).await?;
        manager.create_index(Index::create().name("idx_channel_relay_configs_profile_id").table(ChannelRelayConfigs::Table).col(ChannelRelayConfigs::ProfileId).to_owned()).await?;
        manager.create_index(Index::create().name("idx_relay_runtime_status_running").table(RelayRuntimeStatus::Table).col(RelayRuntimeStatus::IsRunning).to_owned()).await?;
        manager.create_index(Index::create().name("idx_relay_events_config_id").table(RelayEvents::Table).col(RelayEvents::ConfigId).to_owned()).await?;
        manager.create_index(Index::create().name("idx_relay_events_timestamp").table(RelayEvents::Table).col(RelayEvents::Timestamp).to_owned()).await?;
        manager.create_index(Index::create().name("idx_relay_events_event_type").table(RelayEvents::Table).col(RelayEvents::EventType).to_owned()).await?;

        // Linked Xtream Sources indexes
        manager.create_index(Index::create().name("idx_linked_xtream_sources_link_id").table(LinkedXtreamSources::Table).col(LinkedXtreamSources::LinkId).to_owned()).await?;
        manager.create_index(Index::create().name("idx_linked_xtream_sources_stream_source_id").table(LinkedXtreamSources::Table).col(LinkedXtreamSources::StreamSourceId).to_owned()).await?;
        manager.create_index(Index::create().name("idx_linked_xtream_sources_epg_source_id").table(LinkedXtreamSources::Table).col(LinkedXtreamSources::EpgSourceId).to_owned()).await?;

        Ok(())
    }
}

// Entity identifiers for migration
#[derive(DeriveIden)]
pub enum StreamSources {
    Table,
    Id,
    Name,
    SourceType,
    Url,
    MaxConcurrentStreams,
    UpdateCron,
    Username,
    Password,
    FieldMap,
    IgnoreChannelNumbers,
    CreatedAt,
    UpdatedAt,
    LastIngestedAt,
    IsActive,
}

#[derive(DeriveIden)]
pub enum EpgSources {
    Table,
    Id,
    Name,
    SourceType,
    Url,
    UpdateCron,
    Username,
    Password,
    OriginalTimezone,
    TimeOffset,
    CreatedAt,
    UpdatedAt,
    LastIngestedAt,
    IsActive,
}

#[derive(DeriveIden)]
pub enum StreamProxies {
    Table,
    Id,
    Name,
    Description,
    ProxyMode,
    UpstreamTimeout,
    BufferSize,
    MaxConcurrentStreams,
    StartingChannelNumber,
    CreatedAt,
    UpdatedAt,
    LastGeneratedAt,
    IsActive,
    AutoRegenerate,
    CacheChannelLogos,
    CacheProgramLogos,
    RelayProfileId,
}

#[derive(DeriveIden)]
pub enum ProxySources {
    Table,
    ProxyId,
    SourceId,
    PriorityOrder,
    CreatedAt,
}

#[derive(DeriveIden)]
pub enum ProxyEpgSources {
    Table,
    ProxyId,
    EpgSourceId,
    PriorityOrder,
    CreatedAt,
}

#[derive(DeriveIden)]
pub enum Filters {
    Table,
    Id,
    Name,
    SourceType,
    IsInverse,
    IsSystemDefault,
    Expression,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
pub enum ProxyFilters {
    Table,
    ProxyId,
    FilterId,
    PriorityOrder,
    IsActive,
    CreatedAt,
}

#[derive(DeriveIden)]
pub enum Channels {
    Table,
    Id,
    SourceId,
    TvgId,
    TvgName,
    TvgChno,
    ChannelName,
    TvgLogo,
    TvgShift,
    GroupTitle,
    StreamUrl,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
pub enum EpgChannels {
    Table,
    Id,
    SourceId,
    ChannelId,
    ChannelName,
    ChannelLogo,
    ChannelGroup,
    Language,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
pub enum EpgPrograms {
    Table,
    Id,
    SourceId,
    ChannelId,
    ChannelName,
    StartTime,
    EndTime,
    ProgramTitle,
    ProgramDescription,
    ProgramCategory,
    EpisodeNum,
    SeasonNum,
    Rating,
    Language,
    Subtitles,
    AspectRatio,
    ProgramIcon,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
pub enum ChannelEpgMapping {
    Table,
    Id,
    StreamChannelId,
    EpgChannelId,
    MappingType,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
pub enum ProxyGenerations {
    Table,
    Id,
    ProxyId,
    Version,
    M3uContent,
    ChannelCount,
    GeneratedAt,
    GenerationTimeMs,
    IsCurrent,
}

#[derive(DeriveIden)]
pub enum DataMappingRules {
    Table,
    Id,
    Name,
    Description,
    SourceType,
    Expression,
    SortOrder,
    IsActive,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
pub enum LogoAssets {
    Table,
    Id,
    Name,
    Description,
    FileName,
    FilePath,
    FileSize,
    MimeType,
    AssetType,
    SourceUrl,
    Width,
    Height,
    ParentAssetId,
    FormatType,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
pub enum RelayProfiles {
    Table,
    Id,
    Name,
    Description,
    VideoCodec,
    AudioCodec,
    VideoProfile,
    VideoPreset,
    VideoBitrate,
    AudioBitrate,
    AudioSampleRate,
    AudioChannels,
    EnableHardwareAcceleration,
    PreferredHwaccel,
    ManualArgs,
    OutputFormat,
    SegmentDuration,
    MaxSegments,
    InputTimeout,
    IsSystemDefault,
    IsActive,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
pub enum ChannelRelayConfigs {
    Table,
    Id,
    ProxyId,
    ChannelId,
    ProfileId,
    Name,
    Description,
    CustomArgs,
    IsActive,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
pub enum RelayRuntimeStatus {
    Table,
    ChannelRelayConfigId,
    ProcessId,
    SandboxPath,
    IsRunning,
    StartedAt,
    ClientCount,
    BytesServed,
    ErrorMessage,
    LastHeartbeat,
    UpdatedAt,
}

#[derive(DeriveIden)]
pub enum RelayEvents {
    Table,
    Id,
    ConfigId,
    EventType,
    Details,
    Timestamp,
    CreatedAt,
}

#[derive(DeriveIden)]
pub enum MigrationNotes {
    Table,
    Id,
    Version,
    Note,
    CreatedAt,
}

#[derive(DeriveIden)]
pub enum LinkedXtreamSources {
    Table,
    Id,
    LinkId,
    Name,
    Url,
    Username,
    Password,
    StreamSourceId,
    EpgSourceId,
    CreatedAt,
    UpdatedAt,
}