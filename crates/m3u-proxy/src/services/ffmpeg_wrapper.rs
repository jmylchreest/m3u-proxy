//! FFmpeg Process Wrapper
//!
//! This module provides a generic wrapper for FFmpeg processes with support for
//! template variable resolution, process management, and content serving.

use anyhow::Result;
use futures::Stream;
use std::process::Stdio;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
use tokio::process::Command as TokioCommand;
use tracing::{debug, error, info, warn};
use uuid::Uuid;
use std::pin::Pin;
use std::task::{Context, Poll};

use crate::metrics::MetricsLogger;
use crate::models::relay::*;
use crate::services::relay_manager::RelayMetricsExt;
use crate::services::cyclic_buffer::{CyclicBuffer, CyclicBufferConfig, BufferClient};
use crate::services::error_fallback::{ErrorFallbackGenerator, StreamHealthMonitor};
use crate::services::stream_prober::StreamProber;
use crate::models::relay::ErrorFallbackConfig;
use crate::config::BufferConfig;
use sandboxed_file_manager::SandboxedManager;

/// A stream that continuously reads from the cyclic buffer
struct RelayStream {
    buffer: Arc<CyclicBuffer>,
    client: Arc<BufferClient>,
    initial_data: Option<Vec<u8>>,
    receiver: Option<tokio::sync::broadcast::Receiver<crate::services::cyclic_buffer::BufferChunk>>,
}

impl RelayStream {
    fn new(buffer: Arc<CyclicBuffer>, client: Arc<BufferClient>, initial_data: Vec<u8>) -> Self {
        Self {
            buffer,
            client,
            initial_data: if initial_data.is_empty() { None } else { Some(initial_data) },
            receiver: None,
        }
    }
}

impl Stream for RelayStream {
    type Item = Result<bytes::Bytes, std::io::Error>;
    
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        // Send initial data first if we have any
        if let Some(data) = self.initial_data.take() {
            let client_id = self.client.id;
            tracing::info!("Sending {} bytes of initial data to client {}", data.len(), client_id);
            return Poll::Ready(Some(Ok(bytes::Bytes::from(data))));
        }
        
        // Initialize receiver if not already done
        if self.receiver.is_none() {
            self.receiver = Some(self.buffer.subscribe_to_new_chunks());
        }
        
        // Poll the receiver for new chunks
        if let Some(ref mut receiver) = self.receiver {
            match receiver.try_recv() {
                Ok(chunk) => {
                    if chunk.sequence > self.client.get_last_sequence() {
                        self.client.set_last_sequence(chunk.sequence);
                        self.client.add_bytes_read(chunk.data.len() as u64);
                        
                        let client_id = self.client.id;
                        tracing::trace!("Streaming chunk {} ({} bytes) to client {}", chunk.sequence, chunk.data.len(), client_id);
                        
                        // Update last read asynchronously
                        let client_clone = self.client.clone();
                        tokio::spawn(async move {
                            client_clone.update_last_read().await;
                        });
                        
                        Poll::Ready(Some(Ok(chunk.data)))
                    } else {
                        // Skip this chunk and continue polling
                        cx.waker().wake_by_ref();
                        Poll::Pending
                    }
                }
                Err(tokio::sync::broadcast::error::TryRecvError::Empty) => {
                    // No data available right now, wake up later
                    cx.waker().wake_by_ref();
                    Poll::Pending
                }
                Err(tokio::sync::broadcast::error::TryRecvError::Lagged(_)) => {
                    // We've fallen behind, continue with next chunk
                    cx.waker().wake_by_ref();
                    Poll::Pending
                }
                Err(tokio::sync::broadcast::error::TryRecvError::Closed) => {
                    tracing::info!("Relay stream closed for client {}", self.client.id);
                    Poll::Ready(None) // End the stream
                }
            }
        } else {
            Poll::Ready(None)
        }
    }
}

// RelayStream is automatically Unpin since it doesn't contain any !Unpin types
impl Unpin for RelayStream {}

