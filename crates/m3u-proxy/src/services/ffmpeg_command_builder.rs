//! FFmpeg Command Builder Service
//!
//! This service handles generating FFmpeg command arguments from resolved relay configurations,
//! separating command generation logic from data models.

use crate::{
    models::relay::{AudioCodec, HwAccelCapabilities, ResolvedRelayConfig, VideoCodec},
    services::{StreamMappingStrategy, StreamProber},
};
use serde_json;
use tracing::{debug, warn};

/// Service for building FFmpeg command arguments
pub struct FFmpegCommandBuilder {
    stream_prober: Option<StreamProber>,
}

impl FFmpegCommandBuilder {
    pub fn new(stream_prober: Option<StreamProber>) -> Self {
        Self { stream_prober }
    }

    /// Build FFmpeg command arguments from resolved relay configuration
    pub fn build_args(
        &self,
        config: &ResolvedRelayConfig,
        input_url: &str,
        output_path: &str,
        hwaccel_caps: &HwAccelCapabilities,
        mapping_strategy: Option<&StreamMappingStrategy>,
    ) -> Vec<String> {
        debug!(
            "Building FFmpeg command: profile='{}', input='{}', output='{}'",
            config.profile.name, input_url, output_path
        );

        // Handle legacy profiles with pre-generated args
        if !config.effective_args.is_empty() {
            return self.substitute_templates(
                &config.effective_args,
                input_url,
                output_path,
                config,
            );
        }

        let mut args = Vec::new();

        // Add hardware acceleration setup if available
        if config.profile.enable_hardware_acceleration
            && let Some(hwaccel_args) = self.generate_hwaccel_args(&config.profile, hwaccel_caps)
        {
            args.extend(hwaccel_args);
        }

        // Add input arguments with analysis parameters
        self.add_input_args(&mut args, input_url);

        // Add stream mapping
        self.add_stream_mapping(&mut args, mapping_strategy);

        // Add video codec and settings
        self.add_video_codec_args(&mut args, &config.profile, hwaccel_caps, mapping_strategy);

        // Add hardware acceleration filters if needed
        if self.should_apply_hwaccel_filters(&config.profile, hwaccel_caps, mapping_strategy)
            && let Some(hwaccel_filters) =
                self.generate_hwaccel_video_filters(&config.profile, hwaccel_caps)
        {
            args.extend(hwaccel_filters);
        }

        // Add video encoding settings
        if self.should_apply_video_settings(&config.profile, mapping_strategy) {
            self.add_video_encoding_settings(&mut args, &config.profile, mapping_strategy);
        }

        // Add audio codec and settings
        self.add_audio_codec_args(&mut args, &config.profile, mapping_strategy);

        // Add transport stream settings
        self.add_transport_stream_args(&mut args);

        // Add output arguments
        self.add_output_args(&mut args, output_path);

        // Apply manual args override if provided
        if let Some(ref manual_args) = config.profile.manual_args {
            self.add_manual_args(&mut args, manual_args);
        }

        debug!("Generated FFmpeg command with {} arguments", args.len());
        args
    }

    /// Probe input stream and create mapping strategy
    pub async fn create_mapping_strategy(&self, input_url: &str) -> Option<StreamMappingStrategy> {
        if let Some(ref prober) = self.stream_prober {
            match prober.probe_input(input_url).await {
                Ok(probe_result) => {
                    debug!(
                        "Stream probe successful: {} streams found",
                        probe_result.streams.len()
                    );
                    // For now, return a basic strategy - this can be enhanced later
                    Some(StreamMappingStrategy {
                        video_mapping: Some("0:v:0".to_string()),
                        audio_mapping: Some("0:a:0".to_string()),
                        video_copy: false,
                        audio_copy: false,
                        target_video_bitrate: None,
                        target_audio_bitrate: None,
                    })
                }
                Err(e) => {
                    warn!("Stream probe failed for {}: {}", input_url, e);
                    None
                }
            }
        } else {
            debug!("No stream prober available, using default mapping");
            None
        }
    }

