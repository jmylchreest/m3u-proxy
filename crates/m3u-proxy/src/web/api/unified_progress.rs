//! Unified Progress API Endpoints
//!
//! This module provides modern, unified progress tracking endpoints that work
//! across all operation types using the UniversalProgress system.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::sse::{Event, Sse},
    Json,
};
use futures::Stream;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;
use utoipa::{IntoParams, ToSchema};

use crate::web::AppState;
use crate::services::progress_service::{OperationType, UniversalState};

/// Query parameters for filtering progress operations
#[derive(Debug, Deserialize, IntoParams)]
pub struct ProgressQuery {
    /// Filter by operation type (e.g., "stream_ingestion", "epg_ingestion", "proxy_regeneration")
    pub operation_type: Option<String>,
    /// Filter by specific resource ID (source_id, proxy_id, etc.)
    pub resource_id: Option<Uuid>,
    /// Filter by state (e.g., "processing", "completed", "error")  
    pub state: Option<String>,
    /// Include completed operations (default: true for specific resource, false for all)
    pub include_completed: Option<bool>,
    /// Include failed operations (default: true)
    pub include_failed: Option<bool>,
    /// Maximum number of operations to return
    pub limit: Option<usize>,
    /// Only return active operations (default: false)
    pub active_only: Option<bool>,
}

/// Unified progress operation response
#[derive(Debug, Serialize, ToSchema)]
pub struct ProgressOperationResponse {
    pub id: Uuid,
    pub operation_type: String,
    pub operation_name: String,
    pub state: String,
    pub current_step: String,
    pub progress: ProgressDetails,
    pub timing: TimingDetails,
    pub metadata: HashMap<String, serde_json::Value>,
    pub error: Option<String>,
}

/// Progress details
#[derive(Debug, Serialize, ToSchema)]
pub struct ProgressDetails {
    pub percentage: Option<f64>,
    pub items: Option<ItemProgress>,
    pub bytes: Option<ByteProgress>,
}

/// Item progress (channels, programs, etc.)
#[derive(Debug, Serialize, ToSchema)]
pub struct ItemProgress {
    pub processed: Option<usize>,
    pub total: Option<usize>,
}

/// Byte progress (downloads, etc.)
#[derive(Debug, Serialize, ToSchema)]
pub struct ByteProgress {
    pub processed: Option<u64>,
    pub total: Option<u64>,
}

/// Timing information
#[derive(Debug, Serialize, ToSchema)]
pub struct TimingDetails {
    pub started_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub completed_at: Option<chrono::DateTime<chrono::Utc>>,
    pub duration_ms: Option<i64>,
}

/// Summary of all operations
#[derive(Debug, Serialize, ToSchema)]
pub struct ProgressSummary {
    pub total_active: usize,
    pub total_completed: usize,
    pub total_failed: usize,
    pub by_type: HashMap<String, usize>,
    pub by_state: HashMap<String, usize>,
}

