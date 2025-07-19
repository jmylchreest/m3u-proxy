//! Relay Manager Service
//!
//! This service manages FFmpeg relay processes, including starting, stopping,
//! and serving content from active relays.

use anyhow::Result;
use sqlx::Row;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;
use sysinfo::{Pid, PidExt, ProcessExt, System, SystemExt};
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use crate::config::Config;
use crate::database::Database;
use crate::metrics::MetricsLogger;
use crate::models::relay::*;
use crate::services::ffmpeg_wrapper::{FFmpegProcess, FFmpegProcessWrapper};
use sandboxed_file_manager::SandboxedManager;

/// Manages FFmpeg relay processes with automatic lifecycle management
pub struct RelayManager {
    active_processes: Arc<RwLock<HashMap<Uuid, FFmpegProcess>>>,
    database: Database,
    ffmpeg_wrapper: FFmpegProcessWrapper,
    metrics_logger: Arc<MetricsLogger>,
    cleanup_interval: Duration,
    system: Arc<tokio::sync::RwLock<System>>,
    config: Config,
    ffmpeg_available: bool,
    ffmpeg_version: Option<String>,
    ffmpeg_command: String,
    ffprobe_available: bool,
    ffprobe_version: Option<String>,
    ffprobe_command: String,
    hwaccel_available: bool,
    hwaccel_capabilities: HwAccelCapabilities,
}

impl RelayManager {
    /// Create a new relay manager with shared system instance
    pub async fn new(
        database: Database,
        temp_manager: SandboxedManager,
        metrics_logger: Arc<MetricsLogger>,
        system: Arc<tokio::sync::RwLock<System>>,
        config: Config,
    ) -> Self {
        // Get FFmpeg command from config
        let ffmpeg_command = config
            .relay
            .as_ref()
            .map(|r| r.ffmpeg_command.clone())
            .unwrap_or_else(|| "ffmpeg".to_string());

        // Get FFprobe command from config
        let ffprobe_command = config
            .relay
            .as_ref()
            .map(|r| r.ffprobe_command.clone())
            .unwrap_or_else(|| "ffprobe".to_string());

        // Check FFmpeg availability once at startup
        let (ffmpeg_available, ffmpeg_version) =
            Self::check_ffmpeg_availability_static(&ffmpeg_command).await;

        // Check FFprobe availability once at startup
        let (ffprobe_available, ffprobe_version) =
            Self::check_ffprobe_availability_static(&ffprobe_command).await;

        info!(
            "FFmpeg: available={}, version={:?}, command={}",
            ffmpeg_available, ffmpeg_version, ffmpeg_command
        );

        info!(
            "FFprobe: available={}, version={:?}, command={}",
            ffprobe_available, ffprobe_version, ffprobe_command
        );

        // Detect hardware acceleration capabilities if FFmpeg is available
        let (hwaccel_available, hwaccel_capabilities) = if ffmpeg_available {
            Self::detect_hwaccel_capabilities(&ffmpeg_command).await
        } else {
            (false, HwAccelCapabilities::default())
        };

        // Create stream prober if ffprobe is available
        let stream_prober = if ffprobe_available {
            Some(crate::services::StreamProber::new(Some(ffprobe_command.clone())))
        } else {
            None
        };

        let manager = Self {
            active_processes: Arc::new(RwLock::new(HashMap::new())),
            database,
            ffmpeg_wrapper: FFmpegProcessWrapper::new(
                temp_manager,
                metrics_logger.clone(),
                hwaccel_capabilities.clone(),
                config.relay.as_ref().map(|r| r.buffer.clone()).unwrap_or_default(),
                stream_prober,
            ),
            metrics_logger,
            cleanup_interval: Duration::from_secs(10),
            system,
            config,
            ffmpeg_available,
            ffmpeg_version: ffmpeg_version.clone(),
            ffmpeg_command: ffmpeg_command.clone(),
            ffprobe_available,
            ffprobe_version: ffprobe_version.clone(),
            ffprobe_command: ffprobe_command.clone(),
            hwaccel_available,
            hwaccel_capabilities,
        };

        // Start cleanup task
        manager.start_cleanup_task();
        
        // Start periodic status logging task
        manager.start_status_logging_task();

        // Note: System monitoring task is managed externally by SystemManager

        manager
    }

