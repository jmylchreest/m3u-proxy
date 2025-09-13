use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Create tables in order of dependencies
        self.create_stream_sources_table(manager).await?;
        self.create_channels_table(manager).await?;
        self.create_epg_sources_table(manager).await?;
        self.create_epg_programs_table(manager).await?;
        self.create_filters_table(manager).await?;
        self.create_data_mapping_rules_table(manager).await?;
        self.create_relay_profiles_table(manager).await?;
        self.create_stream_proxies_table(manager).await?;
        self.create_proxy_sources_table(manager).await?;
        self.create_proxy_filters_table(manager).await?;
        self.create_proxy_epg_sources_table(manager).await?;
        self.create_logo_assets_table(manager).await?;
        self.create_last_known_codecs_table(manager).await?;
        self.create_migration_notes_table(manager).await?;

        // Create indexes
        self.create_indexes(manager).await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Drop tables in reverse order
        manager
            .drop_table(Table::drop().table(MigrationNotes::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(LastKnownCodecs::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(LogoAssets::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(ProxyEpgSources::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(ProxyFilters::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(ProxySources::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(StreamProxies::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(RelayProfiles::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(DataMappingRules::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(Filters::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(EpgPrograms::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(EpgSources::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(Channels::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(StreamSources::Table).to_owned())
            .await?;

        Ok(())
    }
}

impl Migration {
    // Helper functions for database-specific types
    fn create_id_column(&self, manager: &SchemaManager, column: impl IntoIden) -> ColumnDef {
        let mut col = ColumnDef::new(column);
        match manager.get_database_backend() {
            sea_orm::DatabaseBackend::Postgres => col.uuid().not_null(),
            _ => col.string().not_null(),
        };
        col
    }

    fn create_uuid_fk_column(&self, manager: &SchemaManager, column: impl IntoIden) -> ColumnDef {
        let mut col = ColumnDef::new(column);
        match manager.get_database_backend() {
            sea_orm::DatabaseBackend::Postgres => col.uuid().not_null(),
            _ => col.string().not_null(),
        };
        col
    }

    fn create_nullable_uuid_fk_column(
        &self,
        manager: &SchemaManager,
        column: impl IntoIden,
    ) -> ColumnDef {
        let mut col = ColumnDef::new(column);
        match manager.get_database_backend() {
            sea_orm::DatabaseBackend::Postgres => col.uuid(),
            _ => col.string(),
        };
        col
    }

    fn create_timestamp_column(&self, manager: &SchemaManager, column: impl IntoIden) -> ColumnDef {
        let mut col = ColumnDef::new(column);
        match manager.get_database_backend() {
            sea_orm::DatabaseBackend::Postgres => col.timestamp_with_time_zone().not_null(),
            _ => col.string().not_null(),
        };
        col
    }

    fn create_nullable_timestamp_column(
        &self,
        manager: &SchemaManager,
        column: impl IntoIden,
    ) -> ColumnDef {
        let mut col = ColumnDef::new(column);
        match manager.get_database_backend() {
            sea_orm::DatabaseBackend::Postgres => col.timestamp_with_time_zone(),
            _ => col.string(),
        };
        col
    }

    // Table creation methods
    async fn create_stream_sources_table(&self, manager: &SchemaManager<'_>) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(StreamSources::Table)
                    .if_not_exists()
                    .col(
                        self.create_id_column(manager, StreamSources::Id)
                            .primary_key(),
                    )
                    .col(ColumnDef::new(StreamSources::Name).string().not_null())
                    .col(ColumnDef::new(StreamSources::Url).string().not_null())
                    .col(
                        ColumnDef::new(StreamSources::SourceType)
                            .string()
                            .not_null(),
                    )
                    .col(ColumnDef::new(StreamSources::MaxConcurrentStreams).integer())
                    .col(
                        ColumnDef::new(StreamSources::UpdateCron)
                            .string()
                            .not_null(),
                    )
                    .col(ColumnDef::new(StreamSources::Username).string())
                    .col(ColumnDef::new(StreamSources::Password).string())
                    .col(ColumnDef::new(StreamSources::FieldMap).string())
                    .col(
                        ColumnDef::new(StreamSources::IgnoreChannelNumbers)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .col(self.create_timestamp_column(manager, StreamSources::CreatedAt))
                    .col(self.create_timestamp_column(manager, StreamSources::UpdatedAt))
                    .col(
                        self.create_nullable_timestamp_column(
                            manager,
                            StreamSources::LastIngestedAt,
                        ),
                    )
                    .col(
                        ColumnDef::new(StreamSources::IsActive)
                            .boolean()
                            .not_null()
                            .default(true),
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
                    .col(self.create_id_column(manager, Channels::Id).primary_key())
                    .col(self.create_uuid_fk_column(manager, Channels::SourceId))
                    .col(ColumnDef::new(Channels::TvgId).string())
                    .col(ColumnDef::new(Channels::TvgName).string())
                    .col(ColumnDef::new(Channels::TvgChno).string())
                    .col(ColumnDef::new(Channels::ChannelName).string().not_null())
                    .col(ColumnDef::new(Channels::TvgLogo).string())
                    .col(ColumnDef::new(Channels::TvgShift).string())
                    .col(ColumnDef::new(Channels::GroupTitle).string())
                    .col(ColumnDef::new(Channels::StreamUrl).string().not_null())
                    .col(self.create_timestamp_column(manager, Channels::CreatedAt))
                    .col(self.create_timestamp_column(manager, Channels::UpdatedAt))
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_channels_source_id")
                            .from(Channels::Table, Channels::SourceId)
                            .to(StreamSources::Table, StreamSources::Id)
                            .on_delete(ForeignKeyAction::Cascade)
                            .on_update(ForeignKeyAction::NoAction),
                    )
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
                    .col(self.create_id_column(manager, EpgSources::Id).primary_key())
                    .col(ColumnDef::new(EpgSources::Name).string().not_null())
                    .col(ColumnDef::new(EpgSources::SourceType).string().not_null())
                    .col(ColumnDef::new(EpgSources::Url).string().not_null())
                    .col(ColumnDef::new(EpgSources::UpdateCron).string().not_null())
                    .col(ColumnDef::new(EpgSources::Username).string())
                    .col(ColumnDef::new(EpgSources::Password).string())
                    .col(ColumnDef::new(EpgSources::OriginalTimezone).string())
                    .col(ColumnDef::new(EpgSources::TimeOffset).string())
                    .col(self.create_timestamp_column(manager, EpgSources::CreatedAt))
                    .col(self.create_timestamp_column(manager, EpgSources::UpdatedAt))
                    .col(self.create_nullable_timestamp_column(manager, EpgSources::LastIngestedAt))
                    .col(
                        ColumnDef::new(EpgSources::IsActive)
                            .boolean()
                            .not_null()
                            .default(true),
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
                    .col(
                        self.create_id_column(manager, EpgPrograms::Id)
                            .primary_key(),
                    )
                    .col(self.create_uuid_fk_column(manager, EpgPrograms::SourceId))
                    .col(ColumnDef::new(EpgPrograms::ChannelId).string().not_null())
                    .col(ColumnDef::new(EpgPrograms::ChannelName).string().not_null())
                    .col(self.create_timestamp_column(manager, EpgPrograms::StartTime))
                    .col(self.create_timestamp_column(manager, EpgPrograms::EndTime))
                    .col(
                        ColumnDef::new(EpgPrograms::ProgramTitle)
                            .string()
                            .not_null(),
                    )
                    .col(ColumnDef::new(EpgPrograms::ProgramDescription).string())
                    .col(ColumnDef::new(EpgPrograms::ProgramCategory).string())
                    .col(ColumnDef::new(EpgPrograms::EpisodeNum).string())
                    .col(ColumnDef::new(EpgPrograms::SeasonNum).string())
                    .col(ColumnDef::new(EpgPrograms::Rating).string())
                    .col(ColumnDef::new(EpgPrograms::Language).string())
                    .col(ColumnDef::new(EpgPrograms::Subtitles).string())
                    .col(ColumnDef::new(EpgPrograms::AspectRatio).string())
                    .col(ColumnDef::new(EpgPrograms::ProgramIcon).string())
                    .col(self.create_timestamp_column(manager, EpgPrograms::CreatedAt))
                    .col(self.create_timestamp_column(manager, EpgPrograms::UpdatedAt))
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_epg_programs_source_id")
                            .from(EpgPrograms::Table, EpgPrograms::SourceId)
                            .to(EpgSources::Table, EpgSources::Id)
                            .on_delete(ForeignKeyAction::Cascade)
                            .on_update(ForeignKeyAction::NoAction),
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
                    .col(self.create_id_column(manager, Filters::Id).primary_key())
                    .col(
                        ColumnDef::new(Filters::Name)
                            .string()
                            .not_null()
                            .unique_key(),
                    )
                    .col(ColumnDef::new(Filters::SourceType).string().not_null())
                    .col(
                        ColumnDef::new(Filters::IsInverse)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .col(
                        ColumnDef::new(Filters::IsSystemDefault)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .col(ColumnDef::new(Filters::Expression).string().not_null())
                    .col(self.create_timestamp_column(manager, Filters::CreatedAt))
                    .col(self.create_timestamp_column(manager, Filters::UpdatedAt))
                    .to_owned(),
            )
            .await
    }

    async fn create_data_mapping_rules_table(
        &self,
        manager: &SchemaManager<'_>,
    ) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(DataMappingRules::Table)
                    .if_not_exists()
                    .col(
                        self.create_id_column(manager, DataMappingRules::Id)
                            .primary_key(),
                    )
                    .col(ColumnDef::new(DataMappingRules::Name).string().not_null())
                    .col(ColumnDef::new(DataMappingRules::Description).string())
                    .col(ColumnDef::new(DataMappingRules::SourceType).string())
                    .col(ColumnDef::new(DataMappingRules::Expression).string())
                    .col(
                        ColumnDef::new(DataMappingRules::SortOrder)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(DataMappingRules::IsActive)
                            .boolean()
                            .not_null()
                            .default(true),
                    )
                    .col(self.create_timestamp_column(manager, DataMappingRules::CreatedAt))
                    .col(self.create_timestamp_column(manager, DataMappingRules::UpdatedAt))
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
                    .col(
                        self.create_id_column(manager, RelayProfiles::Id)
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(RelayProfiles::Name)
                            .string()
                            .not_null()
                            .unique_key(),
                    )
                    .col(ColumnDef::new(RelayProfiles::Description).string())
                    .col(
                        ColumnDef::new(RelayProfiles::VideoCodec)
                            .string()
                            .not_null()
                            .default("h264"),
                    )
                    .col(
                        ColumnDef::new(RelayProfiles::AudioCodec)
                            .string()
                            .not_null()
                            .default("aac"),
                    )
                    .col(ColumnDef::new(RelayProfiles::VideoProfile).string())
                    .col(ColumnDef::new(RelayProfiles::VideoPreset).string())
                    .col(ColumnDef::new(RelayProfiles::VideoBitrate).integer())
                    .col(ColumnDef::new(RelayProfiles::AudioBitrate).integer())
                    .col(ColumnDef::new(RelayProfiles::AudioSampleRate).integer())
                    .col(ColumnDef::new(RelayProfiles::AudioChannels).integer())
                    .col(
                        ColumnDef::new(RelayProfiles::EnableHardwareAcceleration)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .col(ColumnDef::new(RelayProfiles::PreferredHwaccel).string())
                    .col(ColumnDef::new(RelayProfiles::ManualArgs).string())
                    .col(
                        ColumnDef::new(RelayProfiles::OutputFormat)
                            .string()
                            .not_null()
                            .default("transport_stream"),
                    )
                    .col(ColumnDef::new(RelayProfiles::SegmentDuration).integer())
                    .col(ColumnDef::new(RelayProfiles::MaxSegments).integer())
                    .col(
                        ColumnDef::new(RelayProfiles::InputTimeout)
                            .integer()
                            .not_null()
                            .default(30),
                    )
                    .col(
                        ColumnDef::new(RelayProfiles::IsSystemDefault)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .col(
                        ColumnDef::new(RelayProfiles::IsActive)
                            .boolean()
                            .not_null()
                            .default(true),
                    )
                    .col(self.create_timestamp_column(manager, RelayProfiles::CreatedAt))
                    .col(self.create_timestamp_column(manager, RelayProfiles::UpdatedAt))
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
                    .col(
                        self.create_id_column(manager, StreamProxies::Id)
                            .primary_key(),
                    )
                    .col(ColumnDef::new(StreamProxies::Name).string().not_null())
                    .col(ColumnDef::new(StreamProxies::Description).string())
                    .col(
                        ColumnDef::new(StreamProxies::ProxyMode)
                            .string()
                            .not_null()
                            .default("redirect"),
                    )
                    .col(
                        ColumnDef::new(StreamProxies::UpstreamTimeout)
                            .integer()
                            .default(30),
                    )
                    .col(
                        ColumnDef::new(StreamProxies::BufferSize)
                            .integer()
                            .default(8192),
                    )
                    .col(
                        ColumnDef::new(StreamProxies::MaxConcurrentStreams)
                            .integer()
                            .default(1),
                    )
                    .col(
                        ColumnDef::new(StreamProxies::StartingChannelNumber)
                            .integer()
                            .not_null()
                            .default(1),
                    )
                    .col(self.create_timestamp_column(manager, StreamProxies::CreatedAt))
                    .col(self.create_timestamp_column(manager, StreamProxies::UpdatedAt))
                    .col(
                        self.create_nullable_timestamp_column(
                            manager,
                            StreamProxies::LastGeneratedAt,
                        ),
                    )
                    .col(
                        ColumnDef::new(StreamProxies::IsActive)
                            .boolean()
                            .not_null()
                            .default(true),
                    )
                    .col(
                        ColumnDef::new(StreamProxies::AutoRegenerate)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .col(
                        ColumnDef::new(StreamProxies::CacheChannelLogos)
                            .boolean()
                            .not_null()
                            .default(true),
                    )
                    .col(
                        ColumnDef::new(StreamProxies::CacheProgramLogos)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .col(
                        self.create_nullable_uuid_fk_column(manager, StreamProxies::RelayProfileId),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_stream_proxies_relay_profile_id")
                            .from(StreamProxies::Table, StreamProxies::RelayProfileId)
                            .to(RelayProfiles::Table, RelayProfiles::Id)
                            .on_delete(ForeignKeyAction::SetNull)
                            .on_update(ForeignKeyAction::NoAction),
                    )
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
                    .col(self.create_uuid_fk_column(manager, ProxySources::ProxyId))
                    .col(self.create_uuid_fk_column(manager, ProxySources::SourceId))
                    .col(
                        ColumnDef::new(ProxySources::PriorityOrder)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(self.create_timestamp_column(manager, ProxySources::CreatedAt))
                    .primary_key(
                        Index::create()
                            .col(ProxySources::ProxyId)
                            .col(ProxySources::SourceId),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_proxy_sources_proxy_id")
                            .from(ProxySources::Table, ProxySources::ProxyId)
                            .to(StreamProxies::Table, StreamProxies::Id)
                            .on_delete(ForeignKeyAction::Cascade)
                            .on_update(ForeignKeyAction::NoAction),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_proxy_sources_source_id")
                            .from(ProxySources::Table, ProxySources::SourceId)
                            .to(StreamSources::Table, StreamSources::Id)
                            .on_delete(ForeignKeyAction::Cascade)
                            .on_update(ForeignKeyAction::NoAction),
                    )
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
                    .col(self.create_uuid_fk_column(manager, ProxyFilters::ProxyId))
                    .col(self.create_uuid_fk_column(manager, ProxyFilters::FilterId))
                    .col(
                        ColumnDef::new(ProxyFilters::PriorityOrder)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(ProxyFilters::IsActive)
                            .boolean()
                            .not_null()
                            .default(true),
                    )
                    .col(self.create_timestamp_column(manager, ProxyFilters::CreatedAt))
                    .primary_key(
                        Index::create()
                            .col(ProxyFilters::ProxyId)
                            .col(ProxyFilters::FilterId),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_proxy_filters_proxy_id")
                            .from(ProxyFilters::Table, ProxyFilters::ProxyId)
                            .to(StreamProxies::Table, StreamProxies::Id)
                            .on_delete(ForeignKeyAction::Cascade)
                            .on_update(ForeignKeyAction::NoAction),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_proxy_filters_filter_id")
                            .from(ProxyFilters::Table, ProxyFilters::FilterId)
                            .to(Filters::Table, Filters::Id)
                            .on_delete(ForeignKeyAction::Cascade)
                            .on_update(ForeignKeyAction::NoAction),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn create_proxy_epg_sources_table(
        &self,
        manager: &SchemaManager<'_>,
    ) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(ProxyEpgSources::Table)
                    .if_not_exists()
                    .col(self.create_uuid_fk_column(manager, ProxyEpgSources::ProxyId))
                    .col(self.create_uuid_fk_column(manager, ProxyEpgSources::EpgSourceId))
                    .col(
                        ColumnDef::new(ProxyEpgSources::PriorityOrder)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(self.create_timestamp_column(manager, ProxyEpgSources::CreatedAt))
                    .primary_key(
                        Index::create()
                            .col(ProxyEpgSources::ProxyId)
                            .col(ProxyEpgSources::EpgSourceId),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_proxy_epg_sources_proxy_id")
                            .from(ProxyEpgSources::Table, ProxyEpgSources::ProxyId)
                            .to(StreamProxies::Table, StreamProxies::Id)
                            .on_delete(ForeignKeyAction::Cascade)
                            .on_update(ForeignKeyAction::NoAction),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_proxy_epg_sources_source_id")
                            .from(ProxyEpgSources::Table, ProxyEpgSources::EpgSourceId)
                            .to(EpgSources::Table, EpgSources::Id)
                            .on_delete(ForeignKeyAction::Cascade)
                            .on_update(ForeignKeyAction::NoAction),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn create_logo_assets_table(&self, manager: &SchemaManager<'_>) -> Result<(), DbErr> {
        let mut table_create = Table::create()
            .table(LogoAssets::Table)
            .if_not_exists()
            .col(self.create_id_column(manager, LogoAssets::Id).primary_key())
            .col(ColumnDef::new(LogoAssets::Name).string().not_null())
            .col(ColumnDef::new(LogoAssets::Description).string())
            .col(
                ColumnDef::new(LogoAssets::FileName)
                    .string()
                    .not_null()
                    .default(""),
            )
            .col(ColumnDef::new(LogoAssets::FilePath).string().not_null())
            .col(
                ColumnDef::new(LogoAssets::FileHash)
                    .string()
                    .not_null()
                    .default(""),
            )
            .col(ColumnDef::new(LogoAssets::FileSize).integer().not_null())
            .col(ColumnDef::new(LogoAssets::MimeType).string().not_null())
            .col(
                ColumnDef::new(LogoAssets::AssetType)
                    .string()
                    .not_null()
                    .default("original"),
            )
            .col(ColumnDef::new(LogoAssets::SourceUrl).string())
            .col(ColumnDef::new(LogoAssets::Width).integer())
            .col(ColumnDef::new(LogoAssets::Height).integer())
            .col(ColumnDef::new(LogoAssets::ParentAssetId).uuid())
            .col(
                ColumnDef::new(LogoAssets::FormatType)
                    .string()
                    .not_null()
                    .default("original"),
            )
            .col(
                ColumnDef::new(LogoAssets::IsSystem)
                    .boolean()
                    .not_null()
                    .default(false),
            )
            .col(self.create_timestamp_column(manager, LogoAssets::CreatedAt))
            .col(self.create_timestamp_column(manager, LogoAssets::UpdatedAt))
            .to_owned();

        // For SQLite, add foreign key constraint during table creation
        if matches!(
            manager.get_database_backend(),
            sea_orm::DatabaseBackend::Sqlite
        ) {
            table_create = table_create
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_logo_assets_parent")
                        .from(LogoAssets::Table, LogoAssets::ParentAssetId)
                        .to(LogoAssets::Table, LogoAssets::Id)
                        .on_delete(ForeignKeyAction::SetNull)
                        .on_update(ForeignKeyAction::NoAction),
                )
                .to_owned();
        }

        manager.create_table(table_create).await?;

        // For PostgreSQL/MySQL, add foreign key constraint after table creation
        if !matches!(
            manager.get_database_backend(),
            sea_orm::DatabaseBackend::Sqlite
        ) {
            manager
                .create_foreign_key(
                    ForeignKey::create()
                        .name("fk_logo_assets_parent")
                        .from(LogoAssets::Table, LogoAssets::ParentAssetId)
                        .to(LogoAssets::Table, LogoAssets::Id)
                        .on_delete(ForeignKeyAction::SetNull)
                        .on_update(ForeignKeyAction::NoAction)
                        .to_owned(),
                )
                .await?;
        }

        Ok(())
    }

    async fn create_last_known_codecs_table(
        &self,
        manager: &SchemaManager<'_>,
    ) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(LastKnownCodecs::Table)
                    .if_not_exists()
                    .col(
                        self.create_id_column(manager, LastKnownCodecs::Id)
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(LastKnownCodecs::StreamUrl)
                            .string()
                            .not_null()
                            .unique_key(),
                    )
                    .col(ColumnDef::new(LastKnownCodecs::VideoCodec).string())
                    .col(ColumnDef::new(LastKnownCodecs::AudioCodec).string())
                    .col(ColumnDef::new(LastKnownCodecs::VideoWidth).integer())
                    .col(ColumnDef::new(LastKnownCodecs::VideoHeight).integer())
                    .col(ColumnDef::new(LastKnownCodecs::VideoBitrate).integer())
                    .col(ColumnDef::new(LastKnownCodecs::AudioBitrate).integer())
                    .col(ColumnDef::new(LastKnownCodecs::AudioChannels).integer())
                    .col(ColumnDef::new(LastKnownCodecs::AudioSampleRate).integer())
                    .col(ColumnDef::new(LastKnownCodecs::ContainerFormat).string())
                    .col(self.create_timestamp_column(manager, LastKnownCodecs::UpdatedAt))
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
                    .col(
                        ColumnDef::new(MigrationNotes::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(MigrationNotes::Version).string().not_null())
                    .col(ColumnDef::new(MigrationNotes::Note).string().not_null())
                    .col(
                        ColumnDef::new(MigrationNotes::CreatedAt)
                            .string()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn create_indexes(&self, manager: &SchemaManager<'_>) -> Result<(), DbErr> {
        // Stream sources indexes
        manager
            .create_index(
                Index::create()
                    .name("idx_stream_sources_name")
                    .table(StreamSources::Table)
                    .col(StreamSources::Name)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_stream_sources_source_type")
                    .table(StreamSources::Table)
                    .col(StreamSources::SourceType)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_stream_sources_is_active")
                    .table(StreamSources::Table)
                    .col(StreamSources::IsActive)
                    .to_owned(),
            )
            .await?;

        // Channels indexes
        manager
            .create_index(
                Index::create()
                    .name("idx_channels_source_id")
                    .table(Channels::Table)
                    .col(Channels::SourceId)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_channels_tvg_id")
                    .table(Channels::Table)
                    .col(Channels::TvgId)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_channels_source_id_channel_name")
                    .table(Channels::Table)
                    .col(Channels::SourceId)
                    .col(Channels::ChannelName)
                    .to_owned(),
            )
            .await?;

        // EPG sources indexes
        manager
            .create_index(
                Index::create()
                    .name("idx_epg_sources_name")
                    .table(EpgSources::Table)
                    .col(EpgSources::Name)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_epg_sources_source_type")
                    .table(EpgSources::Table)
                    .col(EpgSources::SourceType)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_epg_sources_is_active")
                    .table(EpgSources::Table)
                    .col(EpgSources::IsActive)
                    .to_owned(),
            )
            .await?;

        // EPG programs indexes
        manager
            .create_index(
                Index::create()
                    .name("idx_epg_programs_source_id")
                    .table(EpgPrograms::Table)
                    .col(EpgPrograms::SourceId)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_epg_programs_channel_id")
                    .table(EpgPrograms::Table)
                    .col(EpgPrograms::ChannelId)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_epg_programs_start_time")
                    .table(EpgPrograms::Table)
                    .col(EpgPrograms::StartTime)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_epg_programs_end_time")
                    .table(EpgPrograms::Table)
                    .col(EpgPrograms::EndTime)
                    .to_owned(),
            )
            .await?;

        // Filters indexes
        manager
            .create_index(
                Index::create()
                    .name("idx_filters_source_type")
                    .table(Filters::Table)
                    .col(Filters::SourceType)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_filters_is_inverse")
                    .table(Filters::Table)
                    .col(Filters::IsInverse)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_filters_is_system_default")
                    .table(Filters::Table)
                    .col(Filters::IsSystemDefault)
                    .to_owned(),
            )
            .await?;

        // Data mapping rules indexes
        manager
            .create_index(
                Index::create()
                    .name("idx_data_mapping_rules_is_active")
                    .table(DataMappingRules::Table)
                    .col(DataMappingRules::IsActive)
                    .to_owned(),
            )
            .await?;
        // Data mapping rules indexes
        manager
            .create_index(
                Index::create()
                    .name("idx_stream_proxies_is_active")
                    .table(StreamProxies::Table)
                    .col(StreamProxies::IsActive)
                    .to_owned(),
            )
            .await?;

        // Proxy sources indexes
        manager
            .create_index(
                Index::create()
                    .name("idx_proxy_sources_proxy_id")
                    .table(ProxySources::Table)
                    .col(ProxySources::ProxyId)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_proxy_sources_source_id")
                    .table(ProxySources::Table)
                    .col(ProxySources::SourceId)
                    .to_owned(),
            )
            .await?;

        // Proxy filters indexes
        manager
            .create_index(
                Index::create()
                    .name("idx_proxy_filters_proxy_id")
                    .table(ProxyFilters::Table)
                    .col(ProxyFilters::ProxyId)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_proxy_filters_filter_id")
                    .table(ProxyFilters::Table)
                    .col(ProxyFilters::FilterId)
                    .to_owned(),
            )
            .await?;

        // Proxy EPG sources indexes
        manager
            .create_index(
                Index::create()
                    .name("idx_proxy_epg_sources_proxy_id")
                    .table(ProxyEpgSources::Table)
                    .col(ProxyEpgSources::ProxyId)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_proxy_epg_sources_source_id")
                    .table(ProxyEpgSources::Table)
                    .col(ProxyEpgSources::EpgSourceId)
                    .to_owned(),
            )
            .await?;

        // Logo assets indexes
        manager
            .create_index(
                Index::create()
                    .name("idx_logo_assets_file_hash")
                    .table(LogoAssets::Table)
                    .col(LogoAssets::FileHash)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_logo_assets_is_system")
                    .table(LogoAssets::Table)
                    .col(LogoAssets::IsSystem)
                    .to_owned(),
            )
            .await?;

        // Last known codecs indexes
        manager
            .create_index(
                Index::create()
                    .name("idx_last_known_codecs_stream_url")
                    .table(LastKnownCodecs::Table)
                    .col(LastKnownCodecs::StreamUrl)
                    .to_owned(),
            )
            .await?;

        // Additional performance optimization indexes

        // Stream proxies relay profile foreign key
        manager
            .create_index(
                Index::create()
                    .name("idx_stream_proxies_relay_profile_id")
                    .table(StreamProxies::Table)
                    .col(StreamProxies::RelayProfileId)
                    .to_owned(),
            )
            .await?;

        // Composite indexes for common filtered queries
        // Note: Channels table doesn't have IsActive column, so using timestamp-based composite instead
        manager
            .create_index(
                Index::create()
                    .name("idx_channels_source_id_created")
                    .table(Channels::Table)
                    .col(Channels::SourceId)
                    .col(Channels::CreatedAt)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_stream_proxies_active_created")
                    .table(StreamProxies::Table)
                    .col(StreamProxies::IsActive)
                    .col(StreamProxies::CreatedAt)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_stream_sources_type_active")
                    .table(StreamSources::Table)
                    .col(StreamSources::SourceType)
                    .col(StreamSources::IsActive)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_epg_sources_type_active")
                    .table(EpgSources::Table)
                    .col(EpgSources::SourceType)
                    .col(EpgSources::IsActive)
                    .to_owned(),
            )
            .await?;

        // EPG time range composite index for program schedule lookups
        manager
            .create_index(
                Index::create()
                    .name("idx_epg_programs_channel_time_range")
                    .table(EpgPrograms::Table)
                    .col(EpgPrograms::ChannelId)
                    .col(EpgPrograms::StartTime)
                    .col(EpgPrograms::EndTime)
                    .to_owned(),
            )
            .await?;

        // Junction table priority-based composite indexes for ordered lookups
        manager
            .create_index(
                Index::create()
                    .name("idx_proxy_sources_proxy_priority")
                    .table(ProxySources::Table)
                    .col(ProxySources::ProxyId)
                    .col(ProxySources::PriorityOrder)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_proxy_filters_proxy_priority")
                    .table(ProxyFilters::Table)
                    .col(ProxyFilters::ProxyId)
                    .col(ProxyFilters::PriorityOrder)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_proxy_epg_sources_proxy_priority")
                    .table(ProxyEpgSources::Table)
                    .col(ProxyEpgSources::ProxyId)
                    .col(ProxyEpgSources::PriorityOrder)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }
}

// Table identifiers
#[derive(DeriveIden)]
enum StreamSources {
    Table,
    Id,
    Name,
    Url,
    SourceType,
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
enum Channels {
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
enum EpgSources {
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
enum EpgPrograms {
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
enum Filters {
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
enum DataMappingRules {
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
enum RelayProfiles {
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
enum StreamProxies {
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
enum ProxySources {
    Table,
    ProxyId,
    SourceId,
    PriorityOrder,
    CreatedAt,
}

#[derive(DeriveIden)]
enum ProxyFilters {
    Table,
    ProxyId,
    FilterId,
    PriorityOrder,
    IsActive,
    CreatedAt,
}

#[derive(DeriveIden)]
enum ProxyEpgSources {
    Table,
    ProxyId,
    EpgSourceId,
    PriorityOrder,
    CreatedAt,
}

#[derive(DeriveIden)]
enum LogoAssets {
    Table,
    Id,
    Name,
    Description,
    FileName,
    FilePath,
    FileHash,
    FileSize,
    MimeType,
    AssetType,
    SourceUrl,
    Width,
    Height,
    ParentAssetId,
    FormatType,
    IsSystem,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum LastKnownCodecs {
    Table,
    Id,
    StreamUrl,
    VideoCodec,
    AudioCodec,
    VideoWidth,
    VideoHeight,
    VideoBitrate,
    AudioBitrate,
    AudioChannels,
    AudioSampleRate,
    ContainerFormat,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum MigrationNotes {
    Table,
    Id,
    Version,
    Note,
    CreatedAt,
}
