//! SeaORM Relay repository implementation
//!
//! This module provides the SeaORM implementation of relay repository
//! that works across SQLite, PostgreSQL, and MySQL databases.

use anyhow::Result;
use sea_orm::{DatabaseConnection, EntityTrait, QueryOrder, ActiveModelTrait, Set, ModelTrait};
use std::sync::Arc;
use uuid::Uuid;

use crate::entities::{relay_profiles, prelude::*};
use crate::models::relay::{RelayProfile, CreateRelayProfileRequest, UpdateRelayProfileRequest};

/// SeaORM-based Relay repository
#[derive(Clone)]
pub struct RelaySeaOrmRepository {
    connection: Arc<DatabaseConnection>,
}

impl RelaySeaOrmRepository {
    /// Create a new RelaySeaOrmRepository
    pub fn new(connection: Arc<DatabaseConnection>) -> Self {
        Self { connection }
    }

    /// Create a new relay profile
    pub async fn create(&self, request: CreateRelayProfileRequest) -> Result<RelayProfile> {
        let id = Uuid::new_v4();
        let now = chrono::Utc::now();

        let active_model = relay_profiles::ActiveModel {
            id: Set(id),
            name: Set(request.name.clone()),
            description: Set(request.description.clone()),
            video_codec: Set(request.video_codec.to_string()),
            audio_codec: Set(request.audio_codec.to_string()),
            video_profile: Set(request.video_profile.clone()),
            video_preset: Set(request.video_preset.clone()),
            video_bitrate: Set(request.video_bitrate.map(|v| v as i32)),
            audio_bitrate: Set(request.audio_bitrate.map(|v| v as i32)),
            audio_sample_rate: Set(request.audio_sample_rate.map(|v| v as i32)),
            audio_channels: Set(request.audio_channels.map(|v| v as i32)),
            enable_hardware_acceleration: Set(request.enable_hardware_acceleration.unwrap_or(false)),
            preferred_hwaccel: Set(request.preferred_hwaccel.clone()),
            manual_args: Set(request.manual_args.clone()),
            output_format: Set(request.output_format.to_string()),
            segment_duration: Set(request.segment_duration.map(|v| v as i32)),
            max_segments: Set(request.max_segments.map(|v| v as i32)),
            input_timeout: Set(request.input_timeout.unwrap_or(30) as i32),
            is_system_default: Set(false),
            is_active: Set(true),
            created_at: Set(now),
            updated_at: Set(now),
        };

        let model = active_model.insert(&*self.connection).await?;
        Ok(self.model_to_domain(model))
    }

    /// Find relay profile by ID
    pub async fn find_by_id(&self, id: Uuid) -> Result<Option<RelayProfile>> {
        let model = RelayProfiles::find_by_id(id).one(&*self.connection).await?;
        Ok(model.map(|m| self.model_to_domain(m)))
    }

    /// List all relay profiles
    pub async fn get_active_profiles(&self) -> Result<Vec<RelayProfile>> {
        let models = RelayProfiles::find()
            .order_by_asc(relay_profiles::Column::Name)
            .all(&*self.connection)
            .await?;

        Ok(models.into_iter().map(|m| self.model_to_domain(m)).collect())
    }

    /// Update relay profile
    pub async fn update(&self, id: Uuid, request: UpdateRelayProfileRequest) -> Result<RelayProfile> {
        let model = RelayProfiles::find_by_id(id)
            .one(&*self.connection)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Relay profile not found"))?;

        let mut active_model: relay_profiles::ActiveModel = model.into();
        
        if let Some(name) = request.name {
            active_model.name = Set(name);
        }
        if let Some(description) = request.description {
            active_model.description = Set(Some(description));
        }
        if let Some(video_codec) = request.video_codec {
            active_model.video_codec = Set(video_codec.to_string());
        }
        if let Some(audio_codec) = request.audio_codec {
            active_model.audio_codec = Set(audio_codec.to_string());
        }

        active_model.updated_at = Set(chrono::Utc::now());

        let updated_model = active_model.update(&*self.connection).await?;
        Ok(self.model_to_domain(updated_model))
    }

    /// Delete relay profile
    pub async fn delete(&self, id: Uuid) -> Result<()> {
        let model = RelayProfiles::find_by_id(id)
            .one(&*self.connection)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Relay profile not found"))?;

        model.delete(&*self.connection).await?;
        Ok(())
    }

    /// Convert SeaORM model to domain model (only enum parsing needed)
    fn model_to_domain(&self, model: relay_profiles::Model) -> RelayProfile {
        use std::str::FromStr;
        use crate::models::relay::{VideoCodec, AudioCodec, RelayOutputFormat};
        
        RelayProfile {
            id: model.id,
            name: model.name,
            description: model.description,
            video_codec: VideoCodec::from_str(&model.video_codec)
                .unwrap_or(VideoCodec::H264), // graceful fallback
            audio_codec: AudioCodec::from_str(&model.audio_codec)
                .unwrap_or(AudioCodec::AAC), // graceful fallback  
            video_profile: model.video_profile,
            video_preset: model.video_preset,
            video_bitrate: model.video_bitrate.map(|v| v as u32),
            audio_bitrate: model.audio_bitrate.map(|v| v as u32),
            audio_sample_rate: model.audio_sample_rate.map(|v| v as u32),
            audio_channels: model.audio_channels.map(|v| v as u32),
            enable_hardware_acceleration: model.enable_hardware_acceleration,
            preferred_hwaccel: model.preferred_hwaccel,
            manual_args: model.manual_args,
            output_format: RelayOutputFormat::from_str(&model.output_format)
                .unwrap_or(RelayOutputFormat::TransportStream), // graceful fallback
            segment_duration: model.segment_duration,
            max_segments: model.max_segments,
            input_timeout: model.input_timeout,
            is_system_default: model.is_system_default,
            is_active: model.is_active,
            created_at: model.created_at,
            updated_at: model.updated_at,
        }
    }
}