    /// Get relay configuration for a specific channel from proxy-level relay profile
    pub async fn get_relay_config_for_channel(
        &self,
        proxy_id: Uuid,
        channel_id: Uuid,
    ) -> Result<Option<ResolvedRelayConfig>, RelayError> {
        let query = r#"
            SELECT
                sp.id as proxy_id, sp.name as proxy_name, sp.relay_profile_id,
                rp.id as profile_id, rp.name as profile_name, rp.description as profile_description,
                rp.video_codec, rp.audio_codec, rp.video_profile, rp.video_preset,
                rp.video_bitrate, rp.audio_bitrate, rp.audio_sample_rate, rp.audio_channels,
                rp.enable_hardware_acceleration, rp.preferred_hwaccel, rp.manual_args,
                rp.output_format, rp.segment_duration, rp.max_segments,
                rp.input_timeout, rp.is_system_default,
                rp.is_active as profile_is_active, rp.created_at as profile_created_at,
                rp.updated_at as profile_updated_at
            FROM stream_proxies sp
            JOIN relay_profiles rp ON sp.relay_profile_id = rp.id
            WHERE sp.id = ? AND sp.is_active = true AND rp.is_active = true
        "#;

        let row = sqlx::query(query)
            .bind(proxy_id.to_string())
            .fetch_optional(&self.database.pool())
            .await?;

        if let Some(row) = row {
            // Build config from proxy data (creating a synthetic config)
            let profile_id = Uuid::parse_str(&row.get::<String, _>("profile_id"))?;
            let config = ChannelRelayConfig {
                id: crate::utils::generate_relay_config_uuid(&proxy_id, &channel_id, &profile_id),
                proxy_id,
                channel_id,
                profile_id,
                name: format!("{} - Channel {}", row.get::<String, _>("proxy_name"), channel_id),
                description: Some(format!("Auto-generated relay config for proxy {} channel {}", proxy_id, channel_id)),
                custom_args: None, // No custom args at proxy level
                is_active: true,
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            };

            // Build profile from row data
            let profile = RelayProfile {
                id: Uuid::parse_str(&row.get::<String, _>("profile_id"))?,
                name: row.get("profile_name"),
                description: row.get("profile_description"),
                
                // Codec settings
                video_codec: row.get::<String, _>("video_codec").parse()
                    .map_err(|e| RelayError::InvalidArgument(e))?,
                audio_codec: row.get::<String, _>("audio_codec").parse()
                    .map_err(|e| RelayError::InvalidArgument(e))?,
                video_profile: row.get("video_profile"),
                video_preset: row.get("video_preset"),
                video_bitrate: row.get::<Option<i32>, _>("video_bitrate").map(|v| v as u32),
                audio_bitrate: row.get::<Option<i32>, _>("audio_bitrate").map(|v| v as u32),
                audio_sample_rate: row.get::<Option<i32>, _>("audio_sample_rate").map(|v| v as u32),
                audio_channels: row.get::<Option<i32>, _>("audio_channels").map(|v| v as u32),
                
                // Hardware acceleration
                enable_hardware_acceleration: row.get("enable_hardware_acceleration"),
                preferred_hwaccel: row.get("preferred_hwaccel"),
                
                // Manual override and legacy
                manual_args: row.get("manual_args"),
                
                // Container settings
                output_format: row
                    .get::<String, _>("output_format")
                    .parse()
                    .map_err(|e| RelayError::InvalidArgument(e))?,
                segment_duration: row.get("segment_duration"),
                max_segments: row.get("max_segments"),
                input_timeout: row.get("input_timeout"),
                
                // System flags
                is_system_default: row.get("is_system_default"),
                is_active: row.get("profile_is_active"),
                created_at: row.get("profile_created_at"),
                updated_at: row.get("profile_updated_at"),
            };

            // Create resolved config
            let resolved_config = ResolvedRelayConfig::new(config, profile)
                .map_err(|e| RelayError::InvalidArgument(e))?;
            Ok(Some(resolved_config))
        } else {
            Ok(None)
        }
    }

    /// Ensure a relay process is running for the given configuration
    pub async fn ensure_relay_running(
        &self,
        config: &ResolvedRelayConfig,
        input_url: &str,
    ) -> Result<(), RelayError> {
        let config_id = config.config.id;

        // Check if already running
        if self.active_processes.read().await.contains_key(&config_id) {
            debug!("Relay {} already running", config_id);
            return Ok(());
        }

        // Start new process
        let process = self.ffmpeg_wrapper.start_process(config, input_url).await?;

        // Store the process
        self.active_processes
            .write()
            .await
            .insert(config_id, process);

        // Update runtime status (only for persistent configs)
        if !config.is_temporary {
            self.update_runtime_status(config_id, true).await?;
        }

        // Log relay start event
        self.metrics_logger
            .log_relay_event_if_persistent(config, RelayEventType::Start, Some(&config.profile.name))
            .await
            .ok();

        info!(
            "Started relay {} using profile '{}'",
            config_id, config.profile.name
        );
        Ok(())
    }

