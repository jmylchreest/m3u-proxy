//! SSE-based log streaming API
//!
//! This module provides Server-Sent Events (SSE) endpoints for real-time log streaming.
//! Clients can subscribe to log events and receive them in real-time via SSE.

use axum::{
    extract::{Query, State},
    response::{
        IntoResponse,
        sse::{Event, KeepAlive, Sse},
    },
};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, convert::Infallible, time::Duration};
use tokio_stream::{StreamExt, wrappers::BroadcastStream};
use tracing::{debug, error};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use crate::utils::log_capture::MAX_LOG_BUFFER_SIZE;
use crate::web::AppState;

/// Log event structure for SSE streaming
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct LogEvent {
    /// Unique event ID
    pub id: String,
    /// Timestamp in RFC3339 format
    pub timestamp: String,
    /// Log level (ERROR, WARN, INFO, DEBUG, TRACE)
    pub level: String,
    /// Target module/component
    pub target: String,
    /// Log message
    pub message: String,
    /// Additional structured fields
    pub fields: HashMap<String, String>,
    /// Span information if available
    pub span: Option<SpanInfo>,
}

/// Span information for structured logging
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SpanInfo {
    /// Span name
    pub name: String,
    /// Span ID
    pub id: String,
    /// Parent span ID if available
    pub parent_id: Option<String>,
}

/// Query parameters for log streaming
#[derive(Debug, Deserialize, IntoParams)]
pub struct LogStreamParams {
    /// Minimum log level to stream (default: INFO)
    #[serde(default = "default_log_level")]
    pub level: String,
    /// Filter by target module/component
    pub target: Option<String>,
    /// Include structured fields in output
    #[serde(default = "default_include_fields")]
    pub include_fields: bool,
    /// Include span information
    #[serde(default = "default_include_spans")]
    pub include_spans: bool,
}

fn default_log_level() -> String {
    "INFO".to_string()
}

fn default_include_fields() -> bool {
    true
}

fn default_include_spans() -> bool {
    true
}

/// Log level enum for filtering
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum LogLevel {
    Trace = 0,
    Debug = 1,
    Info = 2,
    Warn = 3,
    Error = 4,
}

impl LogLevel {
    fn from_str(s: &str) -> Option<Self> {
        match s.to_uppercase().as_str() {
            "TRACE" => Some(LogLevel::Trace),
            "DEBUG" => Some(LogLevel::Debug),
            "INFO" => Some(LogLevel::Info),
            "WARN" => Some(LogLevel::Warn),
            "ERROR" => Some(LogLevel::Error),
            _ => None,
        }
    }
}

/// Real-time log streaming via Server-Sent Events
///
/// This endpoint provides real-time streaming of application logs via SSE.
/// Clients can filter by log level, target module, and control the format.
#[utoipa::path(
    get,
    path = "/logs/stream",
    tag = "logs",
    summary = "Stream logs via SSE",
    description = "Subscribe to real-time log events via Server-Sent Events (SSE)",
    params(LogStreamParams),
    responses(
        (status = 200, description = "SSE log stream", content_type = "text/event-stream"),
        (status = 400, description = "Invalid parameters"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn stream_logs(
    Query(params): Query<LogStreamParams>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    debug!("Starting log stream with params: {:?}", params);

    // Validate log level parameter
    let min_level = match LogLevel::from_str(&params.level) {
        Some(level) => level,
        None => {
            return Err(axum::http::StatusCode::BAD_REQUEST);
        }
    };

    // Get or create the log broadcast receiver
    let log_receiver = match state.log_broadcaster.as_ref() {
        Some(broadcaster) => broadcaster.subscribe(),
        None => {
            error!("Log broadcaster not initialized");
            return Err(axum::http::StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    // Create the SSE stream
    let stream = BroadcastStream::new(log_receiver).filter_map(move |result| {
        let event = match result {
            Ok(event) => event,
            Err(_) => return None, // Skip lagged events
        };

        // Apply log level filtering
        if let Some(event_level) = LogLevel::from_str(&event.level)
            && event_level < min_level
        {
            return None;
        }

        // Apply target filtering
        if let Some(ref target_filter) = params.target
            && !event.target.contains(target_filter)
        {
            return None;
        }

        // Optionally strip fields/spans based on parameters
        let mut filtered_event = event;
        if !params.include_fields {
            filtered_event.fields.clear();
        }
        if !params.include_spans {
            filtered_event.span = None;
        }

        // Create SSE event
        let sse_event = match Event::default()
            .id(&filtered_event.id)
            .event("log")
            .json_data(&filtered_event)
        {
            Ok(event) => event,
            Err(e) => {
                error!("Failed to serialize log event: {}", e);
                return None;
            }
        };

        Some(Ok::<_, Infallible>(sse_event))
    });

    Ok(Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("heartbeat"),
    ))
}

/// Get log streaming statistics
#[utoipa::path(
    get,
    path = "/logs/stats",
    tag = "logs",
    summary = "Get log streaming statistics",
    description = "Get statistics about the log streaming system",
    responses(
        (status = 200, description = "Log streaming statistics"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_log_stats(State(state): State<AppState>) -> impl IntoResponse {
    let log_stats = match state.log_broadcaster.as_ref() {
        Some(broadcaster) => {
            serde_json::json!({
                "active_subscribers": broadcaster.receiver_count(),
                "buffer_capacity": MAX_LOG_BUFFER_SIZE,
                "max_buffer_size": MAX_LOG_BUFFER_SIZE,
                "total_events_sent": "N/A", // Would need additional tracking
                "status": "active",
                "buffer_usage_percent": 0.0 // Would need additional tracking for current usage
            })
        }
        None => {
            serde_json::json!({
                "active_subscribers": 0,
                "buffer_capacity": 0,
                "max_buffer_size": MAX_LOG_BUFFER_SIZE,
                "total_events_sent": 0,
                "status": "inactive",
                "buffer_usage_percent": 0.0
            })
        }
    };

    axum::response::Json(log_stats)
}

/// Send a test log event (for development/testing)
#[utoipa::path(
    post,
    path = "/logs/test",
    tag = "logs",
    summary = "Send test log event",
    description = "Send a test log event through the streaming system (development only)",
    responses(
        (status = 200, description = "Test log event sent"),
        (status = 404, description = "Log broadcaster not available"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn send_test_log(State(state): State<AppState>) -> impl IntoResponse {
    let operation_id = Uuid::new_v4().to_string();
    let test_event = LogEvent {
        id: operation_id.clone(),
        timestamp: chrono::Utc::now().to_rfc3339(),
        level: "INFO".to_string(),
        target: "m3u_proxy::test".to_string(),
        message: "This is a test log event from the API".to_string(),
        fields: {
            let mut fields = HashMap::new();
            fields.insert("test".to_string(), "true".to_string());
            fields.insert("source".to_string(), "api".to_string());
            fields.insert("operation_id".to_string(), operation_id);
            fields
        },
        span: Some(SpanInfo {
            name: "test_span".to_string(),
            id: Uuid::new_v4().to_string(),
            parent_id: None,
        }),
    };

    match state.log_broadcaster.as_ref() {
        Some(broadcaster) => match broadcaster.send(test_event) {
            Ok(_) => axum::response::Json(serde_json::json!({
                "success": true,
                "message": "Test log event sent successfully"
            })),
            Err(e) => {
                error!("Failed to send test log event: {}", e);
                axum::response::Json(serde_json::json!({
                    "success": false,
                    "message": "Failed to send test log event"
                }))
            }
        },
        None => axum::response::Json(serde_json::json!({
            "success": false,
            "message": "Log broadcaster not available"
        })),
    }
}