impl Drop for RelayStream {
    fn drop(&mut self) {
        // Clean up the client when the stream is dropped
        let client_id = self.client.id;
        let buffer = self.buffer.clone();
        
        // Spawn a task to remove the client from the buffer
        tokio::spawn(async move {
            if buffer.remove_client(client_id).await {
                tracing::info!("Removed client {} from cyclic buffer on stream drop", client_id);
            }
        });
    }
}

/// Generic FFmpeg process wrapper that can handle any FFmpeg configuration
pub struct FFmpegProcessWrapper {
    temp_manager: SandboxedManager,
    metrics: Arc<MetricsLogger>,
    hwaccel_capabilities: HwAccelCapabilities,
    buffer_config: BufferConfig,
    stream_prober: Option<StreamProber>,
}

impl FFmpegProcessWrapper {
    pub fn new(temp_manager: SandboxedManager, metrics: Arc<MetricsLogger>, hwaccel_capabilities: HwAccelCapabilities, buffer_config: BufferConfig, stream_prober: Option<StreamProber>) -> Self {
        Self {
            temp_manager,
            metrics,
            hwaccel_capabilities,
            buffer_config,
            stream_prober,
        }
    }

    /// Start an FFmpeg process with the given configuration
    pub async fn start_process(
        &self,
        config: &ResolvedRelayConfig,
        input_url: &str,
    ) -> Result<FFmpegProcess, RelayError> {
        self.start_process_with_retry(config, input_url, 3).await
    }

    /// Start an FFmpeg process with retry logic
    async fn start_process_with_retry(
        &self,
        config: &ResolvedRelayConfig,
        input_url: &str,
        max_attempts: u32,
    ) -> Result<FFmpegProcess, RelayError> {
        let mut last_error = None;
        
        for attempt in 1..=max_attempts {
            debug!("Starting FFmpeg process attempt {} of {} for relay {}", 
                   attempt, max_attempts, config.config.id);
            
            match self.try_start_process(config, input_url).await {
                Ok(process) => {
                    if attempt > 1 {
                        info!("FFmpeg process started successfully on attempt {} for relay {}", 
                              attempt, config.config.id);
                    }
                    return Ok(process);
                }
                Err(e) => {
                    warn!("FFmpeg process start attempt {} failed for relay {}: {}", 
                          attempt, config.config.id, e);
                    last_error = Some(e);
                    
                    // Apply backoff delay if not the last attempt
                    if attempt < max_attempts {
                        let delay_seconds = 5; // 5 seconds between retries
                        debug!("Waiting {} seconds before retry attempt {} for relay {}", 
                               delay_seconds, attempt + 1, config.config.id);
                        tokio::time::sleep(Duration::from_secs(delay_seconds)).await;
                    }
                }
            }
        }
        
        error!("All {} attempts failed for relay {}", max_attempts, config.config.id);
        Err(last_error.unwrap_or_else(|| RelayError::ProcessFailed("All retry attempts failed".to_string())))
    }