/// Full progress response
#[derive(Debug, Serialize, ToSchema)]
pub struct UnifiedProgressResponse {
    pub operations: Vec<ProgressOperationResponse>,
    pub summary: ProgressSummary,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

/// Get all active operations
#[utoipa::path(
    get,
    path = "/progress",
    params(ProgressQuery),
    responses(
        (status = 200, description = "All progress operations retrieved", body = UnifiedProgressResponse),
        (status = 500, description = "Internal server error")
    ),
    tag = "progress"
)]
pub async fn get_unified_progress(
    Query(query): Query<ProgressQuery>,
    State(state): State<AppState>,
) -> Result<Json<UnifiedProgressResponse>, StatusCode> {
    // Get all progress from UniversalProgress system
    let all_universal_progress = state.progress_service.get_all_progress().await;
    
    let mut operations = Vec::new();
    let mut by_type = HashMap::new();
    let mut by_state = HashMap::new();
    let mut total_active = 0;
    let mut total_completed = 0;
    let mut total_failed = 0;

    // Process each operation
    for (operation_id, progress) in all_universal_progress {
        // Apply filters
        if let Some(ref type_filter) = query.operation_type {
            let op_type_str = operation_type_to_string(&progress.operation_type);
            if op_type_str != type_filter.to_lowercase() {
                continue;
            }
        }

        if let Some(ref state_filter) = query.state {
            let state_str = universal_state_to_string(&progress.state);
            if state_str != state_filter.to_lowercase() {
                continue;
            }
        }

        // Apply resource_id filter by checking operation_id (which is the resource ID for ingestion operations)
        if let Some(resource_id) = query.resource_id {
            if operation_id != resource_id {
                continue;
            }
        }

        // Apply active_only filter
        if query.active_only.unwrap_or(false) {
            match progress.state {
                UniversalState::Completed | UniversalState::Error | UniversalState::Cancelled => {
                    continue;
                }
                _ => {}
            }
        }

        // Apply completion/failure filters
        match progress.state {
            UniversalState::Completed => {
                total_completed += 1;
                if !query.include_completed.unwrap_or(false) {
                    continue;
                }
            }
            UniversalState::Error => {
                total_failed += 1;
                if !query.include_failed.unwrap_or(true) {
                    continue;
                }
            }
            _ => {
                total_active += 1;
            }
        }

        // Count by type and state
        let type_str = operation_type_to_string(&progress.operation_type);
        let state_str = universal_state_to_string(&progress.state);
        
        *by_type.entry(type_str.clone()).or_insert(0) += 1;
        *by_state.entry(state_str.clone()).or_insert(0) += 1;

        // Calculate duration
        let duration_ms = if let Some(completed_at) = progress.completed_at {
            Some((completed_at - progress.started_at).num_milliseconds())
        } else {
            Some((chrono::Utc::now() - progress.started_at).num_milliseconds())
        };

        // Build operation response
        let operation_response = ProgressOperationResponse {
            id: operation_id,
            operation_type: type_str,
            operation_name: progress.operation_name,
            state: state_str,
            current_step: progress.current_step,
            progress: ProgressDetails {
                percentage: progress.progress_percentage,
                items: if progress.items_processed.is_some() || progress.items_total.is_some() {
                    Some(ItemProgress {
                        processed: progress.items_processed,
                        total: progress.items_total,
                    })
                } else {
                    None
                },
                bytes: if progress.bytes_processed.is_some() || progress.bytes_total.is_some() {
                    Some(ByteProgress {
                        processed: progress.bytes_processed,
                        total: progress.bytes_total,
                    })
                } else {
                    None
                },
            },
            timing: TimingDetails {
                started_at: progress.started_at,
                updated_at: progress.updated_at,
                completed_at: progress.completed_at,
                duration_ms,
            },
            metadata: progress.metadata,
            error: progress.error_message,
        };

        operations.push(operation_response);
    }

    // Apply limit
    if let Some(limit) = query.limit {
        operations.truncate(limit);
    }

    // Sort by most recent first
    operations.sort_by(|a, b| b.timing.updated_at.cmp(&a.timing.updated_at));

    let response = UnifiedProgressResponse {
        operations,
        summary: ProgressSummary {
            total_active,
            total_completed,
            total_failed,
            by_type,
            by_state,
        },
        timestamp: chrono::Utc::now(),
    };

    Ok(Json(response))
}

/// Get specific operation progress
#[utoipa::path(
    get,
    path = "/progress/operations/{operation_id}",
    responses(
        (status = 200, description = "Operation progress retrieved", body = ProgressOperationResponse),
        (status = 404, description = "Operation not found"),
        (status = 500, description = "Internal server error")
    ),
    tag = "progress"
)]
pub async fn get_operation_progress(
    Path(operation_id): Path<Uuid>,
    State(state): State<AppState>,
) -> Result<Json<ProgressOperationResponse>, StatusCode> {
    match state.progress_service.get_progress(operation_id).await {
        Some(progress) => {
            let duration_ms = if let Some(completed_at) = progress.completed_at {
                Some((completed_at - progress.started_at).num_milliseconds())
            } else {
                Some((chrono::Utc::now() - progress.started_at).num_milliseconds())
            };

            let operation_response = ProgressOperationResponse {
                id: operation_id,
                operation_type: operation_type_to_string(&progress.operation_type),
                operation_name: progress.operation_name,
                state: universal_state_to_string(&progress.state),
                current_step: progress.current_step,
                progress: ProgressDetails {
                    percentage: progress.progress_percentage,
                    items: if progress.items_processed.is_some() || progress.items_total.is_some() {
                        Some(ItemProgress {
                            processed: progress.items_processed,
                            total: progress.items_total,
                        })
                    } else {
                        None
                    },
                    bytes: if progress.bytes_processed.is_some() || progress.bytes_total.is_some() {
                        Some(ByteProgress {
                            processed: progress.bytes_processed,
                            total: progress.bytes_total,
                        })
                    } else {
                        None
                    },
                },
                timing: TimingDetails {
                    started_at: progress.started_at,
                    updated_at: progress.updated_at,
                    completed_at: progress.completed_at,
                    duration_ms,
                },
                metadata: progress.metadata,
                error: progress.error_message,
            };

            Ok(Json(operation_response))
        }
        None => Err(StatusCode::NOT_FOUND),
    }
}

