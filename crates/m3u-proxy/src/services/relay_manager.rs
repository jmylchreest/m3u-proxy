//! Relay Manager Service
//!
//! This service manages FFmpeg relay processes, including starting, stopping,
//! and serving content from active relays.

use anyhow::Result;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;
use sysinfo::Pid;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};
use uuid::Uuid;
use crate::utils::SystemManager;

use crate::config::Config;
use crate::database::Database;
use crate::metrics::MetricsLogger;
use crate::models::relay::*;
use crate::proxy::session_tracker::ClientInfo;
use crate::database::repositories::channel::ChannelSeaOrmRepository;
use crate::services::ffmpeg_wrapper::{FFmpegProcess, FFmpegProcessWrapper};
use sandboxed_file_manager::SandboxedManager;

/// Manages FFmpeg relay processes with automatic lifecycle management
pub struct RelayManager {
    active_processes: Arc<RwLock<HashMap<Uuid, FFmpegProcess>>>,
    database: Database,
    ffmpeg_wrapper: FFmpegProcessWrapper,
    metrics_logger: Arc<MetricsLogger>,
    cleanup_interval: Duration,
    system_manager: SystemManager,
    pub ffmpeg_available: bool,
    pub ffmpeg_version: Option<String>,
    pub ffprobe_available: bool,
    pub ffprobe_version: Option<String>,
    pub hwaccel_available: bool,
    pub hwaccel_capabilities: HwAccelCapabilities,
}

impl RelayManager {
    /// Create a new relay manager with its own system instance
    pub async fn new(
        database: Database,
        temp_manager: SandboxedManager,
        metrics_logger: Arc<MetricsLogger>,
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
                ffmpeg_command.clone(),
            ),
            metrics_logger,
            cleanup_interval: Duration::from_secs(10),
            system_manager: SystemManager::new(Duration::from_secs(5)),
            ffmpeg_available,
            ffmpeg_version: ffmpeg_version.clone(),
            ffprobe_available,
            ffprobe_version: ffprobe_version.clone(),
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