    /// Single attempt to start an FFmpeg process
    async fn try_start_process(
        &self,
        config: &ResolvedRelayConfig,
        input_url: &str,
    ) -> Result<FFmpegProcess, RelayError> {
        // Probe stream first if prober is available
        let mapping_strategy = if let Some(ref prober) = self.stream_prober {
            debug!("Probing input stream before starting FFmpeg: {}", input_url);
            match prober.probe_input(input_url).await {
                Ok(probe_result) => {
                    debug!("Stream probe successful: has_video={}, has_audio={}, video_streams={}, audio_streams={}", 
                           probe_result.has_video, probe_result.has_audio, 
                           probe_result.video_streams.len(), probe_result.audio_streams.len());
                    
                    // Generate optimal mapping strategy
                    let strategy = prober.generate_mapping_strategy(
                        &probe_result,
                        &config.profile.video_codec.to_string(),
                        &config.profile.audio_codec.to_string(),
                        config.profile.video_bitrate,
                        config.profile.audio_bitrate,
                    );
                    
                    debug!("Generated mapping strategy: video_mapping={:?}, audio_mapping={:?}, video_copy={}, audio_copy={}",
                           strategy.video_mapping, strategy.audio_mapping, strategy.video_copy, strategy.audio_copy);
                    
                    Some(strategy)
                }
                Err(e) => {
                    warn!("Stream probing failed for {}: {}. Falling back to traditional mapping.", input_url, e);
                    None
                }
            }
        } else {
            debug!("No stream prober available, using traditional mapping");
            None
        };

        // For cyclic buffer mode, we don't need sandbox directories since we stream to stdout
        // Generate complete FFmpeg command with hardware acceleration support
        let resolved_args = config.generate_ffmpeg_command_with_mapping(
            input_url,
            "", // No output path needed for stdout streaming
            &self.hwaccel_capabilities,
            mapping_strategy.as_ref(),
        );

        debug!("Starting FFmpeg process for relay {} with args: {:?}", 
               config.config.id, resolved_args);

        // Build FFmpeg command (use ffmpeg_command from config or default)
        let ffmpeg_command = "ffmpeg"; // TODO: Get from config
        let mut cmd = TokioCommand::new(ffmpeg_command);
        cmd.args(&resolved_args);
        cmd.kill_on_drop(true);
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        // Create shared error counter
        let error_count = Arc::new(AtomicU32::new(0));
        let restart_count = Arc::new(AtomicU32::new(0));
        let bytes_served = Arc::new(AtomicU64::new(0));

        // Start the process
        let mut child = cmd.spawn()
            .map_err(|e| RelayError::ProcessFailed(format!("Failed to spawn FFmpeg: {}", e)))?;

        // Create cyclic buffer for multi-client support using configured settings
        let buffer_config = CyclicBufferConfig {
            max_buffer_size: self.buffer_config.max_buffer_size,
            max_chunks: self.buffer_config.max_chunks,
            chunk_timeout: std::time::Duration::from_secs(self.buffer_config.chunk_timeout_seconds),
            client_timeout: std::time::Duration::from_secs(self.buffer_config.client_timeout_seconds),
            cleanup_interval: std::time::Duration::from_secs(self.buffer_config.cleanup_interval_seconds),
            enable_file_spill: self.buffer_config.enable_file_spill,
            max_file_spill_size: self.buffer_config.max_file_spill_size,
        };
        let cyclic_buffer = Arc::new(CyclicBuffer::new(buffer_config, Some(self.temp_manager.clone())));
        
        // Create error fallback system
        let fallback_config = ErrorFallbackConfig::default();
        let error_fallback = Arc::new(ErrorFallbackGenerator::new(fallback_config.clone(), cyclic_buffer.clone()));
        let health_monitor = Arc::new(StreamHealthMonitor::new(config.config.id, fallback_config));
        
        // Start monitoring stderr for errors
        if let Some(stderr) = child.stderr.take() {
            let config_id = config.config.id;
            let is_temporary = config.is_temporary;
            let metrics = self.metrics.clone();
            let error_count_clone = error_count.clone();
            let health_monitor_clone = health_monitor.clone();
            let error_fallback_clone = error_fallback.clone();
            
            tokio::spawn(async move {
                let reader = BufReader::new(stderr);
                let mut lines = reader.lines();
                
                while let Ok(Some(line)) = lines.next_line().await {
                    let line_lower = line.to_lowercase();
                    
                    if line_lower.contains("error") || line_lower.contains("failed") || 
                       line_lower.contains("invalid") || line_lower.contains("could not") ||
                       line_lower.contains("unable to") || line_lower.contains("not found") {
                        error!("FFmpeg error for relay {}: {}", config_id, line);
                        
                        // Increment error count
                        error_count_clone.fetch_add(1, Ordering::SeqCst);
                        
                        // Update health monitor and check for fallback
                        let health = health_monitor_clone.record_error(&line).await;
                        if health_monitor_clone.should_activate_fallback() {
                            warn!("Activating error fallback for relay {} due to health status: {:?}", config_id, health);
                            
                            // Start error fallback
                            if let Err(e) = error_fallback_clone.start_fallback(&line, config_id).await {
                                error!("Failed to start error fallback for relay {}: {}", config_id, e);
                            } else {
                                // Log fallback activation (only for persistent configs)
                                if !is_temporary {
                                    if let Err(e) = metrics.log_relay_event(
                                        config_id,
                                        RelayEventType::FallbackActivated,
                                        Some(&format!("Fallback activated due to: {}", line))
                                    ).await {
                                        error!("Failed to log fallback activation event: {}", e);
                                    }
                                }
                                
                                // Mark health monitor as in fallback mode
                                health_monitor_clone.mark_fallback(&line).await;
                            }
                        }
                        
                        // Log error event (only for persistent configs)
                        if !is_temporary {
                            if let Err(e) = metrics.log_relay_event(
                                config_id,
                                RelayEventType::Error,
                                Some(&line)
                            ).await {
                                error!("Failed to log relay error event: {}", e);
                            }
                        }
                    } else if line_lower.contains("warning") || line_lower.contains("deprecated") {
                        warn!("FFmpeg warning for relay {}: {}", config_id, line);
                    } else if line_lower.contains("opening") || line_lower.contains("input") ||
                              line_lower.contains("output") || line_lower.contains("stream") ||
                              line_lower.contains("encoder") || line_lower.contains("decoder") ||
                              line_lower.contains("fps=") || line_lower.contains("bitrate=") ||
                              line_lower.contains("time=") || line_lower.contains("speed=") {
                        info!("FFmpeg status for relay {}: {}", config_id, line);
                        
                        // Mark as healthy if we see good status messages
                        health_monitor_clone.mark_healthy().await;
                    } else if !line.trim().is_empty() {
                        debug!("FFmpeg output for relay {}: {}", config_id, line);
                    }
                }
            });
        }
        
        // Start reading from stdout and feeding to cyclic buffer
        if let Some(stdout) = child.stdout.take() {
            let buffer = cyclic_buffer.clone();
            let config_id = config.config.id;
            
            tokio::spawn(async move {
                let mut reader = tokio::io::BufReader::new(stdout);
                let mut buffer_bytes = vec![0u8; 8192];
                
                loop {
                    match reader.read(&mut buffer_bytes).await {
                        Ok(0) => {
                            info!("FFmpeg stdout ended for relay {}", config_id);
                            break;
                        }
                        Ok(n) => {
                            let chunk = bytes::Bytes::copy_from_slice(&buffer_bytes[..n]);
                            if let Err(e) = buffer.write_chunk(chunk).await {
                                error!("Failed to write to cyclic buffer for relay {}: {}", config_id, e);
                                break;
                            }
                        }
                        Err(e) => {
                            error!("Error reading FFmpeg stdout for relay {}: {}", config_id, e);
                            break;
                        }
                    }
                }
            });
        }

        // Store the process ID for monitoring
        let process_id = child.id();
        info!("Started FFmpeg process for relay {} with PID: {:?}", config.config.id, process_id);

        // Create process wrapper
        let process = FFmpegProcess {
            config: config.clone(),
            child,
            temp_manager: self.temp_manager.clone(),
            metrics: self.metrics.clone(),
            start_time: Instant::now(),
            client_count: AtomicU32::new(0),
            last_activity: Instant::now(),
            error_count: error_count.clone(),
            restart_count: restart_count.clone(),
            bytes_served: bytes_served.clone(),
            cyclic_buffer,
            error_fallback,
            health_monitor,
            input_url: input_url.to_string(),
            config_snapshot: config.create_config_snapshot(input_url),
        };

        info!("Started FFmpeg process for relay {} using profile '{}' with command: {:?}", 
              config.config.id, config.profile.name, resolved_args);

        Ok(process)
    }

