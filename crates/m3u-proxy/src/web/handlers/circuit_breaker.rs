//! Circuit breaker management HTTP handlers
//!
//! This module provides HTTP endpoints for managing circuit breaker
//! configuration and status at runtime.

use axum::{
    extract::{State, Path},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::{
    config::CircuitBreakerProfileConfig,
    web::{AppState, responses::ApiResponse},
};

/// Get all circuit breaker statistics
#[utoipa::path(
    get,
    path = "/api/v1/circuit-breakers",
    tag = "circuit-breaker",
    summary = "List circuit breaker stats",
    description = "Get statistics for all active circuit breakers",
    responses(
        (status = 200, description = "Circuit breaker statistics")
    )
)]
pub async fn get_circuit_breaker_stats(
    State(state): State<AppState>,
) -> impl IntoResponse {
    match state.circuit_breaker_manager.as_ref() {
        Some(manager) => {
            let stats = manager.get_all_stats().await;
            (StatusCode::OK, Json(ApiResponse::success(serde_json::json!({
                "circuit_breakers": stats,
                "timestamp": chrono::Utc::now()
            })))).into_response()
        }
        None => {
            (StatusCode::SERVICE_UNAVAILABLE, Json(ApiResponse::<()>::error("Circuit breaker manager not available".to_string()))).into_response()
        }
    }
}

/// Get configuration for all circuit breaker profiles
#[utoipa::path(
    get,
    path = "/api/v1/circuit-breakers/config",
    tag = "circuit-breaker", 
    summary = "Get circuit breaker config",
    description = "Get current circuit breaker configuration including global and profile-specific settings",
    responses(
        (status = 200, description = "Circuit breaker configuration")
    )
)]
pub async fn get_circuit_breaker_config(
    State(state): State<AppState>,
) -> impl IntoResponse {
    match state.circuit_breaker_manager.as_ref() {
        Some(manager) => {
            let config = manager.get_current_config().await;
            (StatusCode::OK, Json(ApiResponse::success(serde_json::json!({
                "config": config,
                "timestamp": chrono::Utc::now()
            })))).into_response()
        }
        None => {
            (StatusCode::SERVICE_UNAVAILABLE, Json(ApiResponse::<()>::error("Circuit breaker manager not available".to_string()))).into_response()
        }
    }
}

/// List all active circuit breaker services
#[utoipa::path(
    get,
    path = "/api/v1/circuit-breakers/services",
    tag = "circuit-breaker",
    summary = "List active services",
    description = "Get list of all services with active circuit breakers",
    responses(
        (status = 200, description = "Active circuit breaker services")
    )
)]
pub async fn list_active_services(
    State(state): State<AppState>,
) -> impl IntoResponse {
    match state.circuit_breaker_manager.as_ref() {
        Some(manager) => {
            let services = manager.list_active_services().await;
            (StatusCode::OK, Json(ApiResponse::success(serde_json::json!({
                "services": services,
                "count": services.len(),
                "timestamp": chrono::Utc::now()
            })))).into_response()
        }
        None => {
            (StatusCode::SERVICE_UNAVAILABLE, Json(ApiResponse::<()>::error("Circuit breaker manager not available".to_string()))).into_response()
        }
    }
}

#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct UpdateProfileRequest {
    pub profile: CircuitBreakerProfileConfig,
}

/// Update a specific service's circuit breaker profile
#[utoipa::path(
    put,
    path = "/api/v1/circuit-breakers/services/{service_name}",
    tag = "circuit-breaker",
    summary = "Update service profile",
    description = "Update circuit breaker profile configuration for a specific service",
    params(
        ("service_name" = String, Path, description = "Service name")
    ),
    request_body = UpdateProfileRequest,
    responses(
        (status = 200, description = "Profile updated successfully"),
        (status = 400, description = "Invalid configuration"),
        (status = 503, description = "Circuit breaker manager not available")
    )
)]
pub async fn update_service_profile(
    State(state): State<AppState>,
    Path(service_name): Path<String>,
    Json(request): Json<UpdateProfileRequest>,
) -> impl IntoResponse {
    match state.circuit_breaker_manager.as_ref() {
        Some(manager) => {
            match manager.update_service_profile(&service_name, request.profile.clone()).await {
                Ok(()) => {
                    info!("Updated circuit breaker profile for service '{}'", service_name);
                    (StatusCode::OK, Json(ApiResponse::success(serde_json::json!({
                        "message": format!("Profile updated for service '{}'", service_name),
                        "service": service_name,
                        "profile": request.profile,
                        "timestamp": chrono::Utc::now()
                    })))).into_response()
                }
                Err(e) => {
                    warn!("Failed to update profile for service '{}': {}", service_name, e);
                    (StatusCode::BAD_REQUEST, Json(ApiResponse::<()>::error(format!("Failed to update profile: {}", e)))).into_response()
                }
            }
        }
        None => {
            (StatusCode::SERVICE_UNAVAILABLE, Json(ApiResponse::<()>::error("Circuit breaker manager not available".to_string()))).into_response()
        }
    }
}

