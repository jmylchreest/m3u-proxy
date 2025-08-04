//! Relay System Models
//!
//! This module contains all the data models for the FFmpeg relay system,
//! including profiles, configurations, and runtime status.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::str::FromStr;
use utoipa::ToSchema;
use uuid::Uuid;

/// FFmpeg relay profile containing reusable command configurations
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[schema(description = "FFmpeg relay profile with reusable transcoding configurations")]
pub struct RelayProfile {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    
    // TS-compatible codec selection
    pub video_codec: VideoCodec,
    pub audio_codec: AudioCodec,
    #[schema(example = "main")]
    pub video_profile: Option<String>, // "main", "main10", "high"
    #[schema(example = "medium")]
    pub video_preset: Option<String>,  // "fast", "medium", "slow"
    #[schema(example = 2000)]
    pub video_bitrate: Option<u32>,    // kbps
    #[schema(example = 128)]
    pub audio_bitrate: Option<u32>,    // kbps
    #[schema(example = 48000)]
    pub audio_sample_rate: Option<u32>, // Hz (e.g., 48000, 44100)
    #[schema(example = 2)]
    pub audio_channels: Option<u32>,    // Channel count (e.g., 1, 2, 6)
    
    // Hardware acceleration
    pub enable_hardware_acceleration: bool,
    #[schema(example = "auto")]
    pub preferred_hwaccel: Option<String>, // "auto", "vaapi", "nvenc", "qsv", "amf"
    
    // Manual override
    pub manual_args: Option<String>,   // User-defined args override
    
    // Container and streaming settings
    pub output_format: RelayOutputFormat,
    #[schema(example = 6)]
    pub segment_duration: Option<i32>,
    #[schema(example = 5)]
    pub max_segments: Option<i32>,
    #[schema(example = 30)]
    pub input_timeout: i32,
    
    // System flags
    pub is_system_default: bool,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Channel-specific relay configuration linking channels to profiles
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelRelayConfig {
    pub id: Uuid,
    pub proxy_id: Uuid,
    pub channel_id: Uuid,
    pub profile_id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub custom_args: Option<String>, // JSON array to override/extend profile args
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Resolved relay configuration combining profile and channel config
#[derive(Debug, Clone)]
pub struct ResolvedRelayConfig {
    pub config: ChannelRelayConfig,
    pub profile: RelayProfile,
    pub effective_args: Vec<String>, // Resolved FFmpeg arguments
    pub is_temporary: bool, // Flag to indicate if this is a temporary config
}

/// Runtime status of active relay processes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayRuntimeStatus {
    pub channel_relay_config_id: Uuid,
    pub process_id: Option<String>,
    pub sandbox_path: String,
    pub is_running: bool,
    pub started_at: Option<DateTime<Utc>>,
    pub client_count: i32,
    pub bytes_served: i64,
    pub error_message: Option<String>,
    pub last_heartbeat: Option<DateTime<Utc>>,
    pub updated_at: DateTime<Utc>,
}

/// Relay event for tracking lifecycle and metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayEvent {
    pub id: Option<i64>,
    pub config_id: Uuid,
    pub event_type: RelayEventType,
    pub details: Option<String>,
    pub timestamp: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

/// Types of relay events
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RelayEventType {
    Start,
    Stop,
    Error,
    ClientConnect,
    ClientDisconnect,
    FallbackActivated,
    FallbackDeactivated,
}

/// Stream health status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StreamHealth {
    Healthy,
    Degraded { error_count: u32 },
    Failed { last_error: String },
    Fallback { reason: String },
}

/// Error fallback configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorFallbackConfig {
    pub enabled: bool,
    pub error_image_path: Option<String>,
    pub fallback_timeout_seconds: u32,
    pub max_error_count: u32,
    pub error_threshold_seconds: u32,
}

impl ToString for RelayEventType {
    fn to_string(&self) -> String {
        match self {
            RelayEventType::Start => "start".to_string(),
            RelayEventType::Stop => "stop".to_string(),
            RelayEventType::Error => "error".to_string(),
            RelayEventType::ClientConnect => "client_connect".to_string(),
            RelayEventType::ClientDisconnect => "client_disconnect".to_string(),
            RelayEventType::FallbackActivated => "fallback_activated".to_string(),
            RelayEventType::FallbackDeactivated => "fallback_deactivated".to_string(),
        }
    }
}

impl FromStr for RelayEventType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "start" => Ok(RelayEventType::Start),
            "stop" => Ok(RelayEventType::Stop),
            "error" => Ok(RelayEventType::Error),
            "client_connect" => Ok(RelayEventType::ClientConnect),
            "client_disconnect" => Ok(RelayEventType::ClientDisconnect),
            "fallback_activated" => Ok(RelayEventType::FallbackActivated),
            "fallback_deactivated" => Ok(RelayEventType::FallbackDeactivated),
            _ => Err(format!("Unknown relay event type: {}", s)),
        }
    }
}