    // Note: Hardware acceleration logic moved to ResolvedRelayConfig::generate_ffmpeg_command()
    // This provides better integration with the new codec-based profile system
}

/// Represents an active FFmpeg process
pub struct FFmpegProcess {
    pub config: ResolvedRelayConfig,
    pub child: tokio::process::Child,
    pub temp_manager: SandboxedManager,
    pub metrics: Arc<MetricsLogger>,
    pub start_time: Instant,
    pub client_count: AtomicU32,
    pub last_activity: Instant,
    pub error_count: Arc<AtomicU32>,
    pub restart_count: Arc<AtomicU32>,
    pub bytes_served: Arc<AtomicU64>,
    pub cyclic_buffer: Arc<CyclicBuffer>,
    pub error_fallback: Arc<ErrorFallbackGenerator>,
    pub health_monitor: Arc<StreamHealthMonitor>,
    pub input_url: String,
    pub config_snapshot: String,
}

impl FFmpegProcess {
    /// Serve content from the relay process using the cyclic buffer
    pub async fn serve_content(&mut self, path: &str, client_info: &ClientInfo) -> Result<RelayContent, RelayError> {
        // Update activity timestamp
        self.last_activity = Instant::now();
        
        // Create a relay session ID for tracking
        let relay_session_id = format!("relay_{}_{}", self.config.config.id, Uuid::new_v4());
        
        // Log client connection event (only for persistent configs)
        if !self.config.is_temporary {
            self.metrics.log_relay_event(
                self.config.config.id,
                RelayEventType::ClientConnect,
                Some(&format!("Client: {} - {}", client_info.ip, client_info.user_agent.as_deref().unwrap_or("unknown")))
            ).await.ok();
        }

        match self.config.profile.output_format {
            RelayOutputFormat::TransportStream => {
                self.serve_transport_stream_buffered(path, &relay_session_id, client_info).await
            }
            RelayOutputFormat::HLS => {
                self.serve_hls_content(path, &relay_session_id, client_info).await
            }
            RelayOutputFormat::Dash => {
                self.serve_dash_content(path, &relay_session_id, client_info).await
            }
            RelayOutputFormat::Copy => {
                self.serve_copy_content(path, &relay_session_id, client_info).await
            }
        }
    }
    