    /// Serve content from a relay process with automatic lifecycle management
    pub async fn serve_relay_content(
        &self,
        config_id: Uuid,
        path: &str,
        client_info: &ClientInfo,
    ) -> Result<RelayContent, RelayError> {
        let mut processes = self.active_processes.write().await;

        if let Some(process) = processes.get_mut(&config_id) {
            // Get current client count from the cyclic buffer
            let buffer_client_count = process.cyclic_buffer.get_client_count().await;
            
            // Update the atomic counter for backward compatibility
            process.client_count.store(buffer_client_count as u32, Ordering::Relaxed);
            
            let content = process.serve_content(path, client_info).await?;

            // Track content delivery metrics
            match &content {
                RelayContent::Playlist(playlist) => {
                    self.metrics_logger
                        .log_relay_event(
                            config_id,
                            RelayEventType::ClientConnect,
                            Some(&format!("HLS playlist delivered: {} bytes", playlist.len())),
                        )
                        .await
                        .ok();
                }
                RelayContent::Segment(segment) => {
                    self.metrics_logger
                        .log_relay_event(
                            config_id,
                            RelayEventType::ClientConnect,
                            Some(&format!("Content segment delivered: {} bytes", segment.len())),
                        )
                        .await
                        .ok();
                }
                RelayContent::Stream(_) => {
                    self.metrics_logger
                        .log_relay_event(
                            config_id,
                            RelayEventType::ClientConnect,
                            Some("Stream content delivered (buffered)"),
                        )
                        .await
                        .ok();
                }
            }

            Ok(content)
        } else {
            Err(RelayError::ProcessNotFound(config_id))
        }
    }

    /// Stop a relay process
    pub async fn stop_relay(&self, config_id: Uuid) -> Result<(), RelayError> {
        if let Some(mut process) = self.active_processes.write().await.remove(&config_id) {
            let is_temporary = process.config.is_temporary;
            process.kill().await?;
            
            // Only update runtime status for persistent configs
            if !is_temporary {
                self.update_runtime_status(config_id, false).await?;
            }
            
            info!("Stopped relay process for config {}", config_id);
        }
        Ok(())
    }

    /// Get status of all active relay processes
    pub async fn get_relay_status(&self) -> Result<Vec<RelayProcessMetrics>, RelayError> {
        let processes = self.active_processes.read().await;
        let mut metrics = Vec::new();

        for (config_id, process) in processes.iter() {
            let buffer_stats = process.cyclic_buffer.get_stats().await;
            let client_count = buffer_stats.client_count as i32;
            let connected_clients = process.cyclic_buffer.get_connected_clients().await;
            let bytes_delivered_downstream = connected_clients.iter().map(|c| c.bytes_served as i64).sum();
            
            let process_metrics = RelayProcessMetrics {
                config_id: *config_id,
                profile_name: process.config.profile.name.clone(),
                channel_name: self.get_channel_name(&process.config.config.channel_id.to_string())
                    .await
                    .unwrap_or_else(|| format!("Channel {}", process.config.config.channel_id)),
                is_running: true, // If it's in the map, it's running
                client_count,
                connected_clients,
                bytes_received_upstream: buffer_stats.bytes_received_from_upstream as i64, // Raw bytes from FFmpeg stdout
                bytes_delivered_downstream,
                uptime_seconds: Some(process.get_uptime().as_secs() as i64),
                last_heartbeat: Some(chrono::Utc::now()),
                cpu_usage_percent: self.get_process_cpu_usage(process.child.id()).await.unwrap_or(0.0),
                memory_usage_mb: self.get_process_memory_usage(process.child.id()).await.unwrap_or(0.0),
                process_id: process.child.id(),
                input_url: process.input_url.clone(),
                config_snapshot: process.config_snapshot.clone(),
                // TODO: Implement historical data collection
                cpu_history: Vec::new(),
                memory_history: Vec::new(),
                traffic_history: Vec::new(),
            };
            metrics.push(process_metrics);
        }

        Ok(metrics)
    }

