use axum::{
    extract::{Multipart, Path, Query, State},
    http::StatusCode,
    response::Json,
};
use chrono::{DateTime, Utc};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::Mutex;

use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::Row;
use tracing::{debug, error, info, warn};
use utoipa::{ToSchema, IntoParams};
use uuid::Uuid;

use super::AppState;

pub mod relay;
pub mod active_relays;
pub mod unified_progress;
pub mod log_streaming;
pub mod settings;

use crate::data_mapping::DataMappingService;
use crate::pipeline::engines::validation::StageValidator;
use crate::models::*;

#[derive(Debug, Deserialize)]
pub struct ChannelQueryParams {
    pub page: Option<u32>,
    pub limit: Option<u32>,
    pub filter: Option<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct DataMappingPreviewRequest {
    pub source_type: String,
    pub limit: Option<u32>,
}


// Helper function to get the resolved value for a field from a MappedChannel
#[allow(dead_code)]
fn get_resolved_value(
    mc: &crate::models::data_mapping::MappedChannel,
    field: &str,
) -> Option<String> {
    match field {
        "channel_name" => Some(mc.mapped_channel_name.clone()),
        "tvg_id" => mc.mapped_tvg_id.clone(),
        "tvg_name" => mc.mapped_tvg_name.clone(),
        "tvg_logo" => mc.mapped_tvg_logo.clone(),
        "tvg_shift" => mc.mapped_tvg_shift.clone(),
        "group_title" => mc.mapped_group_title.clone(),
        _ => None,
    }
}

// Helper function to transform MappedChannel to frontend-compatible format
fn mapped_channel_to_frontend_format(
    mc: &crate::models::data_mapping::MappedChannel,
) -> serde_json::Value {
    json!({
        "id": mc.original.id,
        "source_id": mc.original.source_id,
        "channel_name": mc.mapped_channel_name,
        "tvg_id": mc.mapped_tvg_id.as_ref().or(mc.original.tvg_id.as_ref()),
        "tvg_name": mc.mapped_tvg_name.as_ref().or(mc.original.tvg_name.as_ref()),
        "tvg_logo": mc.mapped_tvg_logo.as_ref().or(mc.original.tvg_logo.as_ref()),
        "tvg_shift": mc.mapped_tvg_shift.as_ref().or(mc.original.tvg_shift.as_ref()),
        "group_title": mc.mapped_group_title.as_ref().or(mc.original.group_title.as_ref()),
        "stream_url": mc.original.stream_url,
        "is_removed": mc.is_removed,
        "applied_rules": mc.applied_rules,
        "created_at": mc.original.created_at,
        "updated_at": mc.original.updated_at,
        // Original values with original_ prefix
        "original_channel_name": mc.original.channel_name,
        "original_tvg_id": mc.original.tvg_id,
        "original_tvg_name": mc.original.tvg_name,
        "original_tvg_logo": mc.original.tvg_logo,
        "original_tvg_shift": mc.original.tvg_shift,
        "original_group_title": mc.original.group_title,
        // Mapped values for reference
        "mapped_channel_name": mc.mapped_channel_name,
        "mapped_tvg_id": mc.mapped_tvg_id,
        "mapped_tvg_name": mc.mapped_tvg_name,
        "mapped_tvg_logo": mc.mapped_tvg_logo,
        "mapped_tvg_shift": mc.mapped_tvg_shift,
        "mapped_group_title": mc.mapped_group_title,
        "capture_group_values": mc.capture_group_values
    })
}

// Stream Sources API

// Progress API

/// Get all source progress information
#[utoipa::path(
    get,
    path = "/progress/all",
    tag = "progress",
    summary = "Get all source progress",
    description = "Retrieve progress information for all sources with enhanced processing details",
    responses(
        (status = 200, description = "All source progress retrieved"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_all_progress(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let all_progress = state.state_manager.get_all_progress().await;

    // Get processing info for all sources with progress
    let mut enhanced_progress = std::collections::HashMap::new();

    for (source_id, progress) in all_progress.iter() {
        let processing_info = state.state_manager.get_processing_info(*source_id).await;
        enhanced_progress.insert(
            source_id,
            serde_json::json!({
                "progress": progress,
                "processing_info": processing_info
            }),
        );
    }

    let result = serde_json::json!({
        "success": true,
        "message": "All progress retrieved",
        "progress": enhanced_progress,
        "total_sources": all_progress.len()
    });

    Ok(Json(result))
}

/// Get source progress information
#[utoipa::path(
    get,
    path = "/progress/sources",
    tag = "progress",
    summary = "Get source progress",
    description = "Retrieve progress information for all sources with processing details",
    responses(
        (status = 200, description = "Source progress retrieved"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_all_source_progress(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let all_progress = state.state_manager.get_all_progress().await;

    // Get processing info for all sources with progress
    let mut enhanced_progress = std::collections::HashMap::new();

    for (source_id, progress) in all_progress.iter() {
        let processing_info = state.state_manager.get_processing_info(*source_id).await;
        enhanced_progress.insert(
            source_id,
            serde_json::json!({
                "progress": progress,
                "processing_info": processing_info
            }),
        );
    }

    let result = serde_json::json!({
        "success": true,
        "message": "Source progress retrieved",
        "progress": enhanced_progress,
        "total_sources": all_progress.len()
    });

    Ok(Json(result))
}

/// Get active operation progress
#[utoipa::path(
    get,
    path = "/progress/operations",
    tag = "progress",
    summary = "Get active operation progress",
    description = "Retrieve progress information for currently active operations only",
    responses(
        (status = 200, description = "Active operation progress retrieved"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_operation_progress(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let all_progress = state.state_manager.get_all_progress().await;

    // Filter to show only active operations (not completed or failed)
    let active_operations: std::collections::HashMap<Uuid, IngestionProgress> = all_progress
        .into_iter()
        .filter(|(_, progress)| {
            // Include active operations
            matches!(
                progress.state,
                crate::models::IngestionState::Connecting
                    | crate::models::IngestionState::Downloading
                    | crate::models::IngestionState::Parsing
            )
        })
        .collect();

    let result = serde_json::json!({
        "success": true,
        "message": "Active operation progress retrieved",
        "progress": active_operations,
        "total_operations": active_operations.len()
    });

    Ok(Json(result))
}

/// Get EPG source progress by ID
#[utoipa::path(
    get,
    path = "/progress/epg/{id}",
    tag = "progress",
    summary = "Get EPG source progress",
    description = "Retrieve progress information for a specific EPG source",
    params(
        ("id" = Uuid, Path, description = "EPG source ID")
    ),
    responses(
        (status = 200, description = "EPG source progress retrieved"),
        (status = 404, description = "EPG source not found"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_epg_source_progress(
    Path(id): Path<Uuid>,
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let progress = state.state_manager.get_progress(id).await;
    let processing_info = state.state_manager.get_processing_info(id).await;

    let result = serde_json::json!({
        "success": true,
        "message": "EPG source progress retrieved",
        "source_id": id,
        "progress": progress,
        "processing_info": processing_info
    });

    Ok(Json(result))
}

// Stream Proxies API - implementations are in handlers/proxies.rs

#[utoipa::path(
    post,
    path = "/proxies/{proxy_id}/regenerate",
    tag = "proxies",
    summary = "Regenerate proxy",
    description = "Queue a proxy for background regeneration and return immediately with queue ID",
    params(
        ("proxy_id" = String, Path, description = "Proxy ID (UUID)"),
    ),
    responses(
        (status = 202, description = "Proxy queued for regeneration"),
        (status = 404, description = "Proxy not found"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn regenerate_proxy(
    Path(proxy_id): Path<Uuid>,
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    info!("Queuing proxy {} for background regeneration", proxy_id);
    
    // CRITICAL FIX: Check for duplicate API requests to prevent race conditions
    {
        let mut active_requests = state.active_regeneration_requests.lock().await;
        if active_requests.contains(&proxy_id) {
            warn!("Duplicate regeneration API request for proxy {} - rejecting", proxy_id);
            return Ok(Json(serde_json::json!({
                "success": false,
                "message": "A regeneration request for this proxy is already being processed",
                "proxy_id": proxy_id,
                "status": "duplicate_request"
            })));
        }
        // Reserve this proxy ID to prevent other requests
        active_requests.insert(proxy_id);
    }
    
    // Create a cleanup guard to ensure we always remove the proxy ID from active requests
    let _cleanup_guard = RequestCleanupGuard {
        proxy_id,
        active_requests: state.active_regeneration_requests.clone(),
    };
    
    // Verify the proxy exists first
    match state.database.get_stream_proxy(proxy_id).await {
        Ok(Some(proxy)) => {
            // Queue the proxy for manual regeneration using background processing
            match state.proxy_regeneration_service.queue_manual_regeneration(proxy_id).await {
                Ok(_) => {
                    // Emit scheduler event for immediate processing
                    state.database.emit_scheduler_event(
                        crate::ingestor::scheduler::SchedulerEvent::ManualRefreshTriggered(proxy_id)
                    );
                    
                    info!("Proxy '{}' queued for background regeneration", proxy.name);
                    
                    // _cleanup_guard will automatically clean up when function returns
                    // The service-level deduplication will handle actual regeneration conflicts
                    
                    Ok(Json(serde_json::json!({
                        "success": true,
                        "message": format!("Proxy '{}' queued for regeneration", proxy.name),
                        "queue_id": format!("universal-{}", proxy_id), // Use consistent queue_id format
                        "proxy_id": proxy_id,
                        "status": "queued",
                        "queued_at": chrono::Utc::now()
                    })))
                }
                Err(e) => {
                    error!("Failed to queue proxy {} for regeneration: {}", proxy_id, e);
                    // _cleanup_guard will automatically clean up when function returns
                    Err(StatusCode::INTERNAL_SERVER_ERROR)
                }
            }
        }
        Ok(None) => {
            error!("Proxy {} not found", proxy_id);
            // _cleanup_guard will automatically clean up when function returns
            Err(StatusCode::NOT_FOUND)
        }
        Err(e) => {
            error!("Failed to get proxy {}: {}", proxy_id, e);
            // _cleanup_guard will automatically clean up when function returns
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

#[utoipa::path(
    get,
    path = "/proxies/regeneration/status",
    tag = "proxies",
    summary = "Get regeneration queue status",
    description = "Get current regeneration queue status and statistics",
    responses(
        (status = 200, description = "Queue status retrieved"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_regeneration_queue_status(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    match state.proxy_regeneration_service.get_queue_status().await {
        Ok(status) => {
            Ok(Json(serde_json::json!({
                "success": true,
                "message": "Queue status retrieved",
                "queue_status": status,
                "timestamp": chrono::Utc::now()
            })))
        }
        Err(e) => {
            error!("Failed to get regeneration queue status: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

#[utoipa::path(
    get,
    path = "/progress/regeneration",
    tag = "proxies",
    summary = "Get proxy regeneration progress",
    description = "Get real-time progress of proxy regeneration operations",
    responses(
        (status = 200, description = "Regeneration progress data"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_proxy_regeneration_progress(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    // Use UniversalProgress system to get all proxy regeneration operations
    let regeneration_operations = state.progress_service.get_progress_by_type(
        crate::services::progress_service::OperationType::ProxyRegeneration
    ).await;
    
    let mut progress_data = Vec::new();
    
    // Convert UniversalProgress entries to API format
    for (operation_id, progress) in regeneration_operations {
        // Get proxy name for display
        let proxy_name = match state.database.get_stream_proxy(operation_id).await {
            Ok(Some(proxy)) => proxy.name,
            Ok(None) => format!("Proxy {}", operation_id),
            Err(_) => format!("Proxy {}", operation_id),
        };

        // Convert UniversalState to API status
        let status = match progress.state {
            crate::services::progress_service::UniversalState::Preparing => "pending",
            crate::services::progress_service::UniversalState::Processing => "processing",
            crate::services::progress_service::UniversalState::Completed => "completed",
            crate::services::progress_service::UniversalState::Error => "failed",
            crate::services::progress_service::UniversalState::Cancelled => "cancelled",
            _ => "unknown",
        };

        progress_data.push(serde_json::json!({
            "proxy_id": operation_id,
            "proxy_name": proxy_name,
            "queue_id": format!("universal-{}", operation_id),
            "status": status,
            "progress": {
                "percentage": progress.progress_percentage.unwrap_or(0.0),
                "current_step": progress.current_step,
            },
            "scheduled_at": progress.started_at,
            "created_at": progress.started_at,
            "updated_at": progress.updated_at,
            "completed_at": progress.completed_at,
            "trigger_source_id": null, // Not available in UniversalProgress
            "trigger_type": "auto", // Default since we can't distinguish
            "error_message": progress.error_message
        }));
    }

    Ok(Json(serde_json::json!({
        "success": true,
        "message": "Regeneration progress retrieved",
        "regenerations": progress_data,
        "timestamp": chrono::Utc::now()
    })))
}

// Filters API
/// List all filters
#[utoipa::path(
    get,
    path = "/filters",
    tag = "filters",
    summary = "List filters",
    description = "Retrieve all filters with usage statistics and expression trees",
    responses(
        (status = 200, description = "List of filters with statistics"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn list_filters(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    match state.database.get_filters_with_usage().await {
        Ok(filters) => {
            // Generate expression trees for each filter
            let enhanced_filters: Vec<serde_json::Value> = filters
                .into_iter()
                .map(|filter_with_usage| {
                    let expression_tree =
                        if !filter_with_usage.filter.condition_tree.trim().is_empty() {
                            // Parse the condition_tree JSON to generate expression tree
                            if let Ok(condition_tree) =
                                serde_json::from_str::<crate::models::ConditionTree>(
                                    &filter_with_usage.filter.condition_tree,
                                )
                            {
                                Some(generate_expression_tree_json(&condition_tree))
                            } else {
                                None
                            }
                        } else {
                            None
                        };

                    serde_json::json!({
                        "filter": filter_with_usage.filter,
                        "usage_count": filter_with_usage.usage_count,
                        "expression_tree": expression_tree,
                    })
                })
                .collect();

            Ok(Json(serde_json::Value::Array(enhanced_filters)))
        }
        Err(e) => {
            error!("Failed to list filters: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

#[utoipa::path(
    get,
    path = "/filters/fields",
    tag = "filters",
    summary = "Get available filter fields",
    description = "Retrieve list of fields available for filtering operations",
    responses(
        (status = 200, description = "List of available filter fields"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_filter_fields(
    State(state): State<AppState>,
) -> Result<Json<Vec<FilterFieldInfo>>, StatusCode> {
    match state.database.get_available_filter_fields().await {
        Ok(fields) => Ok(Json(fields)),
        Err(e) => {
            error!("Failed to get filter fields: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

/// Create a new filter
#[utoipa::path(
    post,
    path = "/filters",
    tag = "filters",
    summary = "Create filter",
    description = "Create a new channel filter with conditions",
    request_body = FilterCreateRequest,
    responses(
        (status = 200, description = "Filter created successfully"),
        (status = 400, description = "Invalid filter data"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn create_filter(
    State(state): State<AppState>,
    Json(payload): Json<FilterCreateRequest>,
) -> Result<Json<Filter>, StatusCode> {
    match state.database.create_filter(&payload).await {
        Ok(filter) => Ok(Json(filter)),
        Err(e) => {
            error!("Failed to create filter: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

#[utoipa::path(
    get,
    path = "/filters/{id}",
    tag = "filters",
    summary = "Get filter",
    description = "Retrieve a specific filter by ID",
    params(
        ("id" = String, Path, description = "Filter ID (UUID)"),
    ),
    responses(
        (status = 200, description = "Filter details"),
        (status = 404, description = "Filter not found"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_filter(
    Path(id): Path<Uuid>,
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    match state.database.get_filter(id).await {
        Ok(Some(filter)) => {
            let expression_tree = if !filter.condition_tree.trim().is_empty() {
                // Parse the condition_tree JSON to generate expression tree
                if let Ok(condition_tree) = serde_json::from_str::<crate::models::ConditionTree>(&filter.condition_tree) {
                    Some(generate_expression_tree_json(&condition_tree))
                } else {
                    None
                }
            } else {
                None
            };

            let response = serde_json::json!({
                "filter": filter,
                "expression_tree": expression_tree,
            });
            Ok(Json(response))
        },
        Ok(None) => Err(StatusCode::NOT_FOUND),
        Err(e) => {
            error!("Failed to get filter {}: {}", id, e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

#[utoipa::path(
    put,
    path = "/filters/{id}",
    tag = "filters",
    summary = "Update filter",
    description = "Update an existing filter",
    params(
        ("id" = String, Path, description = "Filter ID (UUID)"),
    ),
    responses(
        (status = 200, description = "Filter updated successfully"),
        (status = 404, description = "Filter not found"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn update_filter(
    Path(id): Path<Uuid>,
    State(state): State<AppState>,
    Json(payload): Json<FilterUpdateRequest>,
) -> Result<Json<Filter>, StatusCode> {
    match state.database.update_filter(id, &payload).await {
        Ok(Some(filter)) => Ok(Json(filter)),
        Ok(None) => Err(StatusCode::NOT_FOUND),
        Err(e) => {
            error!("Failed to update filter {}: {}", id, e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

#[utoipa::path(
    delete,
    path = "/filters/{id}",
    tag = "filters",
    summary = "Delete filter",
    description = "Delete a filter",
    params(
        ("id" = String, Path, description = "Filter ID (UUID)"),
    ),
    responses(
        (status = 204, description = "Filter deleted successfully"),
        (status = 404, description = "Filter not found"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn delete_filter(
    Path(id): Path<Uuid>,
    State(state): State<AppState>,
) -> Result<StatusCode, StatusCode> {
    match state.database.delete_filter(id).await {
        Ok(true) => Ok(StatusCode::NO_CONTENT),
        Ok(false) => Err(StatusCode::NOT_FOUND),
        Err(e) => {
            error!("Failed to delete filter {}: {}", id, e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

#[utoipa::path(
    post,
    path = "/filters/test",
    tag = "filters",
    summary = "Test filter expression",
    description = "Test a filter expression against sample data to validate functionality",
    responses(
        (status = 200, description = "Filter test result"),
        (status = 400, description = "Invalid filter expression"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn test_filter(
    State(state): State<AppState>,
    Json(payload): Json<FilterTestRequest>,
) -> Result<Json<FilterTestResult>, StatusCode> {
    match state
        .database
        .test_filter_pattern(payload.source_id, &payload)
        .await
    {
        Ok(result) => Ok(Json(result)),
        Err(e) => {
            error!("Failed to test filter: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

#[utoipa::path(
    post,
    path = "/filters/validate",
    tag = "filters",
    summary = "Validate filter expression",
    description = "Validate filter expression syntax without testing against data",
    responses(
        (status = 200, description = "Filter validation result"),
        (status = 400, description = "Invalid filter syntax"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn validate_filter(
    Json(payload): Json<FilterTestRequest>,
) -> Result<Json<FilterTestResult>, StatusCode> {
    // Parse the filter expression to validate syntax and generate expression tree
    let parser = crate::filter_parser::FilterParser::new();
    let condition_tree = match parser.parse(&payload.filter_expression) {
        Ok(tree) => tree,
        Err(e) => {
            return Ok(Json(FilterTestResult {
                is_valid: false,
                error: Some(format!("Syntax error: {}", e)),
                matching_channels: vec![],
                total_channels: 0,
                matched_count: 0,
                expression_tree: None,
            }));
        }
    };

    // Generate expression tree for frontend
    let expression_tree = generate_expression_tree_json(&condition_tree);

    Ok(Json(FilterTestResult {
        is_valid: true,
        error: None,
        matching_channels: vec![], // No actual testing for validation
        total_channels: 0,
        matched_count: 0,
        expression_tree: Some(expression_tree),
    }))
}

// Source-specific filter endpoints
/// List filters for a specific stream source
#[utoipa::path(
    get,
    path = "/sources/stream/{source_id}/filters",
    tag = "filters",
    summary = "List stream source filters",
    description = "Get all filters applicable to a specific stream source",
    params(
        ("source_id" = Uuid, Path, description = "Stream source ID")
    ),
    responses(
        (status = 200, description = "Stream source filters retrieved"),
        (status = 404, description = "Stream source not found"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn list_stream_source_filters(
    Path(source_id): Path<Uuid>,
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    // Verify stream source exists
    match state.database.get_stream_source(source_id).await {
        Ok(Some(_)) => {}
        Ok(None) => return Err(StatusCode::NOT_FOUND),
        Err(e) => {
            error!("Failed to verify stream source {}: {}", source_id, e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    }

    // Get filters for this source type
    match state.database.get_filters_with_usage().await {
        Ok(filters) => {
            let stream_filters: Vec<_> = filters
                .into_iter()
                .filter(|f| matches!(f.filter.source_type, FilterSourceType::Stream))
                .collect();

            let result = serde_json::json!({
                "success": true,
                "message": "Stream source filters retrieved",
                "source_id": source_id,
                "source_type": "stream",
                "filters": stream_filters,
                "total_filters": stream_filters.len()
            });

            Ok(Json(result))
        }
        Err(e) => {
            error!("Failed to list stream source filters: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

/// Create a filter for a specific stream source
#[utoipa::path(
    post,
    path = "/sources/stream/{source_id}/filters",
    tag = "filters",
    summary = "Create stream source filter",
    description = "Create a new filter for a specific stream source",
    params(
        ("source_id" = Uuid, Path, description = "Stream source ID")
    ),
    request_body = FilterCreateRequest,
    responses(
        (status = 201, description = "Filter created successfully"),
        (status = 404, description = "Stream source not found"),
        (status = 400, description = "Invalid filter data"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn create_stream_source_filter(
    Path(source_id): Path<Uuid>,
    State(state): State<AppState>,
    Json(mut payload): Json<FilterCreateRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    // Verify stream source exists
    match state.database.get_stream_source(source_id).await {
        Ok(Some(_)) => {}
        Ok(None) => return Err(StatusCode::NOT_FOUND),
        Err(e) => {
            error!("Failed to verify stream source {}: {}", source_id, e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    }

    // Ensure source type is set to Stream
    payload.source_type = FilterSourceType::Stream;

    match state.database.create_filter(&payload).await {
        Ok(filter) => {
            let result = serde_json::json!({
                "success": true,
                "message": "Stream source filter created",
                "source_id": source_id,
                "source_type": "stream",
                "filter": filter
            });

            Ok(Json(result))
        }
        Err(e) => {
            error!("Failed to create stream source filter: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

/// List filters for a specific EPG source
#[utoipa::path(
    get,
    path = "/sources/epg/{source_id}/filters",
    tag = "filters",
    summary = "List EPG source filters",
    description = "Get all filters applicable to a specific EPG source",
    params(
        ("source_id" = Uuid, Path, description = "EPG source ID")
    ),
    responses(
        (status = 200, description = "EPG source filters retrieved"),
        (status = 404, description = "EPG source not found"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn list_epg_source_filters(
    Path(source_id): Path<Uuid>,
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    // Verify EPG source exists
    match state.database.get_epg_source(source_id).await {
        Ok(Some(_)) => {}
        Ok(None) => return Err(StatusCode::NOT_FOUND),
        Err(e) => {
            error!("Failed to verify EPG source {}: {}", source_id, e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    }

    // Get filters for this source type
    match state.database.get_filters_with_usage().await {
        Ok(filters) => {
            let epg_filters: Vec<_> = filters
                .into_iter()
                .filter(|f| matches!(f.filter.source_type, FilterSourceType::Epg))
                .collect();

            let result = serde_json::json!({
                "success": true,
                "message": "EPG source filters retrieved",
                "source_id": source_id,
                "source_type": "epg",
                "filters": epg_filters,
                "total_filters": epg_filters.len()
            });

            Ok(Json(result))
        }
        Err(e) => {
            error!("Failed to list EPG source filters: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

/// Create a filter for a specific EPG source
#[utoipa::path(
    post,
    path = "/sources/epg/{source_id}/filters",
    tag = "filters",
    summary = "Create EPG source filter",
    description = "Create a new filter for a specific EPG source",
    params(
        ("source_id" = Uuid, Path, description = "EPG source ID")
    ),
    request_body = FilterCreateRequest,
    responses(
        (status = 201, description = "Filter created successfully"),
        (status = 404, description = "EPG source not found"),
        (status = 400, description = "Invalid filter data"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn create_epg_source_filter(
    Path(source_id): Path<Uuid>,
    State(state): State<AppState>,
    Json(mut payload): Json<FilterCreateRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    // Verify EPG source exists
    match state.database.get_epg_source(source_id).await {
        Ok(Some(_)) => {}
        Ok(None) => return Err(StatusCode::NOT_FOUND),
        Err(e) => {
            error!("Failed to verify EPG source {}: {}", source_id, e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    }

    // Ensure source type is set to EPG
    payload.source_type = FilterSourceType::Epg;

    match state.database.create_filter(&payload).await {
        Ok(filter) => {
            let result = serde_json::json!({
                "success": true,
                "message": "EPG source filter created",
                "source_id": source_id,
                "source_type": "epg",
                "filter": filter
            });

            Ok(Json(result))
        }
        Err(e) => {
            error!("Failed to create EPG source filter: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

// Cross-source filter operations
/// List all stream filters
#[utoipa::path(
    get,
    path = "/filters/stream",
    tag = "filters",
    summary = "List stream filters",
    description = "Get all filters that apply to stream sources",
    responses(
        (status = 200, description = "Stream filters retrieved"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn list_stream_filters(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    match state.database.get_filters_with_usage().await {
        Ok(filters) => {
            let stream_filters: Vec<_> = filters
                .into_iter()
                .filter(|f| matches!(f.filter.source_type, FilterSourceType::Stream))
                .collect();

            let result = serde_json::json!({
                "success": true,
                "message": "All stream filters retrieved",
                "source_type": "stream",
                "filters": stream_filters,
                "total_filters": stream_filters.len()
            });

            Ok(Json(result))
        }
        Err(e) => {
            error!("Failed to list stream filters: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

/// List all EPG filters
#[utoipa::path(
    get,
    path = "/filters/epg",
    tag = "filters",
    summary = "List EPG filters",
    description = "Get all filters that apply to EPG sources",
    responses(
        (status = 200, description = "EPG filters retrieved"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn list_epg_filters(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    match state.database.get_filters_with_usage().await {
        Ok(filters) => {
            let epg_filters: Vec<_> = filters
                .into_iter()
                .filter(|f| matches!(f.filter.source_type, FilterSourceType::Epg))
                .collect();

            let result = serde_json::json!({
                "success": true,
                "message": "All EPG filters retrieved",
                "source_type": "epg",
                "filters": epg_filters,
                "total_filters": epg_filters.len()
            });

            Ok(Json(result))
        }
        Err(e) => {
            error!("Failed to list EPG filters: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

/// Get available filter fields for stream sources
#[utoipa::path(
    get,
    path = "/filters/fields/stream",
    tag = "filters",
    summary = "Get stream filter fields",
    description = "Get available fields that can be used in stream source filters",
    responses(
        (status = 200, description = "Stream filter fields retrieved"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_stream_filter_fields(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    match state.database.get_available_filter_fields().await {
        Ok(fields) => {
            let stream_fields: Vec<_> = fields
                .into_iter()
                .filter(|f| matches!(f.source_type, FilterSourceType::Stream))
                .collect();

            let result = serde_json::json!({
                "success": true,
                "message": "Stream filter fields retrieved",
                "source_type": "stream",
                "fields": stream_fields,
                "total_fields": stream_fields.len()
            });

            Ok(Json(result))
        }
        Err(e) => {
            error!("Failed to get stream filter fields: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

/// Get available filter fields for EPG sources
#[utoipa::path(
    get,
    path = "/filters/fields/epg",
    tag = "filters",
    summary = "Get EPG filter fields",
    description = "Get available fields that can be used in EPG source filters",
    responses(
        (status = 200, description = "EPG filter fields retrieved"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_epg_filter_fields(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    match state.database.get_available_filter_fields().await {
        Ok(fields) => {
            let epg_fields: Vec<_> = fields
                .into_iter()
                .filter(|f| matches!(f.source_type, FilterSourceType::Epg))
                .collect();

            let result = serde_json::json!({
                "success": true,
                "message": "EPG filter fields retrieved",
                "source_type": "epg",
                "fields": epg_fields,
                "total_fields": epg_fields.len()
            });

            Ok(Json(result))
        }
        Err(e) => {
            error!("Failed to get EPG filter fields: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

// Data Mapping API
/// List all data mapping rules
#[utoipa::path(
    get,
    path = "/data-mapping",
    tag = "data-mapping",
    summary = "List data mapping rules",
    description = "Retrieve all data mapping rules with enhanced metadata",
    responses(
        (status = 200, description = "List of data mapping rules"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn list_data_mapping_rules(
    State(state): State<AppState>,
) -> Result<Json<Vec<serde_json::Value>>, StatusCode> {
    match state.data_mapping_service.get_all_rules().await {
        Ok(rules) => {
            let enhanced_rules = rules
                .into_iter()
                .map(|rule| {
                    // Parse expression to get counts
                    let (condition_count, action_count) = if let Some(expression) = &rule.expression
                    {
                        let available_fields = vec![
                            "tvg_id".to_string(),
                            "tvg_name".to_string(),
                            "tvg_logo".to_string(),
                            "tvg_shift".to_string(),
                            "group_title".to_string(),
                            "channel_name".to_string(),
                        ];
                        let parser =
                            crate::filter_parser::FilterParser::new().with_fields(available_fields);
                        if let Ok(parsed) = parser.parse_extended(expression) {
                            match parsed {
                                crate::models::ExtendedExpression::ConditionOnly(
                                    condition_tree,
                                ) => (count_conditions_in_tree(&condition_tree), 0),
                                crate::models::ExtendedExpression::ConditionWithActions {
                                    condition,
                                    actions,
                                } => (count_conditions_in_tree(&condition), actions.len()),
                                crate::models::ExtendedExpression::ConditionalActionGroups(
                                    groups,
                                ) => {
                                    let condition_count: usize = groups
                                        .iter()
                                        .map(|g| count_conditions_in_tree(&g.conditions))
                                        .sum();
                                    let action_count: usize =
                                        groups.iter().map(|g| g.actions.len()).sum();
                                    (condition_count, action_count)
                                }
                            }
                        } else {
                            (0, 0)
                        }
                    } else {
                        (0, 0)
                    };

                    // Generate JSON expression tree for frontend display
                    let expression_tree = if let Some(expression) = &rule.expression {
                        let available_fields = vec![
                            "tvg_id".to_string(),
                            "tvg_name".to_string(),
                            "tvg_logo".to_string(),
                            "tvg_shift".to_string(),
                            "group_title".to_string(),
                            "channel_name".to_string(),
                        ];
                        let parser =
                            crate::filter_parser::FilterParser::new().with_fields(available_fields);
                        if let Ok(parsed) = parser.parse_extended(expression) {
                            match parsed {
                                crate::models::ExtendedExpression::ConditionOnly(
                                    condition_tree,
                                ) => Some(generate_expression_tree_json(&condition_tree)),
                                crate::models::ExtendedExpression::ConditionWithActions {
                                    condition,
                                    ..
                                } => Some(generate_expression_tree_json(&condition)),
                                crate::models::ExtendedExpression::ConditionalActionGroups(
                                    groups,
                                ) => {
                                    if let Some(first_group) = groups.first() {
                                        Some(generate_expression_tree_json(&first_group.conditions))
                                    } else {
                                        None
                                    }
                                }
                            }
                        } else {
                            None
                        }
                    } else {
                        None
                    };

                    serde_json::json!({
                        "id": rule.id,
                        "name": rule.name,
                        "description": rule.description,
                        "source_type": rule.source_type,
                        "expression": rule.expression,
                        "sort_order": rule.sort_order,
                        "is_active": rule.is_active,
                        "created_at": rule.created_at,
                        "updated_at": rule.updated_at,
                        "condition_count": condition_count,
                        "action_count": action_count,
                        "expression_tree": expression_tree,
                    })
                })
                .collect();

            Ok(Json(enhanced_rules))
        }
        Err(e) => {
            error!("Failed to list data mapping rules: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

#[utoipa::path(
    post,
    path = "/data-mapping",
    tag = "data-mapping",
    summary = "Create data mapping rule",
    description = "Create a new data mapping rule for transforming channel metadata",
    responses(
        (status = 200, description = "Data mapping rule created successfully"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn create_data_mapping_rule(
    State(state): State<AppState>,
    Json(payload): Json<crate::models::data_mapping::DataMappingRuleCreateRequest>,
) -> Result<Json<crate::models::data_mapping::DataMappingRule>, StatusCode> {
    match state.data_mapping_service.create_rule(payload).await {
        Ok(rule) => Ok(Json(rule)),
        Err(e) => {
            error!("Failed to create data mapping rule: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

#[utoipa::path(
    get,
    path = "/data-mapping/{id}",
    tag = "data-mapping",
    summary = "Get data mapping rule",
    description = "Retrieve a specific data mapping rule by ID",
    params(
        ("id" = String, Path, description = "Data mapping rule ID (UUID)"),
    ),
    responses(
        (status = 200, description = "Data mapping rule details"),
        (status = 404, description = "Data mapping rule not found"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_data_mapping_rule(
    Path(id): Path<Uuid>,
    State(state): State<AppState>,
) -> Result<Json<crate::models::data_mapping::DataMappingRule>, StatusCode> {
    match state.data_mapping_service.get_rule_with_details(id).await {
        Ok(Some(rule)) => Ok(Json(rule)),
        Ok(None) => {
            error!("Data mapping rule {} not found", id);
            Err(StatusCode::NOT_FOUND)
        }
        Err(e) => {
            error!("Failed to get data mapping rule {}: {}", id, e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

#[utoipa::path(
    put,
    path = "/data-mapping/{id}",
    tag = "data-mapping",
    summary = "Update data mapping rule",
    description = "Update an existing data mapping rule",
    params(
        ("id" = String, Path, description = "Data mapping rule ID (UUID)"),
    ),
    responses(
        (status = 200, description = "Data mapping rule updated successfully"),
        (status = 404, description = "Data mapping rule not found"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn update_data_mapping_rule(
    Path(id): Path<Uuid>,
    State(state): State<AppState>,
    Json(payload): Json<crate::models::data_mapping::DataMappingRuleUpdateRequest>,
) -> Result<Json<crate::models::data_mapping::DataMappingRule>, StatusCode> {
    match state.data_mapping_service.update_rule(id, payload).await {
        Ok(rule) => Ok(Json(rule)),
        Err(e) => {
            error!("Failed to update data mapping rule {}: {}", id, e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

#[utoipa::path(
    delete,
    path = "/data-mapping/{id}",
    tag = "data-mapping",
    summary = "Delete data mapping rule",
    description = "Delete a data mapping rule",
    params(
        ("id" = String, Path, description = "Data mapping rule ID (UUID)"),
    ),
    responses(
        (status = 204, description = "Data mapping rule deleted successfully"),
        (status = 404, description = "Data mapping rule not found"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn delete_data_mapping_rule(
    Path(id): Path<Uuid>,
    State(state): State<AppState>,
) -> Result<StatusCode, StatusCode> {
    match state.data_mapping_service.delete_rule(id).await {
        Ok(_) => Ok(StatusCode::NO_CONTENT),
        Err(e) => {
            error!("Failed to delete data mapping rule {}: {}", id, e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

#[utoipa::path(
    put,
    path = "/data-mapping/reorder",
    tag = "data-mapping",
    summary = "Reorder data mapping rules",
    description = "Update the order/priority of data mapping rules",
    responses(
        (status = 204, description = "Data mapping rules reordered successfully"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn reorder_data_mapping_rules(
    State(state): State<AppState>,
    Json(payload): Json<Vec<(Uuid, i32)>>,
) -> Result<StatusCode, StatusCode> {
    match state.data_mapping_service.reorder_rules(payload).await {
        Ok(_) => Ok(StatusCode::NO_CONTENT),
        Err(e) => {
            error!("Failed to reorder data mapping rules: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

#[utoipa::path(
    post,
    path = "/data-mapping/validate-expression",
    tag = "data-mapping",
    summary = "Validate data mapping expression",
    description = "Validate a data mapping expression for syntax and available fields",
    responses(
        (status = 200, description = "Expression validation result"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn validate_data_mapping_expression(
    Json(payload): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    use crate::filter_parser::FilterParser;
    use crate::models::data_mapping::DataMappingSourceType;
    use crate::pipeline::engines::DataMappingValidator;

    let expression = payload
        .get("expression")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim();

    let source_type_str = payload
        .get("source_type")
        .and_then(|v| v.as_str())
        .unwrap_or("stream");

    if expression.is_empty() {
        return Ok(Json(serde_json::json!({
            "isValid": false,
            "error": "Expression cannot be empty"
        })));
    }

    // Convert source type string to enum
    let source_type = match source_type_str {
        "stream" => DataMappingSourceType::Stream,
        "epg" => DataMappingSourceType::Epg,
        _ => {
            return Ok(Json(serde_json::json!({
                "isValid": false,
                "error": "Invalid source type"
            })));
        }
    };

    // Use the new validator
    let validation_result = DataMappingValidator::validate_expression(expression, &source_type);

    // Also generate expression tree for UI (maintain existing functionality)
    let expression_tree = if validation_result.is_valid {
        let validator = DataMappingValidator::new(source_type.clone());
        let available_fields = validator.get_available_fields();
        let parser = FilterParser::new().with_fields(available_fields);
        
        match parser.parse_extended(expression) {
            Ok(parsed) => match parsed {
                crate::models::ExtendedExpression::ConditionOnly(condition_tree) => {
                    Some(generate_expression_tree_json(&condition_tree))
                }
                crate::models::ExtendedExpression::ConditionWithActions { condition, .. } => {
                    Some(generate_expression_tree_json(&condition))
                }
                crate::models::ExtendedExpression::ConditionalActionGroups(groups) => {
                    // For action groups, generate tree from first group's condition
                    if let Some(first_group) = groups.first() {
                        Some(generate_expression_tree_json(&first_group.conditions))
                    } else {
                        None
                    }
                }
            },
            Err(_) => None,
        }
    } else {
        None
    };

    let mut response = serde_json::json!({
        "isValid": validation_result.is_valid
    });

    if let Some(error) = validation_result.error {
        response["error"] = serde_json::Value::String(error);
    }

    if let Some(tree) = expression_tree {
        response["expression_tree"] = tree;
    }

    if !validation_result.field_errors.is_empty() {
        response["field_errors"] = serde_json::Value::Array(
            validation_result.field_errors
                .into_iter()
                .map(serde_json::Value::String)
                .collect()
        );
    }

    Ok(Json(response))
}

/// Generalized validation endpoint for any pipeline stage
#[utoipa::path(
    post,
    path = "/pipeline/validate",
    tag = "pipeline",
    summary = "Validate expression for any pipeline stage",
    description = "Validate expressions for data mapping, filtering, numbering, or generation stages",
    responses(
        (status = 200, description = "Validation result"),
        (status = 400, description = "Invalid request"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn validate_pipeline_expression(
    Json(payload): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    use crate::models::data_mapping::DataMappingSourceType;
    use crate::pipeline::{ApiValidationService, PipelineValidationService, PipelineStageType};

    let expression = payload
        .get("expression")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim();

    let stage_name = payload
        .get("stage")
        .and_then(|v| v.as_str())
        .unwrap_or("data_mapping");

    let source_type = payload
        .get("source_type")
        .and_then(|v| v.as_str());

    if expression.is_empty() {
        return Ok(Json(serde_json::json!({
            "isValid": false,
            "error": "Expression cannot be empty"
        })));
    }

    // Use the API validation service for string-based stage identification
    match ApiValidationService::validate_by_stage_name(expression, stage_name, source_type) {
        Ok(validation_result) => {
            let mut response = serde_json::json!({
                "isValid": validation_result.is_valid,
                "stage": stage_name
            });

            if let Some(error) = validation_result.error {
                response["error"] = serde_json::Value::String(error);
            }

            if !validation_result.field_errors.is_empty() {
                response["field_errors"] = serde_json::Value::Array(
                    validation_result.field_errors
                        .into_iter()
                        .map(serde_json::Value::String)
                        .collect()
                );
            }

            // Add expression tree for valid expressions (UI feature)
            if validation_result.is_valid && stage_name == "data_mapping" {
                if let Some(source_type_str) = source_type {
                    let source_type_enum = match source_type_str {
                        "stream" => Some(DataMappingSourceType::Stream),
                        "epg" => Some(DataMappingSourceType::Epg),
                        _ => None,
                    };

                    if let Some(st) = source_type_enum {
                        let stage_type = PipelineStageType::DataMapping;
                        let fields = PipelineValidationService::get_available_fields_for_stage(stage_type, Some(st));
                        
                        let parser = crate::filter_parser::FilterParser::new().with_fields(fields);
                        if let Ok(parsed) = parser.parse_extended(expression) {
                            match parsed {
                                crate::models::ExtendedExpression::ConditionOnly(condition_tree) => {
                                    response["expression_tree"] = generate_expression_tree_json(&condition_tree);
                                }
                                crate::models::ExtendedExpression::ConditionWithActions { condition, .. } => {
                                    response["expression_tree"] = generate_expression_tree_json(&condition);
                                }
                                crate::models::ExtendedExpression::ConditionalActionGroups(groups) => {
                                    // For action groups, generate tree from first group's condition
                                    if let Some(first_group) = groups.first() {
                                        response["expression_tree"] = generate_expression_tree_json(&first_group.conditions);
                                    }
                                }
                            }
                        }
                    }
                }
            }

            Ok(Json(response))
        }
        Err(e) => Ok(Json(serde_json::json!({
            "isValid": false,
            "error": e
        })))
    }
}

/// Get available fields for any pipeline stage
#[utoipa::path(
    get,
    path = "/pipeline/fields/{stage}",
    tag = "pipeline", 
    summary = "Get available fields for a pipeline stage",
    description = "Get available fields for data mapping, filtering, numbering, or generation stages",
    responses(
        (status = 200, description = "List of available fields"),
        (status = 400, description = "Invalid stage"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_pipeline_stage_fields(
    Path(stage_name): Path<String>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    use crate::pipeline::ApiValidationService;

    let source_type = params.get("source_type").map(|s| s.as_str());

    match ApiValidationService::get_fields_by_stage_name(&stage_name, source_type) {
        Ok(fields) => {
            let field_list = fields
                .into_iter()
                .map(|field| {
                    serde_json::json!({
                        "name": field,
                        "description": get_field_description(&field),
                        "type": "string"
                    })
                })
                .collect::<Vec<_>>();

            Ok(Json(serde_json::json!({
                "success": true,
                "stage": stage_name,
                "fields": field_list
            })))
        }
        Err(e) => Ok(Json(serde_json::json!({
            "success": false,
            "error": e
        })))
    }
}

#[utoipa::path(
    get,
    path = "/data-mapping/fields/stream",
    tag = "data-mapping",
    summary = "Get stream mapping fields",
    description = "Get available fields for stream data mapping expressions",
    responses(
        (status = 200, description = "List of available stream mapping fields"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_data_mapping_stream_fields() -> Result<Json<serde_json::Value>, StatusCode> {
    use crate::models::data_mapping::DataMappingSourceType;
    use crate::pipeline::engines::DataMappingValidator;

    let field_infos = DataMappingValidator::get_available_fields_for_source(&DataMappingSourceType::Stream);
    
    let fields = field_infos
        .into_iter()
        .map(|field_info| {
            serde_json::json!({
                "name": field_info.field_name,
                "description": get_field_description(&field_info.field_name),
                "type": "string",
                "display_name": field_info.display_name
            })
        })
        .collect::<Vec<_>>();

    Ok(Json(serde_json::json!({
        "success": true,
        "fields": fields
    })))
}

#[utoipa::path(
    get,
    path = "/data-mapping/fields/epg",
    tag = "data-mapping",
    summary = "Get EPG mapping fields",
    description = "Get available fields for EPG data mapping expressions",
    responses(
        (status = 200, description = "List of available EPG mapping fields"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_data_mapping_epg_fields() -> Result<Json<serde_json::Value>, StatusCode> {
    use crate::models::data_mapping::DataMappingSourceType;
    use crate::pipeline::engines::DataMappingValidator;

    let field_infos = DataMappingValidator::get_available_fields_for_source(&DataMappingSourceType::Epg);
    
    let fields = field_infos
        .into_iter()
        .map(|field_info| {
            serde_json::json!({
                "name": field_info.field_name,
                "description": get_field_description(&field_info.field_name),
                "type": "string",
                "display_name": field_info.display_name
            })
        })
        .collect::<Vec<_>>();

    Ok(Json(serde_json::json!({
        "success": true,
        "fields": fields
    })))
}

fn get_field_description(field: &str) -> &'static str {
    match field {
        "channel_name" => "The name/title of the channel",
        "tvg_id" => "Electronic program guide identifier",
        "tvg_name" => "Name for EPG matching",
        "tvg_logo" => "URL to channel logo image",
        "tvg_shift" => "Time shift in hours for EPG data",
        "group_title" => "Category/group name for the channel",
        "stream_url" => "Direct URL to the media stream",
        "channel_id" => "Unique channel identifier",
        "channel_logo" => "Channel logo URL",
        "channel_group" => "Channel category/group",
        "language" => "Channel language",
        _ => "Channel data field",
    }
}

#[utoipa::path(
    post,
    path = "/data-mapping/test",
    tag = "data-mapping",
    summary = "Test data mapping rule",
    description = "Test a data mapping rule against sample channel data",
    responses(
        (status = 200, description = "Data mapping test result"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn test_data_mapping_rule(
    State(state): State<AppState>,
    Json(payload): Json<crate::models::data_mapping::DataMappingTestRequest>,
) -> Result<Json<crate::models::data_mapping::DataMappingTestResult>, StatusCode> {
    use crate::pipeline::engines::DataMappingTestService;

    // Get channels from the source
    let channels = match state
        .database
        .get_channels_for_source(payload.source_id)
        .await
    {
        Ok(channels) => channels,
        Err(e) => {
            error!(
                "Failed to get channels for source {}: {}",
                payload.source_id, e
            );
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    let total_channels_count = channels.len();

    // Use the new engine-based testing service
    let start_time = std::time::Instant::now();
    match DataMappingTestService::test_single_rule(payload.expression.clone(), channels) {
        Ok(engine_result) => {
            let _execution_time = start_time.elapsed().as_micros();

            // Convert engine result to the expected API format
            let test_channels: Vec<crate::models::data_mapping::DataMappingTestChannel> =
                engine_result.results
                    .into_iter()
                    .filter(|r| r.was_modified) // Only show channels that were modified
                    .map(|r| {
                        // Create original and mapped values in the expected format
                        let original_values = serde_json::json!({
                            "channel_name": r.initial_channel.channel_name,
                            "tvg_id": r.initial_channel.tvg_id,
                            "tvg_name": r.initial_channel.tvg_name,
                            "tvg_logo": r.initial_channel.tvg_logo,
                            "tvg_shift": r.initial_channel.tvg_shift,
                            "group_title": r.initial_channel.group_title,
                        });

                        let mapped_values = serde_json::json!({
                            "channel_name": r.final_channel.channel_name,
                            "tvg_id": r.final_channel.tvg_id,
                            "tvg_name": r.final_channel.tvg_name,
                            "tvg_logo": r.final_channel.tvg_logo,
                            "tvg_shift": r.final_channel.tvg_shift,
                            "group_title": r.final_channel.group_title,
                        });

                        crate::models::data_mapping::DataMappingTestChannel {
                            channel_name: r.channel_name,
                            group_title: r.final_channel.group_title,
                            original_values,
                            mapped_values,
                            applied_actions: r.rule_applications.iter().map(|ra| ra.rule_name.clone()).collect(),
                        }
                    })
                    .collect();

            let result = crate::models::data_mapping::DataMappingTestResult {
                is_valid: true,
                error: None,
                matching_channels: test_channels.clone(),
                total_channels: total_channels_count as i32,
                matched_count: test_channels.len() as i32,
            };

            Ok(Json(result))
        }
        Err(e) => {
            let result = crate::models::data_mapping::DataMappingTestResult {
                is_valid: false,
                error: Some(e.to_string()),
                matching_channels: vec![],
                total_channels: total_channels_count as i32,
                matched_count: 0,
            };
            Ok(Json(result))
        }
    }
}

/// Apply data mapping to stream source
#[utoipa::path(
    post,
    path = "/sources/stream/{source_id}/data-mapping/apply",
    tag = "data-mapping",
    summary = "Apply data mapping to stream source",
    description = "Apply all active data mapping rules to a specific stream source",
    params(
        ("source_id" = String, Path, description = "Stream source ID")
    ),
    responses(
        (status = 200, description = "Data mapping applied successfully"),
        (status = 404, description = "Stream source not found"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn apply_stream_source_data_mapping(
    State(state): State<AppState>,
    Path(source_id): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    info!("Applying data mapping for stream source {}", source_id);

    let source_uuid = match uuid::Uuid::parse_str(&source_id) {
        Ok(uuid) => uuid,
        Err(_) => {
            error!("Invalid source ID format: {}", source_id);
            return Err(StatusCode::BAD_REQUEST);
        }
    };

    // Get source
    let source = match state.database.get_stream_source(source_uuid).await {
        Ok(Some(source)) => source,
        Ok(None) => {
            error!("Stream source {} not found", source_uuid);
            return Err(StatusCode::NOT_FOUND);
        }
        Err(e) => {
            error!("Failed to get stream source {}: {}", source_uuid, e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    // Get original channels
    let channels = match state.database.get_source_channels(source_uuid).await {
        Ok(channels) => channels,
        Err(e) => {
            error!("Failed to get channels for source {}: {}", source_uuid, e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    if channels.is_empty() {
        return Ok(Json(serde_json::json!({
            "success": true,
            "message": "No channels found for preview",
            "original_count": 0,
            "mapped_count": 0,
            "preview_channels": []
        })));
    }

    // Apply data mapping and get channels with metadata
    let (mapped_channels, rule_performance) = match state
        .data_mapping_service
        .apply_mapping_with_metadata(
            channels.clone(),
            source_uuid,
            &state.logo_asset_service,
            &state.config.web.base_url,
            state.config.data_mapping_engine.clone(),
        )
        .await
    {
        Ok(result) => result,
        Err(e) => {
            error!("Data mapping failed for source '{}': {}", source.name, e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    // Debug logging for performance data
    info!(
        "Performance data collected for {} rules",
        rule_performance.len()
    );
    info!("Available performance rule IDs:");
    for (rule_id, &total_time) in rule_performance.iter() {
        info!(
            "  Performance Rule ID '{}': total={}μs",
            rule_id, total_time
        );
    }

    // Filter to show only modified channels for preview
    let modified_channels = DataMappingService::filter_modified_channels(mapped_channels.clone());

    let transformed_channels: Vec<serde_json::Value> = modified_channels
        .iter()
        .map(|mc| mapped_channel_to_frontend_format(mc))
        .collect();

    // Get rule metadata for frontend
    let rules = match state.data_mapping_service.get_all_rules().await {
        Ok(rules) => rules
            .into_iter()
            .filter(|r| {
                r.is_active
                    && r.source_type
                        == crate::models::data_mapping::DataMappingSourceType::Stream
            })
            .map(|rule| {
                let affected_count = modified_channels
                    .iter()
                    .filter(|mc| mc.applied_rules.contains(&rule.name))
                    .count();

                // Get performance stats for this rule
                let (total_execution_time, avg_execution_time) = rule_performance
                    .rule_performance
                    .get(&rule.id.to_string())
                    .map(|&time_ms| (time_ms, time_ms)) // Convert ms to (total, avg)
                    .unwrap_or((0, 0));

                // Debug logging for performance data
                info!(
                    "Looking up rule '{}' (ID: {}) in performance data...",
                    rule.name, rule.id
                );
                info!(
                    "Rule '{}' (ID: {}): performance stats lookup - found: {}, total_time: {}μs, avg_time: {}μs",
                    rule.name,
                    rule.id,
                    rule_performance.rule_performance.contains_key(&rule.id.to_string()),
                    total_execution_time,
                    avg_execution_time
                );
                if !rule_performance.rule_performance.contains_key(&rule.id.to_string()) {
                    info!("Available IDs in performance data: {:?}", rule_performance.rule_performance.keys().collect::<Vec<_>>());
                }

                // Parse expression to get counts
                let (condition_count, action_count) = if let Some(expression) = &rule.expression {
                    let available_fields = vec![
                        "tvg_id".to_string(),
                        "tvg_name".to_string(),
                        "tvg_logo".to_string(),
                        "tvg_shift".to_string(),
                        "group_title".to_string(),
                        "channel_name".to_string(),
                    ];
                    let parser = crate::filter_parser::FilterParser::new().with_fields(available_fields);
                    if let Ok(parsed) = parser.parse_extended(expression) {
                        match parsed {
                            crate::models::ExtendedExpression::ConditionOnly(condition_tree) => {
                                (count_conditions_in_tree(&condition_tree), 0)
                            }
                            crate::models::ExtendedExpression::ConditionWithActions { condition, actions } => {
                                (count_conditions_in_tree(&condition), actions.len())
                            }
                            crate::models::ExtendedExpression::ConditionalActionGroups(groups) => {
                                let condition_count: usize = groups
                                    .iter()
                                    .map(|g| count_conditions_in_tree(&g.conditions))
                                    .sum();
                                let action_count: usize = groups.iter().map(|g| g.actions.len()).sum();
                                (condition_count, action_count)
                            }
                        }
                    } else {
                        (0, 0)
                    }
                } else {
                    (0, 0)
                };

                // Generate human-readable condition tree for frontend display
                let expression_tree = if let Some(expression) = &rule.expression {
                    let available_fields = vec![
                        "tvg_id".to_string(),
                        "tvg_name".to_string(),
                        "tvg_logo".to_string(),
                        "tvg_shift".to_string(),
                        "group_title".to_string(),
                        "channel_name".to_string(),
                    ];
                    let parser = crate::filter_parser::FilterParser::new().with_fields(available_fields);
                    if let Ok(parsed) = parser.parse_extended(expression) {
                        match parsed {
                            crate::models::ExtendedExpression::ConditionOnly(condition_tree) => {
                                Some(generate_expression_tree_json(&condition_tree))
                            }
                            crate::models::ExtendedExpression::ConditionWithActions { condition, .. } => {
                                Some(generate_expression_tree_json(&condition))
                            }
                            crate::models::ExtendedExpression::ConditionalActionGroups(groups) => {
                                if let Some(first_group) = groups.first() {
                                    Some(generate_expression_tree_json(&first_group.conditions))
                                } else {
                                    None
                                }
                            }
                        }
                    } else {
                        None
                    }
                } else {
                    None
                };

                serde_json::json!({
                    "rule_id": rule.id,
                    "rule_name": rule.name,
                    "rule_description": rule.description,
                    "affected_channels_count": affected_count,
                    "expression": rule.expression,
                    "condition_count": condition_count,
                    "action_count": action_count,
                    "expression_tree": expression_tree,
                    "avg_execution_time": avg_execution_time,
                    "total_execution_time": total_execution_time,
                    "sort_order": rule.sort_order,
                })
            })
            .collect::<Vec<_>>(),
        Err(_) => vec![],
    };

    let result = serde_json::json!({
        "success": true,
        "message": "Data mapping applied successfully",
        "source_name": source.name,
        "source_type": "stream",
        "original_count": channels.len(),
        "mapped_count": modified_channels.len(),
        "total_rules": rules.len(),
        "rules": rules,
        "final_channels": transformed_channels
    });

    info!(
        "Data mapping applied for stream source '{}': {} original -> {} modified channels shown in preview",
        source.name,
        channels.len(),
        modified_channels.len()
    );

    Ok(Json(result))
}

/// Apply data mapping to EPG source
#[utoipa::path(
    post,
    path = "/sources/epg/{source_id}/data-mapping/apply",
    tag = "data-mapping",
    summary = "Apply data mapping to EPG source",
    description = "Apply all active data mapping rules to a specific EPG source",
    params(
        ("source_id" = String, Path, description = "EPG source ID")
    ),
    responses(
        (status = 200, description = "Data mapping applied successfully"),
        (status = 404, description = "EPG source not found"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn apply_epg_source_data_mapping(
    State(state): State<AppState>,
    Path(source_id): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    info!("Applying data mapping for EPG source {}", source_id);

    let source_uuid = match uuid::Uuid::parse_str(&source_id) {
        Ok(uuid) => uuid,
        Err(_) => {
            error!("Invalid source ID format: {}", source_id);
            return Err(StatusCode::BAD_REQUEST);
        }
    };

    // Get EPG source
    let source = match state.database.get_epg_source(source_uuid).await {
        Ok(Some(source)) => source,
        Ok(None) => {
            error!("EPG source {} not found", source_uuid);
            return Err(StatusCode::NOT_FOUND);
        }
        Err(e) => {
            error!("Failed to get EPG source {}: {}", source_uuid, e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    // Check if the source has programs (this is our primary EPG data now)
    let program_count = match state.database.get_epg_source_channel_count(source_uuid).await {
        Ok(count) => count,
        Err(e) => {
            error!(
                "Failed to check EPG program count for source {}: {}",
                source_uuid, e
            );
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    if program_count == 0 {
        return Ok(Json(serde_json::json!({
            "success": true,
            "message": "No EPG programs found for preview",
            "original_count": 0,
            "mapped_count": 0,
            "preview_channels": []
        })));
    }

    // For now, return placeholder response since EPG data mapping may need different logic
    let result = serde_json::json!({
        "success": true,
        "message": "EPG data mapping preview completed",
        "source_name": source.name,
        "source_type": "epg",
        "original_count": program_count,
        "mapped_count": program_count,
        "preview_programs": program_count
    });

    info!(
        "EPG data mapping preview completed for source '{}': {} programs available",
        source.name,
        program_count
    );

    Ok(Json(result))
}

// Global data mapping application endpoints for "Preview All Rules" functionality
#[utoipa::path(
    post,
    path = "/data-mapping/preview",
    tag = "data-mapping",
    summary = "Apply data mapping rules (POST)",
    description = "Apply data mapping rules to preview transformations with request body",
    responses(
        (status = 200, description = "Data mapping preview result"),
        (status = 400, description = "Invalid request"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn apply_data_mapping_rules_post(
    State(state): State<AppState>,
    Json(payload): Json<DataMappingPreviewRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    apply_data_mapping_rules_impl(state, &payload.source_type, payload.limit).await
}

#[utoipa::path(
    get,
    path = "/data-mapping/preview",
    tag = "data-mapping",
    summary = "Apply data mapping rules (GET)",
    description = "Apply data mapping rules to preview transformations with query parameters",
    params(
        ("source_type" = Option<String>, Query, description = "Source type to preview (stream/epg)"),
        ("limit" = Option<u32>, Query, description = "Limit number of preview results")
    ),
    responses(
        (status = 200, description = "Data mapping preview result"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn apply_data_mapping_rules(
    Query(params): Query<std::collections::HashMap<String, String>>,
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let source_type = params
        .get("source_type")
        .unwrap_or(&"stream".to_string())
        .clone();

    let limit = params.get("limit").and_then(|s| s.parse::<u32>().ok());

    apply_data_mapping_rules_impl(state, &source_type, limit).await
}

async fn apply_data_mapping_rules_impl(
    state: AppState,
    source_type: &str,
    limit: Option<u32>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    match source_type {
        "stream" => {
            // Get all active stream sources
            let sources = match state.database.list_stream_sources().await {
                Ok(sources) => sources
                    .into_iter()
                    .filter(|s| s.is_active)
                    .collect::<Vec<_>>(),
                Err(e) => {
                    error!("Failed to get stream sources for preview: {}", e);
                    return Err(StatusCode::INTERNAL_SERVER_ERROR);
                }
            };

            if sources.is_empty() {
                return Ok(Json(serde_json::json!({
                    "success": true,
                    "message": "No active stream sources found",
                    "total_sources": 0,
                    "total_channels": 0,
                    "final_channels": []
                })));
            }

            let mut all_preview_channels = Vec::new();
            let mut total_channels = 0;
            let mut combined_performance_data = std::collections::HashMap::new();

            // Process all sources
            for source in sources.iter() {
                let channels = match state.database.get_source_channels(source.id).await {
                    Ok(channels) => channels,
                    Err(_) => continue,
                };

                total_channels += channels.len();

                if !channels.is_empty() {
                    let (mapped_channels, rule_performance) = match state
                        .data_mapping_service
                        .apply_mapping_with_metadata(
                            channels.clone(),
                            source.id,
                            &state.logo_asset_service,
                            &state.config.web.base_url,
                            state.config.data_mapping_engine.clone(),
                        )
                        .await
                    {
                        Ok(result) => result,
                        Err(_) => continue,
                    };

                    // Merge performance data from all sources
                    for (rule_id, &time_ms) in &rule_performance.rule_performance {
                        let entry = combined_performance_data
                            .entry(rule_id.clone())
                            .or_insert((0u128, 0u128, 0usize));
                        entry.0 += time_ms; // Sum total execution times
                        entry.1 = time_ms; // Use the time as average (simplified)
                        entry.2 += 1; // Count number of rule executions
                    }

                    // Filter to show only modified channels
                    let modified_channels =
                        DataMappingService::filter_modified_channels(mapped_channels);
                    all_preview_channels.extend(modified_channels);
                }
            }

            // Apply limit if specified
            let limited_channels = if let Some(limit) = limit {
                all_preview_channels
                    .into_iter()
                    .take(limit as usize)
                    .collect()
            } else {
                all_preview_channels
            };

            let transformed_channels: Vec<serde_json::Value> = limited_channels
                .iter()
                .map(|mc| mapped_channel_to_frontend_format(mc))
                .collect();

            // Get rule metadata for frontend
            let rules = match state.data_mapping_service.get_all_rules().await {
                Ok(rules) => rules
                    .into_iter()
                    .filter(|r| {
                        r.is_active
                            && r.source_type
                                == crate::models::data_mapping::DataMappingSourceType::Stream
                    })
                    .map(|rule| {
                        let affected_count = limited_channels
                            .iter()
                            .filter(|mc| mc.applied_rules.contains(&rule.name))
                            .count();

                        // Get actual performance data for this rule
                        let (total_execution_time, avg_execution_time, _processed_count) =
                            combined_performance_data
                                .get(&rule.id.to_string())
                                .map(|(total, avg, count)| (*total, *avg, *count))
                                .unwrap_or((0, 0, 0));

                        // Parse expression to get counts
                        let (condition_count, action_count) =
                            if let Some(expression) = &rule.expression {
                                let available_fields = vec![
                                    "tvg_id".to_string(),
                                    "tvg_name".to_string(),
                                    "tvg_logo".to_string(),
                                    "tvg_shift".to_string(),
                                    "group_title".to_string(),
                                    "channel_name".to_string(),
                                ];
                                let parser = crate::filter_parser::FilterParser::new()
                                    .with_fields(available_fields);
                                if let Ok(parsed) = parser.parse_extended(expression) {
                                    match parsed {
                                    crate::models::ExtendedExpression::ConditionOnly(
                                        condition_tree,
                                    ) => (count_conditions_in_tree(&condition_tree), 0),
                                    crate::models::ExtendedExpression::ConditionWithActions {
                                        condition,
                                        actions,
                                    } => (count_conditions_in_tree(&condition), actions.len()),
                                    crate::models::ExtendedExpression::ConditionalActionGroups(
                                        groups,
                                    ) => {
                                        let condition_count: usize = groups
                                            .iter()
                                            .map(|g| count_conditions_in_tree(&g.conditions))
                                            .sum();
                                        let action_count: usize =
                                            groups.iter().map(|g| g.actions.len()).sum();
                                        (condition_count, action_count)
                                    }
                                }
                                } else {
                                    (0, 0)
                                }
                            } else {
                                (0, 0)
                            };

                        // Generate JSON expression tree for frontend display
                        let expression_tree = if let Some(expression) = &rule.expression {
                            let available_fields = vec![
                                "tvg_id".to_string(),
                                "tvg_name".to_string(),
                                "tvg_logo".to_string(),
                                "tvg_shift".to_string(),
                                "group_title".to_string(),
                                "channel_name".to_string(),
                            ];
                            let parser = crate::filter_parser::FilterParser::new()
                                .with_fields(available_fields);
                            if let Ok(parsed) = parser.parse_extended(expression) {
                                match parsed {
                                    crate::models::ExtendedExpression::ConditionOnly(
                                        condition_tree,
                                    ) => Some(generate_expression_tree_json(&condition_tree)),
                                    crate::models::ExtendedExpression::ConditionWithActions {
                                        condition,
                                        ..
                                    } => Some(generate_expression_tree_json(&condition)),
                                    crate::models::ExtendedExpression::ConditionalActionGroups(
                                        groups,
                                    ) => {
                                        if let Some(first_group) = groups.first() {
                                            Some(generate_expression_tree_json(
                                                &first_group.conditions,
                                            ))
                                        } else {
                                            None
                                        }
                                    }
                                }
                            } else {
                                None
                            }
                        } else {
                            None
                        };

                        serde_json::json!({
                                "rule_id": rule.id,
                                "rule_name": rule.name,
                                "rule_description": rule.description,
                                "affected_channels_count": affected_count,
                                "expression": rule.expression,
                                "condition_count": condition_count,
                                "action_count": action_count,
                                "expression_tree": expression_tree,
                                "avg_execution_time": avg_execution_time,
                        "total_execution_time": total_execution_time,
                                "sort_order": rule.sort_order,
                        })
                    })
                    .collect::<Vec<_>>(),
                Err(_) => vec![],
            };

            Ok(Json(serde_json::json!({
                "success": true,
                "message": "Stream rules applied successfully",
                "source_type": "stream",
                "total_sources": sources.len(),
                "total_channels": total_channels,
                "total_rules": rules.len(),
                "rules": rules,
                "final_channels": transformed_channels
            })))
        }
        "epg" => {
            // Get all active EPG sources
            let sources = match state.database.list_epg_sources().await {
                Ok(sources) => sources
                    .into_iter()
                    .filter(|s| s.is_active)
                    .collect::<Vec<_>>(),
                Err(e) => {
                    error!("Failed to get EPG sources for preview: {}", e);
                    return Err(StatusCode::INTERNAL_SERVER_ERROR);
                }
            };

            if sources.is_empty() {
                return Ok(Json(serde_json::json!({
                    "success": true,
                    "message": "No active EPG sources found",
                    "total_sources": 0,
                    "total_channels": 0,
                    "final_channels": []
                })));
            }

            // EPG mapping implementation would go here
            // For now, return placeholder
            Ok(Json(serde_json::json!({
                "success": true,
                "message": "EPG rules applied successfully (placeholder)",
                "source_type": "epg",
                "total_sources": sources.len(),
                "total_channels": 0,
                "final_channels": []
            })))
        }
        _ => {
            error!("Invalid source_type parameter: {}", source_type);
            Err(StatusCode::BAD_REQUEST)
        }
    }
}

// Logo Assets API
/// List logo assets with optional filtering
#[utoipa::path(
    get,
    path = "/logos",
    tag = "logos",
    summary = "List logo assets",
    description = "Retrieve a list of logo assets with optional filtering and cached logo inclusion",
    params(
        ("include_cached" = Option<bool>, Query, description = "Include cached logos in the response"),
        ("page" = Option<u32>, Query, description = "Page number for pagination"),
        ("limit" = Option<u32>, Query, description = "Number of items per page"),
    ),
    responses(
        (status = 200, description = "List of logo assets"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn list_logo_assets(
    Query(params): Query<crate::models::logo_asset::LogoAssetListRequest>,
    State(state): State<AppState>,
) -> Result<Json<crate::models::logo_asset::LogoAssetListResponse>, StatusCode> {
    let include_cached = params.include_cached.unwrap_or(true);

    if include_cached && state.logo_cache_scanner.is_some() {
        // Use enhanced listing with cached logos
        match list_logo_assets_with_cached(params, &state).await {
            Ok(response) => Ok(Json(response)),
            Err(e) => {
                error!("Failed to list logo assets with cached: {}", e);
                Err(StatusCode::INTERNAL_SERVER_ERROR)
            }
        }
    } else {
        // Use standard database-only listing
        match state.logo_asset_service.list_assets(params).await {
            Ok(response) => Ok(Json(response)),
            Err(e) => {
                error!("Failed to list logo assets: {}", e);
                Err(StatusCode::INTERNAL_SERVER_ERROR)
            }
        }
    }
}

/// Enhanced logo listing that includes cached logos from filesystem
async fn list_logo_assets_with_cached(
    params: crate::models::logo_asset::LogoAssetListRequest,
    state: &AppState,
) -> Result<crate::models::logo_asset::LogoAssetListResponse, anyhow::Error> {
    use crate::models::logo_asset::*;

    let page = params.page.unwrap_or(1);
    let limit = params.limit.unwrap_or(20);
    let search_query = params.search.as_deref();

    // Get database assets
    let db_response = state.logo_asset_service.list_assets(params.clone()).await?;
    let mut all_assets = db_response.assets;

    // Add cached logos if scanner is available
    if let Some(scanner) = &state.logo_cache_scanner {
        debug!(
            "Logo cache scanner available, searching for cached logos with query: {:?}",
            search_query
        );
        let cached_logos = scanner
            .search_cached_logos(search_query, None)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to search cached logos: {}", e))?;

        debug!("Found {} cached logos from scanner", cached_logos.len());

        // Convert cached logos to LogoAssetWithUrl format
        let mut converted_count = 0;
        for cached_logo in cached_logos {
            let asset_like = cached_logo.to_logo_asset_like(&state.config.web.base_url);

            // Convert JSON value to LogoAssetWithUrl
            match serde_json::from_value::<LogoAssetWithUrl>(asset_like.clone()) {
                Ok(logo_asset_with_url) => {
                    all_assets.push(logo_asset_with_url);
                    converted_count += 1;
                }
                Err(e) => {
                    debug!(
                        "Failed to convert cached logo to LogoAssetWithUrl: {}. JSON: {}",
                        e, asset_like
                    );
                }
            }
        }
        debug!(
            "Successfully converted {} cached logos to LogoAssetWithUrl",
            converted_count
        );
    } else {
        debug!("No logo cache scanner available");
    }

    // Apply search filter if provided (for database assets that might not have been filtered)
    if let Some(query) = search_query {
        let query_lower = query.to_lowercase();
        all_assets.retain(|asset| {
            asset.asset.name.to_lowercase().contains(&query_lower)
                || asset.asset.file_name.to_lowercase().contains(&query_lower)
                || asset
                    .asset
                    .description
                    .as_ref()
                    .map(|desc| desc.to_lowercase().contains(&query_lower))
                    .unwrap_or(false)
        });
    }

    // Sort by updated_at descending (most recent first)
    all_assets.sort_by(|a, b| b.asset.updated_at.cmp(&a.asset.updated_at));

    // Calculate pagination
    let total_count = all_assets.len() as i64;
    let total_pages = ((total_count as f64) / (limit as f64)).ceil() as u32;

    // Apply pagination
    let start_idx = ((page - 1) * limit) as usize;
    let end_idx = (start_idx + limit as usize).min(all_assets.len());
    let paginated_assets = if start_idx < all_assets.len() {
        all_assets[start_idx..end_idx].to_vec()
    } else {
        Vec::new()
    };

    Ok(LogoAssetListResponse {
        assets: paginated_assets,
        total_count,
        page,
        limit,
        total_pages,
    })
}

/// Upload a new logo asset
#[utoipa::path(
    post,
    path = "/logos/upload",
    tag = "logos",
    summary = "Upload logo asset",
    description = "Upload a new logo asset file with metadata",
    request_body(content = String, description = "Multipart form data with logo file and metadata"),
    responses(
        (status = 200, description = "Logo asset uploaded successfully"),
        (status = 400, description = "Invalid upload data"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn upload_logo_asset(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<Json<crate::models::logo_asset::LogoAssetUploadResponse>, StatusCode> {
    use crate::models::logo_asset::{LogoAssetCreateRequest, LogoAssetUploadResponse};

    let mut file_data: Option<(String, String, axum::body::Bytes)> = None;
    let mut logo_name: Option<String> = None;
    let mut logo_description: Option<String> = None;

    // Process all multipart fields
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|_| StatusCode::BAD_REQUEST)?
    {
        match field.name() {
            Some("file") => {
                let file_name = field
                    .file_name()
                    .ok_or(StatusCode::BAD_REQUEST)?
                    .to_string();

                let content_type = field
                    .content_type()
                    .unwrap_or("application/octet-stream")
                    .to_string();

                let data = field.bytes().await.map_err(|_| StatusCode::BAD_REQUEST)?;

                // Validate file type
                if !content_type.starts_with("image/") {
                    return Err(StatusCode::BAD_REQUEST);
                }

                file_data = Some((file_name, content_type, data));
            }
            Some("name") => {
                let data = field.bytes().await.map_err(|_| StatusCode::BAD_REQUEST)?;
                logo_name = Some(String::from_utf8_lossy(&data).to_string());
            }
            Some("description") => {
                let data = field.bytes().await.map_err(|_| StatusCode::BAD_REQUEST)?;
                let desc = String::from_utf8_lossy(&data).to_string();
                if !desc.trim().is_empty() {
                    logo_description = Some(desc);
                }
            }
            _ => {} // Ignore other fields
        }
    }

    // Ensure we have the required file data
    let (file_name, content_type, data) = file_data.ok_or(StatusCode::BAD_REQUEST)?;

    // Create the logo asset with proper name
    let create_request = LogoAssetCreateRequest {
        name: logo_name.unwrap_or(file_name.clone()),
        description: logo_description,
        asset_type: crate::models::logo_asset::LogoAssetType::Uploaded,
        source_url: None,
    };

    // For now, use a simplified upload approach until we implement the full conversion logic
    let asset_id = uuid::Uuid::new_v4();
    let file_extension = content_type.split('/').last().unwrap_or("img");

    match state
        .logo_asset_service
        .storage
        .save_uploaded_file(data.to_vec(), asset_id, file_extension)
        .await
    {
        Ok((file_name, file_path, file_size, mime_type, dimensions)) => {
            match state
                .logo_asset_service
                .create_asset_with_id(
                    asset_id,
                    create_request.name,
                    create_request.description,
                    file_name,
                    file_path,
                    file_size,
                    mime_type,
                    crate::models::logo_asset::LogoAssetType::Uploaded,
                    None, // source_url
                    dimensions.map(|(w, _)| w as i32),
                    dimensions.map(|(_, h)| h as i32),
                )
                .await
            {
                Ok(asset) => Ok(Json(LogoAssetUploadResponse {
                    id: asset.id,
                    name: asset.name,
                    file_name: asset.file_name,
                    file_size: asset.file_size,
                    url: format!("/api/v1/logos/{}", asset.id),
                })),
                Err(e) => {
                    error!("Failed to create logo asset: {}", e);
                    Err(StatusCode::INTERNAL_SERVER_ERROR)
                }
            }
        }
        Err(e) => {
            error!("Failed to save uploaded file: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

/// Get logo asset image bytes with preference for PNG format
/// Returns image bytes for /api/v1/logos/:id endpoint
#[utoipa::path(
    get,
    path = "/logos/{id}",
    tag = "logos",
    summary = "Get logo asset image",
    description = "Retrieve logo asset image data with preference for PNG format",
    params(
        ("id" = String, Path, description = "Logo asset ID (UUID)"),
    ),
    responses(
        (status = 200, description = "Logo asset image data"),
        (status = 404, description = "Logo asset not found"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_logo_asset_image(
    Path(id): Path<Uuid>,
    State(state): State<AppState>,
) -> Result<(axum::http::HeaderMap, Vec<u8>), StatusCode> {
    // Get the main asset first
    let main_asset = match state.logo_asset_service.get_asset(id).await {
        Ok(asset) => asset,
        Err(e) => {
            error!("Failed to get logo asset {}: {}", id, e);
            return Err(StatusCode::NOT_FOUND);
        }
    };

    // Use preference order: png > svg > webp > original
    let preference_order = ["png", "svg", "webp"];

    // Get all assets (main + linked)
    let mut all_assets = vec![main_asset.clone()];
    if let Ok(linked_assets) = state.logo_asset_service.get_linked_assets(id).await {
        for linked_asset in linked_assets {
            all_assets.push(linked_asset);
        }
    }

    // Try preferred formats in order
    for preferred_format in preference_order {
        for asset in &all_assets {
            if asset_matches_format(asset, preferred_format) {
                return serve_asset(&state, asset.clone()).await;
            }
        }
    }

    // Fall back to original asset
    serve_asset(&state, main_asset).await
}

/// Get logo asset in a specific format
/// Returns image bytes for /api/v1/logos/:id/formats/:format endpoint
#[utoipa::path(
    get,
    path = "/logos/{id}/formats/{format}",
    tag = "logos",
    summary = "Get logo asset in specific format",
    description = "Retrieve logo asset data in a specific format (webp, png, etc.)",
    params(
        ("id" = String, Path, description = "Logo asset ID (UUID)"),
        ("format" = String, Path, description = "Desired format (webp, png, etc.)")
    ),
    responses(
        (status = 200, description = "Logo image data", content_type = "image/*"),
        (status = 404, description = "Logo asset or format not found"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_logo_asset_format(
    Path((id, format)): Path<(Uuid, String)>,
    State(state): State<AppState>,
) -> Result<(axum::http::HeaderMap, Vec<u8>), StatusCode> {
    // Get the main asset first
    let main_asset = match state.logo_asset_service.get_asset(id).await {
        Ok(asset) => asset,
        Err(e) => {
            error!("Failed to get logo asset {}: {}", id, e);
            return Err(StatusCode::NOT_FOUND);
        }
    };

    // First check if the main asset matches the requested format
    if asset_matches_format(&main_asset, &format) {
        return serve_asset(&state, main_asset).await;
    }

    // Look for linked assets with the requested format
    if let Ok(linked_assets) = state.logo_asset_service.get_linked_assets(id).await {
        for linked_asset in linked_assets {
            if asset_matches_format(&linked_asset, &format) {
                return serve_asset(&state, linked_asset).await;
            }
        }
    }

    // Requested format not found
    Err(StatusCode::NOT_FOUND)
}

fn asset_matches_format(asset: &crate::models::logo_asset::LogoAsset, format: &str) -> bool {
    match format.to_lowercase().as_str() {
        "png" => asset.mime_type.contains("png") || asset.file_name.ends_with(".png"),
        "jpg" | "jpeg" => {
            asset.mime_type.contains("jpeg")
                || asset.file_name.ends_with(".jpg")
                || asset.file_name.ends_with(".jpeg")
        }
        "svg" => asset.mime_type.contains("svg") || asset.file_name.ends_with(".svg"),
        "webp" => asset.mime_type.contains("webp") || asset.file_name.ends_with(".webp"),
        "gif" => asset.mime_type.contains("gif") || asset.file_name.ends_with(".gif"),
        _ => false,
    }
}

async fn serve_asset(
    state: &AppState,
    asset: crate::models::logo_asset::LogoAsset,
) -> Result<(axum::http::HeaderMap, Vec<u8>), StatusCode> {
    match state.logo_asset_storage.get_file(&asset.file_path).await {
        Ok(file_data) => {
            let mut headers = axum::http::HeaderMap::new();
            headers.insert(
                axum::http::header::CONTENT_TYPE,
                asset
                    .mime_type
                    .parse()
                    .unwrap_or_else(|_| "application/octet-stream".parse().unwrap()),
            );
            headers.insert(
                axum::http::header::CACHE_CONTROL,
                "public, max-age=86400".parse().unwrap(),
            );
            Ok((headers, file_data))
        }
        Err(e) => {
            error!("Failed to read logo file {}: {}", asset.file_path, e);
            Err(StatusCode::NOT_FOUND)
        }
    }
}

#[utoipa::path(
    put,
    path = "/logos/{id}",
    tag = "logos",
    summary = "Update logo asset",
    description = "Update logo asset metadata",
    params(
        ("id" = String, Path, description = "Logo asset ID (UUID)"),
    ),
    responses(
        (status = 200, description = "Logo asset updated successfully"),
        (status = 404, description = "Logo asset not found"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn update_logo_asset(
    Path(id): Path<Uuid>,
    State(state): State<AppState>,
    Json(payload): Json<crate::models::logo_asset::LogoAssetUpdateRequest>,
) -> Result<Json<crate::models::logo_asset::LogoAsset>, StatusCode> {
    match state
        .logo_asset_service
        .update_asset(id, payload.name, payload.description)
        .await
    {
        Ok(asset) => Ok(Json(asset)),
        Err(e) => {
            error!("Failed to update logo asset {}: {}", id, e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

#[utoipa::path(
    delete,
    path = "/logos/{id}",
    tag = "logos",
    summary = "Delete logo asset",
    description = "Delete a logo asset and its file",
    params(
        ("id" = String, Path, description = "Logo asset ID (UUID)"),
    ),
    responses(
        (status = 204, description = "Logo asset deleted successfully"),
        (status = 404, description = "Logo asset not found"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn delete_logo_asset(
    Path(id): Path<Uuid>,
    State(state): State<AppState>,
) -> Result<StatusCode, StatusCode> {
    // Get asset first to get file path
    let asset = match state.logo_asset_service.get_asset(id).await {
        Ok(asset) => asset,
        Err(_) => return Err(StatusCode::NOT_FOUND),
    };

    // Get linked assets before deleting from database
    let linked_assets = match state.logo_asset_service.get_linked_assets(id).await {
        Ok(linked) => linked,
        Err(e) => {
            error!("Failed to get linked assets for {}: {}", id, e);
            Vec::new()
        }
    };

    // Delete from database
    match state.logo_asset_service.delete_asset(id).await {
        Ok(_) => {
            // Delete main asset file from storage
            if let Err(e) = state.logo_asset_storage.delete_file(&asset.file_path).await {
                error!("Failed to delete logo file {}: {}", asset.file_path, e);
            }

            // Delete linked asset files from storage
            for linked_asset in linked_assets {
                if let Err(e) = state
                    .logo_asset_storage
                    .delete_file(&linked_asset.file_path)
                    .await
                {
                    error!(
                        "Failed to delete linked logo file {}: {}",
                        linked_asset.file_path, e
                    );
                }
            }

            Ok(StatusCode::NO_CONTENT)
        }
        Err(e) => {
            error!("Failed to delete logo asset {}: {}", id, e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

/// Get cached logo asset by cache ID
/// This endpoint serves logos cached by the WASM plugin system using the sandboxed file manager
/// Normalized format: cache_id.png, with fallback to legacy formats
#[utoipa::path(
    get,
    path = "/logos/cached/{cache_id}",
    tag = "logos",
    summary = "Get cached logo asset",
    description = "Retrieve cached logo asset by cache ID (served from file system)",
    params(
        ("cache_id" = String, Path, description = "Cached logo identifier")
    ),
    responses(
        (status = 200, description = "Cached logo image data", content_type = "image/*"),
        (status = 404, description = "Cached logo not found"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_cached_logo_asset(
    Path(cache_id): Path<String>,
    State(state): State<AppState>,
) -> Result<(axum::http::HeaderMap, Vec<u8>), StatusCode> {
    // The file manager's base is already the cached logo directory, so use direct paths
    let mut file_extension = "png".to_string();
    let file_data = match state
        .logo_file_manager
        .read(&format!("{}.png", cache_id))
        .await
    {
        Ok(data) => {
            debug!("Serving normalized cached logo: {}.png", cache_id);
            data
        }
        Err(_) => {
            // Fall back to legacy format with other extensions
            let mut found_data = None;
            for ext in &["jpg", "jpeg", "gif", "webp", "svg"] {
                match state
                    .logo_file_manager
                    .read(&format!("{}.{}", cache_id, ext))
                    .await
                {
                    Ok(data) => {
                        file_extension = ext.to_string();
                        found_data = Some(data);
                        debug!("Serving legacy cached logo: {}.{}", cache_id, ext);
                        break;
                    }
                    Err(_) => continue,
                }
            }
            found_data.ok_or_else(|| {
                debug!("Cached logo not found: {}", cache_id);
                StatusCode::NOT_FOUND
            })?
        }
    };

    // Determine content type from extension
    let content_type = match file_extension.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        _ => "application/octet-stream",
    };

    // Set response headers
    let mut headers = axum::http::HeaderMap::new();
    headers.insert(
        axum::http::header::CONTENT_TYPE,
        content_type.parse().unwrap(),
    );
    headers.insert(
        axum::http::header::CACHE_CONTROL,
        "public, max-age=2592000".parse().unwrap(), // 30 days cache
    );

    Ok((headers, file_data))
}

/// Legacy health check endpoint
#[utoipa::path(
    get,
    path = "/health",
    tag = "health",
    summary = "Legacy health check",
    description = "Simple health check endpoint (deprecated - use /health instead)",
    responses(
        (status = 200, description = "Service is healthy")
    )
)]
pub async fn health_check() -> Result<Json<serde_json::Value>, StatusCode> {
    Ok(Json(json!({
        "status": "healthy",
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "service": "m3u-proxy"
    })))
}

/// Refresh stream source
#[utoipa::path(
    post,
    path = "/sources/stream/{id}/refresh",
    tag = "sources",
    summary = "Refresh stream source",
    description = "Manually trigger a refresh of a stream source to reload channels",
    params(
        ("id" = String, Path, description = "Stream source ID (UUID)"),
    ),
    responses(
        (status = 200, description = "Refresh initiated successfully"),
        (status = 404, description = "Stream source not found"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn refresh_stream_source(
    Path(id): Path<Uuid>,
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    match state.database.get_stream_source(id).await {
        Ok(Some(source)) => {
            tokio::spawn({
                let stream_service = state.stream_source_service.clone();
                let progress_service = state.progress_service.clone(); // Use shared instance!
                async move {
                    if let Err(e) = stream_service.refresh_with_progress(&source, &progress_service).await {
                        error!("Failed to refresh stream source {}: {}", source.id, e);
                    }
                }
            });

            // Emit scheduler event for manual refresh trigger
            state.database.emit_scheduler_event(crate::ingestor::scheduler::SchedulerEvent::ManualRefreshTriggered(id));

            Ok(Json(serde_json::json!({
                "success": true,
                "message": "Stream source refresh started",
                "source_id": id
            })))
        }
        Ok(None) => Ok(Json(serde_json::json!({
            "success": false,
            "message": "Stream source not found"
        }))),
        Err(e) => {
            error!("Failed to get stream source {}: {}", id, e);
            Ok(Json(serde_json::json!({
                "success": false,
                "message": "Failed to get stream source"
            })))
        }
    }
}

/// Cancel stream source ingestion
/// Cancel stream source ingestion
#[utoipa::path(
    post,
    path = "/sources/stream/{id}/cancel",
    tag = "stream-sources",
    summary = "Cancel stream source ingestion",
    description = "Cancel an ongoing ingestion operation for a stream source",
    params(
        ("id" = Uuid, Path, description = "Stream source ID")
    ),
    responses(
        (status = 200, description = "Ingestion cancelled successfully"),
        (status = 404, description = "Stream source not found"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn cancel_stream_source_ingestion(
    Path(id): Path<Uuid>,
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    state.state_manager.cancel_ingestion(id).await;
    Ok(Json(json!({"message": "Ingestion cancelled"})))
}

/// Get stream source progress
/// Get stream source progress
#[utoipa::path(
    get,
    path = "/sources/stream/{id}/progress",
    tag = "progress",
    summary = "Get stream source progress",
    description = "Get ingestion progress for a specific stream source",
    params(
        ("id" = Uuid, Path, description = "Stream source ID")
    ),
    responses(
        (status = 200, description = "Stream source progress retrieved"),
        (status = 404, description = "Stream source not found"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_stream_source_progress(
    Path(id): Path<Uuid>,
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let progress = state.state_manager.get_progress(id).await;
    let processing_info = state.state_manager.get_processing_info(id).await;

    let result = serde_json::json!({
        "success": true,
        "message": "Stream source progress retrieved",
        "source_id": id,
        "progress": progress,
        "processing_info": processing_info
    });

    Ok(Json(result))
}

/// Get stream source processing info
/// Get stream source processing info
#[utoipa::path(
    get,
    path = "/sources/stream/{id}/processing-info",
    tag = "stream-sources",
    summary = "Get stream source processing info",
    description = "Get detailed processing information for a stream source",
    params(
        ("id" = Uuid, Path, description = "Stream source ID")
    ),
    responses(
        (status = 200, description = "Processing info retrieved"),
        (status = 404, description = "Stream source not found"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_stream_source_processing_info(
    Path(id): Path<Uuid>,
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let info = state.state_manager.get_progress(id).await;
    Ok(Json(json!(info)))
}

/// Get stream source channels
#[utoipa::path(
    get,
    path = "/sources/stream/{id}/channels",
    tag = "sources",
    summary = "Get stream source channels",
    description = "Retrieve paginated list of channels for a stream source",
    params(
        ("id" = String, Path, description = "Stream source ID (UUID)"),
        ("page" = Option<u32>, Query, description = "Page number (default: 1)"),
        ("limit" = Option<u32>, Query, description = "Items per page (default: 50)"),
        ("filter" = Option<String>, Query, description = "Filter channels by name")
    ),
    responses(
        (status = 200, description = "Paginated list of channels"),
        (status = 404, description = "Stream source not found"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_stream_source_channels(
    Path(id): Path<Uuid>,
    Query(params): Query<ChannelQueryParams>,
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let page = params.page.unwrap_or(1);
    let limit = params.limit.unwrap_or(50);
    let filter = params.filter.as_deref();

    match state
        .database
        .get_source_channels_paginated(id, page, limit, filter)
        .await
    {
        Ok(result) => Ok(Json(json!(result))),
        Err(e) => {
            error!("Failed to get channels for stream source {}: {}", id, e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

/// List EPG sources only

/// Refresh EPG source
#[utoipa::path(
    post,
    path = "/sources/epg/{id}/refresh",
    tag = "sources",
    summary = "Refresh EPG source",
    description = "Manually trigger a refresh of an EPG source to reload program data",
    params(
        ("id" = String, Path, description = "EPG source ID (UUID)"),
    ),
    responses(
        (status = 200, description = "Refresh initiated successfully"),
        (status = 404, description = "EPG source not found"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn refresh_epg_source_unified(
    Path(id): Path<Uuid>,
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    match state.database.get_epg_source(id).await {
        Ok(Some(source)) => {
            // Use EPG source service to trigger refresh with new source handlers
            let epg_service = state.epg_source_service.clone();
            let progress_service = state.progress_service.clone(); // Use shared instance!
            tokio::spawn(async move {
                if let Err(e) = epg_service.refresh_with_progress(&source, &progress_service).await {
                    error!("Failed to refresh EPG source {}: {}", source.id, e);
                }
            });

            // Emit scheduler event for manual refresh trigger
            state.database.emit_scheduler_event(crate::ingestor::scheduler::SchedulerEvent::ManualRefreshTriggered(id));

            Ok(Json(serde_json::json!({
                "success": true,
                "message": "EPG source refresh started",
                "source_id": id
            })))
        }
        Ok(None) => Ok(Json(serde_json::json!({
            "success": false,
            "message": "EPG source not found"
        }))),
        Err(e) => {
            error!("Failed to get EPG source {}: {}", id, e);
            Ok(Json(serde_json::json!({
                "success": false,
                "message": "Failed to get EPG source"
            })))
        }
    }
}

/// Get EPG source channels
#[utoipa::path(
    get,
    path = "/sources/epg/{id}/channels",
    tag = "sources",
    summary = "Get EPG source channels",
    description = "Retrieve list of channels available in an EPG source",
    params(
        ("id" = String, Path, description = "EPG source ID (UUID)"),
        ("page" = Option<u32>, Query, description = "Page number (default: 1)"),
        ("limit" = Option<u32>, Query, description = "Items per page (default: 50)"),
        ("filter" = Option<String>, Query, description = "Filter channels by name")
    ),
    responses(
        (status = 200, description = "List of EPG channels"),
        (status = 404, description = "EPG source not found"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_epg_source_channels_unified(
    Path(id): Path<Uuid>,
    Query(params): Query<ChannelQueryParams>,
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let _page = params.page.unwrap_or(1);
    let _limit = params.limit.unwrap_or(50);
    let _filter = params.filter;

    // Since we've moved to a programs-only approach, return channel information derived from programs
    match sqlx::query(
        "SELECT DISTINCT channel_id, channel_name, COUNT(*) as program_count
         FROM epg_programs 
         WHERE source_id = ? 
         GROUP BY channel_id, channel_name 
         ORDER BY channel_name"
    )
    .bind(id.to_string())
    .fetch_all(&state.database.pool())
    .await {
        Ok(rows) => {
            let channel_summary: Vec<_> = rows.into_iter().map(|row| {
                serde_json::json!({
                    "channel_id": row.get::<String, _>("channel_id"),
                    "channel_name": row.get::<String, _>("channel_name"),
                    "program_count": row.get::<i64, _>("program_count"),
                    "source_id": id
                })
            }).collect();
            Ok(Json(json!(channel_summary)))
        },
        Err(e) => {
            error!("Failed to get channels for EPG source {}: {}", id, e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

// Unified Sources API
#[utoipa::path(
    get,
    path = "/sources/unified",
    tag = "sources",
    summary = "List all sources (unified)",
    description = "Retrieve a unified list of all stream and EPG sources with statistics",
    responses(
        (status = 200, description = "Unified list of all sources with statistics"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn list_all_sources(
    State(state): State<AppState>,
) -> Result<Json<Vec<UnifiedSourceWithStats>>, StatusCode> {
    let mut unified_sources = Vec::new();

    // Get stream sources
    match state.database.list_stream_sources_with_stats().await {
        Ok(stream_sources) => {
            for stream_source in stream_sources {
                unified_sources.push(UnifiedSourceWithStats::from_stream(stream_source));
            }
        }
        Err(e) => {
            error!("Failed to list stream sources: {}", e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    }

    // Get EPG sources
    match state.database.list_epg_sources_with_stats().await {
        Ok(epg_sources) => {
            for epg_source in epg_sources {
                unified_sources.push(UnifiedSourceWithStats::from_epg(epg_source));
            }
        }
        Err(e) => {
            error!("Failed to list EPG sources: {}", e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    }

    // Sort by name for consistent ordering
    unified_sources.sort_by(|a, b| a.get_name().cmp(b.get_name()));

    Ok(Json(unified_sources))
}

/// Search logo assets
#[utoipa::path(
    get,
    path = "/logos/search",
    tag = "logos",
    summary = "Search logo assets",
    description = "Search for logo assets using various criteria",
    params(
        ("q" = Option<String>, Query, description = "Search query"),
        ("include_cached" = Option<bool>, Query, description = "Include cached logos"),
    ),
    responses(
        (status = 200, description = "Search results"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn search_logo_assets(
    Query(params): Query<crate::models::logo_asset::LogoAssetSearchRequest>,
    State(state): State<AppState>,
) -> Result<Json<crate::models::logo_asset::LogoAssetSearchResult>, StatusCode> {
    let include_cached = params.include_cached.unwrap_or(true); // Default to include cached

    if include_cached && state.logo_cache_scanner.is_some() {
        // Use the enhanced search with cached logos
        match state
            .logo_asset_service
            .search_assets_with_cached(
                params,
                &state.config.web.base_url,
                state.logo_cache_scanner.as_ref(),
                include_cached,
            )
            .await
        {
            Ok(result) => Ok(Json(result)),
            Err(e) => {
                error!("Failed to search logo assets with cached: {}", e);
                Err(StatusCode::INTERNAL_SERVER_ERROR)
            }
        }
    } else {
        // Fall back to database-only search
        match state
            .logo_asset_service
            .search_assets(params, &state.config.web.base_url)
            .await
        {
            Ok(result) => Ok(Json(result)),
            Err(e) => {
                error!("Failed to search logo assets: {}", e);
                Err(StatusCode::INTERNAL_SERVER_ERROR)
            }
        }
    }
}

#[utoipa::path(
    get,
    path = "/logos/{id}/info",
    tag = "logos",
    summary = "Get logo asset with formats",
    description = "Retrieve logo asset details including available formats",
    params(
        ("id" = String, Path, description = "Logo asset ID (UUID)")
    ),
    responses(
        (status = 200, description = "Logo asset with format information"),
        (status = 404, description = "Logo asset not found"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_logo_asset_with_formats(
    Path(id): Path<Uuid>,
    State(state): State<AppState>,
) -> Result<Json<crate::models::logo_asset::LogoAssetWithLinked>, StatusCode> {
    match state.logo_asset_service.get_asset(id).await {
        Ok(asset) => {
            // Get linked assets (PNG conversions, etc.)
            let linked_assets = match state.logo_asset_service.get_linked_assets(id).await {
                Ok(linked) => linked,
                Err(e) => {
                    error!("Failed to get linked assets for {}: {}", id, e);
                    Vec::new()
                }
            };

            // Convert linked assets to LogoAssetWithUrl (using relative URLs for web UI)
            let linked_with_urls: Vec<crate::models::logo_asset::LogoAssetWithUrl> = linked_assets
                .into_iter()
                .map(|linked| crate::models::logo_asset::LogoAssetWithUrl {
                    url: crate::utils::logo::LogoUrlGenerator::relative(linked.id),
                    asset: linked,
                })
                .collect();

            // Build available formats list
            let mut available_formats = vec![
                asset
                    .mime_type
                    .split('/')
                    .last()
                    .unwrap_or("unknown")
                    .to_string(),
            ];
            for linked in &linked_with_urls {
                if let Some(format) = linked.asset.mime_type.split('/').last() {
                    if !available_formats.contains(&format.to_string()) {
                        available_formats.push(format.to_string());
                    }
                }
            }

            let response = crate::models::logo_asset::LogoAssetWithLinked {
                asset,
                url: crate::utils::logo::LogoUrlGenerator::relative(id),
                linked_assets: linked_with_urls,
                available_formats,
            };

            Ok(Json(response))
        }
        Err(_) => Err(StatusCode::NOT_FOUND),
    }
}

#[utoipa::path(
    get,
    path = "/logos/stats",
    tag = "logos",
    summary = "Get logo cache statistics",
    description = "Retrieve statistics about logo cache usage and performance",
    responses(
        (status = 200, description = "Logo cache statistics"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_logo_cache_stats(
    State(state): State<AppState>,
) -> Result<Json<crate::models::logo_asset::LogoCacheStats>, StatusCode> {
    match state
        .logo_asset_service
        .get_cache_stats_with_filesystem(state.logo_cache_scanner.as_ref())
        .await
    {
        Ok(stats) => Ok(Json(stats)),
        Err(e) => {
            error!("Failed to get logo cache stats: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

/// Generate missing metadata for cached logos
#[utoipa::path(
    post,
    path = "/logos/generate-metadata",
    tag = "logos",
    summary = "Generate cached logo metadata",
    description = "Scan and generate missing metadata for cached logo files",
    responses(
        (status = 200, description = "Metadata generation completed"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn generate_cached_logo_metadata(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    if let Some(scanner) = &state.logo_cache_scanner {
        match state
            .logo_asset_service
            .ensure_cached_logo_metadata(scanner)
            .await
        {
            Ok(generated_count) => Ok(Json(serde_json::json!({
                "success": true,
                "message": "Metadata generation completed",
                "generated_count": generated_count
            }))),
            Err(e) => {
                error!("Failed to generate cached logo metadata: {}", e);
                Err(StatusCode::INTERNAL_SERVER_ERROR)
            }
        }
    } else {
        error!("Logo cache scanner not available");
        Err(StatusCode::SERVICE_UNAVAILABLE)
    }
}

// EPG Sources API

#[utoipa::path(
    get,
    path = "/epg/viewer",
    tag = "epg",
    summary = "Get EPG viewer data",
    description = "Retrieve EPG program data for the viewer interface within specified time range",
    params(
        ("start_time" = String, Query, description = "Start time in RFC3339 format"),
        ("end_time" = String, Query, description = "End time in RFC3339 format"),
        ("source_id" = Option<String>, Query, description = "EPG source ID to filter by")
    ),
    responses(
        (status = 200, description = "EPG viewer data"),
        (status = 400, description = "Invalid time format"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_epg_viewer_data(
    Query(request): Query<EpgViewerRequest>,
    State(state): State<AppState>,
) -> Result<Json<EpgViewerResponse>, StatusCode> {
    // Parse datetime strings
    let start_time = match DateTime::parse_from_rfc3339(&request.start_time) {
        Ok(dt) => dt.with_timezone(&Utc),
        Err(_) => {
            error!("Invalid start_time format: {}", request.start_time);
            return Err(StatusCode::BAD_REQUEST);
        }
    };

    let end_time = match DateTime::parse_from_rfc3339(&request.end_time) {
        Ok(dt) => dt.with_timezone(&Utc),
        Err(_) => {
            error!("Invalid end_time format: {}", request.end_time);
            return Err(StatusCode::BAD_REQUEST);
        }
    };

    let parsed_request = EpgViewerRequestParsed {
        start_time,
        end_time,
        channel_filter: request.channel_filter,
        source_ids: request.source_ids,
    };

    match state
        .database
        .get_epg_data_for_viewer(&parsed_request)
        .await
    {
        Ok(response) => Ok(Json(response)),
        Err(e) => {
            error!("Failed to get EPG viewer data: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

// Linked Xtream Sources API
/// List linked Xtream sources
#[utoipa::path(
    get,
    path = "/sources/linked-xtream",
    tag = "sources",
    summary = "List linked Xtream sources",
    description = "Get all linked Xtream Codes sources",
    responses(
        (status = 200, description = "Linked Xtream sources retrieved"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn list_linked_xtream_sources(
    State(state): State<AppState>,
) -> Result<Json<Vec<LinkedXtreamSources>>, StatusCode> {
    match state.database.list_linked_xtream_sources().await {
        Ok(sources) => Ok(Json(sources)),
        Err(e) => {
            error!("Failed to list linked Xtream sources: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

/// Create linked Xtream source
#[utoipa::path(
    post,
    path = "/sources/linked-xtream",
    tag = "sources",
    summary = "Create linked Xtream source",
    description = "Create a new linked Xtream Codes source",
    request_body = XtreamCodesCreateRequest,
    responses(
        (status = 201, description = "Linked Xtream source created"),
        (status = 400, description = "Invalid request data"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn create_linked_xtream_source(
    State(state): State<AppState>,
    Json(payload): Json<XtreamCodesCreateRequest>,
) -> Result<Json<XtreamCodesCreateResponse>, StatusCode> {
    match state.database.create_linked_xtream_sources(&payload).await {
        Ok(response) => {
            if response.success {
                // Invalidate scheduler cache since we may have added new sources
                let _ = state.cache_invalidation_tx.send(());
            }
            Ok(Json(response))
        }
        Err(e) => {
            error!("Failed to create linked Xtream source: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

/// Get linked Xtream source by ID
#[utoipa::path(
    get,
    path = "/sources/linked-xtream/{link_id}",
    tag = "sources",
    summary = "Get linked Xtream source",
    description = "Get a specific linked Xtream Codes source by ID",
    params(
        ("link_id" = String, Path, description = "Linked Xtream source ID")
    ),
    responses(
        (status = 200, description = "Linked Xtream source retrieved"),
        (status = 404, description = "Linked Xtream source not found"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_linked_xtream_source(
    Path(link_id): Path<String>,
    State(state): State<AppState>,
) -> Result<Json<LinkedXtreamSources>, StatusCode> {
    match state.database.get_linked_xtream_source(&link_id).await {
        Ok(Some(sources)) => Ok(Json(sources)),
        Ok(None) => Err(StatusCode::NOT_FOUND),
        Err(e) => {
            error!("Failed to get linked Xtream source {}: {}", link_id, e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

/// Update linked Xtream source
#[utoipa::path(
    put,
    path = "/sources/linked-xtream/{link_id}",
    tag = "sources",
    summary = "Update linked Xtream source",
    description = "Update an existing linked Xtream Codes source",
    params(
        ("link_id" = String, Path, description = "Linked Xtream source ID")
    ),
    request_body = XtreamCodesUpdateRequest,
    responses(
        (status = 200, description = "Linked Xtream source updated"),
        (status = 404, description = "Linked Xtream source not found"),
        (status = 400, description = "Invalid request data"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn update_linked_xtream_source(
    Path(link_id): Path<String>,
    State(state): State<AppState>,
    Json(payload): Json<XtreamCodesUpdateRequest>,
) -> Result<StatusCode, StatusCode> {
    match state
        .database
        .update_linked_xtream_sources(&link_id, &payload)
        .await
    {
        Ok(true) => {
            // Invalidate scheduler cache since sources were updated
            let _ = state.cache_invalidation_tx.send(());
            Ok(StatusCode::NO_CONTENT)
        }
        Ok(false) => Err(StatusCode::NOT_FOUND),
        Err(e) => {
            error!("Failed to update linked Xtream source {}: {}", link_id, e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

/// Delete linked Xtream source
#[utoipa::path(
    delete,
    path = "/sources/linked-xtream/{link_id}",
    tag = "sources",
    summary = "Delete linked Xtream source",
    description = "Delete a linked Xtream Codes source",
    params(
        ("link_id" = String, Path, description = "Linked Xtream source ID")
    ),
    responses(
        (status = 200, description = "Linked Xtream source deleted"),
        (status = 404, description = "Linked Xtream source not found"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn delete_linked_xtream_source(
    Path(link_id): Path<String>,
    Query(params): Query<std::collections::HashMap<String, String>>,
    State(state): State<AppState>,
) -> Result<StatusCode, StatusCode> {
    let _delete_sources = params
        .get("delete_sources")
        .map(|s| s == "true")
        .unwrap_or(false);

    match state.database.delete_linked_xtream_sources(&link_id).await {
        Ok(true) => {
            // Invalidate scheduler cache since sources may have been deleted
            let _ = state.cache_invalidation_tx.send(());
            Ok(StatusCode::NO_CONTENT)
        }
        Ok(false) => Err(StatusCode::NOT_FOUND),
        Err(e) => {
            error!("Failed to delete linked Xtream source {}: {}", link_id, e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}


/// Count conditions in a condition tree
fn count_conditions_in_tree(tree: &crate::models::ConditionTree) -> usize {
    count_conditions_in_node(&tree.root)
}

/// Count conditions in a condition node recursively
fn count_conditions_in_node(node: &crate::models::ConditionNode) -> usize {
    match node {
        crate::models::ConditionNode::Condition { .. } => 1,
        crate::models::ConditionNode::Group { children, .. } => {
            children.iter().map(count_conditions_in_node).sum()
        }
    }
}

/// Generate JSON representation of condition tree for frontend rendering (shared by data mapping and filters)
pub fn generate_expression_tree_json(tree: &crate::models::ConditionTree) -> serde_json::Value {
    generate_condition_node_json(&tree.root)
}

/// Generate JSON representation of a condition node
fn generate_condition_node_json(node: &crate::models::ConditionNode) -> serde_json::Value {
    match node {
        crate::models::ConditionNode::Condition {
            field,
            operator,
            value,
            negate,
            case_sensitive,
        } => {
            json!({
                "type": "condition",
                "field": field,
                "operator": format!("{:?}", operator),
                "value": value,
                "negate": negate,
                "case_sensitive": case_sensitive
            })
        }
        crate::models::ConditionNode::Group { operator, children } => {
            let operator_str = match operator {
                crate::models::LogicalOperator::And => "AND",
                crate::models::LogicalOperator::Or => "OR",
            };

            json!({
                "type": "group",
                "operator": operator_str,
                "children": children.iter().map(|child| generate_condition_node_json(child)).collect::<Vec<_>>()
            })
        }
    }
}

/// Get progress for all sources (used by frontend polling)
#[utoipa::path(
    get,
    path = "/progress/sources",
    tag = "progress",
    summary = "Get sources progress",
    description = "Retrieve processing progress for all active stream sources (used by frontend polling)",
    responses(
        (status = 200, description = "Progress information for all sources"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_sources_progress(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    // Get progress for all active stream sources
    let stream_sources = match state.database.list_stream_sources().await {
        Ok(sources) => sources
            .into_iter()
            .filter(|s| s.is_active)
            .collect::<Vec<_>>(),
        Err(e) => {
            error!("Failed to list stream sources for progress: {}", e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    let mut sources_progress = Vec::new();
    for source in stream_sources {
        let progress = state.state_manager.get_progress(source.id).await;
        let processing_info = state.state_manager.get_processing_info(source.id).await;

        sources_progress.push(json!({
            "source_id": source.id,
            "source_name": source.name,
            "source_type": "stream",
            "progress": progress,
            "processing_info": processing_info
        }));
    }

    Ok(Json(json!({
        "success": true,
        "sources": sources_progress
    })))
}

/// Get progress for all EPG sources (used by frontend polling)
#[utoipa::path(
    get,
    path = "/progress/epg",
    tag = "progress",
    summary = "Get EPG progress",
    description = "Retrieve processing progress for all active EPG sources (used by frontend polling)",
    responses(
        (status = 200, description = "Progress information for all EPG sources"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_epg_progress(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    // Get progress for all active EPG sources
    let epg_sources = match state.database.list_epg_sources().await {
        Ok(sources) => sources
            .into_iter()
            .filter(|s| s.is_active)
            .collect::<Vec<_>>(),
        Err(e) => {
            error!("Failed to list EPG sources for progress: {}", e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    let mut sources_progress = Vec::new();
    for source in epg_sources {
        let progress = state.state_manager.get_progress(source.id).await;
        let processing_info = state.state_manager.get_processing_info(source.id).await;

        sources_progress.push(json!({
            "source_id": source.id,
            "source_name": source.name,
            "source_type": "epg",
            "progress": progress,
            "processing_info": processing_info
        }));
    }

    Ok(Json(json!({
        "success": true,
        "sources": sources_progress
    })))
}


/// Preview proxies (placeholder implementation)
/// Preview proxy configurations
#[utoipa::path(
    get,
    path = "/proxies/preview",
    tag = "proxies",
    summary = "Preview proxy configurations",
    description = "Get a preview of all proxy configurations",
    responses(
        (status = 200, description = "Proxy preview data retrieved"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn preview_proxies(
    _state: State<AppState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    Ok(Json(json!({
        "success": true,
        "proxies": [],
        "message": "Proxy preview not yet implemented"
    })))
}

/// Regenerate all active proxies
#[utoipa::path(
    post,
    path = "/proxies/regenerate-all",
    tag = "proxies",
    summary = "Regenerate all active proxies",
    description = "Queue all active proxies for background regeneration.

This endpoint:
- Queues all proxies with `is_active = true` for regeneration
- Uses the proxy regeneration service to avoid conflicts
- Prevents duplicate batch regeneration requests
- Returns immediately with queuing status (regeneration happens in background)
- Updates proxy files with fresh data from all sources

Note: This operation may take several minutes depending on the number of proxies and source response times.",
    responses(
        (status = 200, description = "Batch regeneration queued successfully"),
        (status = 409, description = "Another batch regeneration is already in progress"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn regenerate_all_proxies(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    info!("Queuing all active proxies for background regeneration");
    
    // CRITICAL FIX: Use a special UUID for batch operations to prevent concurrent batch requests
    let batch_operation_id = Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap();
    
    // Check for duplicate batch requests
    {
        let mut active_requests = state.active_regeneration_requests.lock().await;
        if active_requests.contains(&batch_operation_id) {
            tracing::warn!("Duplicate batch regeneration API request - rejecting");
            return Ok(Json(serde_json::json!({
                "success": false,
                "message": "A batch regeneration request is already being processed",
                "status": "duplicate_request"
            })));
        }
        // Reserve the batch operation ID to prevent other requests
        active_requests.insert(batch_operation_id);
    }
    
    // Create a cleanup guard for the batch operation
    let _cleanup_guard = RequestCleanupGuard {
        proxy_id: batch_operation_id,
        active_requests: state.active_regeneration_requests.clone(),
    };
    
    // Get all active proxies count for response
    let active_proxies = match state.database.get_all_active_proxies().await {
        Ok(proxies) => proxies,
        Err(e) => {
            error!("Failed to get active proxies: {}", e);
            // _cleanup_guard will automatically clean up when function returns
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };
    
    if active_proxies.is_empty() {
        // cleanup_guard will automatically clean up when function returns
        return Ok(Json(json!({
            "success": true,
            "message": "No active proxies found to regenerate",
            "count": 0
        })));
    }
    
    // Queue all proxies for regeneration using the service
    match state.proxy_regeneration_service.queue_manual_regeneration_all().await {
        Ok(_) => {
            info!("All {} active proxies queued for background regeneration", active_proxies.len());
            
            // Cleanup guard will automatically clean up when function returns
            // The service-level deduplication will handle actual regeneration conflicts
            
            Ok(Json(serde_json::json!({
                "success": true,
                "message": format!("All {} active proxies queued for regeneration", active_proxies.len()),
                "queue_id": "universal-batch-regeneration", // Use consistent format for batch operations
                "count": active_proxies.len(),
                "total": active_proxies.len(),
                "status": "queued",
                "queued_at": chrono::Utc::now()
            })))
        }
        Err(e) => {
            error!("Failed to queue all proxies for regeneration: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

// Relay Configuration API Endpoints - implemented in web/api/relay.rs

// Metrics API structures
#[derive(Debug, Serialize)]
pub struct DashboardMetrics {
    pub total_channels: u64,
    pub active_streams: u64,
    pub active_clients: u64,
    pub total_proxies: u64,
    pub system_uptime: String,
    pub bytes_served_today: u64,
    pub popular_channels: Vec<PopularChannel>,
}

#[derive(Debug, Serialize)]
pub struct PopularChannel {
    pub channel_id: String,
    pub channel_name: String,
    pub session_count: u64,
    pub bytes_served: u64,
}

#[derive(Debug, Serialize)]
pub struct RealtimeMetrics {
    pub active_sessions: u64,
    pub active_clients: u64,
    pub bytes_per_second: u64,
    pub proxy_breakdown: Vec<ProxyMetrics>,
}

#[derive(Debug, Serialize)]
pub struct ProxyMetrics {
    pub proxy_id: String,
    pub proxy_name: String,
    pub active_sessions: u64,
    pub active_clients: u64,
    pub bytes_per_second: u64,
}

#[derive(Debug, Serialize)]
pub struct UsageMetrics {
    pub hourly_sessions: Vec<HourlyUsage>,
    pub daily_sessions: Vec<DailyUsage>,
}

#[derive(Debug, Serialize)]
pub struct HourlyUsage {
    pub hour: String,
    pub total_sessions: u64,
    pub unique_clients: u64,
    pub bytes_served: u64,
}

#[derive(Debug, Serialize)]
pub struct DailyUsage {
    pub date: String,
    pub total_sessions: u64,
    pub unique_clients: u64,
    pub bytes_served: u64,
}

#[derive(Debug, Deserialize, IntoParams)]
pub struct UsageQuery {
    pub proxy_id: Option<String>,
    pub days: Option<u32>,
    pub hours: Option<u32>,
}

#[utoipa::path(
    get,
    path = "/dashboard/metrics",
    tag = "metrics",
    summary = "Get dashboard metrics",
    description = "Retrieve overview metrics for the dashboard",
    responses(
        (status = 200, description = "Dashboard metrics overview"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_dashboard_metrics(
    State(state): State<AppState>,
) -> Result<Json<DashboardMetrics>, StatusCode> {
    let db = &state.database.pool();

    // Get total channels across all proxies
    let total_channels = match sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM channels")
        .fetch_one(db)
        .await
    {
        Ok(count) => count as u64,
        Err(e) => {
            error!("Failed to get total channels: {}", e);
            0
        }
    };

    // Get total proxies
    let total_proxies = match sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM stream_proxies")
        .fetch_one(db)
        .await
    {
        Ok(count) => count as u64,
        Err(e) => {
            error!("Failed to get total proxies: {}", e);
            0
        }
    };

    Ok(Json(DashboardMetrics {
        total_channels,
        active_streams: 0,
        active_clients: 0,
        total_proxies,
        system_uptime: "99.8%".to_string(),
        bytes_served_today: 0,
        popular_channels: Vec::new(),
    }))
}

#[utoipa::path(
    get,
    path = "/metrics/realtime",
    tag = "metrics",
    summary = "Get real-time metrics",
    description = "Retrieve real-time performance metrics",
    responses(
        (status = 200, description = "Real-time metrics data"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_realtime_metrics(
    State(_state): State<AppState>,
) -> Result<Json<RealtimeMetrics>, StatusCode> {
    Ok(Json(RealtimeMetrics {
        active_sessions: 0,
        active_clients: 0,
        bytes_per_second: 0,
        proxy_breakdown: Vec::new(),
    }))
}

#[utoipa::path(
    get,
    path = "/metrics/usage",
    tag = "metrics",
    summary = "Get usage metrics",
    description = "Retrieve usage metrics over time",
    params(
        ("days" = Option<u32>, Query, description = "Number of days to include"),
        ("hours" = Option<u32>, Query, description = "Number of hours to include"),
    ),
    responses(
        (status = 200, description = "Usage metrics data"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_usage_metrics(
    Query(_params): Query<UsageQuery>,
    State(_state): State<AppState>,
) -> Result<Json<UsageMetrics>, StatusCode> {
    Ok(Json(UsageMetrics {
        hourly_sessions: Vec::new(),
        daily_sessions: Vec::new(),
    }))
}

/// Get popular channels
/// Get popular channels
#[utoipa::path(
    get,
    path = "/metrics/channels/popular",
    tag = "metrics",
    summary = "Get popular channels",
    description = "Get most popular channels based on usage metrics",
    params(
        UsageQuery
    ),
    responses(
        (status = 200, description = "Popular channels retrieved"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_popular_channels(
    Query(_params): Query<UsageQuery>,
    State(_state): State<AppState>,
) -> Result<Json<Vec<PopularChannel>>, StatusCode> {
    Ok(Json(Vec::new()))
}

/// CONCURRENCY FIX: Cleanup guard to ensure API request deduplication is properly cleaned up
struct RequestCleanupGuard {
    proxy_id: Uuid,
    active_requests: Arc<Mutex<HashSet<Uuid>>>,
}

impl Drop for RequestCleanupGuard {
    fn drop(&mut self) {
        let proxy_id = self.proxy_id;
        let active_requests = self.active_requests.clone();
        
        // Spawn a cleanup task since Drop can't be async
        tokio::spawn(async move {
            let mut requests = active_requests.lock().await;
            requests.remove(&proxy_id);
            tracing::debug!("Cleaned up API regeneration request tracking for proxy {}", proxy_id);
        });
    }
}