    /// Add input arguments with analyzeduration and probesize
    fn add_input_args(&self, args: &mut Vec<String>, input_url: &str) {
        args.extend([
            "-analyzeduration".to_string(),
            "10000000".to_string(), // 10 seconds
            "-probesize".to_string(),
            "10000000".to_string(), // 10MB
            "-i".to_string(),
            input_url.to_string(),
        ]);
    }

    /// Add stream mapping arguments
    fn add_stream_mapping(
        &self,
        args: &mut Vec<String>,
        mapping_strategy: Option<&StreamMappingStrategy>,
    ) {
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
    }

    /// Add video codec arguments
    fn add_video_codec_args(
        &self,
        args: &mut Vec<String>,
        profile: &crate::models::relay::RelayProfile,
        hwaccel_caps: &HwAccelCapabilities,
        mapping_strategy: Option<&StreamMappingStrategy>,
    ) {
        let should_copy = mapping_strategy.map(|s| s.video_copy).unwrap_or(false);

        if should_copy {
            args.extend(["-c:v".to_string(), "copy".to_string()]);
        } else if profile.enable_hardware_acceleration {
            args.push("-c:v".to_string());
            if let Some(hw_encoder) = self.get_hwaccel_video_encoder(profile, hwaccel_caps) {
                args.push(hw_encoder);
            } else {
                args.push(profile.get_software_encoder());
            }
        } else {
            args.extend(["-c:v".to_string(), profile.get_software_encoder()]);
        }
    }

    /// Add video encoding settings
    fn add_video_encoding_settings(
        &self,
        args: &mut Vec<String>,
        profile: &crate::models::relay::RelayProfile,
        mapping_strategy: Option<&StreamMappingStrategy>,
    ) {
        // Use optimal bitrate from strategy or profile default
        let target_bitrate = mapping_strategy
            .and_then(|s| s.target_video_bitrate)
            .or(profile.video_bitrate);

        if let Some(bitrate) = target_bitrate {
            args.extend(["-b:v".to_string(), format!("{bitrate}k")]);
        }
        if let Some(ref preset) = profile.video_preset {
            args.extend(["-preset".to_string(), preset.clone()]);
        }
        if let Some(ref video_profile) = profile.video_profile {
            args.extend(["-profile:v".to_string(), video_profile.clone()]);
        }
    }

    /// Add audio codec arguments and settings
    fn add_audio_codec_args(
        &self,
        args: &mut Vec<String>,
        profile: &crate::models::relay::RelayProfile,
        mapping_strategy: Option<&StreamMappingStrategy>,
    ) {
        if let Some(strategy) = mapping_strategy {
            if strategy.audio_mapping.is_some() {
                args.push("-c:a".to_string());
                if strategy.audio_copy {
                    args.push("copy".to_string());
                } else {
                    args.push(profile.get_audio_encoder());
                    self.add_audio_encoding_settings(args, profile, strategy);
                }
            }
        } else {
            // Legacy behavior
            args.extend(["-c:a".to_string(), profile.get_audio_encoder()]);

            // Audio settings - only apply encoding parameters if we're not copying
            if profile.audio_codec != AudioCodec::Copy {
                if let Some(bitrate) = profile.audio_bitrate {
                    args.extend(["-b:a".to_string(), format!("{bitrate}k")]);
                }
                self.add_explicit_audio_params(args, profile);
            }
        }
    }

    /// Add audio encoding settings when transcoding
    fn add_audio_encoding_settings(
        &self,
        args: &mut Vec<String>,
        profile: &crate::models::relay::RelayProfile,
        strategy: &StreamMappingStrategy,
    ) {
        // Use optimal bitrate from strategy or profile default
        let target_bitrate = strategy.target_audio_bitrate.or(profile.audio_bitrate);

        if let Some(bitrate) = target_bitrate {
            args.extend(["-b:a".to_string(), format!("{bitrate}k")]);
        }

        self.add_explicit_audio_params(args, profile);
    }