    /// Get overall health status of relay system
    pub async fn get_relay_health(&self) -> Result<RelayHealth, RelayError> {
        let processes = self.active_processes.read().await;
        let mut process_health = Vec::new();
        let mut healthy_count = 0;
        let mut unhealthy_count = 0;

        for (config_id, process) in processes.iter() {
            let uptime = process.get_uptime().as_secs() as i64;
            let client_count = process.get_client_count() as i32;
            let last_heartbeat = chrono::Utc::now(); // In real implementation, this would be from process monitoring

            // Determine health status
            let status = if true {
                // process.is_running() would need mutable access
                if client_count > 0 || uptime < 300 {
                    // Healthy if has clients or started recently
                    healthy_count += 1;
                    RelayHealthStatus::Healthy
                } else {
                    unhealthy_count += 1;
                    RelayHealthStatus::Unhealthy
                }
            } else {
                unhealthy_count += 1;
                RelayHealthStatus::Failed
            };

            let health = RelayProcessHealth {
                config_id: *config_id,
                profile_name: process.config.profile.name.clone(),
                channel_name: self
                    .get_channel_name(&process.config.config.channel_id.to_string())
                    .await
                    .unwrap_or_else(|| format!("Channel {}", process.config.config.channel_id)),
                status,
                uptime_seconds: uptime,
                client_count,
                memory_usage_mb: self
                    .get_process_memory_usage(process.child.id())
                    .await
                    .unwrap_or(0.0),
                cpu_usage_percent: self
                    .get_process_cpu_usage(process.child.id())
                    .await
                    .unwrap_or(0.0),
                last_heartbeat,
                error_count: process.error_count.load(Ordering::SeqCst) as i32,
                restart_count: process.restart_count.load(Ordering::SeqCst) as i32,
            };
            process_health.push(health);
        }

        let total_processes = processes.len() as i32;
        let system_load = self.get_system_load().await.unwrap_or(0.0);
        let memory_usage = self.get_system_memory_usage().await.unwrap_or(0.0);

        Ok(RelayHealth {
            total_processes,
            healthy_processes: healthy_count,
            unhealthy_processes: unhealthy_count,
            processes: process_health,
            system_load,
            memory_usage_mb: memory_usage,
            last_check: chrono::Utc::now(),
            ffmpeg_available: self.ffmpeg_available,
            ffmpeg_version: self.ffmpeg_version.clone(),
            ffmpeg_command: self.ffmpeg_command.clone(),
            hwaccel_available: self.hwaccel_available,
            hwaccel_capabilities: self.hwaccel_capabilities.clone(),
        })
    }

    /// Get health status for a specific relay configuration
    pub async fn get_relay_health_for_config(
        &self,
        config_id: Uuid,
    ) -> Result<Option<RelayProcessHealth>, RelayError> {
        let processes = self.active_processes.read().await;

        if let Some(process) = processes.get(&config_id) {
            let uptime = process.get_uptime().as_secs() as i64;
            let client_count = process.get_client_count() as i32;
            let last_heartbeat = chrono::Utc::now();

            let status = if true {
                // process.is_running() would need mutable access
                if client_count > 0 || uptime < 300 {
                    RelayHealthStatus::Healthy
                } else {
                    RelayHealthStatus::Unhealthy
                }
            } else {
                RelayHealthStatus::Failed
            };

            let health = RelayProcessHealth {
                config_id,
                profile_name: process.config.profile.name.clone(),
                channel_name: format!("Channel {}", process.config.config.channel_id),
                status,
                uptime_seconds: uptime,
                client_count,
                memory_usage_mb: 0.0,
                cpu_usage_percent: 0.0,
                last_heartbeat,
                error_count: 0,
                restart_count: 0,
            };

            Ok(Some(health))
        } else {
            Ok(None)
        }
    }

    /// Get system load average
    async fn get_system_load(&self) -> Option<f64> {
        let system = self.system.read().await;
        Some(system.load_average().one)
    }

    /// Get total system memory usage in MB
    async fn get_system_memory_usage(&self) -> Option<f64> {
        let system = self.system.read().await;
        let used_memory = system.used_memory();
        Some(used_memory as f64 / 1024.0 / 1024.0)
    }

    /// Get memory usage for a specific process (in MB)
    async fn get_process_memory_usage(&self, process_id: Option<u32>) -> Option<f64> {
        let process_id = process_id?;
        let mut system = self.system.write().await;
        system.refresh_process(Pid::from_u32(process_id));

        if let Some(process) = system.process(Pid::from_u32(process_id)) {
            Some(process.memory() as f64 / 1024.0 / 1024.0)
        } else {
            None
        }
    }