/// Video codec options (Transport Stream compatible)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, ToSchema)]
pub enum VideoCodec {
    H264,      // Most compatible
    H265,      // Better compression
    AV1,       // Next-gen (may need -strict experimental)
    MPEG2,     // Legacy compatibility
    MPEG4,     // Older standard
    Copy,      // Pass-through
}

/// Audio codec options (Transport Stream compatible)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, ToSchema)]
pub enum AudioCodec {
    AAC,       // Most common in TS
    MP3,       // Universal compatibility
    AC3,       // Dolby Digital
    EAC3,      // Enhanced AC3
    MPEG2Audio, // Legacy
    DTS,       // Surround sound
    Copy,      // Pass-through
}

/// FFmpeg output format types
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, ToSchema)]
pub enum RelayOutputFormat {
    TransportStream,
    HLS,
    Dash,
    Copy,
}

impl ToString for VideoCodec {
    fn to_string(&self) -> String {
        match self {
            VideoCodec::H264 => "h264".to_string(),
            VideoCodec::H265 => "h265".to_string(),
            VideoCodec::AV1 => "av1".to_string(),
            VideoCodec::MPEG2 => "mpeg2".to_string(),
            VideoCodec::MPEG4 => "mpeg4".to_string(),
            VideoCodec::Copy => "copy".to_string(),
        }
    }
}

impl FromStr for VideoCodec {
    type Err = String;
    
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "h264" => Ok(VideoCodec::H264),
            "h265" => Ok(VideoCodec::H265),
            "av1" => Ok(VideoCodec::AV1),
            "mpeg2" => Ok(VideoCodec::MPEG2),
            "mpeg4" => Ok(VideoCodec::MPEG4),
            "copy" => Ok(VideoCodec::Copy),
            _ => Err(format!("Unknown video codec: {}", s)),
        }
    }
}

impl ToString for AudioCodec {
    fn to_string(&self) -> String {
        match self {
            AudioCodec::AAC => "aac".to_string(),
            AudioCodec::MP3 => "mp3".to_string(),
            AudioCodec::AC3 => "ac3".to_string(),
            AudioCodec::EAC3 => "eac3".to_string(),
            AudioCodec::MPEG2Audio => "mpeg2audio".to_string(),
            AudioCodec::DTS => "dts".to_string(),
            AudioCodec::Copy => "copy".to_string(),
        }
    }
}

impl FromStr for AudioCodec {
    type Err = String;
    
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "aac" => Ok(AudioCodec::AAC),
            "mp3" => Ok(AudioCodec::MP3),
            "ac3" => Ok(AudioCodec::AC3),
            "eac3" => Ok(AudioCodec::EAC3),
            "mpeg2audio" => Ok(AudioCodec::MPEG2Audio),
            "dts" => Ok(AudioCodec::DTS),
            "copy" => Ok(AudioCodec::Copy),
            _ => Err(format!("Unknown audio codec: {}", s)),
        }
    }
}

impl ToString for RelayOutputFormat {
    fn to_string(&self) -> String {
        match self {
            RelayOutputFormat::TransportStream => "transport_stream".to_string(),
            RelayOutputFormat::HLS => "hls".to_string(),
            RelayOutputFormat::Dash => "dash".to_string(),
            RelayOutputFormat::Copy => "copy".to_string(),
        }
    }
}

impl FromStr for RelayOutputFormat {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "transport_stream" => Ok(RelayOutputFormat::TransportStream),
            "hls" => Ok(RelayOutputFormat::HLS),
            "dash" => Ok(RelayOutputFormat::Dash),
            "copy" => Ok(RelayOutputFormat::Copy),
            _ => Err(format!("Unknown relay output format: {}", s)),
        }
    }
}

/// Content returned by relay processes
pub enum RelayContent {
    Stream(Box<dyn futures::Stream<Item = Result<bytes::Bytes, std::io::Error>> + Send + Unpin>),
    Playlist(String),
    Segment(Vec<u8>),
}

/// Request to create a new relay profile
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateRelayProfileRequest {
    pub name: String,
    pub description: Option<String>,
    
    // Codec selection
    pub video_codec: VideoCodec,
    pub audio_codec: AudioCodec,
    pub video_profile: Option<String>,
    pub video_preset: Option<String>,
    pub video_bitrate: Option<u32>,
    pub audio_bitrate: Option<u32>,
    pub audio_sample_rate: Option<u32>,
    pub audio_channels: Option<u32>,
    
    // Hardware acceleration
    pub enable_hardware_acceleration: Option<bool>,
    pub preferred_hwaccel: Option<String>,
    
    // Manual override
    pub manual_args: Option<String>,
    
    // Container settings
    pub output_format: RelayOutputFormat,
    pub segment_duration: Option<i32>,
    pub max_segments: Option<i32>,
    pub input_timeout: Option<i32>,
    
    // System default flag (ignored by API handlers)
    pub is_system_default: Option<bool>,
}

