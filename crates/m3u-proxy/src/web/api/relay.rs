//! Relay API Endpoints
//!
//! This module provides HTTP API endpoints for managing relay profiles,
//! channel configurations, and relay process control.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use uuid::Uuid;

use crate::models::relay::*;
use crate::repositories::{relay::RelayRepository, traits::Repository};
use crate::web::AppState;

/// Create relay API routes
pub fn relay_routes() -> Router<AppState> {
    Router::new()
        // Profile management
        .route("/relay/profiles", get(list_profiles).post(create_profile))
        .route("/relay/profiles/{id}", get(get_profile).put(update_profile).delete(delete_profile))
        
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
    let relay_repo = crate::repositories::RelayRepository::new(state.database.pool());
    match relay_repo.get_active_profiles().await {
        Ok(profiles) => Json(profiles).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("Repository error: {e}")).into_response(),
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
pub async fn get_profile(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    let relay_repo = crate::repositories::RelayRepository::new(state.database.pool());
    match relay_repo.find_by_id(id).await {
        Ok(Some(profile)) => Json(profile).into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, "Profile not found").into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("Repository error: {e}")).into_response(),
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
    let relay_repo = RelayRepository::new(state.database.pool());
    match relay_repo.create(request).await {
        Ok(profile) => (StatusCode::CREATED, Json(profile)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("Repository error: {e}")).into_response(),
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

    let relay_repo = RelayRepository::new(state.database.pool());
    match relay_repo.update(id, request).await {
        Ok(profile) => Json(profile).into_response(),
        Err(e) => {
            if e.to_string().contains("not found") {
                (StatusCode::NOT_FOUND, "Profile not found").into_response()
            } else {
                (StatusCode::INTERNAL_SERVER_ERROR, format!("Repository error: {e}")).into_response()
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
    let relay_repo = RelayRepository::new(state.database.pool());
    match relay_repo.delete(id).await {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => {
            if e.to_string().contains("not found") {
                (StatusCode::NOT_FOUND, "Profile not found").into_response()
            } else {
                (StatusCode::INTERNAL_SERVER_ERROR, format!("Repository error: {e}")).into_response()
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
        (status = 200, description = "Relay system health with detailed metrics and client information", body = RelayHealth),
        (status = 500, description = "Failed to get health status")
    )
)]
pub async fn get_relay_health(State(state): State<AppState>) -> impl IntoResponse {
    match state.relay_manager.get_relay_health().await {
        Ok(health) => Json(health).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to get health: {e}")).into_response(),
    }
}