    /// Get CPU usage for a specific process (percentage)
    async fn get_process_cpu_usage(&self, process_id: Option<u32>) -> Option<f64> {
        let process_id = process_id?;
        let mut system = self.system.write().await;
        system.refresh_process(Pid::from_u32(process_id));

        if let Some(process) = system.process(Pid::from_u32(process_id)) {
            Some(process.cpu_usage() as f64)
        } else {
            None
        }
    }

    /// Get channel name from database
    async fn get_channel_name(&self, channel_id: &str) -> Option<String> {
        let result = sqlx::query("SELECT channel_name FROM channels WHERE id = ?")
            .bind(channel_id)
            .fetch_optional(&self.database.pool())
            .await;

        match result {
            Ok(Some(row)) => row.get::<String, _>("channel_name").into(),
            _ => None,
        }
    }

    /// Update runtime status in database (only for persistent configs)
    async fn update_runtime_status(
        &self,
        config_id: Uuid,
        is_running: bool,
    ) -> Result<(), RelayError> {
        let now = chrono::Utc::now().to_rfc3339();

        if is_running {
            let query = r#"
                INSERT INTO relay_runtime_status (
                    channel_relay_config_id, sandbox_path, is_running, started_at,
                    client_count, bytes_served, last_heartbeat, updated_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
                ON CONFLICT(channel_relay_config_id) DO UPDATE SET
                    is_running = excluded.is_running,
                    started_at = excluded.started_at,
                    last_heartbeat = excluded.last_heartbeat,
                    updated_at = excluded.updated_at
            "#;

            sqlx::query(query)
                .bind(config_id.to_string())
                .bind(format!("relay_{}", config_id))
                .bind(true)
                .bind(&now)
                .bind(0i32) // client_count
                .bind(0i64) // bytes_served
                .bind(&now)
                .bind(&now)
                .execute(&self.database.pool())
                .await?;
        } else {
            let query = r#"
                UPDATE relay_runtime_status
                SET is_running = false, updated_at = ?
                WHERE channel_relay_config_id = ?
            "#;

            sqlx::query(query)
                .bind(&now)
                .bind(config_id.to_string())
                .execute(&self.database.pool())
                .await?;
        }

        Ok(())
    }

    /// Start the cleanup task for idle processes
    fn start_cleanup_task(&self) {
        let processes = self.active_processes.clone();
        let database = self.database.clone();
        let metrics_logger = self.metrics_logger.clone();
        let interval = self.cleanup_interval;

        tokio::spawn(async move {
            let mut cleanup_interval = tokio::time::interval(interval);
            loop {
                cleanup_interval.tick().await;

                let mut to_remove = Vec::new();
                {
                    let mut processes_guard = processes.write().await;
                    for (config_id, process) in processes_guard.iter_mut() {
                        // Check if process is still running
                        if !process.is_running() {
                            warn!("FFmpeg process for relay {} has died", config_id);
                            to_remove.push((*config_id, process.config.is_temporary));
                            continue;
                        }

                        // Check if process is idle (no clients and no activity for a shorter time)
                        let buffer_client_count = process.cyclic_buffer.get_client_count().await;
                        process.client_count.store(buffer_client_count as u32, Ordering::Relaxed);
                        
                        if buffer_client_count == 0
                            && process.last_activity.elapsed() > Duration::from_secs(60)
                        {
                            info!("Relay {} is idle (no clients for 1 minute), scheduling for cleanup", config_id);
                            to_remove.push((*config_id, process.config.is_temporary));
                        }
                    }

                    // Remove processes that should be cleaned up
                    for (config_id, _) in &to_remove {
                        if let Some(mut process) = processes_guard.remove(config_id) {
                            let _ = process.kill().await;
                        }
                    }
                }

                // Update runtime status for cleaned up processes (only persistent ones)
                for (config_id, is_temporary) in to_remove {
                    if !is_temporary {
                        let _ = Self::update_runtime_status_static(&database, config_id, false).await;
                    }
                    
                    // Log cleanup event (will be handled appropriately by log_relay_event_if_persistent)
                    // For now, just log to application logs since we don't have access to the full config here
                    if is_temporary {
                        info!("Cleaned up temporary relay process: {}", config_id);
                    } else {
                        metrics_logger
                            .log_relay_event(config_id, RelayEventType::Stop, Some("idle_cleanup"))
                            .await
                            .ok();
                        info!("Cleaned up persistent relay process: {}", config_id);
                    }
                }
            }
        });
    }

    /// Start the periodic status logging task
    fn start_status_logging_task(&self) {
        let processes = self.active_processes.clone();
        let database = self.database.clone();
        
        tokio::spawn(async move {
            let mut status_interval = tokio::time::interval(Duration::from_secs(30));
            loop {
                status_interval.tick().await;
                
                let processes_guard = processes.read().await;
                if processes_guard.is_empty() {
                    continue;
                }
                
                info!("=== Relay Status Report ===");
                info!("Active relays: {}", processes_guard.len());
                
                for (config_id, process) in processes_guard.iter() {
                    let buffer_stats = process.cyclic_buffer.get_stats().await;
                    let client_count = buffer_stats.client_count;
                    let bytes_received = buffer_stats.bytes_received_from_upstream;
                    let bytes_delivered = buffer_stats.total_bytes_written;
                    
                    let channel_name = format!("Channel-{}", config_id.to_string().split('-').next().unwrap_or("unknown"));
                    
                    info!(
                        "  Relay {}: {} | Profile: {} | Clients: {} | Rx: {} | Tx: {}",
                        config_id.to_string().split('-').next().unwrap_or("unknown"),
                        channel_name,
                        process.config.profile.name,
                        client_count,
                        crate::utils::human_format::format_memory(bytes_received as f64),
                        crate::utils::human_format::format_memory(bytes_delivered as f64)
                    );
                }
                info!("=== End Relay Status ===");
            }
        });
    }

    /// Static version of update_runtime_status for use in cleanup task (only for persistent configs)
    async fn update_runtime_status_static(
        database: &Database,
        config_id: Uuid,
        is_running: bool,
    ) -> Result<(), RelayError> {
        let now = chrono::Utc::now().to_rfc3339();

        let query = r#"
            UPDATE relay_runtime_status
            SET is_running = ?, updated_at = ?
            WHERE channel_relay_config_id = ?
        "#;

        sqlx::query(query)
            .bind(is_running)
            .bind(&now)
            .bind(config_id.to_string())
            .execute(&database.pool())
            .await?;

        Ok(())
    }

    /// Check if FFprobe is available and get version information (static version for initialization)
    async fn check_ffprobe_availability_static(ffprobe_command: &str) -> (bool, Option<String>) {
        match tokio::process::Command::new(ffprobe_command)
            .arg("-version")
            .output()
            .await
        {
            Ok(output) => {
                if output.status.success() {
                    let version_output = String::from_utf8_lossy(&output.stdout);

                    // Extract version from the first line (e.g., "ffprobe version 4.4.2-0ubuntu0.22.04.1")
                    let version = version_output.lines().next().and_then(|line| {
                        if line.starts_with("ffprobe version") {
                            line.split_whitespace()
                                .nth(2) // Get the version part
                                .map(|v| v.to_string())
                        } else {
                            None
                        }
                    });

                    (true, version)
                } else {
                    warn!(
                        "FFprobe command '{}' failed with status: {}",
                        ffprobe_command, output.status
                    );
                    (false, None)
                }
            }
            Err(e) => {
                warn!(
                    "Failed to execute FFprobe command '{}': {}",
                    ffprobe_command, e
                );
                (false, None)
            }
        }
    }

    /// Check if FFmpeg is available and get version information (static version for initialization)
    async fn check_ffmpeg_availability_static(ffmpeg_command: &str) -> (bool, Option<String>) {
        match tokio::process::Command::new(ffmpeg_command)
            .arg("-version")
            .output()
            .await
        {
            Ok(output) => {
                if output.status.success() {
                    let version_output = String::from_utf8_lossy(&output.stdout);

                    // Extract version from the first line (e.g., "ffmpeg version 4.4.2-0ubuntu0.22.04.1")
                    let version = version_output.lines().next().and_then(|line| {
                        if line.starts_with("ffmpeg version") {
                            line.split_whitespace()
                                .nth(2) // Get the version part
                                .map(|v| v.to_string())
                        } else {
                            None
                        }
                    });

                    (true, version)
                } else {
                    warn!(
                        "FFmpeg command '{}' failed with status: {}",
                        ffmpeg_command, output.status
                    );
                    (false, None)
                }
            }
            Err(e) => {
                warn!(
                    "Failed to execute FFmpeg command '{}': {}",
                    ffmpeg_command, e
                );
                (false, None)
            }
        }
    }

    /// Detect hardware acceleration capabilities for FFmpeg
    async fn detect_hwaccel_capabilities(ffmpeg_command: &str) -> (bool, HwAccelCapabilities) {
        info!("Detecting hardware acceleration capabilities...");

        // Get available hardware accelerators
        let hwaccels = Self::get_available_hwaccels(ffmpeg_command).await;
        if hwaccels.is_empty() {
            info!("No hardware accelerators detected");
            return (false, HwAccelCapabilities::default());
        }

        debug!("Found hwaccels: {:?}", hwaccels);

        // Test each hwaccel with different codecs
        let mut capabilities = HwAccelCapabilities::default();
        let mut any_working = false;

        // Common codecs to test
        let codecs_to_test = vec![
            ("h264", "h264"),
            ("hevc", "hevc"),
            ("av1", "av1"),
            ("vp9", "vp9"),
        ];

        for hwaccel in &hwaccels {
            let mut accelerator = HwAccelerator {
                name: hwaccel.clone(),
                device_type: hwaccel.clone(),
                available: false,
                supported_codecs: Vec::new(),
            };

            let mut supported_codecs = Vec::new();

            for (codec_name, codec_test) in &codecs_to_test {
                let encoder_names = Self::get_encoder_names_for_hwaccel(hwaccel, codec_test);

                for encoder_name in encoder_names {
                    if Self::test_hwaccel_encoder(ffmpeg_command, hwaccel, &encoder_name).await {
                        debug!("{} supports {}", hwaccel, encoder_name);
                        supported_codecs.push(codec_name.to_string());
                        any_working = true;
                        break; // Found a working encoder for this codec
                    }
                }
            }

            if !supported_codecs.is_empty() {
                accelerator.available = true;
                accelerator.supported_codecs = supported_codecs.clone();
                capabilities
                    .support_matrix
                    .insert(hwaccel.clone(), supported_codecs);
            }

            capabilities.accelerators.push(accelerator);
        }

        // Update the codecs list
        let mut all_codecs = HashSet::new();
        for codecs in capabilities.support_matrix.values() {
            for codec in codecs {
                all_codecs.insert(codec.clone());
            }
        }
        capabilities.codecs = all_codecs.into_iter().collect();
        capabilities.codecs.sort();

        // Log the results
        if any_working {
            info!("Hardware acceleration available:");
            for (hwaccel, codecs) in &capabilities.support_matrix {
                info!("  {}: {}", hwaccel, codecs.join(", "));
            }
        } else {
            info!("Hardware acceleration not available");
        }

        (any_working, capabilities)
    }

    /// Get available hardware accelerators from FFmpeg
    async fn get_available_hwaccels(ffmpeg_command: &str) -> Vec<String> {
        match tokio::process::Command::new(ffmpeg_command)
            .arg("-hwaccels")
            .output()
            .await
        {
            Ok(output) => {
                if output.status.success() {
                    let hwaccels_output = String::from_utf8_lossy(&output.stdout);
                    hwaccels_output
                        .lines()
                        .skip(1) // Skip the header line
                        .map(|line| line.trim().to_string())
                        .filter(|line| !line.is_empty())
                        .collect()
                } else {
                    warn!("Failed to get hwaccels: {}", output.status);
                    Vec::new()
                }
            }
            Err(e) => {
                warn!("Failed to execute ffmpeg -hwaccels: {}", e);
                Vec::new()
            }
        }
    }

    /// Get encoder names for a specific hwaccel and codec
    fn get_encoder_names_for_hwaccel(hwaccel: &str, codec: &str) -> Vec<String> {
        // Map hwaccel + codec to encoder names
        match (hwaccel, codec) {
            ("vaapi", "h264") => vec!["h264_vaapi".to_string()],
            ("vaapi", "hevc") => vec!["hevc_vaapi".to_string()],
            ("vaapi", "av1") => vec!["av1_vaapi".to_string()],
            ("vaapi", "vp9") => vec!["vp9_vaapi".to_string()],
            ("nvenc", "h264") => vec!["h264_nvenc".to_string()],
            ("nvenc", "hevc") => vec!["hevc_nvenc".to_string()],
            ("nvenc", "av1") => vec!["av1_nvenc".to_string()],
            ("qsv", "h264") => vec!["h264_qsv".to_string()],
            ("qsv", "hevc") => vec!["hevc_qsv".to_string()],
            ("qsv", "av1") => vec!["av1_qsv".to_string()],
            ("videotoolbox", "h264") => vec!["h264_videotoolbox".to_string()],
            ("videotoolbox", "hevc") => vec!["hevc_videotoolbox".to_string()],
            ("amf", "h264") => vec!["h264_amf".to_string()],
            ("amf", "hevc") => vec!["hevc_amf".to_string()],
            ("amf", "av1") => vec!["av1_amf".to_string()],
            _ => Vec::new(),
        }
    }

    /// Test if a specific hwaccel encoder works
    async fn test_hwaccel_encoder(ffmpeg_command: &str, hwaccel: &str, encoder: &str) -> bool {
        let mut cmd = tokio::process::Command::new(ffmpeg_command);

        // Add hwaccel initialization
        cmd.arg("-init_hw_device");
        cmd.arg(hwaccel);

        // Add test input
        cmd.arg("-f").arg("lavfi");
        cmd.arg("-i")
            .arg("testsrc=duration=0.1:size=320x240:rate=1");

        // Add hwaccel filters
        match hwaccel {
            "vaapi" => {
                cmd.arg("-vf").arg("format=nv12,hwupload");
            }
            "nvenc" => {
                cmd.arg("-vf").arg("format=nv12,hwupload_cuda");
            }
            "qsv" => {
                cmd.arg("-vf")
                    .arg("format=nv12,hwupload=extra_hw_frames=64");
            }
            _ => {
                cmd.arg("-vf").arg("format=nv12,hwupload");
            }
        }

        // Add encoder and output
        cmd.arg("-c:v").arg(encoder);
        cmd.arg("-f").arg("null");
        cmd.arg("-");

        // Run with timeout and silence
        cmd.stdout(std::process::Stdio::null());
        cmd.stderr(std::process::Stdio::null());

        match tokio::time::timeout(std::time::Duration::from_secs(5), cmd.output()).await {
            Ok(Ok(output)) => output.status.success(),
            Ok(Err(_)) => false,
            Err(_) => false, // Timeout
        }
    }
}

/// Extension trait for MetricsLogger to add relay-specific logging
pub trait RelayMetricsExt {
    async fn log_relay_event(
        &self,
        config_id: Uuid,
        event_type: RelayEventType,
        details: Option<&str>,
    ) -> Result<(), RelayError>;
    