/// Request to update an existing relay profile
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UpdateRelayProfileRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    
    // Codec selection
    pub video_codec: Option<VideoCodec>,
    pub audio_codec: Option<AudioCodec>,
    pub video_profile: Option<String>,
    pub video_preset: Option<String>,
    pub video_bitrate: Option<u32>,
    pub audio_bitrate: Option<u32>,
    pub audio_sample_rate: Option<u32>,
    pub audio_channels: Option<u32>,
    
    // Hardware acceleration
    pub enable_hardware_acceleration: Option<bool>,
    pub preferred_hwaccel: Option<String>,
    
    // Manual override
    pub manual_args: Option<String>,
    
    // Container settings
    pub output_format: Option<RelayOutputFormat>,
    pub segment_duration: Option<i32>,
    pub max_segments: Option<i32>,
    pub input_timeout: Option<i32>,
    pub is_active: Option<bool>,
    
    // System default flag (ignored by API handlers)
    pub is_system_default: Option<bool>,
}

/// Request to create a channel relay configuration
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateChannelRelayConfigRequest {
    pub profile_id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub custom_args: Option<Vec<String>>, // Will be serialized to JSON
}

/// Request to update a channel relay configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateChannelRelayConfigRequest {
    pub profile_id: Option<Uuid>,
    pub name: Option<String>,
    pub description: Option<String>,
    pub custom_args: Option<Vec<String>>,
    pub is_active: Option<bool>,
}

/// Relay metrics for monitoring
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayMetrics {
    pub total_active_relays: i64,
    pub total_clients: i64,
    pub total_bytes_upstream: i64,
    pub total_bytes_served: i64,
    pub active_processes: Vec<RelayProcessMetrics>,
}

/// Metrics for a specific relay process
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayProcessMetrics {
    pub config_id: Uuid,
    pub profile_name: String,
    pub channel_name: String,
    pub is_running: bool,
    pub client_count: i32,
    pub connected_clients: Vec<ConnectedClient>,
    pub bytes_received_upstream: i64,  // Bytes received from source
    pub bytes_delivered_downstream: i64, // Total bytes delivered to all clients
    pub uptime_seconds: Option<i64>,
    pub last_heartbeat: Option<DateTime<Utc>>,
    pub cpu_usage_percent: f64,
    pub memory_usage_mb: f64,
    pub process_id: Option<u32>,
    pub input_url: String,
    pub config_snapshot: String, // JSON string of the config used
    // Historical data for graphs
    pub cpu_history: Vec<CpuMemoryDataPoint>,
    pub memory_history: Vec<CpuMemoryDataPoint>,
    pub traffic_history: Vec<TrafficDataPoint>,
}

/// Metrics for a specific relay configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayConfigMetrics {
    pub is_running: bool,
    pub client_count: i32,
    pub bytes_served: i64,
    pub started_at: Option<DateTime<Utc>>,
    pub last_heartbeat: Option<DateTime<Utc>>,
    pub total_events: i64,
}

impl Default for RelayConfigMetrics {
    fn default() -> Self {
        Self {
            is_running: false,
            client_count: 0,
            bytes_served: 0,
            started_at: None,
            last_heartbeat: None,
            total_events: 0,
        }
    }
}

/// Hardware acceleration capabilities for FFmpeg
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HwAccelCapabilities {
    pub accelerators: Vec<HwAccelerator>,
    pub codecs: Vec<String>,
    pub support_matrix: HashMap<String, Vec<String>>, // accelerator -> supported codecs
}

impl Default for HwAccelCapabilities {
    fn default() -> Self {
        Self {
            accelerators: Vec::new(),
            codecs: Vec::new(),
            support_matrix: HashMap::new(),
        }
    }
}

/// Hardware accelerator information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HwAccelerator {
    pub name: String,
    pub device_type: String,
    pub available: bool,
    pub supported_codecs: Vec<String>,
}

/// Health status for relay processes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayHealth {
    pub total_processes: i32,
    pub healthy_processes: i32,
    pub unhealthy_processes: i32,
    pub processes: Vec<RelayProcessHealth>,
    pub system_load: f64,
    pub memory_usage_mb: f64,
    pub last_check: DateTime<Utc>,
    pub ffmpeg_available: bool,
    pub ffmpeg_version: Option<String>,
    pub ffmpeg_command: String,
    pub hwaccel_available: bool,
    pub hwaccel_capabilities: HwAccelCapabilities,
}

