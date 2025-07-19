//! Stream Probing Service
//!
//! This module provides functionality to probe input streams and determine
//! their characteristics before generating FFmpeg commands.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::process::Stdio;
use std::time::Duration;
use tokio::process::Command;
use tracing::{debug, warn};

/// Information about a stream detected by FFprobe
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamInfo {
    pub index: u32,
    pub codec_type: String,  // "video", "audio", "subtitle", etc.
    pub codec_name: String,  // "h264", "aac", etc.
    pub codec_tag_string: Option<String>,
    pub duration: Option<f64>,
    pub bit_rate: Option<u64>,
    pub width: Option<u32>,   // video only
    pub height: Option<u32>,  // video only
    pub r_frame_rate: Option<String>, // video only
    pub sample_rate: Option<u32>, // audio only
    pub channels: Option<u32>,    // audio only
    pub channel_layout: Option<String>, // audio only
}

/// Complete probe result for an input stream
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProbeResult {
    pub streams: Vec<StreamInfo>,
    pub format_name: Option<String>,
    pub duration: Option<f64>,
    pub bit_rate: Option<u64>,
    pub has_video: bool,
    pub has_audio: bool,
    pub video_streams: Vec<StreamInfo>,
    pub audio_streams: Vec<StreamInfo>,
}

/// Stream mapping strategy based on probe results
#[derive(Debug, Clone)]
pub struct StreamMappingStrategy {
    pub video_mapping: Option<String>,  // e.g., "0:v:0" or None if no video
    pub audio_mapping: Option<String>,  // e.g., "0:a:0" or None if no audio
    pub video_copy: bool,               // true if video should be copied
    pub audio_copy: bool,               // true if audio should be copied
    pub target_audio_bitrate: Option<u32>, // optimal audio bitrate (never higher than input)
    pub target_video_bitrate: Option<u32>, // optimal video bitrate (never higher than input)
}

/// Service for probing input streams
pub struct StreamProber {
    ffprobe_command: String,
    probe_timeout: Duration,
}

impl StreamProber {
    pub fn new(ffprobe_command: Option<String>) -> Self {
        Self {
            ffprobe_command: ffprobe_command.unwrap_or_else(|| "ffprobe".to_string()),
            probe_timeout: Duration::from_secs(10),
        }
    }

    /// Probe an input URL to determine stream characteristics
    pub async fn probe_input(&self, input_url: &str) -> Result<ProbeResult> {
        debug!("Probing input stream: {}", input_url);
        
        let mut cmd = Command::new(&self.ffprobe_command);
        cmd.args([
            "-v", "quiet",
            "-print_format", "json",
            "-show_streams",
            "-show_format",
            "-analyzeduration", "5000000",  // 5 seconds
            "-probesize", "5000000",        // 5MB
            input_url
        ]);
        
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());
        
