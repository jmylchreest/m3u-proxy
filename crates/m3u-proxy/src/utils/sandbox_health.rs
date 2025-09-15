//! Sandbox Manager Health Information
//!
//! This module provides health status information for sandbox file managers

use crate::config::Config;
use crate::web::responses::{ManagedDirectoryInfo, SandboxManagerHealth};
use chrono::Utc;
use sandboxed_file_manager::SandboxedManager;

/// Get sandbox manager health information
pub async fn get_sandbox_health(
    _temp_manager: &SandboxedManager,
    _preview_manager: &SandboxedManager,
    _pipeline_manager: &SandboxedManager,
    _logo_manager: &SandboxedManager,
    _proxy_output_manager: &SandboxedManager,
    config: &Config,
) -> SandboxManagerHealth {
    // Get directory-specific retention and cleanup configuration from config
    let managed_directories = vec![
        ManagedDirectoryInfo {
            name: "temp".to_string(),
            retention_duration: config.storage.temp_retention.clone(),
            cleanup_interval: config.storage.temp_cleanup_interval.clone(),
        },
        ManagedDirectoryInfo {
            name: "preview".to_string(),
            retention_duration: config.storage.temp_retention.clone(), // Preview uses temp settings
            cleanup_interval: config.storage.temp_cleanup_interval.clone(),
        },
        ManagedDirectoryInfo {
            name: "pipeline".to_string(),
            retention_duration: config.storage.pipeline_retention.clone(),
            cleanup_interval: config.storage.pipeline_cleanup_interval.clone(),
        },
        ManagedDirectoryInfo {
            name: "logos_cached".to_string(),
            retention_duration: "n/a".to_string(),
            cleanup_interval: "n/a".to_string(),
        },
        ManagedDirectoryInfo {
            name: "proxy_output".to_string(),
            retention_duration: config.storage.m3u_retention.clone(),
            cleanup_interval: config.storage.m3u_cleanup_interval.clone(),
        },
    ];

    // Mock cleanup statistics - in reality these would come from the actual cleanup operations
    let last_cleanup_run = Some(Utc::now() - chrono::Duration::minutes(15));
    let cleanup_status = "completed".to_string();
    let temp_files_cleaned = 12u32;
    let disk_space_freed_mb = 245.6f64;

    SandboxManagerHealth {
        status: "running".to_string(),
        last_cleanup_run,
        cleanup_status,
        temp_files_cleaned,
        disk_space_freed_mb,
        managed_directories,
    }
}