/// Health status for individual relay process
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayProcessHealth {
    pub config_id: Uuid,
    pub profile_name: String,
    pub channel_name: String,
    pub status: RelayHealthStatus,
    pub uptime_seconds: i64,
    pub client_count: i32,
    pub memory_usage_mb: f64,
    pub cpu_usage_percent: f64,
    pub last_heartbeat: DateTime<Utc>,
    pub error_count: i32,
    pub restart_count: i32,
}

/// Health status enumeration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RelayHealthStatus {
    #[serde(rename = "healthy")]
    Healthy,
    #[serde(rename = "unhealthy")]
    Unhealthy,
    #[serde(rename = "starting")]
    Starting,
    #[serde(rename = "stopping")]
    Stopping,
    #[serde(rename = "failed")]
    Failed,
}

impl Default for RelayHealthStatus {
    fn default() -> Self {
        RelayHealthStatus::Healthy
    }
}

/// Client information for relay sessions
#[derive(Debug, Clone)]
pub struct ClientInfo {
    pub ip: String,
    pub user_agent: Option<String>,
    pub referer: Option<String>,
}

/// Connected client information for metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectedClient {
    pub id: Uuid,
    pub ip: String,
    pub user_agent: Option<String>,
    pub connected_at: DateTime<Utc>,
    pub bytes_served: u64,
    pub last_activity: DateTime<Utc>,
}

/// Data point for CPU/Memory usage graphs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CpuMemoryDataPoint {
    pub timestamp: DateTime<Utc>,
    pub value: f64, // Percentage for CPU (0-100), MB for memory
}

/// Data point for traffic graphs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrafficDataPoint {
    pub timestamp: DateTime<Utc>,
    pub bytes_in: u64,  // Bytes received from upstream
    pub bytes_out: u64, // Bytes delivered to clients
}

impl RelayProfile {

    /// Create a new relay profile with validation
    pub fn new(request: CreateRelayProfileRequest) -> Result<Self, String> {
        // Validate Transport Stream compatibility
        Self::validate_ts_compatibility(&request.video_codec, &request.audio_codec)?;
        
        // Validate manual args if provided
        if let Some(ref manual_args) = request.manual_args {
            if let Ok(args_vec) = serde_json::from_str::<Vec<String>>(manual_args) {
                Self::validate_ffmpeg_args(&args_vec)?;
            }
        }
        
        Ok(Self {
            id: Uuid::new_v4(),
            name: request.name,
            description: request.description,
            
            // Codec settings
            video_codec: request.video_codec,
            audio_codec: request.audio_codec,
            video_profile: request.video_profile,
            video_preset: request.video_preset,
            video_bitrate: request.video_bitrate,
            audio_bitrate: request.audio_bitrate,
            audio_sample_rate: request.audio_sample_rate,
            audio_channels: request.audio_channels,
            
            // Hardware acceleration
            enable_hardware_acceleration: request.enable_hardware_acceleration.unwrap_or(false),
            preferred_hwaccel: request.preferred_hwaccel,
            
            // Manual override
            manual_args: request.manual_args,
            
            // Container settings
            output_format: request.output_format,
            segment_duration: request.segment_duration,
            max_segments: request.max_segments,
            input_timeout: request.input_timeout.unwrap_or(30),
            
            // System flags
            is_system_default: false,
            is_active: true,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        })
    }

    /// Validate Transport Stream compatibility
    pub fn validate_ts_compatibility(video_codec: &VideoCodec, audio_codec: &AudioCodec) -> Result<(), String> {
        // All our defined codecs are TS-compatible, but we validate anyway
        match video_codec {
            VideoCodec::H264 | VideoCodec::H265 | VideoCodec::AV1 | 
            VideoCodec::MPEG2 | VideoCodec::MPEG4 | VideoCodec::Copy => {},
        }
        
        match audio_codec {
            AudioCodec::AAC | AudioCodec::MP3 | AudioCodec::AC3 | 
            AudioCodec::EAC3 | AudioCodec::MPEG2Audio | AudioCodec::DTS | 
            AudioCodec::Copy => {},
        }
        
        Ok(())
    }
    
    /// Validate FFmpeg arguments for security and correctness (legacy support)
    pub fn validate_ffmpeg_args(args: &[String]) -> Result<(), String> {
        // Basic validation to prevent command injection
        for arg in args {
            if arg.contains("&&") || arg.contains("||") || arg.contains(";") || arg.contains("`") {
                return Err(format!("Potentially dangerous argument: {}", arg));
            }
        }
        
        Ok(())
    }
    