        let output = tokio::time::timeout(self.probe_timeout, cmd.output()).await
            .map_err(|_| anyhow::anyhow!("FFprobe timeout after {:?}", self.probe_timeout))?
            .map_err(|e| anyhow::anyhow!("Failed to execute ffprobe: {}", e))?;
        
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            warn!("FFprobe failed for {}: {}", input_url, stderr);
            return Err(anyhow::anyhow!("FFprobe failed: {}", stderr));
        }
        
        let stdout = String::from_utf8_lossy(&output.stdout);
        let probe_data: serde_json::Value = serde_json::from_str(&stdout)
            .map_err(|e| anyhow::anyhow!("Failed to parse ffprobe output: {}", e))?;
        
        self.parse_probe_result(probe_data)
    }
    
    /// Parse ffprobe JSON output into our ProbeResult structure
    fn parse_probe_result(&self, data: serde_json::Value) -> Result<ProbeResult> {
        let mut streams = Vec::new();
        let mut video_streams = Vec::new();
        let mut audio_streams = Vec::new();
        
        if let Some(streams_array) = data["streams"].as_array() {
            for (index, stream) in streams_array.iter().enumerate() {
                let codec_type = stream["codec_type"].as_str().unwrap_or("unknown").to_string();
                let codec_name = stream["codec_name"].as_str().unwrap_or("unknown").to_string();
                
                let stream_info = StreamInfo {
                    index: index as u32,
                    codec_type: codec_type.clone(),
                    codec_name: codec_name.clone(),
                    codec_tag_string: stream["codec_tag_string"].as_str().map(|s| s.to_string()),
                    duration: stream["duration"].as_str().and_then(|s| s.parse().ok()),
                    bit_rate: stream["bit_rate"].as_str().and_then(|s| s.parse().ok()),
                    width: stream["width"].as_u64().map(|v| v as u32),
                    height: stream["height"].as_u64().map(|v| v as u32),
                    r_frame_rate: stream["r_frame_rate"].as_str().map(|s| s.to_string()),
                    sample_rate: stream["sample_rate"].as_str().and_then(|s| s.parse().ok()),
                    channels: stream["channels"].as_u64().map(|v| v as u32),
                    channel_layout: stream["channel_layout"].as_str().map(|s| s.to_string()),
                };
                
                match codec_type.as_str() {
                    "video" => video_streams.push(stream_info.clone()),
                    "audio" => audio_streams.push(stream_info.clone()),
                    _ => {}
                }
                
                streams.push(stream_info);
            }
        }
        
        let format = data["format"].as_object();
        let format_name = format
            .and_then(|f| f["format_name"].as_str())
            .map(|s| s.to_string());
        let duration = format
            .and_then(|f| f["duration"].as_str())
            .and_then(|s| s.parse().ok());
        let bit_rate = format
            .and_then(|f| f["bit_rate"].as_str())
            .and_then(|s| s.parse().ok());
        
        Ok(ProbeResult {
            has_video: !video_streams.is_empty(),
            has_audio: !audio_streams.is_empty(),
            streams,
            video_streams,
            audio_streams,
            format_name,
            duration,
            bit_rate,
        })
    }
    
    /// Generate optimal mapping strategy based on probe results and target profile
    pub fn generate_mapping_strategy(
        &self,
        probe_result: &ProbeResult,
        target_video_codec: &str,
        target_audio_codec: &str,
        target_video_bitrate: Option<u32>,
        target_audio_bitrate: Option<u32>,
    ) -> StreamMappingStrategy {
        let mut strategy = StreamMappingStrategy {
            video_mapping: None,
            audio_mapping: None,
            video_copy: false,
            audio_copy: false,
            target_audio_bitrate,
            target_video_bitrate,
        };
        
        // Handle video streams
        if let Some(video_stream) = probe_result.video_streams.first() {
            strategy.video_mapping = Some(format!("0:v:0"));
            
            // Decide if we should copy video
            strategy.video_copy = should_copy_video_stream(
                &video_stream.codec_name,
                target_video_codec,
                video_stream.bit_rate,
                target_video_bitrate,
            );
            
            // Don't exceed input video bitrate
            if let Some(input_bitrate) = video_stream.bit_rate {
                let input_kbps = (input_bitrate / 1000) as u32;
                if let Some(target) = strategy.target_video_bitrate {
                    strategy.target_video_bitrate = Some(target.min(input_kbps));
                }
            }
        }
        
        // Handle audio streams
        if let Some(audio_stream) = probe_result.audio_streams.first() {
            strategy.audio_mapping = Some(format!("0:a:0"));
            
            // Decide if we should copy audio
            strategy.audio_copy = should_copy_audio_stream(
                &audio_stream.codec_name,
                target_audio_codec,
                audio_stream.bit_rate,
                target_audio_bitrate,
            );
            
            // Don't exceed input audio bitrate
            if let Some(input_bitrate) = audio_stream.bit_rate {
                let input_kbps = (input_bitrate / 1000) as u32;
                if let Some(target) = strategy.target_audio_bitrate {
                    strategy.target_audio_bitrate = Some(target.min(input_kbps));
                }
            }
        }
        
        debug!("Generated mapping strategy: video={:?}, audio={:?}, video_copy={}, audio_copy={}",
               strategy.video_mapping, strategy.audio_mapping, 
               strategy.video_copy, strategy.audio_copy);
        
        strategy
    }
}

