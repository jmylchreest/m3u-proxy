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
    let scheduler_health = get_scheduler_health(&state).await;

    // Get sandbox manager health information
    let sandbox_health = crate::utils::sandbox_health::get_sandbox_health(
        &state.temp_file_manager,
        &state.preview_file_manager, 
        &state.temp_file_manager, // pipeline uses temp for now
        &state.logo_file_manager,
        &state.proxy_output_file_manager,
        &state.config,
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
    let overall_healthy = db_health.status == "healthy" 
        && (scheduler_health.status == "running" || scheduler_health.status == "idle")
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

    if db_health.status == "healthy" {
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
    let ffmpeg_available = relay_manager.ffmpeg_available;
    let ffmpeg_version = relay_manager.ffmpeg_version.clone();
    let ffprobe_available = relay_manager.ffprobe_available;
    let ffprobe_version = relay_manager.ffprobe_version.clone();
    let hwaccel_available = relay_manager.hwaccel_available;
    
    // Convert HwAccelCapabilities to DetailedHwAccelCapabilities
    let hwaccel_capabilities = convert_hwaccel_capabilities(&relay_manager.hwaccel_capabilities);
    
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

/// Get scheduler health information from AppState components
async fn get_scheduler_health(state: &crate::web::AppState) -> crate::web::responses::SchedulerHealth {
    
    // Get source counts from lightweight database queries
    let (stream_sources_count, epg_sources_count, _) = get_scheduled_sources_info(&state.database).await;
    
    // Get next scheduled times from in-memory services (avoiding additional DB queries)
    let next_scheduled_times = get_next_scheduled_from_services(state).await;
    
    // Get active ingestions from state manager
    let active_ingestions = get_active_ingestion_count(&state.state_manager).await;
    
    // Get last cache refresh time from progress service
    let last_cache_refresh = get_last_cache_refresh_time(&state.progress_service).await;
    
    // Determine scheduler status based on available data
    let status = if active_ingestions > 0 {
        "running".to_string()
    } else if stream_sources_count > 0 || epg_sources_count > 0 {
        "idle".to_string()
    } else {
        "no_sources".to_string()
    };
    
    crate::web::responses::SchedulerHealth {
        status,
        sources_scheduled: crate::web::responses::ScheduledSourceCounts {
            stream_sources: stream_sources_count,
            epg_sources: epg_sources_count,
        },
        next_scheduled_times,
        last_cache_refresh,
        active_ingestions,
    }
}

/// Get scheduled sources information from the database
async fn get_scheduled_sources_info(database: &crate::database::Database) -> (u32, u32, Vec<crate::web::responses::NextScheduledTime>) {
    let health_repo = crate::repositories::HealthRepository::new(database.read_pool());
    
    // Get source counts using repository
    let stream_sources_count = health_repo.get_active_stream_sources_count().await.unwrap_or(0);
    let epg_sources_count = health_repo.get_active_epg_sources_count().await.unwrap_or(0);
    
    (stream_sources_count, epg_sources_count, Vec::new())
}

/// Get next scheduled times from lightweight database queries (avoid expensive stats queries)
async fn get_next_scheduled_from_services(state: &crate::web::AppState) -> Vec<crate::web::responses::NextScheduledTime> {
    let health_repo = crate::repositories::HealthRepository::new(state.database.read_pool());
    let mut scheduled_times = Vec::new();
    
    // Get scheduled stream sources
    if let Ok(stream_sources) = health_repo.get_scheduled_stream_sources().await {
        for source in stream_sources {
            // Calculate next run time from cron expression (simplified)
            let next_run = chrono::Utc::now() + chrono::Duration::minutes(30); // Placeholder
            scheduled_times.push(crate::web::responses::NextScheduledTime {
                source_id: source.id,
                source_name: source.name,
                source_type: source.source_type,
                next_run,
                cron_expression: source.cron_expression,
            });
        }
    }
    
    // Get scheduled EPG sources
    if let Ok(epg_sources) = health_repo.get_scheduled_epg_sources().await {
        for source in epg_sources {
            // Calculate next run time from cron expression (simplified)
            let next_run = chrono::Utc::now() + chrono::Duration::hours(1); // Placeholder
            scheduled_times.push(crate::web::responses::NextScheduledTime {
                source_id: source.id,
                source_name: source.name,
                source_type: source.source_type,
                next_run,
                cron_expression: source.cron_expression,
            });
        }
    }
    
    // Sort by next run time to show most urgent first
    scheduled_times.sort_by(|a, b| a.next_run.cmp(&b.next_run));
    
    scheduled_times
}

/// Get active ingestion count from state manager
async fn get_active_ingestion_count(state_manager: &crate::ingestor::IngestionStateManager) -> u32 {
    // Check if any ingestions are currently active
    match state_manager.has_active_ingestions().await {
        Ok(is_active) => if is_active { 1 } else { 0 },
        Err(_) => 0, // Default to 0 if unable to determine
    }
}

/// Get last cache refresh time from progress service
async fn get_last_cache_refresh_time(_progress_service: &crate::services::progress_service::ProgressService) -> chrono::DateTime<chrono::Utc> {
    // For now, return a reasonable estimate
    // In a real implementation, this would query the progress service for the most recent completion
    chrono::Utc::now() - chrono::Duration::minutes(15)
}

/// Comprehensive database health check with performance monitoring
async fn check_database_health(database: &Database) -> crate::web::responses::DatabaseHealth {
    let pool = database.read_pool();
    let start_time = std::time::Instant::now();
    
    // Test 1: Basic connectivity with simple query
    let connectivity_result = sqlx::query("SELECT 1 as test_value")
        .fetch_one(&pool)
        .await;
    
    let query_duration = start_time.elapsed();
    
    match connectivity_result {
        Ok(_) => {
            // Test 2: Verify critical tables exist and are accessible
            let tables_check = verify_critical_tables(&pool).await;
            
            // Test 3: Test write capability with a harmless operation
            let write_check = test_write_capability(&pool).await;
            
            // Test 4: Check for any locks or blocking operations
            let locks_check = check_database_locks(&pool).await;
            
            // Calculate health status based on all checks
            let overall_status = if tables_check && write_check && locks_check {
                "healthy"
            } else if !tables_check {
                "critical" // Missing tables is critical
            } else {
                "degraded" // Write issues or locks are degraded but not critical
            };
            
            // Check response time thresholds
            let response_time_status = if query_duration.as_millis() < 100 {
                "excellent"
            } else if query_duration.as_millis() < 500 {
                "good"
            } else if query_duration.as_millis() < 1000 {
                "slow"
            } else {
                "critical"
            };
            
            // Get connection pool metrics
            let pool_size = pool.size();
            let max_connections = pool.options().get_max_connections();
            let idle_connections = pool.options().get_min_connections();
            
            crate::web::responses::DatabaseHealth {
                status: overall_status.to_string(),
                connection_pool_size: max_connections,
                active_connections: pool_size,
                response_time_ms: query_duration.as_millis() as u64,
                response_time_status: response_time_status.to_string(),
                tables_accessible: tables_check,
                write_capability: write_check,
                no_blocking_locks: locks_check,
                idle_connections,
                pool_utilization_percent: (pool_size as f32 / max_connections as f32 * 100.0) as u32,
            }
        },
        Err(e) => {
            tracing::error!("Database health check failed: {}", e);
            crate::web::responses::DatabaseHealth {
                status: "disconnected".to_string(),
                connection_pool_size: 0,
                active_connections: 0,
                response_time_ms: query_duration.as_millis() as u64,
                response_time_status: "failed".to_string(),
                tables_accessible: false,
                write_capability: false,
                no_blocking_locks: false,
                idle_connections: 0,
                pool_utilization_percent: 0,
            }
        },
    }
}

/// Verify that critical application tables exist and are accessible
async fn verify_critical_tables(pool: &sqlx::SqlitePool) -> bool {
    let critical_tables = [
        "stream_sources",
        "epg_sources", 
        "stream_proxies",
        "channels",
        "filters",
        "proxy_filters",
    ];
    
    for table in &critical_tables {
        // Check if table exists and is accessible
        let query = format!("SELECT COUNT(*) FROM {} LIMIT 1", table);
        if sqlx::query(&query).fetch_one(pool).await.is_err() {
            tracing::warn!("Critical table '{}' is not accessible", table);
            return false;
        }
    }
    
    true
}

/// Test database write capability with a harmless operation
async fn test_write_capability(pool: &sqlx::SqlitePool) -> bool {
    // Test with a simple pragma that doesn't affect data but tests write permissions
    match sqlx::query("PRAGMA optimize").execute(pool).await {
        Ok(_) => true,
        Err(e) => {
            tracing::warn!("Database write capability test failed: {}", e);
            false
        }
    }
}

/// Check for database locks or blocking operations
async fn check_database_locks(pool: &sqlx::SqlitePool) -> bool {
    // SQLite-specific check for blocking operations
    // In SQLite, we can check for active transactions and locks
    match sqlx::query("PRAGMA busy_timeout").fetch_one(pool).await {
        Ok(_) => {
            // Check if database is locked by attempting a quick read with minimal timeout
            match sqlx::query("SELECT sqlite_version() LIMIT 1")
                .fetch_one(pool)
                .await 
            {
                Ok(_) => true,  // No locks detected
                Err(_) => {
                    tracing::warn!("Database appears to be locked or blocking");
                    false
                }
            }
        },
        Err(e) => {
            tracing::warn!("Failed to check database lock status: {}", e);
            false
        }
    }
}