    /// Serve transport stream content using the cyclic buffer
    async fn serve_transport_stream_buffered(&self, path: &str, _session_id: &str, client_info: &ClientInfo) -> Result<RelayContent, RelayError> {
        if !path.is_empty() && path != "stream.ts" {
            return Err(RelayError::InvalidPath(path.to_string()));
        }
        
        // Add client to the cyclic buffer
        let client = self.cyclic_buffer.add_client(
            client_info.user_agent.clone(),
            Some(client_info.ip.clone())
        ).await;
        
        // Create a continuous streaming response
        self.create_streaming_response(client).await
    }
    
    /// Create a streaming response that continuously reads from the cyclic buffer
    async fn create_streaming_response(&self, client: Arc<crate::services::cyclic_buffer::BufferClient>) -> Result<RelayContent, RelayError> {
        
        
        let buffer = self.cyclic_buffer.clone();
        let client_id = client.id;
        
        tracing::info!("Starting continuous streaming response for client {}", client_id);
        
        // First, send any existing chunks
        let existing_chunks = buffer.read_chunks_for_client(&client).await;
        let mut initial_data = Vec::new();
        for chunk in existing_chunks {
            initial_data.extend_from_slice(&chunk.data);
        }
        
        if !initial_data.is_empty() {
            tracing::info!("Returning {} bytes from existing chunks for client {}", initial_data.len(), client_id);
        }
        
        // Create a simple stream implementation that is Unpin
        let relay_stream = RelayStream::new(buffer, client, initial_data);
        
        Ok(RelayContent::Stream(Box::new(relay_stream)))
    }
    



    /// Serve HLS content (playlist or segments)
    async fn serve_hls_content(&self, _path: &str, _session_id: &str, _client_info: &ClientInfo) -> Result<RelayContent, RelayError> {
        // HLS support not implemented for cyclic buffer mode
        Err(RelayError::UnsupportedFormat(RelayOutputFormat::HLS))
    }




    /// Serve DASH content (not implemented yet)
    async fn serve_dash_content(&self, _path: &str, _session_id: &str, _client_info: &ClientInfo) -> Result<RelayContent, RelayError> {
        Err(RelayError::UnsupportedFormat(RelayOutputFormat::Dash))
    }

    /// Serve copy content (not implemented yet)
    async fn serve_copy_content(&self, _path: &str, _session_id: &str, _client_info: &ClientInfo) -> Result<RelayContent, RelayError> {
        Err(RelayError::UnsupportedFormat(RelayOutputFormat::Copy))
    }



    /// Increment client count
    pub fn increment_client_count(&self) -> u32 {
        self.client_count.fetch_add(1, Ordering::Relaxed) + 1
    }

