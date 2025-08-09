//! Relay Repository
//!
//! This module provides data access layer for relay profiles and configurations.
//! Handles FFmpeg relay system with profiles, channel configurations, and runtime status.

use async_trait::async_trait;
use chrono::Utc;
use sqlx::{Row, SqlitePool};
use std::str::FromStr;
use uuid::Uuid;

use crate::{
    errors::{RepositoryError, RepositoryResult},
    models::relay::{
        AudioCodec, ChannelRelayConfig, CreateChannelRelayConfigRequest, CreateRelayProfileRequest,
        RelayProfile, RelayOutputFormat, UpdateChannelRelayConfigRequest,
        UpdateRelayProfileRequest, VideoCodec,
    },
    repositories::traits::{QueryParams, Repository},
    utils::sqlite::SqliteRowExt,
};

#[derive(Clone)]
pub struct RelayRepository {
    pool: SqlitePool,
}

impl RelayRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Helper function to construct a RelayProfile from a database row
    fn relay_profile_from_row(row: &sqlx::sqlite::SqliteRow) -> RepositoryResult<RelayProfile> {
        let video_codec_str = row.get::<String, _>("video_codec");
        let video_codec = VideoCodec::from_str(&video_codec_str)
            .map_err(|e| RepositoryError::QueryFailed {
                query: "relay_profile_from_row".to_string(),
                message: format!("Failed to parse video_codec: {}", e),
            })?;

        let audio_codec_str = row.get::<String, _>("audio_codec");
        let audio_codec = AudioCodec::from_str(&audio_codec_str)
            .map_err(|e| RepositoryError::QueryFailed {
                query: "relay_profile_from_row".to_string(),
                message: format!("Failed to parse audio_codec: {}", e),
            })?;

        let output_format_str = row.get::<String, _>("output_format");
        let output_format = RelayOutputFormat::from_str(&output_format_str)
            .map_err(|e| RepositoryError::QueryFailed {
                query: "relay_profile_from_row".to_string(),
                message: format!("Failed to parse output_format: {}", e),
            })?;

        Ok(RelayProfile {
            id: row
                .get_uuid("id")
                .map_err(|e| RepositoryError::QueryFailed {
                    query: "relay_profile_from_row".to_string(),
                    message: format!("Failed to parse id: {}", e),
                })?,
            name: row.get("name"),
            description: row.get("description"),
            video_codec,
            audio_codec,
            video_profile: row.get("video_profile"),
            video_preset: row.get("video_preset"),
            video_bitrate: row.get::<Option<i64>, _>("video_bitrate").map(|v| v as u32),
            audio_bitrate: row.get::<Option<i64>, _>("audio_bitrate").map(|v| v as u32),
            audio_sample_rate: row.get::<Option<i64>, _>("audio_sample_rate").map(|v| v as u32),
            audio_channels: row.get::<Option<i64>, _>("audio_channels").map(|v| v as u32),
            enable_hardware_acceleration: row.get("enable_hardware_acceleration"),
            preferred_hwaccel: row.get("preferred_hwaccel"),
            manual_args: row.get("manual_args"),
            output_format,
            segment_duration: row.get::<Option<i64>, _>("segment_duration").map(|v| v as i32),
            max_segments: row.get::<Option<i64>, _>("max_segments").map(|v| v as i32),
            input_timeout: row.get::<i64, _>("input_timeout") as i32,
            is_system_default: row.get("is_system_default"),
            is_active: row.get("is_active"),
            created_at: row.get_datetime("created_at"),
            updated_at: row.get_datetime("updated_at"),
        })
    }

    /// Helper function to construct a ChannelRelayConfig from a database row
    fn channel_relay_config_from_row(row: &sqlx::sqlite::SqliteRow) -> RepositoryResult<ChannelRelayConfig> {
        Ok(ChannelRelayConfig {
            id: row
                .get_uuid("id")
                .map_err(|e| RepositoryError::QueryFailed {
                    query: "channel_relay_config_from_row".to_string(),
                    message: format!("Failed to parse id: {}", e),
                })?,
            proxy_id: row
                .get_uuid("proxy_id")
                .map_err(|e| RepositoryError::QueryFailed {
                    query: "channel_relay_config_from_row".to_string(),
                    message: format!("Failed to parse proxy_id: {}", e),
                })?,
            channel_id: row
                .get_uuid("channel_id")
                .map_err(|e| RepositoryError::QueryFailed {
                    query: "channel_relay_config_from_row".to_string(),
                    message: format!("Failed to parse channel_id: {}", e),
                })?,
            profile_id: row
                .get_uuid("profile_id")
                .map_err(|e| RepositoryError::QueryFailed {
                    query: "channel_relay_config_from_row".to_string(),
                    message: format!("Failed to parse profile_id: {}", e),
                })?,
            name: row.get("name"),
            description: row.get("description"),
            custom_args: row.get("custom_args"),
            is_active: row.get("is_active"),
            created_at: row.get_datetime("created_at"),
            updated_at: row.get_datetime("updated_at"),
        })
    }

    /// Get all active relay profiles
    pub async fn get_active_profiles(&self) -> RepositoryResult<Vec<RelayProfile>> {
        let rows = sqlx::query(
            r#"
            SELECT id, name, description, video_codec, audio_codec, video_profile,
                   video_preset, video_bitrate, audio_bitrate, audio_sample_rate,
                   audio_channels, enable_hardware_acceleration, preferred_hwaccel,
                   manual_args, output_format, segment_duration, max_segments,
                   input_timeout, is_system_default, is_active, created_at, updated_at
            FROM relay_profiles
            WHERE is_active = 1
            ORDER BY is_system_default DESC, name ASC
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| RepositoryError::QueryFailed {
            query: "get_active_profiles".to_string(),
            message: e.to_string(),
        })?;

        let mut profiles = Vec::new();
        for row in rows {
            profiles.push(Self::relay_profile_from_row(&row)?);
        }

        Ok(profiles)
    }

    /// Get system default relay profile
    pub async fn get_system_default_profile(&self) -> RepositoryResult<Option<RelayProfile>> {
        let row = sqlx::query(
            r#"
            SELECT id, name, description, video_codec, audio_codec, video_profile,
                   video_preset, video_bitrate, audio_bitrate, audio_sample_rate,
                   audio_channels, enable_hardware_acceleration, preferred_hwaccel,
                   manual_args, output_format, segment_duration, max_segments,
                   input_timeout, is_system_default, is_active, created_at, updated_at
            FROM relay_profiles
            WHERE is_system_default = 1 AND is_active = 1
            LIMIT 1
            "#,
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| RepositoryError::QueryFailed {
            query: "get_system_default_profile".to_string(),
            message: e.to_string(),
        })?;

        match row {
            Some(row) => Ok(Some(Self::relay_profile_from_row(&row)?)),
            None => Ok(None),
        }
    }

    /// Create a channel relay configuration
    pub async fn create_channel_config(
        &self,
        proxy_id: Uuid,
        channel_id: Uuid,
        request: CreateChannelRelayConfigRequest,
    ) -> RepositoryResult<ChannelRelayConfig> {
        let config_id = Uuid::new_v4();
        let now = Utc::now();
        let now_str = now.to_rfc3339();
        let config_id_str = config_id.to_string();
        let proxy_id_str = proxy_id.to_string();
        let channel_id_str = channel_id.to_string();
        let profile_id_str = request.profile_id.to_string();

        // Serialize custom args if provided
        let custom_args_json = if let Some(args) = request.custom_args {
            Some(serde_json::to_string(&args).map_err(|e| RepositoryError::QueryFailed {
                query: "create_channel_config".to_string(),
                message: format!("Failed to serialize custom_args: {}", e),
            })?)
        } else {
            None
        };

        sqlx::query(
            r#"
            INSERT INTO channel_relay_configs (
                id, proxy_id, channel_id, profile_id, name, description,
                custom_args, is_active, created_at, updated_at
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&config_id_str)
        .bind(&proxy_id_str)
        .bind(&channel_id_str)
        .bind(&profile_id_str)
        .bind(&request.name)
        .bind(&request.description)
        .bind(&custom_args_json)
        .bind(true)
        .bind(&now_str)
        .bind(&now_str)
        .execute(&self.pool)
        .await
        .map_err(|e| RepositoryError::QueryFailed {
            query: "create_channel_config".to_string(),
            message: e.to_string(),
        })?;

        // Return the created configuration
        self.find_channel_config_by_id(config_id).await?
            .ok_or_else(|| RepositoryError::RecordNotFound {
                table: "channel_relay_configs".to_string(),
                field: "id".to_string(),
                value: config_id_str,
            })
    }

    /// Update a channel relay configuration
    pub async fn update_channel_config(
        &self,
        config_id: Uuid,
        request: UpdateChannelRelayConfigRequest,
    ) -> RepositoryResult<ChannelRelayConfig> {
        let now = Utc::now();
        let now_str = now.to_rfc3339();
        let config_id_str = config_id.to_string();

        // Use SQLx QueryBuilder for dynamic updates
        let mut query_builder = sqlx::QueryBuilder::new("UPDATE channel_relay_configs SET ");
        let mut has_updates = false;

        if let Some(profile_id) = request.profile_id {
            if has_updates {
                query_builder.push(", ");
            }
            query_builder.push("profile_id = ");
            query_builder.push_bind(profile_id.to_string());
            has_updates = true;
        }

        if let Some(name) = request.name {
            if has_updates {
                query_builder.push(", ");
            }
            query_builder.push("name = ");
            query_builder.push_bind(name);
            has_updates = true;
        }

        if let Some(description) = request.description {
            if has_updates {
                query_builder.push(", ");
            }
            query_builder.push("description = ");
            query_builder.push_bind(description);
            has_updates = true;
        }

        if let Some(custom_args) = request.custom_args {
            let custom_args_json = serde_json::to_string(&custom_args).map_err(|e| RepositoryError::QueryFailed {
                query: "update_channel_config".to_string(),
                message: format!("Failed to serialize custom_args: {}", e),
            })?;
            if has_updates {
                query_builder.push(", ");
            }
            query_builder.push("custom_args = ");
            query_builder.push_bind(custom_args_json);
            has_updates = true;
        }

        if let Some(is_active) = request.is_active {
            if has_updates {
                query_builder.push(", ");
            }
            query_builder.push("is_active = ");
            query_builder.push_bind(is_active);
            has_updates = true;
        }

        // Always update the timestamp
        if has_updates {
            query_builder.push(", ");
        }
        query_builder.push("updated_at = ");
        query_builder.push_bind(&now_str);
        has_updates = true;

        if !has_updates {
            return Err(RepositoryError::QueryFailed {
                query: "update_channel_config".to_string(),
                message: "No fields to update".to_string(),
            });
        }

        query_builder.push(" WHERE id = ");
        query_builder.push_bind(&config_id_str);

        let query = query_builder.build();
        let result = query.execute(&self.pool).await.map_err(|e| RepositoryError::QueryFailed {
            query: "update_channel_config".to_string(),
            message: e.to_string(),
        })?;

        if result.rows_affected() == 0 {
            return Err(RepositoryError::RecordNotFound {
                table: "channel_relay_configs".to_string(),
                field: "id".to_string(),
                value: config_id_str,
            });
        }

        // Return the updated configuration
        self.find_channel_config_by_id(config_id).await?
            .ok_or_else(|| RepositoryError::RecordNotFound {
                table: "channel_relay_configs".to_string(),
                field: "id".to_string(),
                value: config_id_str,
            })
    }

    /// Find channel relay configuration by ID
    pub async fn find_channel_config_by_id(&self, config_id: Uuid) -> RepositoryResult<Option<ChannelRelayConfig>> {
        let config_id_str = config_id.to_string();
        let row = sqlx::query(
            r#"
            SELECT id, proxy_id, channel_id, profile_id, name, description,
                   custom_args, is_active, created_at, updated_at
            FROM channel_relay_configs
            WHERE id = ?
            "#,
        )
        .bind(config_id_str)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| RepositoryError::QueryFailed {
            query: "find_channel_config_by_id".to_string(),
            message: e.to_string(),
        })?;

        match row {
            Some(row) => Ok(Some(Self::channel_relay_config_from_row(&row)?)),
            None => Ok(None),
        }
    }

    /// Get channel relay configurations for a proxy
    pub async fn get_channel_configs_for_proxy(&self, proxy_id: Uuid) -> RepositoryResult<Vec<ChannelRelayConfig>> {
        let proxy_id_str = proxy_id.to_string();
        let rows = sqlx::query(
            r#"
            SELECT id, proxy_id, channel_id, profile_id, name, description,
                   custom_args, is_active, created_at, updated_at
            FROM channel_relay_configs
            WHERE proxy_id = ? AND is_active = 1
            ORDER BY name ASC
            "#,
        )
        .bind(proxy_id_str)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| RepositoryError::QueryFailed {
            query: "get_channel_configs_for_proxy".to_string(),
            message: e.to_string(),
        })?;

        let mut configs = Vec::new();
        for row in rows {
            configs.push(Self::channel_relay_config_from_row(&row)?);
        }

        Ok(configs)
    }

    /// Get channel relay configuration for a specific channel in a proxy
    pub async fn get_channel_config_for_channel(
        &self,
        proxy_id: Uuid,
        channel_id: Uuid,
    ) -> RepositoryResult<Option<ChannelRelayConfig>> {
        let proxy_id_str = proxy_id.to_string();
        let channel_id_str = channel_id.to_string();
        let row = sqlx::query(
            r#"
            SELECT id, proxy_id, channel_id, profile_id, name, description,
                   custom_args, is_active, created_at, updated_at
            FROM channel_relay_configs
            WHERE proxy_id = ? AND channel_id = ? AND is_active = 1
            LIMIT 1
            "#,
        )
        .bind(proxy_id_str)
        .bind(channel_id_str)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| RepositoryError::QueryFailed {
            query: "get_channel_config_for_channel".to_string(),
            message: e.to_string(),
        })?;

        match row {
            Some(row) => Ok(Some(Self::channel_relay_config_from_row(&row)?)),
            None => Ok(None),
        }
    }

    /// Delete a channel relay configuration
    pub async fn delete_channel_config(&self, config_id: Uuid) -> RepositoryResult<()> {
        let config_id_str = config_id.to_string();

        let result = sqlx::query("DELETE FROM channel_relay_configs WHERE id = ?")
            .bind(&config_id_str)
            .execute(&self.pool)
            .await
            .map_err(|e| RepositoryError::QueryFailed {
                query: "delete_channel_config".to_string(),
                message: e.to_string(),
            })?;

        if result.rows_affected() == 0 {
            return Err(RepositoryError::RecordNotFound {
                table: "channel_relay_configs".to_string(),
                field: "id".to_string(),
                value: config_id_str,
            });
        }

        Ok(())
    }

}

