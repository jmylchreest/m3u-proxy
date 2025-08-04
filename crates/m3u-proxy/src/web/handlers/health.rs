//! Health check HTTP handlers
//!
//! This module provides health check endpoints for monitoring
//! the application's status and dependencies.

use axum::{extract::State, response::IntoResponse};
use sysinfo::SystemExt;
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

    // Get system load and CPU information
    let (system_load, cpu_info) = {
        let system = state.system.read().await;
        let cpu_count = system.cpus().len() as u32;
        let load_avg = system.load_average();
        
        // Calculate CPU usage percentages based on load average
        let cpu_info = serde_json::json!({
            "cores": cpu_count,
            "load_1min": load_avg.one,
            "load_5min": load_avg.five,
            "load_15min": load_avg.fifteen,
            "load_percentage_1min": (load_avg.one / cpu_count as f64 * 100.0).min(100.0),
        });
        
        (load_avg.one, cpu_info)
    };

    // Get comprehensive memory breakdown
    let memory_breakdown = crate::utils::memory_stats::get_memory_breakdown(
        state.system.clone(),
        &state.relay_manager,
    ).await;

    // Get scheduler health information
    let scheduler_health = get_mock_scheduler_health().await;

    // Get sandbox manager health information
    let sandbox_health = crate::utils::sandbox_health::get_sandbox_health(
        &state.temp_file_manager,
        &state.preview_file_manager, 
        &state.temp_file_manager, // pipeline uses temp for now
        &state.logo_file_manager,
        &state.proxy_output_file_manager,
    ).await;

    // Get relay system health information
    let relay_health = match state.relay_manager.get_relay_health().await {
        Ok(relay_health) => {
            // Get FFmpeg and hardware acceleration info directly from relay manager
            let (ffmpeg_available, ffmpeg_version, ffprobe_available, ffprobe_version, hwaccel_available, hwaccel_capabilities) = 
                get_relay_manager_capabilities(&state.relay_manager).await;

            // Convert relay health to our simplified format for main health endpoint
            crate::web::responses::RelaySystemHealth {
                status: if relay_health.healthy_processes == relay_health.total_processes {
                    "healthy".to_string()
                } else if relay_health.healthy_processes > 0 {
                    "degraded".to_string()
                } else {
                    "unhealthy".to_string()
                },
                total_processes: relay_health.total_processes,
                healthy_processes: relay_health.healthy_processes,
                unhealthy_processes: relay_health.unhealthy_processes,
                ffmpeg_available,
                ffmpeg_version,
                ffprobe_available,
                ffprobe_version,
                hwaccel_available,
                hwaccel_capabilities,
            }
        }
        Err(_) => {
            // Fallback relay health if we can't get it from relay manager
            crate::web::responses::RelaySystemHealth {
                status: "unknown".to_string(),
                total_processes: 0,
                healthy_processes: 0,
                unhealthy_processes: 0,
                ffmpeg_available: false,
                ffmpeg_version: None,
                ffprobe_available: false,
                ffprobe_version: None,
                hwaccel_available: false,
                hwaccel_capabilities: crate::web::responses::DetailedHwAccelCapabilities {
                    accelerators: Vec::new(),
                    codecs: Vec::new(),
                    support_matrix: std::collections::HashMap::new(),
                },
            }
        }
    };

    // Gather component health status
    let mut health_details = std::collections::HashMap::new();

    // Database health
    health_details.insert(
        "database".to_string(),
        serde_json::to_value(&db_health).unwrap_or_default(),
    );

    // Scheduler health
    health_details.insert(
        "scheduler".to_string(),
        serde_json::to_value(&scheduler_health).unwrap_or_default(),
    );

    // Sandbox manager health
    health_details.insert(
        "sandbox_manager".to_string(),
        serde_json::to_value(&sandbox_health).unwrap_or_default(),
    );

    // Relay system health
    health_details.insert(
        "relay_system".to_string(),
        serde_json::to_value(&relay_health).unwrap_or_default(),
    );

    // Determine overall health status
    let overall_healthy = db_health.status == "connected" 
        && scheduler_health.status == "running"
        && sandbox_health.status == "running"
        && (relay_health.status == "healthy" || relay_health.status == "degraded");

    let response = if overall_healthy {
        serde_json::json!({
            "status": "healthy",
            "timestamp": chrono::Utc::now(),
            "version": env!("CARGO_PKG_VERSION"),
            "uptime_seconds": uptime_seconds,
            "system_load": system_load,
            "cpu_info": cpu_info,
            "memory": memory_breakdown,
            "components": health_details
        })
    } else {
        serde_json::json!({
            "status": "unhealthy",
            "timestamp": chrono::Utc::now(),
            "version": env!("CARGO_PKG_VERSION"),
            "uptime_seconds": uptime_seconds,
            "system_load": system_load,
            "cpu_info": cpu_info,
            "memory": memory_breakdown,
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

/// Get FFmpeg and hardware acceleration capabilities from relay manager
async fn get_relay_manager_capabilities(
    relay_manager: &crate::services::relay_manager::RelayManager,
) -> (bool, Option<String>, bool, Option<String>, bool, crate::web::responses::DetailedHwAccelCapabilities) {
    // Get capabilities from relay manager (which already detected them at startup)
    let ffmpeg_available = relay_manager.is_ffmpeg_available();
    let ffmpeg_version = relay_manager.get_ffmpeg_version();
    let ffprobe_available = relay_manager.is_ffprobe_available();
    let ffprobe_version = relay_manager.get_ffprobe_version();
    let hwaccel_available = relay_manager.is_hwaccel_available();
    
    // Convert HwAccelCapabilities to DetailedHwAccelCapabilities
    let hwaccel_capabilities = convert_hwaccel_capabilities(relay_manager.get_hwaccel_capabilities());
    
    (ffmpeg_available, ffmpeg_version, ffprobe_available, ffprobe_version, hwaccel_available, hwaccel_capabilities)
}


/// Convert HwAccelCapabilities to DetailedHwAccelCapabilities
fn convert_hwaccel_capabilities(
    capabilities: &crate::models::relay::HwAccelCapabilities,
) -> crate::web::responses::DetailedHwAccelCapabilities {
    let mut support_matrix = std::collections::HashMap::new();
    let mut accelerators = Vec::new();
    let mut codecs = Vec::new();
    
    // Convert the relay manager's HwAccelCapabilities to health endpoint format
    for accelerator in &capabilities.accelerators {
        accelerators.push(accelerator.name.clone());
        
        // Map codec names to our standard format
        let mut accel_support = crate::web::responses::AcceleratorSupport {
            h264: false,
            hevc: false,
            av1: false,
        };
        
        for codec in &accelerator.supported_codecs {
            codecs.push(codec.clone());
            
            // Determine codec type from encoder name
            if codec.contains("h264") {
                accel_support.h264 = true;
            } else if codec.contains("hevc") || codec.contains("h265") {
                accel_support.hevc = true;
            } else if codec.contains("av1") {
                accel_support.av1 = true;
            }
        }
        
        support_matrix.insert(accelerator.name.clone(), accel_support);
    }
    
    // Remove duplicates from codecs
    codecs.sort();
    codecs.dedup();
    
    crate::web::responses::DetailedHwAccelCapabilities {
        accelerators,
        codecs,
        support_matrix,
    }
}

/// Get mock scheduler health information
/// TODO: Replace with actual scheduler health when scheduler is accessible from AppState
async fn get_mock_scheduler_health() -> crate::web::responses::SchedulerHealth {
    use chrono::Utc;
    use uuid::Uuid;
    
    // Mock data - in reality this would come from the actual scheduler
    let next_scheduled_times = vec![
        crate::web::responses::NextScheduledTime {
            source_id: Uuid::new_v4(),
            source_name: "Stream Source 1".to_string(),
            source_type: "Stream".to_string(),
            next_run: Utc::now() + chrono::Duration::minutes(30),
            cron_expression: "0 */1 * * *".to_string(),
        },
        crate::web::responses::NextScheduledTime {
            source_id: Uuid::new_v4(),
            source_name: "EPG Source 1".to_string(),
            source_type: "EPG".to_string(),
            next_run: Utc::now() + chrono::Duration::hours(2),
            cron_expression: "0 */6 * * *".to_string(),
        },
    ];
    
    crate::web::responses::SchedulerHealth {
        status: "running".to_string(),
        sources_scheduled: crate::web::responses::ScheduledSourceCounts {
            stream_sources: 3,
            epg_sources: 2,
        },
        next_scheduled_times,
        last_cache_refresh: Utc::now() - chrono::Duration::minutes(5),
        active_ingestions: 0,
    }
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
        Ok(_) => {
            let pool = database.pool();
            crate::web::responses::DatabaseHealth {
                status: "connected".to_string(),
                connection_pool_size: pool.options().get_max_connections(),
                active_connections: pool.size(),
            }
        },
        Err(_) => crate::web::responses::DatabaseHealth {
            status: "disconnected".to_string(),
            connection_pool_size: 0,
            active_connections: 0,
        },
    }
}