    /// Decrement client count
    pub fn decrement_client_count(&self) -> u32 {
        let remaining = self.client_count.fetch_sub(1, Ordering::Relaxed).saturating_sub(1);
        if remaining == 0 {
            // Log that all clients have disconnected
            let metrics = self.metrics.clone();
            let config_id = self.config.config.id;
            tokio::spawn(async move {
                metrics.log_relay_event(
                    config_id,
                    RelayEventType::ClientDisconnect,
                    Some("All clients disconnected")
                ).await.ok();
            });
        }
        remaining
    }

    /// Get current client count
    pub fn get_client_count(&self) -> u32 {
        self.client_count.load(Ordering::Relaxed)
    }

    /// Get process uptime
    pub fn get_uptime(&self) -> std::time::Duration {
        self.start_time.elapsed()
    }

    /// Get detailed process status for debugging
    pub fn get_status(&mut self) -> String {
        let uptime = self.get_uptime();
        let is_running = self.is_running();
        let client_count = self.get_client_count();
        let error_count = self.error_count.load(Ordering::Relaxed);
        let restart_count = self.restart_count.load(Ordering::Relaxed);
        let bytes_served = self.bytes_served.load(Ordering::Relaxed);
        
        format!(
            "FFmpeg Process Status for relay {}:\n\
            - Profile: {}\n\
            - Running: {}\n\
            - Uptime: {:?}\n\
            - Active clients: {}\n\
            - Error count: {}\n\
            - Restart count: {}\n\
            - Bytes served: {}\n\
            - Output: stdout streaming",
            self.config.config.id,
            self.config.profile.name,
            is_running,
            uptime,
            client_count,
            error_count,
            restart_count,
            bytes_served
        )
    }

    /// Check if process is still running
    pub fn is_running(&mut self) -> bool {
        match self.child.try_wait() {
            Ok(Some(status)) => {
                // Process has exited - log the exit status and potentially trigger fallback
                if status.success() {
                    info!("FFmpeg process for relay {} exited successfully", self.config.config.id);
                    let metrics = self.metrics.clone();
                    let config_id = self.config.config.id;
                    tokio::spawn(async move {
                        metrics.log_relay_event(
                            config_id,
                            RelayEventType::Stop,
                            Some("FFmpeg process exited successfully")
                        ).await.ok();
                    });
                } else {
                    error!("FFmpeg process for relay {} exited with error: {:?}", self.config.config.id, status);
                    
                    // Trigger error fallback for process exit
                    let error_message = format!("FFmpeg process exited with error: {:?}", status);
                    let health_monitor = self.health_monitor.clone();
                    let error_fallback = self.error_fallback.clone();
                    let metrics = self.metrics.clone();
                    let config_id = self.config.config.id;
                    
                    tokio::spawn(async move {
                        // Record the error in health monitor
                        health_monitor.record_error(&error_message).await;
                        
                        // Always trigger fallback on process exit
                        warn!("Activating error fallback for relay {} due to process exit: {:?}", config_id, status);
                        
                        if let Err(e) = error_fallback.start_fallback(&error_message, config_id).await {
                            error!("Failed to start error fallback for relay {}: {}", config_id, e);
                        } else {
                            // Log fallback activation
                            if let Err(e) = metrics.log_relay_event(
                                config_id,
                                RelayEventType::FallbackActivated,
                                Some(&format!("Fallback activated due to process exit: {:?}", status))
                            ).await {
                                error!("Failed to log fallback activation event: {}", e);
                            }
                            
                            // Mark health monitor as in fallback mode
                            health_monitor.mark_fallback(&error_message).await;
                        }
                        
                        // Log error event
                        metrics.log_relay_event(
                            config_id,
                            RelayEventType::Error,
                            Some(&error_message)
                        ).await.ok();
                    });
                }
                false
            }
            Ok(None) => true,     // Process is still running
            Err(e) => {
                error!("Error checking FFmpeg process status for relay {}: {}", self.config.config.id, e);
                
                // Trigger error fallback for process monitoring error
                let error_message = format!("Error checking FFmpeg process status: {}", e);
                let health_monitor = self.health_monitor.clone();
                let error_fallback = self.error_fallback.clone();
                let metrics = self.metrics.clone();
                let config_id = self.config.config.id;
                
                tokio::spawn(async move {
                    // Record the error and check if fallback should be triggered
                    health_monitor.record_error(&error_message).await;
                    if health_monitor.should_activate_fallback() {
                        warn!("Activating error fallback for relay {} due to process monitoring error", config_id);
                        
                        if let Err(e) = error_fallback.start_fallback(&error_message, config_id).await {
                            error!("Failed to start error fallback for relay {}: {}", config_id, e);
                        } else {
                            // Log fallback activation
                            if let Err(e) = metrics.log_relay_event(
                                config_id,
                                RelayEventType::FallbackActivated,
                                Some(&format!("Fallback activated due to monitoring error: {}", e))
                            ).await {
                                error!("Failed to log fallback activation event: {}", e);
                            }
                            
                            // Mark health monitor as in fallback mode
                            health_monitor.mark_fallback(&error_message).await;
                        }
                    }
                });
                
                false
            }
        }
    }

