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
use crate::services::cyclic_buffer::CyclicBuffer;

/// Error fallback generator that creates Transport Stream content from error images
pub struct ErrorFallbackGenerator {
    config: ErrorFallbackConfig,
    cyclic_buffer: Arc<CyclicBuffer>,
    current_token: std::sync::Mutex<Option<CancellationToken>>,
    is_active: std::sync::atomic::AtomicBool,
}

impl ErrorFallbackGenerator {
    /// Create a new error fallback generator
    pub fn new(config: ErrorFallbackConfig, cyclic_buffer: Arc<CyclicBuffer>) -> Self {
        Self {
            config,
            cyclic_buffer,
            current_token: std::sync::Mutex::new(None),
            is_active: std::sync::atomic::AtomicBool::new(false),
        }
    }

    /// Start generating error fallback content
    pub async fn start_fallback(&self, error_message: &str, config_id: Uuid) -> Result<(), RelayError> {
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
        self.is_active.store(true, std::sync::atomic::Ordering::Relaxed);

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

        info!("Started error fallback generator for config {} with message: {}", config_id, error_message);
        Ok(())
    }

    /// Stop generating error fallback content
    pub fn stop_fallback(&self) {
        self.is_active.store(false, std::sync::atomic::Ordering::Relaxed);
        
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
    async fn generate_error_image(&self, error_message: &str, config_id: Uuid) -> Result<bytes::Bytes, RelayError> {
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

    /// Generate error fallback using FFmpeg (future implementation)
    
    async fn generate_ffmpeg_error_image(&self, error_message: &str, config_id: Uuid) -> Result<bytes::Bytes, RelayError> {
        // This would use FFmpeg to create a proper error image and convert to TS
        // Example command:
        // ffmpeg -f lavfi -i "color=red:size=640x480:rate=25" 
        //        -vf "drawtext=text='ERROR: {error_message}':fontcolor=white:fontsize=24:x=(w-text_w)/2:y=(h-text_h)/2"
        //        -t 1 -f mpegts -y pipe:1
        
        let timestamp = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC");
        let error_text = format!("ERROR: {error_message}\\nConfig: {config_id}\\nTime: {timestamp}");

        // Build FFmpeg command for error image generation
        let mut cmd = tokio::process::Command::new("ffmpeg");
        cmd.args([
            "-f", "lavfi",
            "-i", "color=red:size=640x480:rate=25",
            "-vf", &format!("drawtext=text='{error_text}':fontcolor=white:fontsize=24:x=(w-text_w)/2:y=(h-text_h)/2"),
            "-t", "0.1", // Generate 0.1 seconds of video
            "-f", "mpegts",
            "-y", "pipe:1"
        ]);

        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::null());

        match cmd.spawn() {
            Ok(mut child) => {
                if let Some(stdout) = child.stdout.take() {
                    let mut output = Vec::new();
                    let mut reader = tokio::io::BufReader::new(stdout);
                    
                    use tokio::io::AsyncReadExt;
                    if (reader.read_to_end(&mut output).await).is_ok() {
                        let _ = child.wait().await;
                        return Ok(bytes::Bytes::from(output));
                    }
                }
                let _ = child.wait().await;
            }
            Err(e) => {
                warn!("Failed to generate FFmpeg error image: {}", e);
            }
        }

        // Fallback to simple packet generation
        let error_packet = self.create_error_transport_stream_packet(error_message);
        Ok(bytes::Bytes::from(error_packet))
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
        if let Some(last_time) = *last_error {
            if now.duration_since(last_time).as_secs() > self.fallback_config.error_threshold_seconds as u64 {
                self.error_count.store(0, std::sync::atomic::Ordering::Relaxed);
            }
        }
        
        *last_error = Some(now);
        let error_count = self.error_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
        
        let new_health = if error_count >= self.fallback_config.max_error_count {
            StreamHealth::Failed { 
                last_error: error_message.to_string() 
            }
        } else {
            StreamHealth::Degraded { error_count }
        };
        
        *self.health_status.write().unwrap() = new_health.clone();
        
        info!("Stream health updated for config {}: {:?}", self.config_id, new_health);
        new_health
    }

    /// Mark stream as healthy
    pub async fn mark_healthy(&self) {
        self.error_count.store(0, std::sync::atomic::Ordering::Relaxed);
        *self.health_status.write().unwrap() = StreamHealth::Healthy;
    }

    /// Mark stream as using fallback
    pub async fn mark_fallback(&self, reason: &str) {
        *self.health_status.write().unwrap() = StreamHealth::Fallback { 
            reason: reason.to_string() 
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