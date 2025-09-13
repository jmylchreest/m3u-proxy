//! Error Fallback Generator
//!
//! This module generates error images and converts them to Transport Stream format
//! for seamless fallback when upstream connections fail.

use std::sync::Arc;
use std::time::Duration;
use tokio::time::interval;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::models::relay::*;
use crate::services::connection_limiter::LimitExceededError;
use crate::services::cyclic_buffer::CyclicBuffer;
use crate::services::embedded_font::EmbeddedFontManager;

/// Error fallback generator that creates Transport Stream content from error images  
pub struct ErrorFallbackGenerator {
    config: ErrorFallbackConfig,
    cyclic_buffer: Arc<CyclicBuffer>,
    current_token: std::sync::Mutex<Option<CancellationToken>>,
    is_active: std::sync::atomic::AtomicBool,
    font_manager: tokio::sync::Mutex<EmbeddedFontManager>,
}

impl ErrorFallbackGenerator {
    /// Create a new error fallback generator
    pub fn new(config: ErrorFallbackConfig, cyclic_buffer: Arc<CyclicBuffer>) -> Self {
        Self {
            config,
            cyclic_buffer,
            current_token: std::sync::Mutex::new(None),
            is_active: std::sync::atomic::AtomicBool::new(false),
            font_manager: tokio::sync::Mutex::new(EmbeddedFontManager::new()),
        }
    }

    /// Start generating error fallback content
    pub async fn start_fallback(
        &self,
        error_message: &str,
        config_id: Uuid,
    ) -> Result<(), RelayError> {
        if !self.config.enabled {
            return Ok(());
        }

        // Stop any existing fallback task
        self.stop_fallback();

        // Create new cancellation token for this session
        let cancellation_token = CancellationToken::new();

        // Store the token so we can cancel it later
        *self.current_token.lock().unwrap() = Some(cancellation_token.clone());

        // Mark as active
        self.is_active
            .store(true, std::sync::atomic::Ordering::Relaxed);

        // Generate error image with embedded message
        let error_image = self.generate_error_image(error_message, config_id).await?;

        // Convert image to Transport Stream and feed to cyclic buffer
        let buffer = self.cyclic_buffer.clone();
        let fps = 25; // 25 FPS
        let frame_duration = Duration::from_millis(1000 / fps);
        let token = cancellation_token.clone();

        tokio::spawn(async move {
            let mut interval = interval(frame_duration);

            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        if let Err(e) = buffer.write_chunk(error_image.clone()).await {
                            error!("Failed to write error fallback chunk: {}", e);
                            break;
                        }
                    }
                    _ = token.cancelled() => {
                        info!("Error fallback generator cancelled for config {}", config_id);
                        break;
                    }
                }
            }

