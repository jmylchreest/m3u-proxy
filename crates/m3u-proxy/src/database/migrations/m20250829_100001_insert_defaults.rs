//! Migration to insert default data for consolidated schema
//!
//! This inserts default filters, relay profiles, data mapping rules, and migration notes
//! that are essential for the system to function properly.

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

impl Migration {
    /// Create UUID value for database insertion with proper type casting
    fn create_uuid_value(manager: &SchemaManager<'_>, uuid_str: &str) -> SimpleExpr {
        match manager.get_database_backend() {
            sea_orm::DatabaseBackend::Postgres => {
                // PostgreSQL requires explicit UUID casting
                Expr::cust(&format!("'{}'::uuid", uuid_str))
            }
            _ => {
                // SQLite and MySQL use string values
                Expr::value(uuid_str)
            }
        }
    }

    /// Create timestamp value for database insertion with proper type casting
    fn create_timestamp_value(
        manager: &SchemaManager<'_>,
        timestamp: &chrono::DateTime<chrono::Utc>,
    ) -> SimpleExpr {
        match manager.get_database_backend() {
            sea_orm::DatabaseBackend::Postgres => {
                // PostgreSQL timestamptz - use custom SQL for proper timezone handling
                Expr::cust(&format!("'{}'::timestamptz", timestamp.to_rfc3339()))
            }
            sea_orm::DatabaseBackend::MySql => {
                // MySQL expects proper timestamp format
                Expr::value(timestamp.format("%Y-%m-%d %H:%M:%S%.6f").to_string())
            }
            _ => {
                // SQLite uses string format for backward compatibility
                Expr::value(timestamp.format("%Y-%m-%d %H:%M:%S").to_string())
            }
        }
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let _db = manager.get_connection();
        let now = chrono::Utc::now();

        // Insert default filters
        let default_filters = vec![
            // Include All Valid Stream URLs
            (
                "a1b2c3d4-e5f6-7890-abcd-ef1234567890",
                "Include All Valid Stream URLs",
                "stream",
                false,
                true,
                "(stream_url starts_with \"http\")",
            ),
            // Exclude Adult Content
            (
                "b2c3d4e5-f6a7-8901-bcde-f23456789012",
                "Exclude Adult Content",
                "stream",
                true,
                true,
                "(group_title contains \"adult\" OR group_title contains \"xxx\" OR group_title contains \"porn\" OR channel_name contains \"adult\" OR channel_name contains \"xxx\" OR channel_name contains \"porn\")",
            ),
        ];

        for (id, name, source_type, is_inverse, is_system_default, expression) in default_filters {
            manager
                .exec_stmt(
                    Query::insert()
                        .into_table(Filters::Table)
                        .columns([
                            Filters::Id,
                            Filters::Name,
                            Filters::SourceType,
                            Filters::IsInverse,
                            Filters::IsSystemDefault,
                            Filters::Expression,
                            Filters::CreatedAt,
                            Filters::UpdatedAt,
                        ])
                        .values_panic([
                            Self::create_uuid_value(manager, id),
                            name.into(),
                            source_type.into(),
                            is_inverse.into(),
                            is_system_default.into(),
                            expression.into(),
                            Self::create_timestamp_value(manager, &now),
                            Self::create_timestamp_value(manager, &now),
                        ])
                        .to_owned(),
                )
                .await?;
        }

        // Insert default relay profiles
        let default_relay_profiles = vec![
            // H.264 + AAC (Maximum compatibility)
            (
                "a9b8c7d6-e5f4-3210-9876-543210987654",
                "H.264 + AAC (Standard)",
                "Maximum compatibility profile with H.264 video and AAC audio",
                "h264",
                "aac",
                Some("main"),
                Some("fast"),
                Some(2000),
                Some(128),
                Some(48000),
                Some(2),
                true,
                Some("auto"),
                "transport_stream",
                None::<i32>,
                None::<i32>,
                30,
                true,
                true,
            ),
            // H.265 + AAC (Better compression)
            (
                "b8c7d6e5-f4a3-2109-8765-432109876543",
                "H.265 + AAC (Standard)",
                "Better compression with H.265 video and AAC audio",
                "h265",
                "aac",
                Some("main"),
                Some("fast"),
                Some(1500),
                Some(128),
                Some(48000),
                Some(2),
                true,
                Some("auto"),
                "transport_stream",
                None::<i32>,
                None::<i32>,
                30,
                true,
                true,
            ),
            // H.264 High Quality
            (
                "c7d6e5f4-a3b2-1098-7654-321098765432",
                "H.264 + AAC (High Quality)",
                "High quality H.264 profile for better video quality",
                "h264",
                "aac",
                Some("high"),
                Some("slower"),
                Some(4000),
                Some(192),
                Some(48000),
                Some(2),
                true,
                Some("auto"),
                "transport_stream",
                None::<i32>,
                None::<i32>,
                30,
                false,
                true,
            ),
            // H.264 Low Bitrate (Mobile/Bandwidth Limited)
            (
                "d6e5f4c3-b2a1-0987-6543-210987654321",
                "H.264 + AAC (Low Bitrate)",
                "Low bitrate H.264 profile for mobile devices or limited bandwidth",
                "h264",
                "aac",
                Some("baseline"),
                Some("veryfast"),
                Some(800),
                Some(96),
                Some(48000),
                Some(2),
                true,
                Some("auto"),
                "transport_stream",
                None::<i32>,
                None::<i32>,
                30,
                false,
                true,
            ),
            // H.265 High Quality
            (
                "e5f4c3b2-a109-8765-4321-098765432109",
                "H.265 + AAC (High Quality)",
                "High quality H.265/HEVC profile with better compression",
                "h265",
                "aac",
                Some("main"),
                Some("slow"),
                Some(3000),
                Some(192),
                Some(48000),
                Some(2),
                true,
                Some("auto"),
                "transport_stream",
                None::<i32>,
                None::<i32>,
                30,
                false,
                true,
            ),
            // AV1 (Next-gen codec)
            (
                "f4c3b2a1-0987-6543-2109-876543210987",
                "AV1 + AAC (Next-gen)",
                "Next-generation AV1 codec for best compression efficiency",
                "av1",
                "aac",
                None,
                Some("medium"),
                Some(2500),
                Some(128),
                Some(48000),
                Some(2),
                true,
                Some("auto"),
                "transport_stream",
                None::<i32>,
                None::<i32>,
                30,
                false,
                true,
            ),
            // Copy streams (No transcoding)
            (
                "03b2a109-8765-4321-0987-65432109876f",
                "Copy Streams (No Transcoding)",
                "Pass-through profile that copies streams without transcoding",
                "copy",
                "copy",
                None,
                None,
                None,
                None,
                None,
                None,
                false,
                None,
                "transport_stream",
                None::<i32>,
                None::<i32>,
                30,
                false,
                true,
            ),
        ];

        for (
            id,
            name,
            description,
            video_codec,
            audio_codec,
            video_profile,
            video_preset,
            video_bitrate,
            audio_bitrate,
            audio_sample_rate,
            audio_channels,
            enable_hw_accel,
            preferred_hwaccel,
            output_format,
            segment_duration,
            max_segments,
            input_timeout,
            is_system_default,
            is_active,
        ) in default_relay_profiles
        {
            manager
                .exec_stmt(
                    Query::insert()
                        .into_table(RelayProfiles::Table)
                        .columns([
                            RelayProfiles::Id,
                            RelayProfiles::Name,
                            RelayProfiles::Description,
                            RelayProfiles::VideoCodec,
                            RelayProfiles::AudioCodec,
                            RelayProfiles::VideoProfile,
                            RelayProfiles::VideoPreset,
                            RelayProfiles::VideoBitrate,
                            RelayProfiles::AudioBitrate,
                            RelayProfiles::AudioSampleRate,
                            RelayProfiles::AudioChannels,
                            RelayProfiles::EnableHardwareAcceleration,
                            RelayProfiles::PreferredHwaccel,
                            RelayProfiles::ManualArgs,
                            RelayProfiles::OutputFormat,
                            RelayProfiles::SegmentDuration,
                            RelayProfiles::MaxSegments,
                            RelayProfiles::InputTimeout,
                            RelayProfiles::IsSystemDefault,
                            RelayProfiles::IsActive,
                            RelayProfiles::CreatedAt,
                            RelayProfiles::UpdatedAt,
                        ])
                        .values_panic([
                            Self::create_uuid_value(manager, id),
                            name.into(),
                            description.into(),
                            video_codec.into(),
                            audio_codec.into(),
                            video_profile.into(),
                            video_preset.into(),
                            video_bitrate.into(),
                            audio_bitrate.into(),
                            audio_sample_rate.into(),
                            audio_channels.into(),
                            enable_hw_accel.into(),
                            preferred_hwaccel.into(),
                            None::<String>.into(), // manual_args - set to None for default profiles
                            output_format.into(),
                            segment_duration.into(),
                            max_segments.into(),
                            input_timeout.into(),
                            is_system_default.into(),
                            is_active.into(),
                            Self::create_timestamp_value(manager, &now),
                            Self::create_timestamp_value(manager, &now),
                        ])
                        .to_owned(),
                )
                .await?;
        }

        // Insert default data mapping rules
        manager.exec_stmt(
            Query::insert()
                .into_table(DataMappingRules::Table)
                .columns([
                    DataMappingRules::Id,
                    DataMappingRules::Name,
                    DataMappingRules::Description,
                    DataMappingRules::SourceType,
                    DataMappingRules::Expression,
                    DataMappingRules::SortOrder,
                    DataMappingRules::IsActive,
                    DataMappingRules::CreatedAt,
                    DataMappingRules::UpdatedAt,
                ])
                .values_panic([
                    Self::create_uuid_value(manager, "7f4a2e5c-1b3d-4a7e-9f8b-2c5e7a9d1f3b"),
                    "Default Timeshift Detection (Regex)".into(),
                    "Automatically detects timeshift channels (+1, +24, etc.) and sets tvg-shift field using regex capture groups.".into(),
                    "stream".into(),
                    "channel_name matches \".*[ ](?:\\+([0-9]{1,2})|(-[0-9]{1,2}))([hH]?)(?:$|[ ]).*\" AND channel_name not matches \".*(?:start:|stop:|24[-/]7).*\" AND tvg_id matches \"^.+$\" SET tvg_shift = \"$1$2\"".into(),
                    1.into(),
                    true.into(),
                    Self::create_timestamp_value(manager, &now),
                    Self::create_timestamp_value(manager, &now),
                ])
                .to_owned()
        ).await?;

        // Insert migration notes
        let migration_notes = vec![
            ("001", "Consolidated initial schema with core tables"),
            (
                "002",
                "Default filters, data mapping rules, and codec-based relay profiles",
            ),
        ];

        for (version, note) in migration_notes {
            manager
                .exec_stmt(
                    Query::insert()
                        .into_table(MigrationNotes::Table)
                        .columns([
                            MigrationNotes::Version,
                            MigrationNotes::Note,
                            MigrationNotes::CreatedAt,
                        ])
                        .values_panic([
                            version.into(),
                            note.into(),
                            now.format("%Y-%m-%d %H:%M:%S").to_string().into(),
                        ])
                        .to_owned(),
                )
                .await?;
        }

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Delete default filters
        manager
            .exec_stmt(
                Query::delete()
                    .from_table(Filters::Table)
                    .and_where(Expr::col(Filters::IsSystemDefault).eq(true))
                    .to_owned(),
            )
            .await?;

        // Delete default relay profiles
        manager
            .exec_stmt(
                Query::delete()
                    .from_table(RelayProfiles::Table)
                    .and_where(Expr::col(RelayProfiles::IsSystemDefault).eq(true))
                    .to_owned(),
            )
            .await?;

        // Delete default data mapping rules
        manager
            .exec_stmt(
                Query::delete()
                    .from_table(DataMappingRules::Table)
                    .and_where(Expr::col(DataMappingRules::Id).eq(Self::create_uuid_value(
                        manager,
                        "7f4a2e5c-1b3d-4a7e-9f8b-2c5e7a9d1f3b",
                    )))
                    .to_owned(),
            )
            .await?;

        // Delete migration notes
        manager
            .exec_stmt(
                Query::delete()
                    .from_table(MigrationNotes::Table)
                    .and_where(Expr::col(MigrationNotes::Version).is_in(["001", "002"]))
                    .to_owned(),
            )
            .await?;

        Ok(())
    }
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
enum MigrationNotes {
    Table,
    Version,
    Note,
    CreatedAt,
}