#[async_trait]
impl Repository<RelayProfile, Uuid> for RelayRepository {
    type CreateRequest = CreateRelayProfileRequest;
    type UpdateRequest = UpdateRelayProfileRequest;
    type Query = QueryParams;

    async fn find_by_id(&self, id: Uuid) -> RepositoryResult<Option<RelayProfile>> {
        let id_str = id.to_string();
        let row = sqlx::query(
            r#"
            SELECT id, name, description, video_codec, audio_codec, video_profile,
                   video_preset, video_bitrate, audio_bitrate, audio_sample_rate,
                   audio_channels, enable_hardware_acceleration, preferred_hwaccel,
                   manual_args, output_format, segment_duration, max_segments,
                   input_timeout, is_system_default, is_active, created_at, updated_at
            FROM relay_profiles
            WHERE id = ?
            "#,
        )
        .bind(id_str)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| RepositoryError::QueryFailed {
            query: "find_by_id".to_string(),
            message: e.to_string(),
        })?;

        match row {
            Some(row) => Ok(Some(Self::relay_profile_from_row(&row)?)),
            None => Ok(None),
        }
    }

    async fn find_all(&self, _query: Self::Query) -> RepositoryResult<Vec<RelayProfile>> {
        let rows = sqlx::query(
            r#"
            SELECT id, name, description, video_codec, audio_codec, video_profile,
                   video_preset, video_bitrate, audio_bitrate, audio_sample_rate,
                   audio_channels, enable_hardware_acceleration, preferred_hwaccel,
                   manual_args, output_format, segment_duration, max_segments,
                   input_timeout, is_system_default, is_active, created_at, updated_at
            FROM relay_profiles
            ORDER BY is_system_default DESC, name ASC
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| RepositoryError::QueryFailed {
            query: "find_all".to_string(),
            message: e.to_string(),
        })?;

        let mut profiles = Vec::new();
        for row in rows {
            profiles.push(Self::relay_profile_from_row(&row)?);
        }

        Ok(profiles)
    }

    async fn create(&self, request: Self::CreateRequest) -> RepositoryResult<RelayProfile> {
        let profile_id = Uuid::new_v4();
        let now = Utc::now();
        let now_str = now.to_rfc3339();
        let profile_id_str = profile_id.to_string();

        // Convert numeric values to prevent temporary value drops
        let video_bitrate = request.video_bitrate.map(|v| v as i64);
        let audio_bitrate = request.audio_bitrate.map(|v| v as i64);
        let audio_sample_rate = request.audio_sample_rate.map(|v| v as i64);
        let audio_channels = request.audio_channels.map(|v| v as i64);
        let segment_duration = request.segment_duration.map(|v| v as i64);
        let max_segments = request.max_segments.map(|v| v as i64);
        let input_timeout = request.input_timeout.unwrap_or(30) as i64;
        let enable_hardware_acceleration = request.enable_hardware_acceleration.unwrap_or(false);
        let is_system_default = request.is_system_default.unwrap_or(false);

        sqlx::query(
            r#"
            INSERT INTO relay_profiles (
                id, name, description, video_codec, audio_codec, video_profile,
                video_preset, video_bitrate, audio_bitrate, audio_sample_rate,
                audio_channels, enable_hardware_acceleration, preferred_hwaccel,
                manual_args, output_format, segment_duration, max_segments,
                input_timeout, is_system_default, is_active, created_at, updated_at
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&profile_id_str)
        .bind(&request.name)
        .bind(&request.description)
        .bind(request.video_codec.to_string())
        .bind(request.audio_codec.to_string())
        .bind(&request.video_profile)
        .bind(&request.video_preset)
        .bind(video_bitrate)
        .bind(audio_bitrate)
        .bind(audio_sample_rate)
        .bind(audio_channels)
        .bind(enable_hardware_acceleration)
        .bind(&request.preferred_hwaccel)
        .bind(&request.manual_args)
        .bind(request.output_format.to_string())
        .bind(segment_duration)
        .bind(max_segments)
        .bind(input_timeout)
        .bind(is_system_default)
        .bind(true)
        .bind(&now_str)
        .bind(&now_str)
        .execute(&self.pool)
        .await
        .map_err(|e| RepositoryError::QueryFailed {
            query: "create_relay_profile".to_string(),
            message: e.to_string(),
        })?;

        // Return the created profile
        self.find_by_id(profile_id)
            .await?
            .ok_or_else(|| RepositoryError::RecordNotFound {
                table: "relay_profiles".to_string(),
                field: "id".to_string(),
                value: profile_id_str,
            })
    }

    async fn update(&self, id: Uuid, request: Self::UpdateRequest) -> RepositoryResult<RelayProfile> {
        let now = Utc::now();
        let now_str = now.to_rfc3339();
        let id_str = id.to_string();

        // Build dynamic query based on provided fields
        let mut set_clauses = Vec::new();
        let mut params: Vec<String> = Vec::new();

        if let Some(name) = request.name {
            set_clauses.push("name = ?");
            params.push(name);
        }

        if let Some(description) = request.description {
            set_clauses.push("description = ?");
            params.push(description);
        }

        if let Some(video_codec) = request.video_codec {
            set_clauses.push("video_codec = ?");
            params.push(video_codec.to_string());
        }

        if let Some(audio_codec) = request.audio_codec {
            set_clauses.push("audio_codec = ?");
            params.push(audio_codec.to_string());
        }

        if let Some(video_profile) = request.video_profile {
            set_clauses.push("video_profile = ?");
            params.push(video_profile);
        }

        if let Some(video_preset) = request.video_preset {
            set_clauses.push("video_preset = ?");
            params.push(video_preset);
        }

        if let Some(video_bitrate) = request.video_bitrate {
            set_clauses.push("video_bitrate = ?");
            params.push((video_bitrate as i64).to_string());
        }

        if let Some(audio_bitrate) = request.audio_bitrate {
            set_clauses.push("audio_bitrate = ?");
            params.push((audio_bitrate as i64).to_string());
        }

        if let Some(audio_sample_rate) = request.audio_sample_rate {
            set_clauses.push("audio_sample_rate = ?");
            params.push((audio_sample_rate as i64).to_string());
        }

        if let Some(audio_channels) = request.audio_channels {
            set_clauses.push("audio_channels = ?");
            params.push((audio_channels as i64).to_string());
        }

        if let Some(enable_hardware_acceleration) = request.enable_hardware_acceleration {
            set_clauses.push("enable_hardware_acceleration = ?");
            params.push(enable_hardware_acceleration.to_string());
        }

        if let Some(preferred_hwaccel) = request.preferred_hwaccel {
            set_clauses.push("preferred_hwaccel = ?");
            params.push(preferred_hwaccel);
        }

        if let Some(manual_args) = request.manual_args {
            set_clauses.push("manual_args = ?");
            params.push(manual_args);
        }

        if let Some(output_format) = request.output_format {
            set_clauses.push("output_format = ?");
            params.push(output_format.to_string());
        }

        if let Some(segment_duration) = request.segment_duration {
            set_clauses.push("segment_duration = ?");
            params.push((segment_duration as i64).to_string());
        }

        if let Some(max_segments) = request.max_segments {
            set_clauses.push("max_segments = ?");
            params.push((max_segments as i64).to_string());
        }

        if let Some(input_timeout) = request.input_timeout {
            set_clauses.push("input_timeout = ?");
            params.push((input_timeout as i64).to_string());
        }

        if let Some(is_active) = request.is_active {
            set_clauses.push("is_active = ?");
            params.push(is_active.to_string());
        }

        if let Some(is_system_default) = request.is_system_default {
            set_clauses.push("is_system_default = ?");
            params.push(is_system_default.to_string());
        }

        set_clauses.push("updated_at = ?");
        params.push(now_str);

        if set_clauses.len() == 1 {
            // Only updated_at was added, meaning no fields to update
            return Err(RepositoryError::QueryFailed {
                query: "update_relay_profile".to_string(),
                message: "No fields to update".to_string(),
            });
        }

        let query = format!(
            "UPDATE relay_profiles SET {} WHERE id = ?",
            set_clauses.join(", ")
        );

        let mut query_builder = sqlx::query(&query);
        for param in &params {
            query_builder = query_builder.bind(param);
        }
        query_builder = query_builder.bind(&id_str);

        let result = query_builder.execute(&self.pool).await.map_err(|e| RepositoryError::QueryFailed {
            query: "update_relay_profile".to_string(),
            message: e.to_string(),
        })?;

        if result.rows_affected() == 0 {
            return Err(RepositoryError::RecordNotFound {
                table: "relay_profiles".to_string(),
                field: "id".to_string(),
                value: id_str,
            });
        }

        // Return the updated profile
        self.find_by_id(id)
            .await?
            .ok_or_else(|| RepositoryError::RecordNotFound {
                table: "relay_profiles".to_string(),
                field: "id".to_string(),
                value: id_str,
            })
    }

    async fn delete(&self, id: Uuid) -> RepositoryResult<()> {
        let id_str = id.to_string();

        let result = sqlx::query("DELETE FROM relay_profiles WHERE id = ?")
            .bind(&id_str)
            .execute(&self.pool)
            .await
            .map_err(|e| RepositoryError::QueryFailed {
                query: "delete_relay_profile".to_string(),
                message: e.to_string(),
            })?;

        if result.rows_affected() == 0 {
            return Err(RepositoryError::RecordNotFound {
                table: "relay_profiles".to_string(),
                field: "id".to_string(),
                value: id_str,
            });
        }

        Ok(())
    }

    async fn count(&self, _query: Self::Query) -> RepositoryResult<u64> {
        let row = sqlx::query("SELECT COUNT(*) as count FROM relay_profiles")
            .fetch_one(&self.pool)
            .await
            .map_err(|e| RepositoryError::QueryFailed {
                query: "count_relay_profiles".to_string(),
                message: e.to_string(),
            })?;

        Ok(row.get::<i64, _>("count") as u64)
    }
}
