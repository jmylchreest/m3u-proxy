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

use crate::web::AppState;

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