/// Helper function to convert OperationType to string
fn operation_type_to_string(op_type: &OperationType) -> String {
    match op_type {
        OperationType::StreamIngestion => "stream_ingestion".to_string(),
        OperationType::EpgIngestion => "epg_ingestion".to_string(),
        OperationType::ProxyRegeneration => "proxy_regeneration".to_string(),
        OperationType::Pipeline => "pipeline".to_string(),
        OperationType::DataMapping => "data_mapping".to_string(),
        OperationType::LogoCaching => "logo_caching".to_string(),
        OperationType::Filtering => "filtering".to_string(),
        OperationType::Maintenance => "maintenance".to_string(),
        OperationType::Database => "database".to_string(),
        OperationType::Custom(name) => format!("custom_{}", name.to_lowercase()),
    }
}

/// Helper function to convert UniversalState to string
fn universal_state_to_string(state: &UniversalState) -> String {
    match state {
        UniversalState::Idle => "idle".to_string(),
        UniversalState::Preparing => "preparing".to_string(),
        UniversalState::Connecting => "connecting".to_string(),
        UniversalState::Downloading => "downloading".to_string(),
        UniversalState::Processing => "processing".to_string(),
        UniversalState::Saving => "saving".to_string(),
        UniversalState::Cleanup => "cleanup".to_string(),
        UniversalState::Completed => "completed".to_string(),
        UniversalState::Error => "error".to_string(),
        UniversalState::Cancelled => "cancelled".to_string(),
    }
}

// ============================================================================
// Resource-Specific Endpoints (using unified format)
// ============================================================================

/// Get progress for all stream sources  
#[utoipa::path(
    get,
    path = "/progress/streams",
    params(ProgressQuery),
    responses(
        (status = 200, description = "Stream source progress retrieved", body = UnifiedProgressResponse),
        (status = 500, description = "Internal server error")
    ),
    tag = "progress"
)]
pub async fn get_stream_progress(
    Query(mut query): Query<ProgressQuery>,
    State(state): State<AppState>,
) -> Result<Json<UnifiedProgressResponse>, StatusCode> {
    // Force filter to stream ingestion operations
    query.operation_type = Some("stream_ingestion".to_string());
    get_unified_progress(Query(query), State(state)).await
}

/// Get progress for all EPG sources
#[utoipa::path(
    get,
    path = "/progress/epg",
    params(ProgressQuery),
    responses(
        (status = 200, description = "EPG source progress retrieved", body = UnifiedProgressResponse),
        (status = 500, description = "Internal server error")
    ),
    tag = "progress"
)]
pub async fn get_epg_progress(
    Query(mut query): Query<ProgressQuery>,
    State(state): State<AppState>,
) -> Result<Json<UnifiedProgressResponse>, StatusCode> {
    // Force filter to EPG ingestion operations
    query.operation_type = Some("epg_ingestion".to_string());
    get_unified_progress(Query(query), State(state)).await
}

/// Get progress for all proxy regeneration operations
#[utoipa::path(
    get,
    path = "/progress/proxies", 
    params(ProgressQuery),
    responses(
        (status = 200, description = "Proxy regeneration progress retrieved", body = UnifiedProgressResponse),
        (status = 500, description = "Internal server error")
    ),
    tag = "progress"
)]
pub async fn get_proxy_progress(
    Query(mut query): Query<ProgressQuery>,
    State(state): State<AppState>,
) -> Result<Json<UnifiedProgressResponse>, StatusCode> {
    // Force filter to proxy regeneration operations
    query.operation_type = Some("proxy_regeneration".to_string());
    get_unified_progress(Query(query), State(state)).await
}

/// Get progress for a specific stream source
#[utoipa::path(
    get,
    path = "/progress/resources/streams/{source_id}",
    responses(
        (status = 200, description = "Stream source progress retrieved", body = UnifiedProgressResponse),
        (status = 404, description = "No progress found for stream source"),
        (status = 500, description = "Internal server error")
    ),
    tag = "progress"
)]
pub async fn get_stream_source_progress(
    Path(source_id): Path<Uuid>,
    State(state): State<AppState>,
) -> Result<Json<UnifiedProgressResponse>, StatusCode> {
    let query = ProgressQuery {
        operation_type: Some("stream_ingestion".to_string()),
        resource_id: Some(source_id),
        include_completed: Some(true), // Include recent completed operations
        include_failed: Some(true),
        limit: Some(10), // Recent operations for this source
        state: None,
        active_only: None,
    };
    get_unified_progress(Query(query), State(state)).await
}