    /// Kill the FFmpeg process
    pub async fn kill(&mut self) -> Result<(), RelayError> {
        // Stop error fallback
        self.error_fallback.stop_fallback();
        
        if let Err(e) = self.child.kill().await {
            warn!("Failed to kill FFmpeg process for relay {}: {}", self.config.config.id, e);
            return Err(RelayError::ProcessFailed(format!("Failed to kill process: {}", e)));
        }

        // Log stop event (only for persistent configs)
        if !self.config.is_temporary {
            self.metrics.log_relay_event(
                self.config.config.id,
                RelayEventType::Stop,
                Some("Process killed")
            ).await.ok();
        }

        // Log fallback deactivation if it was active (only for persistent configs)
        if self.error_fallback.is_fallback_active() && !self.config.is_temporary {
            self.metrics.log_relay_event(
                self.config.config.id,
                RelayEventType::FallbackDeactivated,
                Some("Fallback stopped due to process termination")
            ).await.ok();
        }

        info!("Killed FFmpeg process for relay {}", self.config.config.id);
        Ok(())
    }
}

impl Drop for FFmpegProcess {
    fn drop(&mut self) {
        // Stop error fallback
        self.error_fallback.stop_fallback();
        
        // Ensure the child process is killed when the wrapper is dropped
        if let Err(e) = self.child.start_kill() {
            warn!("Failed to kill FFmpeg process in drop: {}", e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use uuid::Uuid;

    fn create_test_config() -> ResolvedRelayConfig {
        let profile = RelayProfile {
            id: Uuid::new_v4(),
            name: "Test Profile".to_string(),
            description: None,
            
            // Codec settings
            video_codec: VideoCodec::H264,
            audio_codec: AudioCodec::AAC,
            video_profile: Some("main".to_string()),
            video_preset: Some("fast".to_string()),
            video_bitrate: Some(2000),
            audio_bitrate: Some(128),
            audio_sample_rate: Some(48000),
            audio_channels: Some(2),
            
            // Hardware acceleration
            enable_hardware_acceleration: false,
            preferred_hwaccel: None,
            
            // Manual override
            manual_args: None,
            
            // Container settings
            output_format: RelayOutputFormat::TransportStream,
            segment_duration: None,
            max_segments: None,
            input_timeout: 30,
            
            // System flags
            is_system_default: false,
            is_active: true,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        let config = ChannelRelayConfig {
            id: Uuid::new_v4(),
            proxy_id: Uuid::new_v4(),
            channel_id: Uuid::new_v4(),
            profile_id: profile.id,
            name: "Test Config".to_string(),
            description: None,
            custom_args: None,
            is_active: true,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        ResolvedRelayConfig::new(config, profile).unwrap()
    }

    #[test]
    fn test_template_variable_resolution() {
        let config = create_test_config();
        let resolved = config.resolve_template_variables(
            "http://example.com/stream.ts",
            "/tmp/output"
        );

        assert_eq!(resolved, vec![
            "-i", "http://example.com/stream.ts",
            "-c", "copy",
            "-f", "mpegts",
            "-y", "/tmp/output/stream.ts"
        ]);
    }

}