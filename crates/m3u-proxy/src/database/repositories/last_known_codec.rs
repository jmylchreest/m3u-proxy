//! SeaORM-based LastKnownCodec repository implementation
//!
//! This provides a database-agnostic repository for LastKnownCodec operations using SeaORM.

use anyhow::Result;
use sea_orm::{ActiveModelTrait, DatabaseConnection, EntityTrait, Set, ColumnTrait, QueryFilter};
use std::sync::Arc;
use uuid::Uuid;

use crate::entities::{prelude::LastKnownCodecs, last_known_codecs};
use crate::models::last_known_codec::{LastKnownCodec, CreateLastKnownCodecRequest, ProbeMethod};

/// SeaORM-based repository for LastKnownCodec operations
pub struct LastKnownCodecSeaOrmRepository {
    connection: Arc<DatabaseConnection>,
}

impl LastKnownCodecSeaOrmRepository {
    /// Create a new repository instance
    pub fn new(connection: Arc<DatabaseConnection>) -> Self {
        Self { connection }
    }

    /// Create or update codec information for a stream (upsert operation)
    pub async fn upsert_codec_info(&self, stream_url: &str, request: CreateLastKnownCodecRequest) -> Result<LastKnownCodec> {
        // Check if there's already a codec record for this stream
        let existing = LastKnownCodecs::find()
            .filter(last_known_codecs::Column::StreamUrl.eq(stream_url))
            .one(&*self.connection)
            .await?;

        let now = chrono::Utc::now();

        match existing {
            Some(existing_model) => {
                // Update existing record
                let mut active_model: last_known_codecs::ActiveModel = existing_model.into();
                
                active_model.video_codec = Set(request.video_codec);
                active_model.audio_codec = Set(request.audio_codec);
                active_model.container_format = Set(request.container_format);
                active_model.resolution = Set(request.resolution);
                active_model.framerate = Set(request.framerate);
                active_model.bitrate = Set(request.bitrate);
                active_model.video_bitrate = Set(request.video_bitrate);
                active_model.audio_bitrate = Set(request.audio_bitrate);
                active_model.audio_channels = Set(request.audio_channels);
                active_model.audio_sample_rate = Set(request.audio_sample_rate);
                active_model.probe_method = Set(request.probe_method.to_string());
                active_model.probe_source = Set(request.probe_source);
                active_model.detected_at = Set(now);
                active_model.updated_at = Set(now);

                let updated_model = active_model.update(&*self.connection).await?;
                self.model_to_domain(updated_model)
            }
            None => {
                // Create new record
                let id = Uuid::new_v4();
                
                let active_model = last_known_codecs::ActiveModel {
                    id: Set(id),
                    stream_url: Set(stream_url.to_string()),
                    video_codec: Set(request.video_codec),
                    audio_codec: Set(request.audio_codec),
                    container_format: Set(request.container_format),
                    resolution: Set(request.resolution),
                    framerate: Set(request.framerate),
                    bitrate: Set(request.bitrate),
                    video_bitrate: Set(request.video_bitrate),
                    audio_bitrate: Set(request.audio_bitrate),
                    audio_channels: Set(request.audio_channels),
                    audio_sample_rate: Set(request.audio_sample_rate),
                    probe_method: Set(request.probe_method.to_string()),
                    probe_source: Set(request.probe_source),
                    detected_at: Set(now),
                    created_at: Set(now),
                    updated_at: Set(now),
                };

                let model = active_model.insert(&*self.connection).await?;
                self.model_to_domain(model)
            }
        }
    }

    /// Find codec info by stream URL
    pub async fn find_by_stream_url(&self, stream_url: &str) -> Result<Option<LastKnownCodec>> {
        let model = LastKnownCodecs::find()
            .filter(last_known_codecs::Column::StreamUrl.eq(stream_url))
            .one(&*self.connection)
            .await?;

        match model {
            Some(m) => Ok(Some(self.model_to_domain(m)?)),
            None => Ok(None),
        }
    }

    /// Get the latest codec info for a stream (alias for find_by_stream_url)
    pub async fn get_latest_codec_info(&self, stream_url: &str) -> Result<Option<LastKnownCodec>> {
        self.find_by_stream_url(stream_url).await
    }

    /// Convert SeaORM model to domain model
    fn model_to_domain(&self, model: last_known_codecs::Model) -> Result<LastKnownCodec> {
        use std::str::FromStr;
        
        let id = model.id;
        let stream_url = model.stream_url;
        let detected_at = model.detected_at;
        let created_at = model.created_at;
        let updated_at = model.updated_at;
        
        let probe_method = ProbeMethod::from_str(&model.probe_method)
            .map_err(|e| anyhow::anyhow!("Invalid probe method: {}", e))?;
        
        Ok(LastKnownCodec {
            id,
            stream_url,
            video_codec: model.video_codec,
            audio_codec: model.audio_codec,
            container_format: model.container_format,
            resolution: model.resolution,
            framerate: model.framerate,
            bitrate: model.bitrate,
            video_bitrate: model.video_bitrate,
            audio_bitrate: model.audio_bitrate,
            audio_channels: model.audio_channels,
            audio_sample_rate: model.audio_sample_rate,
            probe_method,
            probe_source: model.probe_source,
            detected_at,
            created_at,
            updated_at,
        })
    }
}