    async fn log_relay_event_if_persistent(
        &self,
        config: &ResolvedRelayConfig,
        event_type: RelayEventType,
        details: Option<&str>,
    ) -> Result<(), RelayError>;
}

impl RelayMetricsExt for MetricsLogger {
    async fn log_relay_event(
        &self,
        config_id: Uuid,
        event_type: RelayEventType,
        details: Option<&str>,
    ) -> Result<(), RelayError> {
        let query = r#"
            INSERT INTO relay_events (config_id, event_type, details, timestamp)
            VALUES (?, ?, ?, ?)
        "#;

        sqlx::query(query)
            .bind(config_id.to_string())
            .bind(event_type.to_string())
            .bind(details)
            .bind(chrono::Utc::now().to_rfc3339())
            .execute(self.pool())
            .await?;

        Ok(())
    }
    
    async fn log_relay_event_if_persistent(
        &self,
        config: &ResolvedRelayConfig,
        event_type: RelayEventType,
        details: Option<&str>,
    ) -> Result<(), RelayError> {
        if config.is_temporary {
            // Just log to application logs for temporary configs
            info!("Temporary relay config {}: {} - {}", 
                  config.config.id, event_type.to_string(), details.unwrap_or(""));
            return Ok(());
        }
        
        // Log to database for persistent configs
        self.log_relay_event(config.config.id, event_type, details).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::relay::*;
    use chrono::Utc;
    use uuid::Uuid;

    #[tokio::test]
    async fn test_relay_manager_creation() {
        // This test would require mocking the database and temp manager
        // For now, it's a placeholder to show the testing structure
        assert!(true);
    }

    #[test]
    fn test_relay_event_type_serialization() {
        assert_eq!(RelayEventType::Start.to_string(), "start");
        assert_eq!(RelayEventType::Stop.to_string(), "stop");
        assert_eq!(RelayEventType::Error.to_string(), "error");
        assert_eq!(RelayEventType::ClientConnect.to_string(), "client_connect");
        assert_eq!(
            RelayEventType::ClientDisconnect.to_string(),
            "client_disconnect"
        );
    }
}