/// Get progress for a specific EPG source
#[utoipa::path(
    get,
    path = "/progress/resources/epg/{source_id}",
    responses(
        (status = 200, description = "EPG source progress retrieved", body = UnifiedProgressResponse),
        (status = 404, description = "No progress found for EPG source"),
        (status = 500, description = "Internal server error")
    ),
    tag = "progress"
)]
pub async fn get_epg_source_progress(
    Path(source_id): Path<Uuid>,
    State(state): State<AppState>,
) -> Result<Json<UnifiedProgressResponse>, StatusCode> {
    let query = ProgressQuery {
        operation_type: Some("epg_ingestion".to_string()),
        resource_id: Some(source_id),
        include_completed: Some(true),
        include_failed: Some(true),
        limit: Some(10),
        state: None,
        active_only: None,
    };
    get_unified_progress(Query(query), State(state)).await
}

/// Get progress for a specific proxy
#[utoipa::path(
    get,
    path = "/progress/resources/proxies/{proxy_id}",
    responses(
        (status = 200, description = "Proxy regeneration progress retrieved", body = UnifiedProgressResponse),
        (status = 404, description = "No progress found for proxy"),
        (status = 500, description = "Internal server error")
    ),
    tag = "progress"
)]
pub async fn get_proxy_regeneration_progress(
    Path(proxy_id): Path<Uuid>,
    State(state): State<AppState>,
) -> Result<Json<UnifiedProgressResponse>, StatusCode> {
    let query = ProgressQuery {
        operation_type: Some("proxy_regeneration".to_string()),
        resource_id: Some(proxy_id),
        include_completed: Some(true),
        include_failed: Some(true),
        limit: Some(10),
        state: None,
        active_only: None,
    };
    get_unified_progress(Query(query), State(state)).await
}

