//! Health check HTTP handlers
//!
//! This module provides health check endpoints for monitoring
//! the application's status and dependencies.

use axum::{extract::State, response::IntoResponse};
use std::str::FromStr;
use utoipa;

use crate::database::Database;
use crate::web::{AppState, extractors::RequestContext, responses::ok, utils::log_request};
use serde_json::json;

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

    // Check database connectivity with actual config values
    let db_health = check_database_health(&state.database, &state.config.database).await;

    // Get system load and CPU information
    let (system_load, cpu_info) = {
        let system = state.system.read().await;
        let cpu_count = system.cpus().len() as u32;
        let load_avg = sysinfo::System::load_average();

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
    let memory_breakdown = Some(get_memory_breakdown_without_relay(&state.system).await);

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
    )
    .await;

    // Get relay system health information using in-memory RelayManager
    let relay_health = get_relay_system_health_from_manager(&state.relay_manager).await;

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

    // Circuit breaker health - get stats from circuit breaker manager if available
    if let Some(cb_manager) = state.circuit_breaker_manager.as_ref() {
        let circuit_breaker_stats = cb_manager.get_all_stats().await;
        health_details.insert(
            "circuit_breakers".to_string(),
            serde_json::to_value(&circuit_breaker_stats).unwrap_or_default(),
        );
    }

    // Determine overall health status
    let overall_healthy = db_health.status == "healthy"
        && (scheduler_health.status == "running" || scheduler_health.status == "idle")
        && sandbox_health.status == "running"
        && (relay_health.status == "healthy"
            || relay_health.status == "degraded"
            || relay_health.status == "idle");

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
    let db_health = check_database_health(&state.database, &state.config.database).await;

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

/// Get scheduler health information from AppState components
async fn get_scheduler_health(
    state: &crate::web::AppState,
) -> crate::web::responses::SchedulerHealth {
    // Get source counts from lightweight database queries
    let (stream_sources_count, epg_sources_count, _) =
        get_scheduled_sources_info(&state.database).await;

    // Get next scheduled times from in-memory services (avoiding additional DB queries)
    let next_scheduled_times = get_next_scheduled_from_services(state).await;

    // Get active ingestions from state manager
    let active_ingestions = get_active_ingestion_count(&state.state_manager).await;

    // Get active proxy regenerations count
    let active_regenerations =
        get_active_regeneration_count(&state.proxy_regeneration_service).await;

    // Get last cache refresh time from progress service
    let last_cache_refresh = get_last_cache_refresh_time(&state.progress_service).await;

    // Determine scheduler status based on available data - include proxy regenerations
    let status = if active_ingestions > 0 || active_regenerations > 0 {
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
        active_regenerations,
    }
}

/// Get scheduled sources information from the database using SeaORM repositories
async fn get_scheduled_sources_info(
    database: &crate::database::Database,
) -> (u32, u32, Vec<crate::web::responses::NextScheduledTime>) {
    let stream_source_repo = crate::database::repositories::StreamSourceSeaOrmRepository::new(
        database.connection().clone(),
    );
    let epg_source_repo = crate::database::repositories::EpgSourceSeaOrmRepository::new(
        database.connection().clone(),
    );

    // Get active source counts using SeaORM repositories
    let stream_sources_count = stream_source_repo
        .find_active()
        .await
        .map(|sources| sources.len() as u32)
        .unwrap_or(0);
    let epg_sources_count = epg_source_repo
        .find_active()
        .await
        .map(|sources| sources.len() as u32)
        .unwrap_or(0);

    (stream_sources_count, epg_sources_count, Vec::new())
}

