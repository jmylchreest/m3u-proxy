//! Health check HTTP handlers
//!
//! This module provides health check endpoints for monitoring
//! the application's status and dependencies.

use axum::{extract::State, response::IntoResponse};
use utoipa;

use crate::database::Database;
use crate::web::{
    AppState,
    extractors::RequestContext,
    responses::ok,
    utils::log_request,
};

/// Health check endpoint with comprehensive system status
///
/// Returns detailed application health status including database connectivity,
/// uptime, and component status
#[utoipa::path(
    get,
    path = "/health",
    tag = "health",
    summary = "Health check",
    description = "Comprehensive health check endpoint for monitoring application status",
    responses(
        (status = 200, description = "Health status"),
        (status = 503, description = "Service unhealthy")
    )
)]
pub async fn health_check(
    State(state): State<AppState>,
    context: RequestContext,
) -> impl IntoResponse {
    log_request(
        &axum::http::Method::GET,
        &"/health".parse().unwrap(),
        &context,
    );

    // Calculate uptime
    let uptime_seconds = chrono::Utc::now()
        .signed_duration_since(state.start_time)
        .num_seconds()
        .max(0) as u64;

    // Check database connectivity
    let db_health = check_database_health(&state.database).await;

    // Gather detailed component status
    let mut health_details = std::collections::HashMap::new();

    // Database health
    health_details.insert(
        "database".to_string(),
        serde_json::to_value(&db_health).unwrap_or_default(),
    );

    // Native pipeline health (no plugins)
    health_details.insert(
        "pipeline".to_string(),
        serde_json::json!({
            "status": "active",
            "type": "native",
            "message": "Native pipeline processing enabled"
        }),
    );

    // Ingestion state manager health
    health_details.insert(
        "ingestion_manager".to_string(),
        serde_json::json!({
            "status": "active",
            "active_operations": 0  // Would get from actual state manager
        }),
    );

    // Cache health
    health_details.insert(
        "cache".to_string(),
        serde_json::json!({
            "status": "active"
        }),
    );

    let overall_healthy = db_health.status == "connected";

    let response = if overall_healthy {
        serde_json::json!({
            "status": "healthy",
            "timestamp": chrono::Utc::now(),
            "version": env!("CARGO_PKG_VERSION"),
            "uptime_seconds": uptime_seconds,
            "components": health_details
        })
    } else {
        serde_json::json!({
            "status": "unhealthy",
            "timestamp": chrono::Utc::now(),
            "version": env!("CARGO_PKG_VERSION"),
            "uptime_seconds": uptime_seconds,
            "components": health_details
        })
    };

    ok(response)
}


/// Readiness check (for Kubernetes probes)
#[utoipa::path(
    get,
    path = "/ready",
    tag = "health",
    summary = "Readiness check",
    description = "Kubernetes readiness probe endpoint - checks if service is ready to accept traffic",
    responses(
        (status = 200, description = "Service ready"),
        (status = 503, description = "Service not ready")
    )
)]
pub async fn readiness_check(
    State(state): State<AppState>,
    context: RequestContext,
) -> impl IntoResponse {
    log_request(
        &axum::http::Method::GET,
        &"/ready".parse().unwrap(),
        &context,
    );

    // Check if all critical services are ready
    let db_health = check_database_health(&state.database).await;

    if db_health.status == "connected" {
        ok(serde_json::json!({
            "status": "ready",
            "timestamp": chrono::Utc::now()
        }))
        .into_response()
    } else {
        // Return 503 Service Unavailable for readiness failures
        axum::http::StatusCode::SERVICE_UNAVAILABLE.into_response()
    }
}

/// Liveness check (for Kubernetes probes)
#[utoipa::path(
    get,
    path = "/live",
    tag = "health",
    summary = "Liveness check",
    description = "Kubernetes liveness probe endpoint - checks if service is alive",
    responses(
        (status = 200, description = "Service alive")
    )
)]
pub async fn liveness_check(_context: RequestContext) -> impl IntoResponse {
    log_request(
        &axum::http::Method::GET,
        &"/live".parse().unwrap(),
        &_context,
    );

    // Simple liveness check - if we can respond, we're alive
    ok(serde_json::json!({
        "status": "alive",
        "timestamp": chrono::Utc::now()
    }))
}

/// Check database health
async fn check_database_health(database: &Database) -> crate::web::responses::DatabaseHealth {
    // TODO: Implement actual database health check
    // This would typically involve:
    // - Testing a simple query
    // - Checking connection pool status
    // - Measuring response time

    // Simple health check by executing a basic query
    match sqlx::query("SELECT 1").fetch_one(&database.pool()).await {
        Ok(_) => crate::web::responses::DatabaseHealth {
            status: "connected".to_string(),
            connection_pool_size: 10, // Would get from actual pool
            active_connections: 1,    // Would get from actual pool
        },
        Err(_) => crate::web::responses::DatabaseHealth {
            status: "disconnected".to_string(),
            connection_pool_size: 0,
            active_connections: 0,
        },
    }
}