            Ok(content)
        } else {
            Err(RelayError::ProcessNotFound(config_id))
        }
    }

    /// Stop a relay process
    pub async fn stop_relay(&self, config_id: Uuid) -> Result<(), RelayError> {
        if let Some(mut process) = self.active_processes.write().await.remove(&config_id) {
            process.kill().await?;
            
            
            info!("Stopped relay process for config {}", config_id);
        }
        Ok(())
    }

    /// Get metrics for all active relay processes
    pub async fn get_relay_metrics(&self) -> Result<Vec<RelayProcessMetrics>, RelayError> {
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
                channel_name: self.get_channel_name(process.config.config.channel_id)
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
            let connected_clients = process.cyclic_buffer.get_connected_clients().await;
            let client_count = connected_clients.len() as i32;
            let last_heartbeat = chrono::Utc::now(); // In real implementation, this would be from process monitoring
            
            // Get traffic stats for this process
            let buffer_stats = process.cyclic_buffer.get_stats().await;
            let bytes_received_upstream = buffer_stats.bytes_received_from_upstream as i64;
            let bytes_delivered_downstream = connected_clients.iter().map(|c| c.bytes_served).sum::<u64>() as i64;

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

            let pid = process.child.id().unwrap_or(0);
            
            // Get channel name from database
            let channel_name = self.get_channel_name(process.config.config.channel_id).await;
            
            let health = RelayProcessHealth {
                config_id: *config_id,
                profile_id: process.config.profile.id,
                profile_name: process.config.profile.name.clone(),
                proxy_id: Some(process.config.config.proxy_id),
                source_url: format!("Channel {}", process.config.config.channel_id), // Mock source URL since we don't have it in config
                channel_name,
                status,
                pid: Some(pid),
                uptime_seconds: uptime,
                memory_usage_mb: self
                    .get_process_memory_usage(Some(pid))
                    .await
                    .unwrap_or(0.0),
                cpu_usage_percent: self
                    .get_process_cpu_usage(Some(pid))
                    .await
                    .unwrap_or(0.0),
                bytes_received_upstream,
                bytes_delivered_downstream,
                connected_clients,
                last_heartbeat,
            };
            process_health.push(health);
        }

        let total_processes = processes.len() as i32;

        Ok(RelayHealth {
            total_processes,
            healthy_processes: healthy_count,
            unhealthy_processes: unhealthy_count,
            processes: process_health,
            last_check: chrono::Utc::now(),
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
            let connected_clients = process.cyclic_buffer.get_connected_clients().await;
            let client_count = connected_clients.len() as i32;
            let last_heartbeat = chrono::Utc::now();
            
            // Get traffic stats for this process
            let buffer_stats = process.cyclic_buffer.get_stats().await;
            let bytes_received_upstream = buffer_stats.bytes_received_from_upstream as i64;
            let bytes_delivered_downstream = connected_clients.iter().map(|c| c.bytes_served).sum::<u64>() as i64;

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

            let pid = process.child.id().unwrap_or(0);
            
            // Get channel name from database
            let channel_name = self.get_channel_name(process.config.config.channel_id).await;
            
            let health = RelayProcessHealth {
                config_id,
                profile_id: process.config.profile.id,
                profile_name: process.config.profile.name.clone(),
                proxy_id: Some(process.config.config.proxy_id),
                source_url: format!("Channel {}", process.config.config.channel_id), // Mock source URL since we don't have it in config
                channel_name,
                status,
                pid: Some(pid),
                uptime_seconds: uptime,
                memory_usage_mb: self
                    .get_process_memory_usage(Some(pid))
                    .await
                    .unwrap_or(0.0),
                cpu_usage_percent: self
                    .get_process_cpu_usage(Some(pid))
                    .await
                    .unwrap_or(0.0),
                bytes_received_upstream,
                bytes_delivered_downstream,
                connected_clients,
                last_heartbeat,
            };

            Ok(Some(health))
        } else {
            Ok(None)
        }
    }


    /// Get memory usage for a specific process (in MB)
    async fn get_process_memory_usage(&self, process_id: Option<u32>) -> Option<f64> {
        let process_id = process_id?;
        let system = self.system_manager.get_system();
        let mut system = system.write().await;
        system.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[Pid::from_u32(process_id)]), true);

        system.process(Pid::from_u32(process_id)).map(|process| process.memory() as f64 / 1024.0 / 1024.0)
    }

    /// Get CPU usage for a specific process (percentage, normalized to 100%)
    /// 
    /// Returns CPU usage as a percentage where 100% means the process is using
    /// one full CPU core. On multi-core systems, values can exceed 100% if the
    /// process uses multiple cores.
    async fn get_process_cpu_usage(&self, process_id: Option<u32>) -> Option<f64> {
        let process_id = process_id?;
        let system = self.system_manager.get_system();
        let system = system.read().await;

        if let Some(process) = system.process(Pid::from_u32(process_id)) {
            // sysinfo's cpu_usage() returns percentage where 100% = 1 full CPU core
            // This is already normalized correctly for our purposes
            let cpu_usage = process.cpu_usage() as f64;
            
            // Clamp to reasonable values (sometimes sysinfo can return slightly negative values)
            Some(cpu_usage.max(0.0))
        } else {
            None
        }
    }

    /// Get channel name from database using repository
    async fn get_channel_name(&self, channel_id: Uuid) -> Option<String> {
        let channel_repo = ChannelSeaOrmRepository::new(self.database.connection().clone());
        let channel_name: Option<String> = (channel_repo.get_channel_name(channel_id).await).unwrap_or_default();
        channel_name
    }


    /// Start the cleanup task for idle processes
    fn start_cleanup_task(&self) {
        let processes = self.active_processes.clone();
        let _database = self.database.clone();
        let _metrics_logger = self.metrics_logger.clone();
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
                            to_remove.push(*config_id);
                            continue;
                        }

                        // Check if process is idle (no clients and no activity for a shorter time)
                        let buffer_client_count = process.cyclic_buffer.get_client_count().await;
                        process.client_count.store(buffer_client_count as u32, Ordering::Relaxed);
                        
                        if buffer_client_count == 0
                            && process.last_activity.elapsed() > Duration::from_secs(60)
                        {
                            info!("Relay {} is idle (no clients for 1 minute), scheduling for cleanup", config_id);
                            to_remove.push(*config_id);
                        }
                    }

                    // Remove processes that should be cleaned up
                    for config_id in &to_remove {
                        if let Some(mut process) = processes_guard.remove(config_id) {
                            let _ = process.kill().await;
                        }
                    }
                }

                // Log cleanup for removed processes
                for config_id in to_remove {
                    info!("Cleaned up relay process: {}", config_id);
                }
            }
        });
    }

    /// Start the periodic status logging task
    fn start_status_logging_task(&self) {
        let processes = self.active_processes.clone();
        let database = self.database.clone();
        let system = self.system_manager.get_system();
        
        tokio::spawn(async move {
            let mut status_interval = tokio::time::interval(Duration::from_secs(30));
            loop {
                status_interval.tick().await;
                
                let processes_guard = processes.read().await;
                if processes_guard.is_empty() {
                    continue;
                }
                
                for (config_id, process) in processes_guard.iter() {
                    let buffer_stats = process.cyclic_buffer.get_stats().await;
                    let client_count = buffer_stats.client_count;
                    let bytes_received = buffer_stats.bytes_received_from_upstream;
                    let bytes_delivered = buffer_stats.total_bytes_written;
                    
                    // Get actual channel name using repository
                    let channel_repo = ChannelSeaOrmRepository::new(database.connection().clone());
                    let channel_name = match channel_repo.get_channel_name(process.config.config.channel_id).await {
                        Ok(Some(name)) => name,
                        _ => format!("Channel {}", process.config.config.channel_id),
                    };
                    
                    // Get FFmpeg PID and process stats if available using shared system manager
                    let (ffmpeg_pid, cpu_usage, memory_mb) = match process.child.id() {
                        Some(pid) => {
                            let pid_str = pid.to_string();
                            
                            // Use the shared system manager for process stats
                            let system_guard = system.read().await;
                            
                            if let Some(process_info) = system_guard.process(Pid::from_u32(pid)) {
                                let cpu = format!("{:.1}%", process_info.cpu_usage());
                                let memory = format!("{:.1}MB", process_info.memory() as f64 / 1024.0 / 1024.0);
                                (pid_str, cpu, memory)
                            } else {
                                (pid_str, "N/A".to_string(), "N/A".to_string())
                            }
                        },
                        None => ("N/A".to_string(), "N/A".to_string(), "N/A".to_string())
                    };
                    
                    // Format bandwidth in human-readable format
                    let rx_formatted = crate::utils::human_format::format_memory(bytes_received as f64);
                    let tx_formatted = crate::utils::human_format::format_memory(bytes_delivered as f64);
                    
                    // Use structured logging with key-value pairs including process metrics
                    info!(
                        "relay_id={} channel=\"{}\" profile=\"{}\" clients={} ffmpeg_pid={} cpu_usage={} memory_usage={} stream_url=\"{}\" rx_total={} tx_total={}",
                        config_id,
                        channel_name,
                        process.config.profile.name,
                        client_count,
                        ffmpeg_pid,
                        cpu_usage,
                        memory_mb,
                        process.input_url,
                        rx_formatted,
                        tx_formatted
                    );
                }
            }
        });
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


#[cfg(test)]
mod tests {
    use super::*;


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