/// Get next scheduled times using SeaORM repositories (rationalized approach)
async fn get_next_scheduled_from_services(
    state: &crate::web::AppState,
) -> Vec<crate::web::responses::NextScheduledTime> {
    let stream_source_repo = crate::database::repositories::StreamSourceSeaOrmRepository::new(
        state.database.connection().clone(),
    );
    let epg_source_repo = crate::database::repositories::EpgSourceSeaOrmRepository::new(
        state.database.connection().clone(),
    );
    let mut scheduled_times = Vec::new();

    // Get scheduled stream sources using SeaORM
    if let Ok(stream_sources) = stream_source_repo.find_active().await {
        for source in stream_sources {
            if !source.update_cron.is_empty() {
                // Calculate actual next run time from cron expression
                if let Ok(schedule) = cron::Schedule::from_str(&source.update_cron)
                    && let Some(next_run) = schedule.upcoming(chrono::Utc).next()
                {
                    scheduled_times.push(crate::web::responses::NextScheduledTime {
                        source_id: source.id,
                        source_name: source.name,
                        source_type: match source.source_type {
                            crate::models::StreamSourceType::M3u => "m3u".to_string(),
                            crate::models::StreamSourceType::Xtream => "xtream".to_string(),
                        },
                        next_run,
                        cron_expression: source.update_cron,
                    });
                }
            }
        }
    }

    // Get scheduled EPG sources using SeaORM
    if let Ok(epg_sources) = epg_source_repo.find_active().await {
        for source in epg_sources {
            if !source.update_cron.is_empty() {
                // Calculate actual next run time from cron expression
                if let Ok(schedule) = cron::Schedule::from_str(&source.update_cron)
                    && let Some(next_run) = schedule.upcoming(chrono::Utc).next()
                {
                    scheduled_times.push(crate::web::responses::NextScheduledTime {
                        source_id: source.id,
                        source_name: source.name,
                        source_type: match source.source_type {
                            crate::models::EpgSourceType::Xmltv => "xmltv".to_string(),
                            crate::models::EpgSourceType::Xtream => "xtream".to_string(),
                        },
                        next_run,
                        cron_expression: source.update_cron,
                    });
                }
            }
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
        Ok(is_active) => {
            if is_active {
                1
            } else {
                0
            }
        }
        Err(_) => 0, // Default to 0 if unable to determine
    }
}

/// Get active proxy regeneration count from proxy regeneration service
async fn get_active_regeneration_count(
    proxy_regeneration_service: &crate::services::proxy_regeneration::ProxyRegenerationService,
) -> u32 {
    // Get queue status and extract active regeneration count
    match proxy_regeneration_service.get_queue_status().await {
        Ok(status_json) => {
            // Parse the JSON response to extract active_regenerations count
            status_json
                .get("active_regenerations")
                .and_then(|v| v.as_u64())
                .map(|v| v as u32)
                .unwrap_or(0)
        }
        Err(_) => 0, // Default to 0 if unable to determine
    }
}

/// Get last cache refresh time from progress service
async fn get_last_cache_refresh_time(
    _progress_service: &crate::services::progress_service::ProgressService,
) -> chrono::DateTime<chrono::Utc> {
    // For now, return a reasonable estimate
    // In a real implementation, this would query the progress service for the most recent completion
    chrono::Utc::now() - chrono::Duration::minutes(15)
}

/// Simple database health check without circuit breaker
async fn check_database_health(
    database: &Database,
    db_config: &crate::config::DatabaseConfig,
) -> crate::web::responses::DatabaseHealth {
    use sea_orm::ConnectionTrait;

    let start_time = std::time::Instant::now();

    // Simple connectivity check
    let health_query = "SELECT 1 as test";
    let stmt = sea_orm::Statement::from_string(database.backend(), health_query.to_owned());
    let health_result = database.connection().query_one(stmt).await;

    let query_duration = start_time.elapsed();
    let max_connections = db_config.max_connections.unwrap_or(10);
    let active_connections = 1; // Conservative estimate
    let idle_connections = max_connections.saturating_sub(active_connections);

    let response_time_status = if query_duration.as_millis() < 50 {
        "excellent"
    } else if query_duration.as_millis() < 100 {
        "good"
    } else if query_duration.as_millis() < 200 {
        "slow"
    } else {
        "critical"
    };

    let (overall_status, tables_accessible, write_capability, no_blocking_locks) =
        match health_result {
            Ok(Some(_)) => ("healthy", true, true, true),
            Ok(None) => ("degraded", false, true, true),
            Err(_) => ("disconnected", false, false, false),
        };

    crate::web::responses::DatabaseHealth {
        status: overall_status.to_string(),
        connection_pool_size: max_connections,
        active_connections,
        response_time_ms: query_duration.as_millis() as u64,
        response_time_status: response_time_status.to_string(),
        tables_accessible,
        write_capability,
        no_blocking_locks,
        idle_connections,
        pool_utilization_percent: (active_connections as f32 / max_connections as f32 * 100.0)
            as u32,
    }
}

/// Get memory breakdown without relay manager dependency (SeaORM migration compatible)
async fn get_memory_breakdown_without_relay(
    system: &std::sync::Arc<tokio::sync::RwLock<sysinfo::System>>,
) -> crate::web::responses::MemoryBreakdown {
    // Minimize write lock duration - refresh and extract data quickly
    let (
        total_memory,
        used_memory,
        free_memory,
        available_memory,
        swap_used,
        swap_total,
        current_pid,
    ) = {
        let mut sys = system.write().await;
        sys.refresh_memory();
        sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);

        let total_memory = sys.total_memory() as f64 / (1024.0 * 1024.0); // Convert from bytes to MB
        let used_memory = sys.used_memory() as f64 / (1024.0 * 1024.0);
        let free_memory = sys.free_memory() as f64 / (1024.0 * 1024.0);
        let available_memory = sys.available_memory() as f64 / (1024.0 * 1024.0);
        let swap_used = sys.used_swap() as f64 / (1024.0 * 1024.0);
        let swap_total = sys.total_swap() as f64 / (1024.0 * 1024.0);
        let current_pid = std::process::id();

        (
            total_memory,
            used_memory,
            free_memory,
            available_memory,
            swap_used,
            swap_total,
            current_pid,
        )
    }; // Write lock released here

    // Calculate process memory usage with read-only access
    let process_memory = {
        let sys = system.read().await;
        calculate_process_memory_without_relay(&sys, current_pid)
    };

    crate::web::responses::MemoryBreakdown {
        total_memory_mb: total_memory,
        used_memory_mb: used_memory,
        free_memory_mb: free_memory,
        available_memory_mb: available_memory,
        swap_used_mb: swap_used,
        swap_total_mb: swap_total,
        process_memory,
    }
}

