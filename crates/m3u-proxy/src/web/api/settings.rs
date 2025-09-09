//! Runtime Settings API
//!
//! This module provides endpoints for managing runtime server settings
//! that can be changed without restarting the service.

use axum::{
    extract::State,
    response::{IntoResponse, Json},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::{web::AppState, config::JobSchedulingConfig};

/// Runtime server settings that can be changed without restart
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct RuntimeSettings {
    /// Current log level (TRACE, DEBUG, INFO, WARN, ERROR)
    pub log_level: String,
    /// Enable/disable request logging
    pub enable_request_logging: bool,
}

/// Request to update runtime settings
#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateSettingsRequest {
    /// New log level (optional)
    pub log_level: Option<String>,
    /// Enable/disable request logging (optional)
    pub enable_request_logging: Option<bool>,
}

/// Response for settings operations
#[derive(Debug, Serialize, ToSchema)]
pub struct SettingsResponse {
    pub success: bool,
    pub message: String,
    pub settings: RuntimeSettings,
    pub applied_changes: Vec<String>,
}

/// Valid log levels for validation
const VALID_LOG_LEVELS: &[&str] = &["TRACE", "DEBUG", "INFO", "WARN", "ERROR"];

/// Get current runtime settings
#[utoipa::path(
    get,
    path = "/settings",
    tag = "settings",
    summary = "Get runtime settings",
    description = "Get current runtime server settings that can be changed without restart",
    responses(
        (status = 200, description = "Current runtime settings", body = SettingsResponse),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_settings(
    State(state): State<AppState>,
) -> impl IntoResponse {
    let runtime_settings = state.runtime_settings_store.get().await;
    let settings = RuntimeSettings {
        log_level: runtime_settings.log_level,
        enable_request_logging: runtime_settings.enable_request_logging,
    };

    let response = SettingsResponse {
        success: true,
        message: "Runtime settings retrieved successfully".to_string(),
        settings,
        applied_changes: vec![],
    };

    Json(response)
}

/// Update runtime settings
#[utoipa::path(
    put,
    path = "/settings",
    tag = "settings",
    summary = "Update runtime settings",
    description = "Update runtime server settings that can be changed without restart",
    request_body = UpdateSettingsRequest,
    responses(
        (status = 200, description = "Settings updated successfully", body = SettingsResponse),
        (status = 400, description = "Invalid settings values"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn update_settings(
    State(state): State<AppState>,
    Json(request): Json<UpdateSettingsRequest>,
) -> impl IntoResponse {
    let mut validation_errors = Vec::new();

    // Validate log level if provided
    if let Some(ref log_level) = request.log_level {
        let log_level_upper = log_level.to_uppercase();
        if !VALID_LOG_LEVELS.contains(&log_level_upper.as_str()) {
            validation_errors.push(format!(
                "Invalid log level '{}'. Valid levels: {}",
                log_level,
                VALID_LOG_LEVELS.join(", ")
            ));
        }
    }



    // Return validation errors if any
    if !validation_errors.is_empty() {
        let error_response = serde_json::json!({
            "success": false,
            "message": "Validation failed",
            "errors": validation_errors
        });
        return (StatusCode::BAD_REQUEST, Json(error_response)).into_response();
    }

    // Apply changes using the runtime settings store
    let applied_changes = state.runtime_settings_store.update_multiple(
        request.log_level.as_deref(),
        request.enable_request_logging,
    ).await;

    // Get current settings after update
    let updated_settings = state.runtime_settings_store.get().await;
    let current_settings = RuntimeSettings {
        log_level: updated_settings.log_level,
        enable_request_logging: updated_settings.enable_request_logging,
    };

    let response = SettingsResponse {
        success: true,
        message: format!("Applied {} setting change(s)", applied_changes.len()),
        settings: current_settings,
        applied_changes,
    };

    Json(response).into_response()
}

/// Get available runtime settings info
#[utoipa::path(
    get,
    path = "/settings/info",
    tag = "settings",
    summary = "Get settings information",
    description = "Get information about available runtime settings and their constraints",
    responses(
        (status = 200, description = "Settings information"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_settings_info(
    State(state): State<AppState>,
) -> impl IntoResponse {
    let current_settings = state.runtime_settings_store.get().await;
    
    let info = serde_json::json!({
        "available_settings": {
            "log_level": {
                "description": "Current logging level",
                "type": "string",
                "valid_values": VALID_LOG_LEVELS,
                "current_value": current_settings.log_level,
                "changeable_at_runtime": true
            },
            "enable_request_logging": {
                "description": "Enable or disable request logging",
                "type": "boolean",
                "current_value": current_settings.enable_request_logging,
                "changeable_at_runtime": true
            }
        },
        "note": "Only settings marked as 'changeable_at_runtime': true can be modified without service restart"
    });

    Json(info)
}

/// Request to update job scheduling configuration
#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateJobSchedulingRequest {
    /// Global maximum concurrent jobs (optional)
    pub global_max_jobs: Option<usize>,
    /// Stream ingestion concurrency limit (optional)
    pub stream_ingestion_limit: Option<usize>,
    /// EPG ingestion concurrency limit (optional)
    pub epg_ingestion_limit: Option<usize>,
    /// Proxy regeneration concurrency limit (optional)
    pub proxy_regeneration_limit: Option<usize>,
    /// Maintenance job concurrency limit (optional)
    pub maintenance_limit: Option<usize>,
}

/// Response for job scheduling configuration operations
#[derive(Debug, Serialize, ToSchema)]
pub struct JobSchedulingResponse {
    pub success: bool,
    pub message: String,
    pub config: JobSchedulingConfig,
    pub applied_changes: Vec<String>,
}

/// Get current job scheduling configuration
#[utoipa::path(
    get,
    path = "/settings/job-scheduling",
    tag = "settings",
    summary = "Get job scheduling configuration",
    description = "Get current job scheduling concurrency settings",
    responses(
        (status = 200, description = "Current job scheduling configuration", body = JobSchedulingResponse),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_job_scheduling_config(
    State(state): State<AppState>,
) -> impl IntoResponse {
    let config = state.job_queue_runner.get_concurrency_config().await;
    
    let response = JobSchedulingResponse {
        success: true,
        message: "Job scheduling configuration retrieved successfully".to_string(),
        config,
        applied_changes: vec![],
    };

    Json(response)
}

/// Update job scheduling configuration
#[utoipa::path(
    put,
    path = "/settings/job-scheduling",
    tag = "settings",
    summary = "Update job scheduling configuration",
    description = "Update job scheduling concurrency limits at runtime",
    request_body = UpdateJobSchedulingRequest,
    responses(
        (status = 200, description = "Job scheduling configuration updated successfully", body = JobSchedulingResponse),
        (status = 400, description = "Invalid configuration values"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn update_job_scheduling_config(
    State(state): State<AppState>,
    Json(request): Json<UpdateJobSchedulingRequest>,
) -> impl IntoResponse {
    let mut validation_errors = Vec::new();
    let mut applied_changes = Vec::new();

    // Get current configuration
    let current_config = state.job_queue_runner.get_concurrency_config().await;
    let mut new_config = current_config.clone();

    // Validate and apply changes
    if let Some(global_max) = request.global_max_jobs {
        if global_max == 0 || global_max > 100 {
            validation_errors.push("Global maximum jobs must be between 1 and 100".to_string());
        } else {
            new_config.global_max_jobs = global_max;
            applied_changes.push(format!("Updated global maximum jobs to {}", global_max));
        }
    }

    if let Some(stream_limit) = request.stream_ingestion_limit {
        if stream_limit == 0 || stream_limit > 50 {
            validation_errors.push("Stream ingestion limit must be between 1 and 50".to_string());
        } else {
            new_config.stream_ingestion_limit = stream_limit;
            applied_changes.push(format!("Updated stream ingestion limit to {}", stream_limit));
        }
    }

    if let Some(epg_limit) = request.epg_ingestion_limit {
        if epg_limit == 0 || epg_limit > 20 {
            validation_errors.push("EPG ingestion limit must be between 1 and 20".to_string());
        } else {
            new_config.epg_ingestion_limit = epg_limit;
            applied_changes.push(format!("Updated EPG ingestion limit to {}", epg_limit));
        }
    }

    if let Some(proxy_limit) = request.proxy_regeneration_limit {
        if proxy_limit == 0 || proxy_limit > 50 {
            validation_errors.push("Proxy regeneration limit must be between 1 and 50".to_string());
        } else {
            new_config.proxy_regeneration_limit = proxy_limit;
            applied_changes.push(format!("Updated proxy regeneration limit to {}", proxy_limit));
        }
    }

    if let Some(maintenance_limit) = request.maintenance_limit {
        if maintenance_limit == 0 || maintenance_limit > 10 {
            validation_errors.push("Maintenance limit must be between 1 and 10".to_string());
        } else {
            new_config.maintenance_limit = maintenance_limit;
            applied_changes.push(format!("Updated maintenance limit to {}", maintenance_limit));
        }
    }

    // Additional validation: ensure type limits don't exceed global limit
    let total_max_possible = new_config.stream_ingestion_limit 
        + new_config.epg_ingestion_limit 
        + new_config.proxy_regeneration_limit 
        + new_config.maintenance_limit;
        
    if total_max_possible > new_config.global_max_jobs {
        validation_errors.push(format!(
            "Sum of type limits ({}) cannot exceed global maximum ({})", 
            total_max_possible, 
            new_config.global_max_jobs
        ));
    }

    // Return validation errors if any
    if !validation_errors.is_empty() {
        let error_response = serde_json::json!({
            "success": false,
            "message": "Validation failed",
            "errors": validation_errors
        });
        return (StatusCode::BAD_REQUEST, Json(error_response)).into_response();
    }

    // Apply the configuration if there were changes
    if !applied_changes.is_empty() {
        state.job_queue_runner.update_concurrency_config(&new_config).await;
    }

    let response = JobSchedulingResponse {
        success: true,
        message: if applied_changes.is_empty() {
            "No changes were made".to_string()
        } else {
            format!("Applied {} configuration change(s)", applied_changes.len())
        },
        config: new_config,
        applied_changes,
    };

    Json(response).into_response()
}