    /// Get hardware acceleration encoder name for video codec
    pub fn get_hwaccel_encoder(&self, hwaccel: &str) -> Option<String> {
        match (hwaccel, &self.video_codec) {
            ("vaapi", VideoCodec::H264) => Some("h264_vaapi".to_string()),
            ("vaapi", VideoCodec::H265) => Some("hevc_vaapi".to_string()),
            ("vaapi", VideoCodec::AV1) => Some("av1_vaapi".to_string()),
            ("vaapi", VideoCodec::MPEG2) => Some("mpeg2_vaapi".to_string()),
            
            ("nvenc", VideoCodec::H264) => Some("h264_nvenc".to_string()),
            ("nvenc", VideoCodec::H265) => Some("hevc_nvenc".to_string()),
            ("nvenc", VideoCodec::AV1) => Some("av1_nvenc".to_string()),
            
            ("qsv", VideoCodec::H264) => Some("h264_qsv".to_string()),
            ("qsv", VideoCodec::H265) => Some("hevc_qsv".to_string()),
            ("qsv", VideoCodec::AV1) => Some("av1_qsv".to_string()),
            ("qsv", VideoCodec::MPEG2) => Some("mpeg2_qsv".to_string()),
            
            ("amf", VideoCodec::H264) => Some("h264_amf".to_string()),
            ("amf", VideoCodec::H265) => Some("hevc_amf".to_string()),
            ("amf", VideoCodec::AV1) => Some("av1_amf".to_string()),
            
            _ => None,
        }
    }
    
    /// Get software encoder name for video codec
    pub fn get_software_encoder(&self) -> String {
        match self.video_codec {
            VideoCodec::H264 => "libx264".to_string(),
            VideoCodec::H265 => "libx265".to_string(),
            VideoCodec::AV1 => "libaom-av1".to_string(),
            VideoCodec::MPEG2 => "mpeg2video".to_string(),
            VideoCodec::MPEG4 => "libxvid".to_string(),
            VideoCodec::Copy => "copy".to_string(),
        }
    }
    
    /// Get audio encoder name
    pub fn get_audio_encoder(&self) -> String {
        match self.audio_codec {
            AudioCodec::AAC => "aac".to_string(),
            AudioCodec::MP3 => "libmp3lame".to_string(),
            AudioCodec::AC3 => "ac3".to_string(),
            AudioCodec::EAC3 => "eac3".to_string(),
            AudioCodec::MPEG2Audio => "mp2".to_string(),
            AudioCodec::DTS => "dca".to_string(),
            AudioCodec::Copy => "copy".to_string(),
        }
    }
}

impl ChannelRelayConfig {
    /// Create a new channel relay configuration
    pub fn new(
        proxy_id: Uuid,
        channel_id: Uuid,
        request: CreateChannelRelayConfigRequest,
    ) -> Result<Self, String> {
        // Validate custom args if provided
        if let Some(ref custom_args) = request.custom_args {
            RelayProfile::validate_ffmpeg_args(custom_args)?;
        }
        
        let custom_args_json = if let Some(args) = request.custom_args {
            Some(serde_json::to_string(&args)
                .map_err(|e| format!("Invalid custom arguments: {}", e))?)
        } else {
            None
        };
        
        Ok(Self {
            id: Uuid::new_v4(),
            proxy_id,
            channel_id,
            profile_id: request.profile_id,
            name: request.name,
            description: request.description,
            custom_args: custom_args_json,
            is_active: true,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        })
    }

    /// Parse custom arguments from JSON string
    pub fn parse_custom_args(&self) -> Result<Option<Vec<String>>, serde_json::Error> {
        if let Some(ref args_json) = self.custom_args {
            Ok(Some(serde_json::from_str(args_json)?))
        } else {
            Ok(None)
        }
    }
}

impl ResolvedRelayConfig {
    /// Create a resolved configuration by combining profile and channel config
    pub fn new(config: ChannelRelayConfig, profile: RelayProfile) -> Result<Self, String> {
        Self::new_with_temporary_flag(config, profile, false)
    }
    
    /// Create a resolved configuration with a temporary flag
    pub fn new_with_temporary_flag(config: ChannelRelayConfig, profile: RelayProfile, is_temporary: bool) -> Result<Self, String> {
        // For codec-based profiles, we generate them dynamically in generate_ffmpeg_command()
        let effective_args = {
            // New codec-based profile - args generated dynamically
            Vec::new()
        };
        
        Ok(Self {
            config,
            profile,
            effective_args,
            is_temporary,
        })
    }