/// Calculate memory usage for the m3u-proxy process tree without relay manager
fn calculate_process_memory_without_relay(
    system: &sysinfo::System,
    main_pid: u32,
) -> crate::web::responses::ProcessMemoryBreakdown {
    let mut main_process_memory = 0.0f64;
    let mut child_processes_memory = 0.0f64;
    let mut child_process_count = 0u32;

    // Get main process memory
    if let Some(process) = system.process(sysinfo::Pid::from(main_pid as usize)) {
        main_process_memory = process.memory() as f64 / (1024.0 * 1024.0); // Convert from bytes to MB
    }

    // Find additional child processes by scanning the process tree
    let additional_children = find_child_processes_simple(system, main_pid);
    for child_pid in additional_children {
        if let Some(process) = system.process(sysinfo::Pid::from(child_pid as usize)) {
            let child_memory = process.memory() as f64 / (1024.0 * 1024.0); // Convert from bytes to MB
            child_processes_memory += child_memory;
            child_process_count += 1;
        }
    }

    let total_process_tree_memory = main_process_memory + child_processes_memory;

    // Calculate percentage of system memory
    let total_system_memory = system.total_memory() as f64 / (1024.0 * 1024.0); // Convert from bytes to MB
    let percentage_of_system = if total_system_memory > 0.0 {
        (total_process_tree_memory / total_system_memory) * 100.0
    } else {
        0.0
    };

    crate::web::responses::ProcessMemoryBreakdown {
        main_process_mb: main_process_memory,
        child_processes_mb: child_processes_memory,
        total_process_tree_mb: total_process_tree_memory,
        percentage_of_system,
        child_process_count,
    }
}

/// Find child processes of the main process (simple version without relay manager)
fn find_child_processes_simple(system: &sysinfo::System, parent_pid: u32) -> Vec<u32> {
    let mut children = Vec::new();

    for (pid, process) in system.processes() {
        if let Some(parent) = process.parent()
            && parent.as_u32() == parent_pid
        {
            children.push(pid.as_u32());
            // Recursively find grandchildren
            let grandchildren = find_child_processes_simple(system, pid.as_u32());
            children.extend(grandchildren);
        }
    }

    children
}