            info!("Error fallback generator stopped for config {}", config_id);
        });

        info!(
            "Started error fallback generator for config {} with message: {}",
            config_id, error_message
        );
        Ok(())
    }

    /// Start generating error fallback content using the new error video system
    pub async fn start_error_video_fallback(
        &self,
        error_type: &LimitExceededError,
        config_id: Uuid,
        width: u32,
        height: u32,
        bitrate_kbps: u32,
    ) -> Result<(), RelayError> {
        if !self.config.enabled {
            return Ok(());
        }

        // Stop any existing fallback task
        self.stop_fallback();

        // Create new cancellation token for this session
        let cancellation_token = CancellationToken::new();

        // Store the token so we can cancel it later
        *self.current_token.lock().unwrap() = Some(cancellation_token.clone());

        // Mark as active
        self.is_active
            .store(true, std::sync::atomic::Ordering::Relaxed);

        // Generate error video (short duration, we'll loop it)
        let error_video = self
            .generate_error_video_stream(error_type, config_id, width, height, bitrate_kbps)
            .await?;

        // Convert video to chunks and feed to cyclic buffer continuously
        let buffer = self.cyclic_buffer.clone();
        let token = cancellation_token.clone();
        let error_type_name = error_type.error_type().to_string();
        let error_type_name_for_spawn = error_type_name.clone();

        tokio::spawn(async move {
            // Calculate chunk size (typical TS packet size)
            const CHUNK_SIZE: usize = 188 * 7; // 7 TS packets per chunk
            let video_chunks: Vec<bytes::Bytes> = error_video
                .chunks(CHUNK_SIZE)
                .map(bytes::Bytes::copy_from_slice)
                .collect();

            if video_chunks.is_empty() {
                warn!("Error video has no chunks to loop");
                return;
            }

            let mut chunk_index = 0;
            let mut interval = interval(Duration::from_millis(40)); // ~25fps worth of chunks

            info!(
                "Starting error video loop with {} chunks for {}",
                video_chunks.len(),
                error_type_name_for_spawn
            );

            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        let chunk = &video_chunks[chunk_index];
                        if let Err(e) = buffer.write_chunk(chunk.clone()).await {
                            error!("Failed to write error video chunk: {}", e);
                            break;
                        }

                        // Loop back to beginning when we reach the end
                        chunk_index = (chunk_index + 1) % video_chunks.len();
                    }
                    _ = token.cancelled() => {
                        info!("Error video fallback cancelled for config {}", config_id);
                        break;
                    }
                }
            }

            info!("Error video fallback stopped for config {}", config_id);
        });

        info!(
            "Started error video fallback for config {} with error type: {}",
            config_id, error_type_name
        );
        Ok(())
    }

    /// Stop generating error fallback content
    pub fn stop_fallback(&self) {
        self.is_active
            .store(false, std::sync::atomic::Ordering::Relaxed);

        // Cancel the current token if it exists
        if let Some(token) = self.current_token.lock().unwrap().take() {
            token.cancel();
        }
    }

    /// Check if fallback is currently active
    pub fn is_fallback_active(&self) -> bool {
        self.is_active.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Generate an error image with embedded error message
    async fn generate_error_image(
        &self,
        error_message: &str,
        config_id: Uuid,
    ) -> Result<bytes::Bytes, RelayError> {
        // For now, we'll generate a simple Transport Stream packet with error text
        // In a full implementation, this would use FFmpeg to create an actual image
        // and convert it to Transport Stream format

        let timestamp = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC");
        let error_content = format!(
            "STREAM ERROR\n\n{error_message}\n\nConfig ID: {config_id}\nTimestamp: {timestamp}\n\nPlease check your stream configuration and try again."
        );

        // Generate a basic Transport Stream packet with error information
        // This is a simplified implementation - in production, you'd want to use FFmpeg
        // to generate proper Transport Stream packets with embedded images
        let error_packet = self.create_error_transport_stream_packet(&error_content);

        Ok(bytes::Bytes::from(error_packet))
    }

    /// Create a Transport Stream packet with error information
    fn create_error_transport_stream_packet(&self, error_text: &str) -> Vec<u8> {
        // This is a simplified TS packet structure
        // In a real implementation, you'd want to create proper MPEG-TS packets
        // with embedded images or generate them using FFmpeg

        let mut packet = vec![0x47]; // TS sync byte

        // Add basic TS header (simplified)
        packet.extend_from_slice(&[0x00, 0x00, 0x10]); // Basic header

        // Add error text as payload (in a real implementation, this would be video data)
        let error_bytes = error_text.as_bytes();
        let payload_size = std::cmp::min(error_bytes.len(), 184); // Max TS payload size

        packet.extend_from_slice(&error_bytes[..payload_size]);

        // Pad to 188 bytes (standard TS packet size)
        packet.resize(188, 0xFF);

        packet
    }

    /// Generate error video stream with proper FFmpeg rendering and font support
    pub async fn generate_error_video_stream(
        &self,
        error_type: &LimitExceededError,
        config_id: Uuid,
        width: u32,
        height: u32,
        bitrate_kbps: u32,
    ) -> Result<bytes::Bytes, RelayError> {
        let error_message = self.format_error_message(error_type);
        let font_param = {
            let mut font_manager = self.font_manager.lock().await;
            font_manager.get_ffmpeg_font_param().await.unwrap_or(None)
        };

        let ffmpeg_cmd = self.build_error_video_command(
            &error_message,
            config_id,
            width,
            height,
            bitrate_kbps,
            font_param.as_deref(),
        );

        // Generate a single loop of the error video, then we'll loop it in memory
        self.execute_ffmpeg_command(ffmpeg_cmd).await
    }

    /// Build FFmpeg command for error video generation
    fn build_error_video_command(
        &self,
        error_message: &str,
        _config_id: Uuid,
        width: u32,
        height: u32,
        bitrate_kbps: u32,
        font_param: Option<&str>,
    ) -> Vec<String> {
        let duration = self.config.error_video_duration_seconds.unwrap_or(5);
        let title_font_size = std::cmp::max(24, width / 20); // Larger title font
        let body_font_size = std::cmp::max(16, width / 30); // Smaller body font

        // Parse error message into title and body
        let lines: Vec<&str> = error_message.lines().collect();
        let title = lines.first().unwrap_or(&"ERROR").to_string();
        let body_lines: Vec<&str> = if lines.len() > 1 {
            lines[1..].to_vec()
        } else {
            vec![]
        };

        // Build elegant multi-layered text filter
        let mut text_filters = Vec::new();

        // 1. Title (first line) - Large, bold, centered at top
        let clean_title = title
            .replace('\'', "\\'")
            .replace('"', "\\\"")
            .replace(':', "\\:");
        let title_filter = if let Some(font) = font_param {
            format!(
                "drawtext=text='{}':{}:fontcolor=white:fontsize={}:x=(w-text_w)/2:y={}:box=1:boxcolor=0x1a1a1a@0.8:boxborderw=8",
                clean_title,
                font,
                title_font_size,
                height / 6 // Position in upper area
            )
        } else {
            format!(
                "drawtext=text='{}':fontcolor=white:fontsize={}:x=(w-text_w)/2:y={}:box=1:boxcolor=0x1a1a1a@0.8:boxborderw=8",
                clean_title,
                title_font_size,
                height / 6
            )
        };
        text_filters.push(title_filter);

        // 2. Body lines - Centered, properly spaced
        if !body_lines.is_empty() {
            let line_height = body_font_size + 8; // Space between lines
            let total_text_height = body_lines.len() as u32 * line_height;
            let start_y = (height / 2) - (total_text_height / 2) + (height / 8); // Center vertically, offset from title

            for (i, line) in body_lines.iter().enumerate() {
                if line.trim().is_empty() {
                    continue; // Skip empty lines
                }

                let y_pos = start_y + (i as u32 * line_height);
                let clean_line = line
                    .trim()
                    .replace('\'', "\\'")
                    .replace('"', "\\\"")
                    .replace(':', "\\:");
                let line_filter = if let Some(font) = font_param {
                    format!(
                        "drawtext=text='{}':{}:fontcolor=0xcccccc:fontsize={}:x=(w-text_w)/2:y={}:box=1:boxcolor=0x0d0d0d@0.7:boxborderw=4",
                        clean_line, font, body_font_size, y_pos
                    )
                } else {
                    format!(
                        "drawtext=text='{}':fontcolor=0xcccccc:fontsize={}:x=(w-text_w)/2:y={}:box=1:boxcolor=0x0d0d0d@0.7:boxborderw=4",
                        clean_line, body_font_size, y_pos
                    )
                };
                text_filters.push(line_filter);
            }
        }

        // 3. Add subtle border/frame effect
        text_filters.push(format!(
            "drawbox=x={}:y={}:w={}:h={}:color=0x333333@0.3:t=2",
            width / 20,             // x offset
            height / 20,            // y offset
            width - (width / 10),   // width (leaving margins)
            height - (height / 10)  // height (leaving margins)
        ));

        // Combine all filters
        let text_filter = text_filters.join(",");

        vec![
            "-f".to_string(),
            "lavfi".to_string(),
            "-i".to_string(),
            format!("color=black:size={}x{}:rate=25", width, height), // Black background
            "-vf".to_string(),
            text_filter,
            "-c:v".to_string(),
            "libx264".to_string(),
            "-preset".to_string(),
            "ultrafast".to_string(), // Fast encoding for error videos
            "-b:v".to_string(),
            format!("{}k", bitrate_kbps),
            "-maxrate".to_string(),
            format!("{}k", bitrate_kbps),
            "-bufsize".to_string(),
            format!("{}k", bitrate_kbps * 2),
            "-g".to_string(),
            "25".to_string(), // GOP size
            "-f".to_string(),
            "mpegts".to_string(),
            "-t".to_string(),
            duration.to_string(), // Configurable duration
            "pipe:1".to_string(),
        ]
    }

    /// Execute FFmpeg command and return the output
    async fn execute_ffmpeg_command(&self, args: Vec<String>) -> Result<bytes::Bytes, RelayError> {
        let mut cmd = tokio::process::Command::new("ffmpeg");
        cmd.args(&args);
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        info!("Executing FFmpeg command: ffmpeg {}", args.join(" "));

        match cmd.spawn() {
            Ok(mut child) => {
                let stdout_handle = child.stdout.take().map(|stdout| {
                    tokio::spawn(async move {
                        let mut output = Vec::new();
                        let mut reader = tokio::io::BufReader::new(stdout);
                        use tokio::io::AsyncReadExt;
                        let _ = reader.read_to_end(&mut output).await;
                        output
                    })
                });

                let stderr_handle = child.stderr.take().map(|stderr| {
                    tokio::spawn(async move {
                        let mut error_output = Vec::new();
                        let mut reader = tokio::io::BufReader::new(stderr);
                        use tokio::io::AsyncReadExt;
                        let _ = reader.read_to_end(&mut error_output).await;
                        String::from_utf8_lossy(&error_output).to_string()
                    })
                });

                let exit_status = child.wait().await;

                if let Some(stdout_handle) = stdout_handle
                    && let Ok(output) = stdout_handle.await
                {
                    if let Some(stderr_handle) = stderr_handle
                        && let Ok(stderr_output) = stderr_handle.await
                        && !stderr_output.is_empty()
                    {
                        warn!("FFmpeg stderr: {}", stderr_output);
                    }

                    match exit_status {
                        Ok(status) if status.success() && !output.is_empty() => {
                            info!("Generated error video: {} bytes", output.len());
                            return Ok(bytes::Bytes::from(output));
                        }
                        Ok(status) => {
                            warn!(
                                "FFmpeg exited with status: {} (output {} bytes)",
                                status,
                                output.len()
                            );
                        }
                        Err(e) => {
                            warn!("FFmpeg process error: {}", e);
                        }
                    }
                }
            }
            Err(e) => {
                warn!("Failed to spawn FFmpeg for error video generation: {}", e);
            }
        }

        // Fallback to simple packet generation
        warn!("Falling back to simple error packet generation");
        let error_packet = self
            .create_error_transport_stream_packet(&format!("ERROR: {}", "Video generation failed"));
        Ok(bytes::Bytes::from(error_packet))
    }

    /// Format error message based on error type with elegant styling
    fn format_error_message(&self, error: &LimitExceededError) -> String {
        match error {
            LimitExceededError::ChannelClientLimit {
                channel_id: _,
                current,
                max,
            } => {
                format!(
                    "CHANNEL BUSY\n\n\
                    This channel has reached its maximum viewer limit\n\
                    \n\
                    Current Viewers: {}\n\
                    Maximum Allowed: {}\n\
                    \n\
                    Please try again in a few moments\n\
                    Thank you for your patience",
                    current, max
                )
            }
            LimitExceededError::ProxyClientLimit {
                proxy_id: _,
                current,
                max,
            } => {
                format!(
                    "STREAM PROXY BUSY\n\n\
                    This stream proxy has reached its connection limit\n\
                    \n\
                    Active Connections: {}\n\
                    Maximum Allowed: {}\n\
                    \n\
                    Please try again in a few moments\n\
                    Thank you for your patience",
                    current, max
                )
            }
            LimitExceededError::UpstreamSourceLimit {
                source_url: _,
                error,
            } => {
                let clean_error = error.lines().next().unwrap_or("Connection failed");
                format!(
                    "SOURCE UNAVAILABLE\n\n\
                    The upstream source is temporarily unavailable\n\
                    \n\
                    Error Details: {}\n\
                    \n\
                    Our team has been notified\n\
                    Please check back shortly",
                    clean_error
                )
            }
            LimitExceededError::StreamUnavailable { reason } => {
                format!(
                    "STREAM UNAVAILABLE\n\n\
                    {}\n\
                    \n\
                    We apologize for the inconvenience\n\
                    Please try again later",
                    reason
                )
            }
        }
    }
}

