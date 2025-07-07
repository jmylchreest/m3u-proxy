//! Health check HTTP handlers
//!
//! This module provides health check endpoints for monitoring
//! the application's status and dependencies.

use axum::{extract::State, response::IntoResponse};

use crate::database::Database;
use crate::web::{
    AppState,
    extractors::RequestContext,
    responses::{HealthResponse, ok},
    utils::log_request,
};

/// Health check endpoint
///
/// Returns basic application health status including database connectivity
pub async fn health_check(
    State(state): State<AppState>,
    context: RequestContext,
) -> impl IntoResponse {
    log_request(
        &axum::http::Method::GET,
        &"/health".parse().unwrap(),
        &context,
    );

    // Check database connectivity
    let db_health = check_database_health(&state.database).await;

    let response = if db_health.status == "connected" {
        HealthResponse::healthy()
    } else {
        HealthResponse::unhealthy("Database connection failed".to_string())
    };

    ok(response)
}

/// Detailed health check with more comprehensive status
pub async fn detailed_health_check(
    State(state): State<AppState>,
    context: RequestContext,
) -> impl IntoResponse {
    log_request(
        &axum::http::Method::GET,
        &"/health/detailed".parse().unwrap(),
        &context,
    );

    let mut health_details = std::collections::HashMap::new();

    // Database health
    let db_health = check_database_health(&state.database).await;
    health_details.insert(
        "database".to_string(),
        serde_json::to_value(&db_health).unwrap_or_default(),
    );

    // Plugin system health
    if let Some(ref plugin_manager) = state.plugin_manager {
        match plugin_manager.get_detailed_statistics() {
            Ok(plugin_stats) => {
                health_details.insert(
                    "plugins".to_string(),
                    serde_json::json!({
                        "status": "healthy",
                        "statistics": plugin_stats
                    }),
                );
            }
            Err(e) => {
                health_details.insert(
                    "plugins".to_string(),
                    serde_json::json!({
                        "status": "error",
                        "error": e.to_string()
                    }),
                );
            }
        }
    } else {
        health_details.insert(
            "plugins".to_string(),
            serde_json::json!({
                "status": "disabled",
                "message": "WASM plugin system is disabled"
            }),
        );
    }

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
            "components": health_details
        })
    } else {
        serde_json::json!({
            "status": "unhealthy",
            "timestamp": chrono::Utc::now(),
            "version": env!("CARGO_PKG_VERSION"),
            "components": health_details
        })
    };

    ok(response)
}

/// Readiness check (for Kubernetes probes)
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