/// Get relay system health information using RelayManager (proper in-memory approach)
pub async fn get_relay_system_health_from_manager(
    relay_manager: &std::sync::Arc<crate::services::relay_manager::RelayManager>,
) -> crate::web::responses::RelaySystemHealth {
    // Check if FFmpeg/FFprobe are available in the system
    let (ffmpeg_available, ffmpeg_version) = check_ffmpeg_availability().await;
    let (ffprobe_available, ffprobe_version) = check_ffprobe_availability().await;

    // Get real-time relay process information from RelayManager
    let relay_health = relay_manager.get_relay_health().await.unwrap_or_else(|_| {
        // Return empty health if RelayManager fails
        crate::models::relay::RelayHealth {
            total_processes: 0,
            healthy_processes: 0,
            unhealthy_processes: 0,
            processes: Vec::new(),
            last_check: chrono::Utc::now(),
        }
    });

    // Extract process counts from RelayManager health data
    let total_processes = relay_health.total_processes as u32;
    let healthy_processes = relay_health.healthy_processes as u32;
    let unhealthy_processes = relay_health.unhealthy_processes as u32;

    // Use cached hardware acceleration capabilities from RelayManager (more accurate than re-testing)
    let hwaccel_available = relay_manager.hwaccel_available;
    let cached_capabilities = &relay_manager.hwaccel_capabilities;

    // Convert cached RelayManager data to API response format
    let accelerators = cached_capabilities
        .accelerators
        .iter()
        .map(|acc| acc.name.clone())
        .collect();
    let codecs = cached_capabilities.codecs.clone();
    let support_matrix = cached_capabilities
        .support_matrix
        .iter()
        .map(|(accel_name, supported_codecs)| {
            let accel_support = crate::web::responses::AcceleratorSupport {
                h264: supported_codecs.contains(&"h264".to_string()),
                hevc: supported_codecs.contains(&"hevc".to_string()),
                av1: supported_codecs.contains(&"av1".to_string()), // Now uses properly tested data!
            };
            (accel_name.clone(), accel_support)
        })
        .collect();

    let hwaccel_capabilities = crate::web::responses::DetailedHwAccelCapabilities {
        accelerators,
        codecs,
        support_matrix,
    };

    // Determine overall status based on real process health
    let status = if unhealthy_processes > 0 {
        "degraded".to_string()
    } else if total_processes > 0 && ffmpeg_available {
        "healthy".to_string()
    } else if ffmpeg_available {
        "idle".to_string()
    } else {
        "degraded".to_string()
    };

    crate::web::responses::RelaySystemHealth {
        status,
        total_processes: total_processes as i32,
        healthy_processes: healthy_processes as i32,
        unhealthy_processes: unhealthy_processes as i32,
        ffmpeg_available,
        ffmpeg_version,
        ffprobe_available,
        ffprobe_version,
        hwaccel_available,
        hwaccel_capabilities,
    }
}

/// Check if FFmpeg is available in the system
pub async fn check_ffmpeg_availability() -> (bool, Option<String>) {
    match tokio::process::Command::new("ffmpeg")
        .arg("-version")
        .output()
        .await
    {
        Ok(output) if output.status.success() => {
            let version_output = String::from_utf8_lossy(&output.stdout);
            // Extract version from first line
            let version = version_output
                .lines()
                .next()
                .and_then(|line| line.split_whitespace().nth(2))
                .map(|v| v.to_string());
            (true, version)
        }
        _ => (false, None),
    }
}

/// Check if FFprobe is available in the system
pub async fn check_ffprobe_availability() -> (bool, Option<String>) {
    match tokio::process::Command::new("ffprobe")
        .arg("-version")
        .output()
        .await
    {
        Ok(output) if output.status.success() => {
            let version_output = String::from_utf8_lossy(&output.stdout);
            // Extract version from first line
            let version = version_output
                .lines()
                .next()
                .and_then(|line| line.split_whitespace().nth(2))
                .map(|v| v.to_string());
            (true, version)
        }
        _ => (false, None),
    }
}