    /// Generate complete FFmpeg command with hardware acceleration support
    pub fn generate_ffmpeg_command(
        &self,
        input_url: &str,
        output_path: &str,
        hwaccel_caps: &HwAccelCapabilities,
    ) -> Vec<String> {
        // Use traditional hardcoded method for backward compatibility
        self.generate_ffmpeg_command_with_mapping(input_url, output_path, hwaccel_caps, None)
    }
    
    /// Generate complete FFmpeg command with dynamic stream mapping
    pub fn generate_ffmpeg_command_with_mapping(
        &self,
        input_url: &str,
        output_path: &str,
        hwaccel_caps: &HwAccelCapabilities,
        mapping_strategy: Option<&crate::services::StreamMappingStrategy>,
    ) -> Vec<String> {
        // If this is a legacy profile, use the old method
        if !self.effective_args.is_empty() {
            return self.resolve_template_variables(input_url, output_path);
        }
        
        let mut args = Vec::new();
        
        // Hardware acceleration setup (if enabled)
        if self.profile.enable_hardware_acceleration {
            if let Some(hwaccel_args) = self.generate_hwaccel_args(hwaccel_caps) {
                args.extend(hwaccel_args);
            }
        }
        
        // Input with analyzeduration and probesize for better stream analysis
        args.extend([
            "-analyzeduration".to_string(), "10000000".to_string(),  // 10 seconds
            "-probesize".to_string(), "10000000".to_string(),        // 10MB
            "-i".to_string(), input_url.to_string()
        ]);
        
        // Dynamic stream mapping based on probe results or fallback to hardcoded
        if let Some(strategy) = mapping_strategy {
            // Use dynamic mapping based on probe results
            if let Some(ref video_mapping) = strategy.video_mapping {
                args.extend(["-map".to_string(), video_mapping.clone()]);
            }
            if let Some(ref audio_mapping) = strategy.audio_mapping {
                args.extend(["-map".to_string(), audio_mapping.clone()]);
            }
        } else {
            // Fallback to hardcoded mapping (legacy behavior)
            args.extend(["-map".to_string(), "0:v:0".to_string()]); // First video stream
            args.extend(["-map".to_string(), "0:a:0".to_string()]); // First audio stream
        }
        
        // Video codec - use copy if strategy suggests it or handle normally
        if let Some(strategy) = mapping_strategy {
            if strategy.video_mapping.is_some() {
                args.push("-c:v".to_string());
                if strategy.video_copy {
                    args.push("copy".to_string());
                } else if self.profile.enable_hardware_acceleration {
                    if let Some(hw_encoder) = self.get_hwaccel_video_encoder(hwaccel_caps) {
                        args.push(hw_encoder);
                    } else {
                        args.push(self.profile.get_software_encoder());
                    }
                } else {
                    args.push(self.profile.get_software_encoder());
                }
            }
        } else {
            // Legacy behavior
            args.push("-c:v".to_string());
            if self.profile.enable_hardware_acceleration {
                if let Some(hw_encoder) = self.get_hwaccel_video_encoder(hwaccel_caps) {
                    args.push(hw_encoder);
                } else {
                    args.push(self.profile.get_software_encoder());
                }
            } else {
                args.push(self.profile.get_software_encoder());
            }
        }
        
        // Hardware acceleration video filters (only when encoding, not copying)
        let should_apply_hwaccel_filters = if let Some(strategy) = mapping_strategy {
            self.profile.enable_hardware_acceleration && strategy.video_mapping.is_some() && !strategy.video_copy
        } else {
            self.profile.enable_hardware_acceleration && self.profile.video_codec != VideoCodec::Copy
        };
        
        if should_apply_hwaccel_filters {
            if let Some(hwaccel_filters) = self.generate_hwaccel_video_filters(hwaccel_caps) {
                args.extend(hwaccel_filters);
            }
        }
        
        // Video settings - only apply encoding parameters if we're not copying
        let should_apply_video_settings = if let Some(strategy) = mapping_strategy {
            strategy.video_mapping.is_some() && !strategy.video_copy
        } else {
            self.profile.video_codec != VideoCodec::Copy
        };
        
        if should_apply_video_settings {
            // Use optimal bitrate from strategy or profile default
            let target_bitrate = mapping_strategy
                .and_then(|s| s.target_video_bitrate)
                .or(self.profile.video_bitrate);
                
            if let Some(bitrate) = target_bitrate {
                args.extend(["-b:v".to_string(), format!("{}k", bitrate)]);
            }
            if let Some(ref preset) = self.profile.video_preset {
                args.extend(["-preset".to_string(), preset.clone()]);
            }
            if let Some(ref profile) = self.profile.video_profile {
                args.extend(["-profile:v".to_string(), profile.clone()]);
            }
        }
        
        // Audio codec - use copy if strategy suggests it or handle normally
        if let Some(strategy) = mapping_strategy {
            if strategy.audio_mapping.is_some() {
                args.push("-c:a".to_string());
                if strategy.audio_copy {
                    args.push("copy".to_string());
                } else {
                    args.push(self.profile.get_audio_encoder());
                    
                    // Use optimal bitrate from strategy or profile default
                    let target_bitrate = strategy.target_audio_bitrate
                        .or(self.profile.audio_bitrate);
                    
                    if let Some(bitrate) = target_bitrate {
                        args.extend(["-b:a".to_string(), format!("{}k", bitrate)]);
                    }
                    
                    // Set explicit audio parameters to avoid codec parameter issues when transcoding
                    let sample_rate = self.profile.audio_sample_rate.unwrap_or(48000);
                    let channels = self.profile.audio_channels.unwrap_or(2);
                    args.extend(["-ar".to_string(), sample_rate.to_string()]);
                    args.extend(["-ac".to_string(), channels.to_string()]);
                }
            }
        } else {
            // Legacy behavior
            args.push("-c:a".to_string());
            args.push(self.profile.get_audio_encoder());
            
            // Audio settings - only apply encoding parameters if we're not copying
            if self.profile.audio_codec != AudioCodec::Copy {
                if let Some(bitrate) = self.profile.audio_bitrate {
                    args.extend(["-b:a".to_string(), format!("{}k", bitrate)]);
                }
                // Set explicit audio parameters to avoid codec parameter issues when transcoding
                let sample_rate = self.profile.audio_sample_rate.unwrap_or(48000);
                let channels = self.profile.audio_channels.unwrap_or(2);
                args.extend(["-ar".to_string(), sample_rate.to_string()]);
                args.extend(["-ac".to_string(), channels.to_string()]);
            }
        }
        
        // Transport Stream specific settings
        args.extend([
            "-f".to_string(), "mpegts".to_string(),
            "-mpegts_copyts".to_string(), "1".to_string(),
            "-avoid_negative_ts".to_string(), "disabled".to_string(),
            "-mpegts_start_pid".to_string(), "256".to_string(),  // Start PID for streams
            "-mpegts_pmt_start_pid".to_string(), "4096".to_string(),  // Different PID for PMT
        ]);
        
        // Output to stdout for cyclic buffer consumption
        args.extend(["-y".to_string(), "pipe:1".to_string()]);
        
        // Apply manual args override (if provided)
        if let Some(ref manual_args) = self.profile.manual_args {
            if let Ok(manual_vec) = serde_json::from_str::<Vec<String>>(manual_args) {
                args.extend(manual_vec);
            }
        }
        
        args
    }
    