    /// Add explicit audio parameters
    fn add_explicit_audio_params(
        &self,
        args: &mut Vec<String>,
        profile: &crate::models::relay::RelayProfile,
    ) {
        let sample_rate = profile.audio_sample_rate.unwrap_or(48000);
        let channels = profile.audio_channels.unwrap_or(2);
        args.extend(["-ar".to_string(), sample_rate.to_string()]);
        args.extend(["-ac".to_string(), channels.to_string()]);
    }

    /// Add transport stream settings
    fn add_transport_stream_args(&self, args: &mut Vec<String>) {
        args.extend([
            "-f".to_string(),
            "mpegts".to_string(),
            "-mpegts_copyts".to_string(),
            "1".to_string(),
            "-avoid_negative_ts".to_string(),
            "disabled".to_string(),
            "-mpegts_start_pid".to_string(),
            "256".to_string(), // Start PID for streams
            "-mpegts_pmt_start_pid".to_string(),
            "4096".to_string(), // Different PID for PMT
        ]);
    }

    /// Add output arguments
    fn add_output_args(&self, args: &mut Vec<String>, _output_path: &str) {
        // Output to stdout for cyclic buffer consumption
        args.extend(["-y".to_string(), "pipe:1".to_string()]);
    }

    /// Add manual arguments override
    fn add_manual_args(&self, args: &mut Vec<String>, manual_args: &str) {
        let manual_args_trimmed = manual_args.trim();
        if !manual_args_trimmed.is_empty() {
            // Try JSON array format first
            if let Ok(manual_vec) = serde_json::from_str::<Vec<String>>(manual_args_trimmed) {
                args.extend(manual_vec);
            } else {
                // Fallback to space-separated string format
                let manual_vec: Vec<String> = manual_args_trimmed
                    .split_whitespace()
                    .map(|s| s.to_string())
                    .collect();
                args.extend(manual_vec);
            }
        }
    }

    /// Generate hardware acceleration arguments (input setup only)
    fn generate_hwaccel_args(
        &self,
        profile: &crate::models::relay::RelayProfile,
        hwaccel_caps: &HwAccelCapabilities,
    ) -> Option<Vec<String>> {
        let hwaccel = self.select_hwaccel(profile, hwaccel_caps)?;

        // Check if this hwaccel supports the selected video codec
        if !self.hwaccel_supports_codec(&hwaccel, profile, hwaccel_caps) {
            return None;
        }

        let mut args = Vec::new();

        // Add hardware device initialization
        args.extend(["-init_hw_device".to_string(), hwaccel.clone()]);

        // Add hardware acceleration flag
        args.extend(["-hwaccel".to_string(), hwaccel]);

        Some(args)
    }