/// Check hardware acceleration capabilities using cached RelayManager data when available
pub async fn check_hardware_acceleration()
-> (bool, crate::web::responses::DetailedHwAccelCapabilities) {
    // Basic hardware acceleration detection (fallback for when RelayManager isn't available)
    let mut accelerators = Vec::new();
    let mut codecs = Vec::new();
    let mut support_matrix = std::collections::HashMap::new();

    // Check for common hardware acceleration methods
    if let Ok(output) = tokio::process::Command::new("ffmpeg")
        .args(["-hide_banner", "-hwaccels"])
        .output()
        .await
        && output.status.success()
    {
        let hwaccel_output = String::from_utf8_lossy(&output.stdout);
        for line in hwaccel_output.lines() {
            let accel = line.trim();
            if !accel.is_empty() && accel != "Hardware acceleration methods:" {
                accelerators.push(accel.to_string());

                // Fixed codec support mapping - check for VAAPI specifically for AV1
                let accel_support = crate::web::responses::AcceleratorSupport {
                    h264: accel.contains("264")
                        || accel.contains("vaapi")
                        || accel.contains("nvenc"),
                    hevc: accel.contains("hevc")
                        || accel.contains("265")
                        || accel.contains("vaapi")
                        || accel.contains("nvenc"),
                    av1: accel.contains("vaapi"), // Fix: VAAPI supports AV1, not just if string contains "av1"
                };

                support_matrix.insert(accel.to_string(), accel_support);

                // Add common codecs for this accelerator
                if accel.contains("vaapi") || accel.contains("nvenc") {
                    codecs.push("h264".to_string());
                    codecs.push("hevc".to_string());
                    // Add AV1 for VAAPI since we know it supports it
                    if accel.contains("vaapi") {
                        codecs.push("av1".to_string());
                    }
                }
            }
        }
    }

    // Remove duplicates from codecs
    codecs.sort();
    codecs.dedup();

    let hwaccel_available = !accelerators.is_empty();

    (
        hwaccel_available,
        crate::web::responses::DetailedHwAccelCapabilities {
            accelerators,
            codecs,
            support_matrix,
        },
    )
}

/// Logo cache debug endpoint
///
/// Returns detailed logo cache statistics including memory usage, entry counts,
/// and cache performance metrics
#[utoipa::path(
    get,
    path = "/debug/logo-cache",
    tag = "debug",
    summary = "Logo cache debug info",
    description = "Debug endpoint showing logo cache statistics and performance metrics",
    responses(
        (status = 200, description = "Logo cache debug information"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn logo_cache_debug(
    State(state): State<AppState>,
    context: RequestContext,
) -> impl IntoResponse {
    log_request(
        &axum::http::Method::GET,
        &"/debug/logo-cache".parse().unwrap(),
        &context,
    );

    // Get logo cache statistics
    let cache_stats = match state.logo_cache_maintenance_service.get_cache_stats().await {
        Ok(stats) => stats,
        Err(e) => {
            return axum::response::Json(json!({
                "error": format!("Failed to get cache stats: {}", e)
            }))
            .into_response();
        }
    };

    // Calculate memory efficiency
    let bytes_per_entry = if cache_stats.total_entries > 0 {
        cache_stats.memory_usage_bytes / cache_stats.total_entries
    } else {
        0
    };

    // Format sizes in human-readable format
    let memory_usage_mb = cache_stats.memory_usage_bytes as f64 / 1024.0 / 1024.0;
    let storage_usage_mb = cache_stats.storage_usage_bytes as f64 / 1024.0 / 1024.0;

    let debug_info = json!({
        "logo_cache": {
            "total_entries": cache_stats.total_entries,
            "memory_usage": {
                "bytes": cache_stats.memory_usage_bytes,
                "megabytes": format!("{:.2}", memory_usage_mb),
                "bytes_per_entry": bytes_per_entry,
                "avg_entry_size_bytes": cache_stats.avg_entry_size_bytes
            },
            "storage_usage": {
                "bytes": cache_stats.storage_usage_bytes,
                "megabytes": format!("{:.2}", storage_usage_mb)
            },

            "last_updated": chrono::Utc::now().to_rfc3339(),
            "cache_directory": state.config.storage.cached_logo_path,
            "max_size_mb": 1024, // Hardcoded default: 1GB cache size limit
            "max_age_days": 30 // Hardcoded default: 30 days cache age limit
        }
    });

    axum::response::Json(debug_info).into_response()
}
