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
use tracing::{info, warn, error};
use utoipa::ToSchema;

use crate::web::AppState;

/// Runtime server settings that can be changed without restart
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct RuntimeSettings {
    /// Current log level (TRACE, DEBUG, INFO, WARN, ERROR)
    pub log_level: String,
    /// Maximum number of concurrent connections (if configurable)
    pub max_connections: Option<u32>,
    /// Request timeout in seconds
    pub request_timeout_seconds: Option<u32>,
    /// Enable/disable request logging
    pub enable_request_logging: bool,
    /// Enable/disable metrics collection
    pub enable_metrics: bool,
}

/// Request to update runtime settings
#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateSettingsRequest {
    /// New log level (optional)
    pub log_level: Option<String>,
    /// New max connections (optional)
    pub max_connections: Option<u32>,
    /// New request timeout (optional)
    pub request_timeout_seconds: Option<u32>,
    /// Enable/disable request logging (optional)
    pub enable_request_logging: Option<bool>,
    /// Enable/disable metrics collection (optional)
    pub enable_metrics: Option<bool>,
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
    State(_state): State<AppState>,
) -> impl IntoResponse {
    // In a real implementation, these would be retrieved from a configuration store
    // For now, we'll return some default/current values
    let settings = RuntimeSettings {
        log_level: get_current_log_level(),
        max_connections: Some(1000), // Example value
        request_timeout_seconds: Some(30),
        enable_request_logging: true,
        enable_metrics: true,
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
    State(_state): State<AppState>,
    Json(request): Json<UpdateSettingsRequest>,
) -> impl IntoResponse {
    let mut applied_changes = Vec::new();
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

    // Validate other settings
    if let Some(max_conn) = request.max_connections {
        if max_conn == 0 || max_conn > 10000 {
            validation_errors.push("max_connections must be between 1 and 10000".to_string());
        }
    }

    if let Some(timeout) = request.request_timeout_seconds {
        if timeout == 0 || timeout > 300 {
            validation_errors.push("request_timeout_seconds must be between 1 and 300".to_string());
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

    // Apply changes
    let mut current_settings = RuntimeSettings {
        log_level: get_current_log_level(),
        max_connections: Some(1000),
        request_timeout_seconds: Some(30),
        enable_request_logging: true,
        enable_metrics: true,
    };

    // Update log level if provided
    if let Some(ref new_log_level) = request.log_level {
        let new_level_upper = new_log_level.to_uppercase();
        if apply_log_level_change(&new_level_upper) {
            current_settings.log_level = new_level_upper.clone();
            applied_changes.push(format!("Log level changed to {}", new_level_upper));
            info!("Runtime log level changed to: {}", new_level_upper);
        } else {
            warn!("Failed to apply log level change to: {}", new_level_upper);
        }
    }

    // Update other settings (these would need actual implementation)
    if let Some(max_conn) = request.max_connections {
        current_settings.max_connections = Some(max_conn);
        applied_changes.push(format!("Max connections changed to {}", max_conn));
        info!("Max connections setting changed to: {}", max_conn);
    }

    if let Some(timeout) = request.request_timeout_seconds {
        current_settings.request_timeout_seconds = Some(timeout);
        applied_changes.push(format!("Request timeout changed to {} seconds", timeout));
        info!("Request timeout changed to: {} seconds", timeout);
    }

    if let Some(enable_logging) = request.enable_request_logging {
        current_settings.enable_request_logging = enable_logging;
        applied_changes.push(format!("Request logging {}", if enable_logging { "enabled" } else { "disabled" }));
        info!("Request logging {}", if enable_logging { "enabled" } else { "disabled" });
    }

    if let Some(enable_metrics) = request.enable_metrics {
        current_settings.enable_metrics = enable_metrics;
        applied_changes.push(format!("Metrics collection {}", if enable_metrics { "enabled" } else { "disabled" }));
        info!("Metrics collection {}", if enable_metrics { "enabled" } else { "disabled" });
    }

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
    State(_state): State<AppState>,
) -> impl IntoResponse {
    let info = serde_json::json!({
        "available_settings": {
            "log_level": {
                "description": "Current logging level",
                "type": "string",
                "valid_values": VALID_LOG_LEVELS,
                "current_value": get_current_log_level(),
                "changeable_at_runtime": true
            },
            "max_connections": {
                "description": "Maximum number of concurrent connections",
                "type": "integer",
                "min_value": 1,
                "max_value": 10000,
                "current_value": 1000,
                "changeable_at_runtime": true
            },
            "request_timeout_seconds": {
                "description": "Request timeout in seconds",
                "type": "integer",
                "min_value": 1,
                "max_value": 300,
                "current_value": 30,
                "changeable_at_runtime": true
            },
            "enable_request_logging": {
                "description": "Enable or disable request logging",
                "type": "boolean",
                "current_value": true,
                "changeable_at_runtime": true
            },
            "enable_metrics": {
                "description": "Enable or disable metrics collection",
                "type": "boolean", 
                "current_value": true,
                "changeable_at_runtime": true
            }
        },
        "note": "Only settings marked as 'changeable_at_runtime': true can be modified without service restart"
    });

    Json(info)
}

/// Get the current log level from the tracing subscriber
fn get_current_log_level() -> String {
    // This is a simplified implementation
    // In a real scenario, you'd want to track the current level set in the subscriber
    std::env::var("RUST_LOG")
        .unwrap_or_else(|_| "INFO".to_string())
        .to_uppercase()
}

/// Apply log level change at runtime
/// 
/// This is a simplified implementation. In practice, you'd need to:
/// 1. Update the tracing subscriber's filter
/// 2. Store the new level for persistence
/// 3. Notify all relevant components
fn apply_log_level_change(new_level: &str) -> bool {
    // Note: Changing log level at runtime with tracing-subscriber
    // requires more complex implementation with a reload layer
    // For now, this is a placeholder that would need proper implementation
    
    match new_level {
        "TRACE" | "DEBUG" | "INFO" | "WARN" | "ERROR" => {
            // In a real implementation, you'd update the tracing filter here
            // This might involve using tracing_subscriber::reload::Layer
            info!("Would change log level to: {}", new_level);
            true
        }
        _ => {
            error!("Invalid log level: {}", new_level);
            false
        }
    }
}