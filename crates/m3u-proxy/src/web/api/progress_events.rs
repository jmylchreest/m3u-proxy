//! SSE-based progress streaming API
//!
//! This module provides Server-Sent Events (SSE) for real-time progress updates
//! using the ProgressManager system.

use axum::{
    extract::{Query, State},
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse,
    },
};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::{debug, error};
use utoipa::{ToSchema, IntoParams};

use crate::web::AppState;
use crate::services::progress_service::{OperationType, UniversalState};

/// Convert operation type enum to lowercase string
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
        OperationType::Custom(name) => name.to_lowercase(),
    }
}

/// Convert universal state enum to lowercase string
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

/// Query parameters for progress event filtering
#[derive(Debug, Clone, Deserialize, IntoParams)]
pub struct ProgressEventQuery {
    /// Filter by operation type (e.g., "stream_ingestion", "epg_ingestion", "proxy_regeneration")
    pub operation_type: Option<String>,
    /// Filter by specific resource ID (source_id, proxy_id, etc.)
    pub resource_id: Option<String>,
    /// Filter by owner ID (same as resource_id for compatibility)
    pub owner_id: Option<String>,
    /// Filter by state (e.g., "processing", "completed", "error")  
    pub state: Option<String>,
    /// Include completed operations (default: true)
    pub include_completed: Option<bool>,
    /// Include failed operations (default: true)
    pub include_failed: Option<bool>,
    /// Only return active operations (default: false)
    pub active_only: Option<bool>,
}

/// Stage information for progress events
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ProgressStageEvent {
    /// Stage ID
    pub id: String,
    /// Stage display name
    pub name: String,
    /// Stage progress percentage (0-100)
    pub percentage: f64,
    /// Stage state
    pub state: String,
    /// Stage step description
    pub stage_step: String,
}

/// Progress event structure for SSE streaming (matches original format)
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ProgressEvent {
    /// Unique operation ID (included for consistency with UI expectations)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Resource that owns this operation (proxy ID, source ID, etc.)
    pub owner_id: String,
    /// Owner type (proxy, epg_source, stream_source, etc.)
    pub owner_type: String,
    /// Operation type (stream_ingestion, epg_ingestion, proxy_regeneration, etc.)
    pub operation_type: String,
    /// Operation name/description
    pub operation_name: String,
    /// Current state (idle, processing, completed, failed, etc.)
    pub state: String,
    /// Current stage ID
    pub current_stage: String,
    /// Overall progress percentage (0-100)
    pub overall_percentage: f64,
    /// Detailed stage information
    pub stages: Vec<ProgressStageEvent>,
    /// When operation started
    pub started_at: String,
    /// Last update timestamp
    pub last_update: String,
    /// When operation completed (if applicable)
    pub completed_at: Option<String>,
    /// Error message if failed
    pub error: Option<String>,
}

