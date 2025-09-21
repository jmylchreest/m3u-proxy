use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct LastKnownCodec {
    pub id: Uuid,
    pub stream_url: String,
    pub video_codec: Option<String>,
    pub audio_codec: Option<String>,
    pub container_format: Option<String>,
    /// Stored as separate width / height values (preferred canonical form)
    pub video_width: Option<i32>,
    pub video_height: Option<i32>,
    pub framerate: Option<String>,
    /// Overall/container bitrate
    pub bitrate: Option<i32>,
    pub video_bitrate: Option<i32>,
    pub audio_bitrate: Option<i32>,
    /// Number of audio channels (e.g. 2)
    pub audio_channels: Option<i32>,
    pub audio_sample_rate: Option<i32>,
    pub probe_method: ProbeMethod,
    pub probe_source: Option<String>,
    pub detected_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// Derived convenience field (not stored directly; serialize if present)
    pub resolution: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProbeMethod {
    MpegtsPlayer,
    FfprobeManual,
    FfprobeRelay,
    FfprobeAuto,
}

impl std::fmt::Display for ProbeMethod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProbeMethod::MpegtsPlayer => write!(f, "mpegts_player"),
            ProbeMethod::FfprobeManual => write!(f, "ffprobe_manual"),
            ProbeMethod::FfprobeRelay => write!(f, "ffprobe_relay"),
            ProbeMethod::FfprobeAuto => write!(f, "ffprobe_auto"),
        }
    }
}

impl std::str::FromStr for ProbeMethod {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "mpegts_player" => Ok(ProbeMethod::MpegtsPlayer),
            "ffprobe_manual" => Ok(ProbeMethod::FfprobeManual),
            "ffprobe_relay" => Ok(ProbeMethod::FfprobeRelay),
            "ffprobe_auto" => Ok(ProbeMethod::FfprobeAuto),
            _ => Err(format!("Unknown probe method: {}", s)),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct CreateLastKnownCodecRequest {
    pub video_codec: Option<String>,
    pub audio_codec: Option<String>,
    pub container_format: Option<String>,
    pub video_width: Option<i32>,
    pub video_height: Option<i32>,
    pub framerate: Option<String>,
    pub bitrate: Option<i32>,
    pub video_bitrate: Option<i32>,
    pub audio_bitrate: Option<i32>,
    pub audio_channels: Option<i32>,
    pub audio_sample_rate: Option<i32>,
    pub probe_method: ProbeMethod,
    pub probe_source: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct UpdateLastKnownCodecRequest {
    pub video_codec: Option<String>,
    pub audio_codec: Option<String>,
    pub container_format: Option<String>,
    pub video_width: Option<i32>,
    pub video_height: Option<i32>,
    pub framerate: Option<String>,
    pub bitrate: Option<i32>,
    pub video_bitrate: Option<i32>,
    pub audio_bitrate: Option<i32>,
    pub audio_channels: Option<i32>,
    pub audio_sample_rate: Option<i32>,
    pub probe_method: ProbeMethod,
    pub probe_source: Option<String>,
}

/// Channel with last known codec information
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct ChannelWithCodec {
    pub id: Uuid,
    pub source_id: Uuid,
    pub tvg_id: Option<String>,
    pub tvg_name: Option<String>,
    pub tvg_chno: Option<String>,
    pub channel_name: String,
    pub tvg_logo: Option<String>,
    pub tvg_shift: Option<String>,
    pub group_title: Option<String>,
    pub stream_url: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,

    // Last known codec information
    pub video_codec: Option<String>,
    pub audio_codec: Option<String>,
    pub video_width: Option<i32>,
    pub video_height: Option<i32>,
    /// Derived convenience (width x height)
    pub resolution: Option<String>,
    pub last_probed_at: Option<DateTime<Utc>>,
    pub probe_method: Option<ProbeMethod>,
}