    /// Create a JSON snapshot of the configuration for reference
    pub fn create_config_snapshot(&self, input_url: &str) -> String {
        let snapshot = serde_json::json!({
            "config_id": self.config.id,
            "profile_name": self.profile.name,
            "channel_id": self.config.channel_id,
            "input_url": input_url,
            "video_codec": self.profile.video_codec.to_string(),
            "audio_codec": self.profile.audio_codec.to_string(),
            "video_bitrate": self.profile.video_bitrate,
            "audio_bitrate": self.profile.audio_bitrate,
            "hardware_acceleration": self.profile.enable_hardware_acceleration,
            "preferred_hwaccel": self.profile.preferred_hwaccel,
            "output_format": self.profile.output_format.to_string(),
            "is_temporary": self.is_temporary,
            "created_at": chrono::Utc::now()
        });
        
        serde_json::to_string(&snapshot).unwrap_or_else(|_| "{}".to_string())
    }
    
    /// Resolve template variables in FFmpeg arguments (legacy support)
    pub fn resolve_template_variables(
        &self,
        input_url: &str,
        output_path: &str,
    ) -> Vec<String> {
        let mut resolved_args = Vec::new();
        
        for arg in &self.effective_args {
            let resolved = arg
                .replace("{input_url}", input_url)
                .replace("{output_path}", output_path)
                .replace("{segment_duration}", &self.profile.segment_duration.unwrap_or(30).to_string())
                .replace("{max_segments}", &self.profile.max_segments.unwrap_or(4).to_string())
                .replace("{input_timeout}", &self.profile.input_timeout.to_string());
            
            resolved_args.push(resolved);
        }
        
        resolved_args
    }
    
