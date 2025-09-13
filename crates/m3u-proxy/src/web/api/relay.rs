//! Relay API Endpoints
//!
//! This module provides HTTP API endpoints for managing relay profiles,
//! channel configurations, and relay process control.

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::get,
};
use uuid::Uuid;

use crate::models::relay::*;
use crate::web::AppState;
use crate::web::handlers::health::{
    check_ffmpeg_availability, check_ffprobe_availability, check_hardware_acceleration,
};
use crate::web::responses::RelayHealthApiResponse;

/// Create relay API routes
pub fn relay_routes() -> Router<AppState> {
    Router::new()
        // Profile management
        .route("/relay/profiles", get(list_profiles).post(create_profile))
        .route(
            "/relay/profiles/{id}",
            get(get_profile).put(update_profile).delete(delete_profile),
        )
        // System monitoring
        .route("/relay/health", get(get_relay_health))
}

/// List all relay profiles
#[utoipa::path(
    get,
    path = "/relay/profiles",
    tag = "relay",
    summary = "List relay profiles",
    description = "Retrieve all relay profiles for stream transcoding",
    responses(
        (status = 200, description = "List of relay profiles"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn list_profiles(State(state): State<AppState>) -> impl IntoResponse {
    let relay_repo = crate::database::repositories::RelaySeaOrmRepository::new(
        state.database.connection().clone(),
    );
    match relay_repo.get_active_profiles().await {
        Ok(profiles) => Json(profiles).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Repository error: {e}"),
        )
            .into_response(),
    }
}

/// Get a specific relay profile
#[utoipa::path(
    get,
    path = "/relay/profiles/{id}",
    tag = "relay",
    summary = "Get relay profile",
    description = "Retrieve a specific relay profile by ID",
    params(
        ("id" = String, Path, description = "Relay profile ID (UUID)"),
    ),
    responses(
        (status = 200, description = "Relay profile details"),
        (status = 404, description = "Relay profile not found"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_profile(State(state): State<AppState>, Path(id): Path<Uuid>) -> impl IntoResponse {
    let relay_repo = crate::database::repositories::RelaySeaOrmRepository::new(
        state.database.connection().clone(),
    );
    match relay_repo.find_by_id(id).await {
        Ok(Some(profile)) => Json(profile).into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, "Profile not found").into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Repository error: {e}"),
        )
            .into_response(),
    }
}

/// Create a new relay profile
#[utoipa::path(
    post,
    path = "/relay/profiles",
    tag = "relay",
    summary = "Create relay profile",
    description = "Create a new relay profile for stream transcoding",
    responses(
        (status = 201, description = "Relay profile created successfully"),
        (status = 400, description = "Invalid request"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn create_profile(
    State(state): State<AppState>,
    Json(request): Json<CreateRelayProfileRequest>,
) -> impl IntoResponse {
    // Create the profile using repository
    let relay_repo = crate::database::repositories::RelaySeaOrmRepository::new(
        state.database.connection().clone(),
    );
    match relay_repo.create(request).await {
        Ok(profile) => (StatusCode::CREATED, Json(profile)).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Repository error: {e}"),
        )
            .into_response(),
    }
}

/// Update an existing relay profile
#[utoipa::path(
    put,
    path = "/relay/profiles/{id}",
    tag = "relay",
    summary = "Update relay profile",
    description = "Update an existing relay profile",
    params(
        ("id" = String, Path, description = "Relay profile ID (UUID)"),
    ),
    responses(
        (status = 200, description = "Relay profile updated successfully"),
        (status = 400, description = "Invalid request"),
        (status = 404, description = "Relay profile not found"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn update_profile(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(request): Json<UpdateRelayProfileRequest>,
) -> impl IntoResponse {
    let relay_repo = crate::database::repositories::RelaySeaOrmRepository::new(
        state.database.connection().clone(),
    );
    match relay_repo.update(id, request).await {
        Ok(profile) => Json(profile).into_response(),
        Err(e) => {
            if e.to_string().contains("not found") {
                (StatusCode::NOT_FOUND, "Profile not found").into_response()
            } else {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Repository error: {e}"),
                )
                    .into_response()
            }
        }
    }
}

/// Delete a relay profile
#[utoipa::path(
    delete,
    path = "/relay/profiles/{id}",
    tag = "relay",
    summary = "Delete relay profile",
    description = "Delete a relay profile",
    params(
        ("id" = String, Path, description = "Relay profile ID (UUID)"),
    ),
    responses(
        (status = 204, description = "Relay profile deleted successfully"),
        (status = 404, description = "Relay profile not found"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn delete_profile(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    let relay_repo = crate::database::repositories::RelaySeaOrmRepository::new(
        state.database.connection().clone(),
    );
    match relay_repo.delete(id).await {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => {
            if e.to_string().contains("not found") {
                (StatusCode::NOT_FOUND, "Profile not found").into_response()
            } else {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Repository error: {e}"),
                )
                    .into_response()
            }
        }
    }
}

/// Get comprehensive relay system health and metrics
#[utoipa::path(
    get,
    path = "/relay/health",
    tag = "relay",
    summary = "Get relay system health",
    description = "Retrieve comprehensive health status, metrics, and connected client information for the relay system",
    responses(
        (status = 200, description = "Relay system health with detailed metrics and client information", body = RelayHealthApiResponse),
        (status = 500, description = "Failed to get health status")
    )
)]
pub async fn get_relay_health(State(state): State<AppState>) -> impl IntoResponse {
    // Get actual relay health from the in-memory relay manager
    match state.relay_manager.get_relay_health().await {
        Ok(relay_health) => {
            // Get system capabilities (FFmpeg, hardware acceleration)
            let (ffmpeg_available, ffmpeg_version) = check_ffmpeg_availability().await;
            let (ffprobe_available, ffprobe_version) = check_ffprobe_availability().await;
            let (hwaccel_available, hwaccel_capabilities) = check_hardware_acceleration().await;

            // Convert relay manager's RelayHealth to dashboard-compatible format
            let processes: Vec<crate::web::responses::RelayProcess> = relay_health
                .processes
                .into_iter()
                .map(|p| crate::web::responses::RelayProcess {
                    config_id: p.config_id.to_string(),
                    profile_id: p.profile_id.to_string(),
                    profile_name: p.profile_name,
                    proxy_id: p.proxy_id.map(|id| id.to_string()),
                    channel_name: p.channel_name,
                    source_url: p.source_url,
                    status: format!("{:?}", p.status).to_lowercase(), // Convert enum to string
                    pid: p.pid.map(|pid| pid.to_string()),
                    uptime_seconds: p.uptime_seconds.to_string(),
                    memory_usage_mb: p.memory_usage_mb.to_string(),
                    cpu_usage_percent: p.cpu_usage_percent.to_string(),
                    bytes_received_upstream: p.bytes_received_upstream.to_string(),
                    bytes_delivered_downstream: p.bytes_delivered_downstream.to_string(),
                    connected_clients: p
                        .connected_clients
                        .into_iter()
                        .map(|c| crate::web::responses::RelayConnectedClient {
                            id: c.id.to_string(),
                            ip: c.ip, // Correct field name is 'ip', not 'ip_address'
                            user_agent: c.user_agent,
                            connected_at: c.connected_at.to_rfc3339(),
                            bytes_served: c.bytes_served.to_string(),
                            last_activity: c.last_activity.to_rfc3339(),
                        })
                        .collect(),
                })
                .collect();

            let dashboard_response = crate::web::responses::RelayHealthApiResponse {
                status: if relay_health.unhealthy_processes == 0 {
                    "healthy".to_string()
                } else {
                    "degraded".to_string()
                },
                healthy_processes: relay_health.healthy_processes.to_string(),
                unhealthy_processes: relay_health.unhealthy_processes.to_string(),
                total_processes: relay_health.total_processes.to_string(),
                last_check: relay_health.last_check.to_rfc3339(),
                processes, // Real process data from relay manager!
                ffmpeg_available,
                ffmpeg_version,
                ffprobe_available,
                ffprobe_version,
                hwaccel_available,
                hwaccel_capabilities,
            };

            Json(dashboard_response).into_response()
        }
        Err(e) => {
            tracing::error!("Failed to get relay health from relay manager: {}", e);

            // Get system capabilities even on error (using proper capability checks)
            let (ffmpeg_available, ffmpeg_version) = check_ffmpeg_availability().await;
            let (ffprobe_available, ffprobe_version) = check_ffprobe_availability().await;
            let (hwaccel_available, hwaccel_capabilities) = check_hardware_acceleration().await;

            // Use repository to get profile count for fallback
            let relay_repo = crate::database::repositories::RelaySeaOrmRepository::new(
                state.database.connection().clone(),
            );
            let total_profiles = match relay_repo.get_active_profiles().await {
                Ok(profiles) => profiles.len().to_string(),
                Err(_) => "0".to_string(),
            };

            let dashboard_response = crate::web::responses::RelayHealthApiResponse {
                status: "error".to_string(),
                healthy_processes: "0".to_string(),
                unhealthy_processes: "0".to_string(),
                total_processes: total_profiles,
                last_check: chrono::Utc::now().to_rfc3339(),
                processes: vec![], // Empty on error
                ffmpeg_available,
                ffmpeg_version,
                ffprobe_available,
                ffprobe_version,
                hwaccel_available,
                hwaccel_capabilities,
            };
            Json(dashboard_response).into_response()
        }
    }
}