impl Default for ErrorFallbackConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            error_image_path: None,
            fallback_timeout_seconds: 30,
            max_error_count: 5,
            error_threshold_seconds: 60,
            error_video_duration_seconds: Some(5), // Default to 5 seconds
        }
    }
}

/// Stream health monitor that tracks upstream connection health
pub struct StreamHealthMonitor {
    config_id: Uuid,
    error_count: std::sync::atomic::AtomicU32,
    last_error_time: std::sync::Mutex<Option<std::time::Instant>>,
    health_status: std::sync::RwLock<StreamHealth>,
    fallback_config: ErrorFallbackConfig,
}

impl StreamHealthMonitor {
    pub fn new(config_id: Uuid, fallback_config: ErrorFallbackConfig) -> Self {
        Self {
            config_id,
            error_count: std::sync::atomic::AtomicU32::new(0),
            last_error_time: std::sync::Mutex::new(None),
            health_status: std::sync::RwLock::new(StreamHealth::Healthy),
            fallback_config,
        }
    }

    /// Record an error and update health status
    pub async fn record_error(&self, error_message: &str) -> StreamHealth {
        let now = std::time::Instant::now();
        let mut last_error = self.last_error_time.lock().unwrap();

        // Reset error count if enough time has passed
        if let Some(last_time) = *last_error
            && now.duration_since(last_time).as_secs()
                > self.fallback_config.error_threshold_seconds as u64
        {
            self.error_count
                .store(0, std::sync::atomic::Ordering::Relaxed);
        }

        *last_error = Some(now);
        let error_count = self
            .error_count
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            + 1;

        let new_health = if error_count >= self.fallback_config.max_error_count {
            StreamHealth::Failed {
                last_error: error_message.to_string(),
            }
        } else {
            StreamHealth::Degraded { error_count }
        };

        *self.health_status.write().unwrap() = new_health.clone();

        info!(
            "Stream health updated for config {}: {:?}",
            self.config_id, new_health
        );
        new_health
    }

    /// Mark stream as healthy
    pub async fn mark_healthy(&self) {
        self.error_count
            .store(0, std::sync::atomic::Ordering::Relaxed);
        *self.health_status.write().unwrap() = StreamHealth::Healthy;
    }

    /// Mark stream as using fallback
    pub async fn mark_fallback(&self, reason: &str) {
        *self.health_status.write().unwrap() = StreamHealth::Fallback {
            reason: reason.to_string(),
        };
    }

    /// Get current health status
    pub fn get_health(&self) -> StreamHealth {
        self.health_status.read().unwrap().clone()
    }

    /// Check if fallback should be activated
    pub fn should_activate_fallback(&self) -> bool {
        matches!(self.get_health(), StreamHealth::Failed { .. })
    }
}
