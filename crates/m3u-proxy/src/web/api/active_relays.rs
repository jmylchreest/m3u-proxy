//! Active Relay Metrics API
//!
//! This module provides API endpoints for retrieving real-time metrics
//! of active FFmpeg relay processes for the monitoring UI.

use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Json},
};
use serde_json::json;
use tracing::{debug, error, info};

use crate::{
    models::relay::{RelayProcessMetrics, RelayMetrics},
    web::AppState,
};

/// Get all active relay process metrics
/// GET /api/v1/active-relays
pub async fn get_active_relays(
    State(state): State<AppState>,
) -> impl IntoResponse {
    debug!("Fetching active relay metrics");

    match state.relay_manager.get_relay_status().await {
        Ok(process_metrics) => {
            let total_clients: i64 = process_metrics.iter().map(|p| p.client_count as i64).sum();
            let total_bytes_upstream: i64 = process_metrics.iter().map(|p| p.bytes_received_upstream).sum();
            let total_bytes_downstream: i64 = process_metrics.iter().map(|p| p.bytes_delivered_downstream).sum();
            
            let relay_metrics = RelayMetrics {
                total_active_relays: process_metrics.len() as i64,
                total_clients,
                total_bytes_served: total_bytes_downstream, // Use downstream for UI compatibility
                active_processes: process_metrics,
            };

            Json(json!({
                "status": "success",
                "data": relay_metrics
            })).into_response()
        }
        Err(e) => {
            error!("Failed to get active relay metrics: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "status": "error",
                    "message": "Failed to retrieve active relay metrics"
                }))
            ).into_response()
        }
    }
}

/// Get metrics for a specific active relay
/// GET /api/v1/active-relays/:config_id
pub async fn get_active_relay_by_id(
    State(state): State<AppState>,
    axum::extract::Path(config_id): axum::extract::Path<String>,
) -> impl IntoResponse {
    info!("Fetching metrics for relay: {}", config_id);

    let config_uuid = match uuid::Uuid::parse_str(&config_id) {
        Ok(uuid) => uuid,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "status": "error",
                    "message": "Invalid config ID format"
                }))
            ).into_response();
        }
    };

    match state.relay_manager.get_relay_status().await {
        Ok(process_metrics) => {
            if let Some(relay_metrics) = process_metrics.into_iter().find(|p| p.config_id == config_uuid) {
                Json(json!({
                    "status": "success",
                    "data": relay_metrics
                })).into_response()
            } else {
                (
                    StatusCode::NOT_FOUND,
                    Json(json!({
                        "status": "error",
                        "message": "Active relay not found"
                    }))
                ).into_response()
            }
        }
        Err(e) => {
            error!("Failed to get relay metrics: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "status": "error",
                    "message": "Failed to retrieve relay metrics"
                }))
            ).into_response()
        }
    }
}

/// Get system health status for active relays
/// GET /api/v1/active-relays/health
pub async fn get_relay_health(
    State(state): State<AppState>,
) -> impl IntoResponse {
    info!("Fetching relay system health");

    match state.relay_manager.get_relay_health().await {
        Ok(health) => {
            Json(json!({
                "status": "success",
                "data": health
            })).into_response()
        }
        Err(e) => {
            error!("Failed to get relay health: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "status": "error",
                    "message": "Failed to retrieve relay health"
                }))
            ).into_response()
        }
    }
}