/// Stream real-time progress events via SSE
#[utoipa::path(
    get,
    path = "/progress/events",
    params(ProgressEventQuery),
    responses(
        (status = 200, description = "Progress events stream (SSE)", content_type = "text/event-stream"),
        (status = 500, description = "Internal server error")
    ),
    tag = "Progress"
)]
pub async fn progress_events_stream(
    Query(query): Query<ProgressEventQuery>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    debug!("Starting progress events SSE stream with filters: {:?}", query);

    // Subscribe to progress updates from ProgressService
    let progress_service = state.progress_service.clone();
    let mut receiver = progress_service.subscribe();

    let stream = async_stream::stream! {
        // Send initial heartbeat
        yield Ok::<Event, axum::Error>(Event::default()
            .event("heartbeat")
            .data("connected"));
            
        // Only query database for historical events if explicitly requested
        // Send historical completed operations if include_completed is true (default)
        let send_historical_completed = query.include_completed.unwrap_or(true);
        let send_historical_failed = query.include_failed.unwrap_or(true);
        
        // Send historical events if requested
        if send_historical_completed || send_historical_failed {
            let all_progress = progress_service.get_all_progress().await;
            for progress in all_progress {
                let should_send = match progress.state {
                    crate::services::progress_service::UniversalState::Completed => send_historical_completed,
                    crate::services::progress_service::UniversalState::Error => send_historical_failed,
                    _ => false, // Don't send other historical states
                };
                
                if should_send {
                    // Apply other filters before sending historical event
                    let passes_filters = {
                        let op_type_match = query.operation_type.as_ref()
                            .map(|t| operation_type_to_string(&progress.operation_type) == t.to_lowercase())
                            .unwrap_or(true);
                        let state_match = query.state.as_ref()
                            .map(|s| universal_state_to_string(&progress.state) == s.to_lowercase())
                            .unwrap_or(true);
                        let resource_match = query.resource_id.as_ref()
                            .map(|r| progress.owner_id.to_string() == *r)
                            .unwrap_or(true);
                        let owner_match = query.owner_id.as_ref()
                            .map(|o| progress.owner_id.to_string() == *o)
                            .unwrap_or(true);
                        
                        op_type_match && state_match && resource_match && owner_match
                    };
                    
                    if passes_filters {
                        // Convert and send historical event
                        let stages: Vec<ProgressStageEvent> = progress.stages.iter().map(|stage| {
                            ProgressStageEvent {
                                id: stage.id.clone(),
                                name: stage.name.clone(),
                                percentage: stage.percentage,
                                state: universal_state_to_string(&stage.state),
                                stage_step: stage.stage_step.clone(),
                            }
                        }).collect();

                        let event = ProgressEvent {
                            id: Some(progress.id.to_string()),
                            owner_id: progress.owner_id.to_string(),
                            owner_type: progress.owner_type,
                            operation_type: operation_type_to_string(&progress.operation_type),
                            operation_name: progress.operation_name,
                            state: universal_state_to_string(&progress.state),
                            current_stage: progress.current_stage,
                            overall_percentage: progress.overall_percentage,
                            stages,
                            started_at: progress.started_at.to_rfc3339(),
                            last_update: progress.last_update.to_rfc3339(),
                            completed_at: progress.completed_at.map(|dt| dt.to_rfc3339()),
                            error: progress.error_message,
                        };

                        match serde_json::to_string(&event) {
                            Ok(json) => {
                                yield Ok::<Event, axum::Error>(Event::default()
                                    .event("progress")
                                    .id(progress.id.to_string())
                                    .data(json));
                            }
                            Err(e) => {
                                error!("Failed to serialize historical progress event: {}", e);
                            }
                        }
                    }
                }
            }
        }
            
        // Listen for real-time updates
        loop {
            match receiver.recv().await {
                Ok(progress) => {
                    // Only log meaningful state changes to reduce log noise
                    if matches!(progress.state, crate::services::progress_service::UniversalState::Completed | 
                                               crate::services::progress_service::UniversalState::Error |
                                               crate::services::progress_service::UniversalState::Cancelled) {
                        debug!("SSE progress update for operation {} ({:?}): {} -> {}", 
                              progress.id, progress.operation_type, progress.current_stage, 
                              universal_state_to_string(&progress.state));
                    }
                    
                    // Apply filters (matching original logic)
                    if let Some(ref op_type) = query.operation_type {
                        let progress_op_type = operation_type_to_string(&progress.operation_type);
                        if progress_op_type != op_type.to_lowercase() {
                            debug!("SSE filtering out progress update: type {} doesn't match filter {}", 
                                  progress_op_type, op_type);
                            continue;
                        }
                    }
                    
                    if let Some(ref state_filter) = query.state {
                        let state_str = universal_state_to_string(&progress.state);
                        if state_str != state_filter.to_lowercase() {
                            continue;
                        }
                    }
                    
                    // Support both resource_id and owner_id (for compatibility)
                    if let Some(ref resource_id) = query.resource_id {
                        if progress.owner_id.to_string() != *resource_id {
                            continue;
                        }
                    }
                    
                    if let Some(ref owner_id) = query.owner_id {
                        if progress.owner_id.to_string() != *owner_id {
                            continue;
                        }
                    }
                    
                    // Filter by completion status
                    if query.active_only.unwrap_or(false) {
                        use crate::services::progress_service::UniversalState;
                        match progress.state {
                            UniversalState::Completed | UniversalState::Error | UniversalState::Cancelled => {
                                continue;
                            }
                            _ => {}
                        }
                    }
                    
                    // Filter real-time completion/failure events based on client preferences
                    // Always send events for operations that transition to completed/failed during the session
                    if let Some(include_completed) = query.include_completed {
                        if !include_completed && progress.state == crate::services::progress_service::UniversalState::Completed {
                            debug!("SSE filtering out real-time completion event per client request: {}", progress.id);
                            continue;
                        }
                    }
                    
                    if let Some(include_failed) = query.include_failed {
                        if !include_failed && progress.state == crate::services::progress_service::UniversalState::Error {
                            debug!("SSE filtering out real-time failure event per client request: {}", progress.id);
                            continue;
                        }
                    }

                    // Convert stages from UniversalProgress to ProgressStageEvent
                    let stages: Vec<ProgressStageEvent> = progress.stages.iter().map(|stage| {
                        ProgressStageEvent {
                            id: stage.id.clone(),
                            name: stage.name.clone(),
                            percentage: stage.percentage,
                            state: universal_state_to_string(&stage.state),
                            stage_step: stage.stage_step.clone(),
                        }
                    }).collect();

                    // Convert UniversalProgress to ProgressEvent for SSE (matching original format)
                    // Include id in JSON data for consistency with UI expectations
                    let event = ProgressEvent {
                        id: Some(progress.id.to_string()),
                        owner_id: progress.owner_id.to_string(),
                        owner_type: progress.owner_type,
                        operation_type: operation_type_to_string(&progress.operation_type),
                        operation_name: progress.operation_name,
                        state: universal_state_to_string(&progress.state),
                        current_stage: progress.current_stage,
                        overall_percentage: progress.overall_percentage,
                        stages,
                        started_at: progress.started_at.to_rfc3339(),
                        last_update: progress.last_update.to_rfc3339(),
                        completed_at: progress.completed_at.map(|dt| dt.to_rfc3339()),
                        error: progress.error_message,
                    };

                    // Serialize to JSON for SSE
                    match serde_json::to_string(&event) {
                        Ok(json) => {
                            yield Ok::<Event, axum::Error>(Event::default()
                                .event("progress")  // Use "progress" event type to match original
                                .id(progress.id.to_string())
                                .data(json));
                        }
                        Err(e) => {
                            error!("Failed to serialize progress event: {}", e);
                        }
                    }
                }
                Err(e) => {
                    error!("Error receiving progress update: {}", e);
                    break;
                }
            }
        }
    };

    Sse::new(stream)
        .keep_alive(
            KeepAlive::new()
                .interval(Duration::from_secs(30))
                .text("heartbeat"),
        )
}