    /// Generate hardware acceleration video filters
    fn generate_hwaccel_video_filters(
        &self,
        profile: &crate::models::relay::RelayProfile,
        hwaccel_caps: &HwAccelCapabilities,
    ) -> Option<Vec<String>> {
        let hwaccel = self.select_hwaccel(profile, hwaccel_caps)?;

        // Check if this hwaccel supports the selected video codec
        if !self.hwaccel_supports_codec(&hwaccel, profile, hwaccel_caps) {
            return None;
        }

        let mut args = Vec::new();

        // Add video filter for hardware upload
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
    fn get_hwaccel_video_encoder(
        &self,
        profile: &crate::models::relay::RelayProfile,
        hwaccel_caps: &HwAccelCapabilities,
    ) -> Option<String> {
        let hwaccel = self.select_hwaccel(profile, hwaccel_caps)?;
        profile.get_hwaccel_encoder(&hwaccel)
    }

    /// Select appropriate hardware acceleration
    fn select_hwaccel(
        &self,
        profile: &crate::models::relay::RelayProfile,
        hwaccel_caps: &HwAccelCapabilities,
    ) -> Option<String> {
        if let Some(ref preferred) = profile.preferred_hwaccel {
            if preferred == "auto" {
                self.select_best_hwaccel(hwaccel_caps)
            } else {
                Some(preferred.clone())
            }
        } else {
            self.select_best_hwaccel(hwaccel_caps)
        }
    }

    /// Select best available hardware acceleration
    fn select_best_hwaccel(&self, hwaccel_caps: &HwAccelCapabilities) -> Option<String> {
        // Preference order: vaapi, nvenc, qsv
        for preferred in ["vaapi", "nvenc", "qsv"] {
            if hwaccel_caps
                .accelerators
                .iter()
                .any(|a| a.name == preferred && a.available)
            {
                return Some(preferred.to_string());
            }
        }
        None
    }

    /// Check if hardware acceleration supports the codec
    fn hwaccel_supports_codec(
        &self,
        hwaccel: &str,
        profile: &crate::models::relay::RelayProfile,
        hwaccel_caps: &HwAccelCapabilities,
    ) -> bool {
        let codec_name = profile.video_codec.to_string().to_lowercase();

        hwaccel_caps
            .support_matrix
            .get(hwaccel)
            .map(|codecs| codecs.contains(&codec_name))
            .unwrap_or(false)
    }

    /// Check if hardware acceleration filters should be applied
    fn should_apply_hwaccel_filters(
        &self,
        profile: &crate::models::relay::RelayProfile,
        _hwaccel_caps: &HwAccelCapabilities,
        mapping_strategy: Option<&StreamMappingStrategy>,
    ) -> bool {
        if let Some(strategy) = mapping_strategy {
            profile.enable_hardware_acceleration
                && strategy.video_mapping.is_some()
                && !strategy.video_copy
        } else {
            profile.enable_hardware_acceleration && profile.video_codec != VideoCodec::Copy
        }
    }

    /// Check if video encoding settings should be applied
    fn should_apply_video_settings(
        &self,
        profile: &crate::models::relay::RelayProfile,
        mapping_strategy: Option<&StreamMappingStrategy>,
    ) -> bool {
        if let Some(strategy) = mapping_strategy {
            strategy.video_mapping.is_some() && !strategy.video_copy
        } else {
            profile.video_codec != VideoCodec::Copy
        }
    }

    /// Substitute template variables in arguments (legacy support)
    fn substitute_templates(
        &self,
        effective_args: &[String],
        input_url: &str,
        output_path: &str,
        config: &ResolvedRelayConfig,
    ) -> Vec<String> {
        let mut resolved_args = Vec::new();

        for arg in effective_args {
            let resolved = arg
                .replace("{input_url}", input_url)
                .replace("{output_path}", output_path)
                .replace(
                    "{segment_duration}",
                    &config.profile.segment_duration.unwrap_or(30).to_string(),
                )
                .replace(
                    "{max_segments}",
                    &config.profile.max_segments.unwrap_or(4).to_string(),
                )
                .replace("{input_timeout}", &config.profile.input_timeout.to_string());

            resolved_args.push(resolved);
        }

        resolved_args
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_builder_creation() {
        let builder = FFmpegCommandBuilder::new(None);
        // Test basic functionality
        assert!(builder.stream_prober.is_none());
    }

    #[test]
    fn test_input_args_generation() {
        let builder = FFmpegCommandBuilder::new(None);
        let mut args = Vec::new();
        builder.add_input_args(&mut args, "http://example.com/stream");

        assert!(args.contains(&"-i".to_string()));
        assert!(args.contains(&"http://example.com/stream".to_string()));
        assert!(args.contains(&"-analyzeduration".to_string()));
        assert!(args.contains(&"-probesize".to_string()));
    }

    #[test]
    fn test_transport_stream_args() {
        let builder = FFmpegCommandBuilder::new(None);
        let mut args = Vec::new();
        builder.add_transport_stream_args(&mut args);

        assert!(args.contains(&"-f".to_string()));
        assert!(args.contains(&"mpegts".to_string()));
        assert!(args.contains(&"-mpegts_copyts".to_string()));
    }
}