/// SSE endpoint for real-time progress updates
#[utoipa::path(
    get,
    path = "/progress/events",
    params(ProgressQuery),
    responses(
        (status = 200, 
         description = "SSE stream for real-time progress updates", 
         content_type = "text/event-stream",
         body = String,
         example = json!("event: progress\ndata: {\"id\":\"123e4567-e89b-12d3-a456-426614174000\",\"operation_type\":\"proxy_regeneration\",\"state\":\"processing\",\"progress\":{\"percentage\":45.5}}\n\n"))
    ),
    tag = "progress"
)]
pub async fn progress_events_stream(
    Query(query): Query<ProgressQuery>,
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, axum::Error>>> {
    tracing::debug!("SSE progress stream started with query: {:?}", query);
    let progress_service = state.progress_service.clone();
    
    // Create receiver OUTSIDE the stream to ensure it stays alive for the entire connection
    tracing::debug!("SSE stream starting - subscribing to progress updates with query: {:?}", query);
    let mut receiver = progress_service.subscribe();
    tracing::debug!("SSE receiver created successfully, receiver count: {}", progress_service.get_receiver_count());
    
    let stream = async_stream::stream! {
        // Receiver is captured by the stream closure, keeping it alive
        
        // Send initial heartbeat
        yield Ok(Event::default()
            .event("heartbeat")
            .data("connected"));
        
        // Send current progress state  
        let current_progress = progress_service.get_all_progress().await;
        for (operation_id, progress) in current_progress {
            // Apply same filtering logic as get_unified_progress
            if let Some(ref type_filter) = query.operation_type {
                let op_type_str = operation_type_to_string(&progress.operation_type);
                if op_type_str != type_filter.to_lowercase() {
                    continue;
                }
            }

            if let Some(ref state_filter) = query.state {
                let state_str = universal_state_to_string(&progress.state);
                if state_str != state_filter.to_lowercase() {
                    continue;
                }
            }

            if let Some(resource_id) = query.resource_id {
                if operation_id != resource_id {
                    continue;
                }
            }

            if query.active_only.unwrap_or(false) {
                match progress.state {
                    UniversalState::Completed | UniversalState::Error | UniversalState::Cancelled => {
                        continue;
                    }
                    _ => {}
                }
            }
            
            // Build progress response
            let progress_response = ProgressOperationResponse {
                id: operation_id,
                operation_type: operation_type_to_string(&progress.operation_type),
                operation_name: progress.operation_name.clone(),
                state: universal_state_to_string(&progress.state),
                current_step: progress.current_step.clone(),
                progress: ProgressDetails {
                    percentage: progress.progress_percentage,
                    items: if progress.items_processed.is_some() || progress.items_total.is_some() {
                        Some(ItemProgress {
                            processed: progress.items_processed,
                            total: progress.items_total,
                        })
                    } else {
                        None
                    },
                    bytes: if progress.bytes_processed.is_some() || progress.bytes_total.is_some() {
                        Some(ByteProgress {
                            processed: progress.bytes_processed,
                            total: progress.bytes_total,
                        })
                    } else {
                        None
                    },
                },
                timing: TimingDetails {
                    started_at: progress.started_at,
                    updated_at: progress.updated_at,
                    completed_at: progress.completed_at,
                    duration_ms: if let Some(completed_at) = progress.completed_at {
                        Some((completed_at - progress.started_at).num_milliseconds())
                    } else {
                        Some((chrono::Utc::now() - progress.started_at).num_milliseconds())
                    },
                },
                metadata: progress.metadata.clone(),
                error: progress.error_message.clone(),
            };
            
            let data = serde_json::to_string(&progress_response).unwrap_or_else(|_| "{}".to_string());
            yield Ok(Event::default()
                .event("progress")
                .data(data));
        }
        
        // Listen for real-time updates
        loop {
            match receiver.recv().await {
                Ok(progress) => {
                    tracing::debug!("SSE received progress update for operation {} ({:?}): {}", 
                                  progress.operation_id, progress.operation_type, progress.current_step);
                    
                    // Apply same filtering logic as above
                    if let Some(ref type_filter) = query.operation_type {
                        let op_type_str = operation_type_to_string(&progress.operation_type);
                        if op_type_str != type_filter.to_lowercase() {
                            tracing::debug!("SSE filtering out progress update: type {} doesn't match filter {}", 
                                          op_type_str, type_filter);
                            continue;
                        }
                    }

                    if let Some(ref state_filter) = query.state {
                        let state_str = universal_state_to_string(&progress.state);
                        if state_str != state_filter.to_lowercase() {
                            continue;
                        }
                    }

                    if let Some(resource_id) = query.resource_id {
                        if progress.operation_id != resource_id {
                            continue;
                        }
                    }

                    if query.active_only.unwrap_or(false) {
                        match progress.state {
                            UniversalState::Completed | UniversalState::Error | UniversalState::Cancelled => {
                                continue;
                            }
                            _ => {}
                        }
                    }
                    
                    // Build and send progress update
                    let progress_response = ProgressOperationResponse {
                        id: progress.operation_id,
                        operation_type: operation_type_to_string(&progress.operation_type),
                        operation_name: progress.operation_name,
                        state: universal_state_to_string(&progress.state),
                        current_step: progress.current_step,
                        progress: ProgressDetails {
                            percentage: progress.progress_percentage,
                            items: if progress.items_processed.is_some() || progress.items_total.is_some() {
                                Some(ItemProgress {
                                    processed: progress.items_processed,
                                    total: progress.items_total,
                                })
                            } else {
                                None
                            },
                            bytes: if progress.bytes_processed.is_some() || progress.bytes_total.is_some() {
                                Some(ByteProgress {
                                    processed: progress.bytes_processed,
                                    total: progress.bytes_total,
                                })
                            } else {
                                None
                            },
                        },
                        timing: TimingDetails {
                            started_at: progress.started_at,
                            updated_at: progress.updated_at,
                            completed_at: progress.completed_at,
                            duration_ms: if let Some(completed_at) = progress.completed_at {
                                Some((completed_at - progress.started_at).num_milliseconds())
                            } else {
                                Some((chrono::Utc::now() - progress.started_at).num_milliseconds())
                            },
                        },
                        metadata: progress.metadata,
                        error: progress.error_message,
                    };
                    
                    let data = serde_json::to_string(&progress_response).unwrap_or_else(|_| "{}".to_string());
                    tracing::debug!("SSE sending progress update for operation {} ({:?})", 
                                  progress.operation_id, progress.operation_type);
                    yield Ok(Event::default()
                        .event("progress")
                        .data(data));
                },
                Err(_) => {
                    // Channel closed or error - send disconnect event and break
                    tracing::debug!("SSE receiver error or channel closed, ending stream");
                    yield Ok(Event::default()
                        .event("disconnect")
                        .data("Connection closed"));
                    break;
                }
            }
        }
        
        tracing::debug!("SSE stream ended for query: {:?}", query);
    };
    
    // Log receiver count after stream creation to verify it's still alive
    tracing::debug!("SSE stream created, final receiver count: {}", state.progress_service.get_receiver_count());

    Sse::new(stream)
        .keep_alive(
            axum::response::sse::KeepAlive::new()
                .interval(std::time::Duration::from_secs(30))
                .text("heartbeat")
        )
}