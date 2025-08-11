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

/// Error information from ffprobe
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProbeError {
    pub code: Option<i32>,
    pub string: Option<String>,
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
    pub error: Option<ProbeError>,
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
            "-show_error",
            "-show_entries", "stream=index,codec_type,codec_name,codec_tag_string,duration,bit_rate,width,height,r_frame_rate,sample_rate,channels,channel_layout:format=format_name,duration,bit_rate",
            "-analyzeduration", "5000000",  // 5 seconds
            "-probesize", "5000000",        // 5MB
            input_url
        ]);
        
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());
        
        let output = tokio::time::timeout(self.probe_timeout, cmd.output()).await
            .map_err(|_| anyhow::anyhow!("FFprobe timeout after {:?}", self.probe_timeout))?
            .map_err(|e| anyhow::anyhow!("Failed to execute ffprobe: {}", e))?;
        
        let stdout = String::from_utf8_lossy(&output.stdout);
        
        // Parse JSON output even if command failed - ffprobe may still provide structured error info
        let probe_data: serde_json::Value = if stdout.trim().is_empty() {
            // If no JSON output, create minimal error structure
            let stderr = String::from_utf8_lossy(&output.stderr);
            serde_json::json!({
                "error": {
                    "code": output.status.code().unwrap_or(-1),
                    "string": stderr.to_string()
                }
            })
        } else {
            serde_json::from_str(&stdout)
                .map_err(|e| anyhow::anyhow!("Failed to parse ffprobe output: {}", e))?
        };
        
        let result = self.parse_probe_result(probe_data)?;
        
        // Check for structured errors in the result
        if let Some(error) = &result.error {
            let error_msg = error.string.as_deref().unwrap_or("Unknown ffprobe error");
            warn!("FFprobe reported error for {}: {} (code: {:?})", input_url, error_msg, error.code);
            
            // Still return error for compatibility, but now with structured info
            return Err(anyhow::anyhow!("FFprobe error: {} (code: {:?})", error_msg, error.code));
        }
        
        // Log success with useful info
        debug!("Successfully probed {}: {} streams ({} video, {} audio), format: {:?}", 
               input_url, 
               result.streams.len(), 
               result.video_streams.len(), 
               result.audio_streams.len(),
               result.format_name);
        
        Ok(result)
    }
    
    /// Parse ffprobe JSON output into our ProbeResult structure
    fn parse_probe_result(&self, data: serde_json::Value) -> Result<ProbeResult> {
        let mut streams = Vec::new();
        let mut video_streams = Vec::new();
        let mut audio_streams = Vec::new();
        
        // Parse error section first
        let error = data.get("error").map(|error_obj| {
            ProbeError {
                code: error_obj.get("code").and_then(|v| v.as_i64()).map(|v| v as i32),
                string: error_obj.get("string").and_then(|v| v.as_str()).map(|s| s.to_string()),
            }
        });
        
        // Parse streams section - may be empty if there was an error
        if let Some(streams_array) = data.get("streams").and_then(|v| v.as_array()) {
            for (index, stream) in streams_array.iter().enumerate() {
                let codec_type = stream.get("codec_type").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();
                let codec_name = stream.get("codec_name").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();
                
                let stream_info = StreamInfo {
                    index: stream.get("index").and_then(|v| v.as_u64()).map(|v| v as u32).unwrap_or(index as u32),
                    codec_type: codec_type.clone(),
                    codec_name: codec_name.clone(),
                    codec_tag_string: stream.get("codec_tag_string").and_then(|v| v.as_str()).map(|s| s.to_string()),
                    duration: stream.get("duration").and_then(|v| v.as_str()).and_then(|s| s.parse().ok()),
                    bit_rate: stream.get("bit_rate").and_then(|v| v.as_str()).and_then(|s| s.parse().ok()),
                    width: stream.get("width").and_then(|v| v.as_u64()).map(|v| v as u32),
                    height: stream.get("height").and_then(|v| v.as_u64()).map(|v| v as u32),
                    r_frame_rate: stream.get("r_frame_rate").and_then(|v| v.as_str()).map(|s| s.to_string()),
                    sample_rate: stream.get("sample_rate").and_then(|v| v.as_str()).and_then(|s| s.parse().ok()),
                    channels: stream.get("channels").and_then(|v| v.as_u64()).map(|v| v as u32),
                    channel_layout: stream.get("channel_layout").and_then(|v| v.as_str()).map(|s| s.to_string()),
                };
                
                match codec_type.as_str() {
                    "video" => video_streams.push(stream_info.clone()),
                    "audio" => audio_streams.push(stream_info.clone()),
                    _ => {}
                }
                
                streams.push(stream_info);
            }
        }
        
        // Parse format section - may be empty if there was an error
        let format = data.get("format").and_then(|v| v.as_object());
        let format_name = format
            .and_then(|f| f.get("format_name"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let duration = format
            .and_then(|f| f.get("duration"))
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse().ok());
        let bit_rate = format
            .and_then(|f| f.get("bit_rate"))
            .and_then(|v| v.as_str())
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
            error,
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
            strategy.video_mapping = Some("0:v:0".to_string());
            
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
            strategy.audio_mapping = Some("0:a:0".to_string());
            
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
            // Copy if input bitrate is equal or higher than target (no upconversion)
            return input_kbps >= target_br;
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

    #[test]
    fn test_parse_probe_result_with_error() {
        let prober = StreamProber::new(None);
        
        // Test error parsing
        let error_data = serde_json::json!({
            "error": {
                "code": 1,
                "string": "Connection refused"
            }
        });
        
        let result = prober.parse_probe_result(error_data).unwrap();
        assert!(result.error.is_some());
        assert_eq!(result.error.as_ref().unwrap().code, Some(1));
        assert_eq!(result.error.as_ref().unwrap().string, Some("Connection refused".to_string()));
        assert!(result.streams.is_empty());
    }

    #[test] 
    fn test_parse_probe_result_success() {
        let prober = StreamProber::new(None);
        
        // Test successful parsing with limited fields (from show_entries)
        let success_data = serde_json::json!({
            "streams": [
                {
                    "index": 0,
                    "codec_type": "video",
                    "codec_name": "h264",
                    "width": 1920,
                    "height": 1080,
                    "r_frame_rate": "25/1",
                    "bit_rate": "2000000"
                },
                {
                    "index": 1,
                    "codec_type": "audio", 
                    "codec_name": "aac",
                    "sample_rate": "48000",
                    "channels": 2,
                    "bit_rate": "128000"
                }
            ],
            "format": {
                "format_name": "mpegts",
                "duration": "3600.0",
                "bit_rate": "2128000"
            }
        });
        
        let result = prober.parse_probe_result(success_data).unwrap();
        assert!(result.error.is_none());
        assert_eq!(result.streams.len(), 2);
        assert_eq!(result.video_streams.len(), 1);
        assert_eq!(result.audio_streams.len(), 1);
        assert!(result.has_video);
        assert!(result.has_audio);
        assert_eq!(result.format_name, Some("mpegts".to_string()));
        assert_eq!(result.duration, Some(3600.0));
        assert_eq!(result.bit_rate, Some(2128000));
        
        // Check stream details
        let video = &result.video_streams[0];
        assert_eq!(video.codec_name, "h264");
        assert_eq!(video.width, Some(1920));
        assert_eq!(video.height, Some(1080));
        
        let audio = &result.audio_streams[0];
        assert_eq!(audio.codec_name, "aac");
        assert_eq!(audio.channels, Some(2));
        assert_eq!(audio.sample_rate, Some(48000));
    }
}