#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct ForceStateRequest {
    pub action: String, // "open" or "closed"
}

/// Force a circuit breaker to a specific state (for testing/emergency)
#[utoipa::path(
    post,
    path = "/api/v1/circuit-breakers/services/{service_name}/force",
    tag = "circuit-breaker",
    summary = "Force circuit state",
    description = "Manually force a circuit breaker to open or closed state",
    params(
        ("service_name" = String, Path, description = "Service name")
    ),
    request_body = ForceStateRequest,
    responses(
        (status = 200, description = "Circuit state forced successfully"),
        (status = 400, description = "Invalid action or service not found"),
        (status = 503, description = "Circuit breaker manager not available")
    )
)]
pub async fn force_circuit_state(
    State(state): State<AppState>,
    Path(service_name): Path<String>,
    Json(request): Json<ForceStateRequest>,
) -> impl IntoResponse {
    match state.circuit_breaker_manager.as_ref() {
        Some(manager) => {
            let result = match request.action.as_str() {
                "open" => manager.force_circuit_open(&service_name).await,
                "closed" => manager.force_circuit_closed(&service_name).await,
                _ => {
                    return (StatusCode::BAD_REQUEST, Json(ApiResponse::<()>::error("Invalid action. Use 'open' or 'closed'".to_string()))).into_response()
                }
            };

            match result {
                Ok(()) => {
                    info!("Forced circuit breaker {} for service '{}'", request.action, service_name);
                    (StatusCode::OK, Json(ApiResponse::success(serde_json::json!({
                        "message": format!("Circuit breaker forced {} for service '{}'", request.action, service_name),
                        "service": service_name,
                        "action": request.action,
                        "timestamp": chrono::Utc::now()
                    })))).into_response()
                }
                Err(e) => {
                    warn!("Failed to force circuit {} for service '{}': {}", request.action, service_name, e);
                    (StatusCode::BAD_REQUEST, Json(ApiResponse::<()>::error(format!("Failed to force circuit {}: {}", request.action, e)))).into_response()
                }
            }
        }
        None => {
            (StatusCode::SERVICE_UNAVAILABLE, Json(ApiResponse::<()>::error("Circuit breaker manager not available".to_string()))).into_response()
        }
    }
}

#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct UpdateConfigRequest {
    pub config: crate::config::CircuitBreakerConfig,
}

/// Update the entire circuit breaker configuration
#[utoipa::path(
    put,
    path = "/api/v1/circuit-breakers/config",
    tag = "circuit-breaker",
    summary = "Update global config",
    description = "Update the entire circuit breaker configuration including global settings and all profiles",
    request_body = UpdateConfigRequest,
    responses(
        (status = 200, description = "Configuration updated successfully"),
        (status = 400, description = "Invalid configuration"),
        (status = 503, description = "Circuit breaker manager not available")
    )
)]
pub async fn update_circuit_breaker_config(
    State(state): State<AppState>,
    Json(request): Json<UpdateConfigRequest>,
) -> impl IntoResponse {
    match state.circuit_breaker_manager.as_ref() {
        Some(manager) => {
            match manager.update_configuration(request.config.clone()).await {
                Ok(updated_services) => {
                    info!("Updated circuit breaker configuration. Affected services: {:?}", updated_services);
                    (StatusCode::OK, Json(ApiResponse::success(serde_json::json!({
                        "message": "Circuit breaker configuration updated successfully",
                        "updated_services": updated_services,
                        "updated_count": updated_services.len(),
                        "config": request.config,
                        "timestamp": chrono::Utc::now()
                    })))).into_response()
                }
                Err(e) => {
                    warn!("Failed to update circuit breaker configuration: {}", e);
                    (StatusCode::BAD_REQUEST, Json(ApiResponse::<()>::error(format!("Failed to update configuration: {}", e)))).into_response()
                }
            }
        }
        None => {
            (StatusCode::SERVICE_UNAVAILABLE, Json(ApiResponse::<()>::error("Circuit breaker manager not available".to_string()))).into_response()
        }
    }
}