    /// Generate hardware acceleration arguments (input setup only)
    pub fn generate_hwaccel_args(&self, hwaccel_caps: &HwAccelCapabilities) -> Option<Vec<String>> {
        // Determine which hwaccel to use
        let hwaccel = if let Some(ref preferred) = self.profile.preferred_hwaccel {
            if preferred == "auto" {
                self.select_best_hwaccel(hwaccel_caps)?
            } else {
                preferred.clone()
            }
        } else {
            self.select_best_hwaccel(hwaccel_caps)?
        };
        
        // Check if this hwaccel supports the selected video codec
        if !self.hwaccel_supports_codec(&hwaccel, hwaccel_caps) {
            return None;
        }
        
        let mut args = Vec::new();
        
        // Add hardware device initialization
        args.extend(["-init_hw_device".to_string(), hwaccel.clone()]);
        
        // Add hardware acceleration flag
        args.extend(["-hwaccel".to_string(), hwaccel.clone()]);
        
        // NOTE: Video filters (-vf) are now added after the input and codec specifications
        // in the generate_hwaccel_video_filters method
        
        Some(args)
    }
    
    /// Generate hardware acceleration video filters (for use after codec specification)
    pub fn generate_hwaccel_video_filters(&self, hwaccel_caps: &HwAccelCapabilities) -> Option<Vec<String>> {
        // Determine which hwaccel to use
        let hwaccel = if let Some(ref preferred) = self.profile.preferred_hwaccel {
            if preferred == "auto" {
                self.select_best_hwaccel(hwaccel_caps)?
            } else {
                preferred.clone()
            }
        } else {
            self.select_best_hwaccel(hwaccel_caps)?
        };
        
        // Check if this hwaccel supports the selected video codec
        if !self.hwaccel_supports_codec(&hwaccel, hwaccel_caps) {
            return None;
        }
        
        let mut args = Vec::new();
        
        // Add video filter for hardware upload (placed after input and codec specs)
        args.push("-vf".to_string());
        match hwaccel.as_str() {
            "vaapi" => args.push("format=nv12,hwupload".to_string()),
            "nvenc" => args.push("format=nv12,hwupload_cuda".to_string()),
            "qsv" => args.push("format=nv12,hwupload=extra_hw_frames=64".to_string()),
            _ => args.push("format=nv12,hwupload".to_string()),
        }
        
        Some(args)
    }
    
    /// Get hardware-accelerated video encoder
    pub fn get_hwaccel_video_encoder(&self, hwaccel_caps: &HwAccelCapabilities) -> Option<String> {
        let hwaccel = if let Some(ref preferred) = self.profile.preferred_hwaccel {
            if preferred == "auto" {
                self.select_best_hwaccel(hwaccel_caps)?
            } else {
                preferred.clone()
            }
        } else {
            self.select_best_hwaccel(hwaccel_caps)?
        };
        
        self.profile.get_hwaccel_encoder(&hwaccel)
    }
    
    /// Select the best available hardware accelerator
    fn select_best_hwaccel(&self, hwaccel_caps: &HwAccelCapabilities) -> Option<String> {
        // Priority order: nvenc > qsv > vaapi > amf
        let priority_order = ["nvenc", "qsv", "vaapi", "amf"];
        
        for hwaccel in &priority_order {
            if self.hwaccel_supports_codec(hwaccel, hwaccel_caps) {
                return Some(hwaccel.to_string());
            }
        }
        
        None
    }
    
    /// Check if hardware accelerator supports the profile's video codec
    fn hwaccel_supports_codec(&self, hwaccel: &str, hwaccel_caps: &HwAccelCapabilities) -> bool {
        let codec_name = match self.profile.video_codec {
            VideoCodec::H264 => "h264",
            VideoCodec::H265 => "hevc",
            VideoCodec::AV1 => "av1",
            VideoCodec::MPEG2 => "mpeg2",
            VideoCodec::MPEG4 => "mpeg4",
            VideoCodec::Copy => return false, // No hwaccel for copy
        };
        
        if let Some(supported_codecs) = hwaccel_caps.support_matrix.get(hwaccel) {
            supported_codecs.contains(&codec_name.to_string())
        } else {
            false
        }
    }
}




/// Error types for relay operations
#[derive(Debug, thiserror::Error)]
pub enum RelayError {
    #[error("Relay configuration not found: {0}")]
    ConfigNotFound(Uuid),
    
    #[error("Relay profile not found: {0}")]
    ProfileNotFound(Uuid),
    
    #[error("Relay process not found: {0}")]
    ProcessNotFound(Uuid),
    
    #[error("Invalid FFmpeg argument: {0}")]
    InvalidArgument(String),
    
    #[error("Unsupported output format: {0:?}")]
    UnsupportedFormat(RelayOutputFormat),
    
    #[error("Invalid path: {0}")]
    InvalidPath(String),
    
    #[error("Segment not found: {0}")]
    SegmentNotFound(String),
    
    #[error("FFmpeg process failed: {0}")]
    ProcessFailed(String),
    
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),
    
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    
    #[error("UUID error: {0}")]
    Uuid(#[from] uuid::Error),
}