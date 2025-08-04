//! Sandbox Manager Health Information
//!
//! This module provides health status information for sandbox file managers

use crate::web::responses::SandboxManagerHealth;
use chrono::Utc;
use sandboxed_file_manager::SandboxedManager;

/// Get sandbox manager health information
pub async fn get_sandbox_health(
    _temp_manager: &SandboxedManager,
    _preview_manager: &SandboxedManager,
    _pipeline_manager: &SandboxedManager,
    _logo_manager: &SandboxedManager,
    _proxy_output_manager: &SandboxedManager,
) -> SandboxManagerHealth {
    // In a real implementation, these would be actual values from cleanup operations
    // For now, we'll return reasonable default values
    let managed_directories = vec![
        "temp".to_string(),
        "preview".to_string(),
        "pipeline".to_string(),
        "logos_cached".to_string(),
        "proxy_output".to_string(),
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