/// Determine if video stream should be copied instead of transcoded
fn should_copy_video_stream(
    input_codec: &str,
    target_codec: &str,
    input_bitrate: Option<u64>,
    target_bitrate: Option<u32>,
) -> bool {
    // If target is copy, always copy
    if target_codec == "copy" {
        return true;
    }
    
    // If codecs match and no transcoding needed
    let input_normalized = normalize_codec_name(input_codec);
    let target_normalized = normalize_codec_name(target_codec);
    
    if input_normalized == target_normalized {
        // Check if bitrate is acceptable
        if let (Some(input_br), Some(target_br)) = (input_bitrate, target_bitrate) {
            let input_kbps = (input_br / 1000) as u32;
            // Copy if input bitrate is within 20% of target or lower
            return input_kbps <= (target_br as f32 * 1.2) as u32;
        }
        return true;
    }
    
    false
}

/// Determine if audio stream should be copied instead of transcoded
fn should_copy_audio_stream(
    input_codec: &str,
    target_codec: &str,
    input_bitrate: Option<u64>,
    target_bitrate: Option<u32>,
) -> bool {
    // If target is copy, always copy
    if target_codec == "copy" {
        return true;
    }
    
    // If codecs match and no transcoding needed
    let input_normalized = normalize_codec_name(input_codec);
    let target_normalized = normalize_codec_name(target_codec);
    
    if input_normalized == target_normalized {
        // Check if bitrate is acceptable - never upconvert audio
        if let (Some(input_br), Some(target_br)) = (input_bitrate, target_bitrate) {
            let input_kbps = (input_br / 1000) as u32;
            // Copy if input bitrate is equal or lower than target
            return input_kbps <= target_br;
        }
        return true;
    }
    
    false
}

/// Normalize codec names for comparison
fn normalize_codec_name(codec: &str) -> String {
    match codec.to_lowercase().as_str() {
        "h264" | "avc" | "avc1" => "h264".to_string(),
        "h265" | "hevc" | "hev1" => "h265".to_string(),
        "aac" | "mp4a" => "aac".to_string(),
        "mp3" | "mp3float" => "mp3".to_string(),
        "ac3" | "ac-3" => "ac3".to_string(),
        "eac3" | "eac-3" => "eac3".to_string(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_codec_name() {
        assert_eq!(normalize_codec_name("h264"), "h264");
        assert_eq!(normalize_codec_name("avc"), "h264");
        assert_eq!(normalize_codec_name("H264"), "h264");
        assert_eq!(normalize_codec_name("AAC"), "aac");
        assert_eq!(normalize_codec_name("mp4a"), "aac");
    }

    #[test]
    fn test_should_copy_video_stream() {
        // Same codec, no bitrate constraints
        assert!(should_copy_video_stream("h264", "h264", None, None));
        
        // Same codec, bitrate within range
        assert!(should_copy_video_stream("h264", "h264", Some(2000000), Some(2500)));
        
        // Same codec, bitrate too high
        assert!(!should_copy_video_stream("h264", "h264", Some(4000000), Some(2000)));
        
        // Different codecs
        assert!(!should_copy_video_stream("h264", "h265", Some(2000000), Some(2500)));
        
        // Copy target
        assert!(should_copy_video_stream("h264", "copy", Some(2000000), Some(2500)));
    }

    #[test]
    fn test_should_copy_audio_stream() {
        // Same codec, no bitrate constraints
        assert!(should_copy_audio_stream("aac", "aac", None, None));
        
        // Same codec, bitrate within range
        assert!(should_copy_audio_stream("aac", "aac", Some(128000), Some(128)));
        
        // Same codec, would upconvert (should not copy)
        assert!(!should_copy_audio_stream("aac", "aac", Some(96000), Some(128)));
        
        // Different codecs
        assert!(!should_copy_audio_stream("aac", "mp3", Some(128000), Some(128)));
        
        // Copy target
        assert!(should_copy_audio_stream("aac", "copy", Some(128000), Some(128)));
    }
}