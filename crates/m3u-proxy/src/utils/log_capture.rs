//! Log capture layer for real-time streaming
//!
//! This module provides a custom tracing layer that captures log events
//! and broadcasts them to SSE subscribers in real-time.

use std::{
    collections::HashMap,
    sync::{Arc, atomic::{AtomicU64, Ordering}, OnceLock},
};
use tokio::sync::broadcast;
use tracing::{Event, Subscriber};
use tracing_subscriber::{
    layer::Context,
    Layer,
};
use uuid::Uuid;

use crate::web::api::log_streaming::LogEvent;

/// Maximum number of log events to keep in memory for SSE streaming
/// This prevents unbounded memory growth from log events
pub const MAX_LOG_BUFFER_SIZE: usize = 200;

/// Global log broadcaster for accessing from different parts of the application
static GLOBAL_LOG_BROADCASTER: OnceLock<broadcast::Sender<LogEvent>> = OnceLock::new();

/// Log capture layer that broadcasts events to SSE subscribers
pub struct LogCaptureLayer {
    sender: broadcast::Sender<LogEvent>,
    event_counter: Arc<AtomicU64>,
}

impl LogCaptureLayer {
    /// Create a new log capture layer with the default buffer capacity
    pub fn new() -> (Self, broadcast::Receiver<LogEvent>) {
        Self::new_with_capacity(MAX_LOG_BUFFER_SIZE)
    }

    /// Create a new log capture layer with the specified buffer capacity
    pub fn new_with_capacity(buffer_capacity: usize) -> (Self, broadcast::Receiver<LogEvent>) {
        let (sender, receiver) = broadcast::channel(buffer_capacity);
        let layer = Self {
            sender,
            event_counter: Arc::new(AtomicU64::new(0)),
        };
        (layer, receiver)
    }

    /// Get the broadcast sender for external use
    pub fn sender(&self) -> broadcast::Sender<LogEvent> {
        self.sender.clone()
    }

    /// Get the total number of events processed
    pub fn event_count(&self) -> u64 {
        self.event_counter.load(Ordering::Relaxed)
    }

    /// Extract fields and message from a tracing event
    fn extract_fields_and_message(&self, event: &Event<'_>) -> (HashMap<String, String>, String) {
        let mut fields = HashMap::new();
        
        // Use our field visitor to extract structured fields
        let mut visitor = FieldVisitor::new(&mut fields);
        event.record(&mut visitor);
        
        // Extract message - try to get it from fields first, fallback to event name
        let message = fields.get("message")
            .cloned()
            .unwrap_or_else(|| event.metadata().name().to_string());
        
        (fields, message)
    }


    /// Try to extract span info in a way that's compatible with any subscriber
    fn try_extract_span_info<S>(&self, ctx: Context<'_, S>) -> Option<crate::web::api::log_streaming::SpanInfo> 
    where
        S: Subscriber,
    {
        // Use a safer approach - check if we can access span information
        if let Some(current_span_id) = ctx.current_span().id() {
            // Get the current span metadata if available
            let current_span = ctx.current_span();
            let metadata = current_span.metadata();
            
            Some(crate::web::api::log_streaming::SpanInfo {
                name: metadata.map(|m| m.name().to_string()).unwrap_or_else(|| "unknown".to_string()),
                id: format!("{current_span_id:?}"),
                parent_id: None, // Keep this simple for now
            })
        } else {
            None
        }
    }
}

impl<S> Layer<S> for LogCaptureLayer
where
    S: Subscriber,
{
    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
        // Skip events from the log capture system itself to avoid recursion
        let target = event.metadata().target();
        if target.starts_with("m3u_proxy::web::api::log_streaming") 
            || target.starts_with("m3u_proxy::utils::log_capture") {
            return;
        }

        // Extract log information
        let level = event.metadata().level().to_string().to_uppercase();
        let (fields, message) = self.extract_fields_and_message(event);
        
        // Try to extract span info if the subscriber supports it
        let span_info = self.try_extract_span_info(ctx);
        

        // Derive event ID from operation context or span
        let event_id = if let Some(operation_id) = fields.get("operation_id") {
            // Use operation_id from structured fields if available
            operation_id.clone()
        } else if let Some(span_info) = &span_info {
            // Use span ID to group events from the same operation
            span_info.id.clone()
        } else {
            // Fallback to unique event ID
            Uuid::new_v4().to_string()
        };

        // Create log event
        let log_event = LogEvent {
            id: event_id,
            timestamp: chrono::Utc::now().to_rfc3339(),
            level,
            target: target.to_string(),
            message,
            fields,
            span: span_info,
        };

        // Increment counter
        self.event_counter.fetch_add(1, Ordering::Relaxed);

        // Broadcast the event (ignore send errors - means no subscribers)
        let _ = self.sender.send(log_event);
    }
}

/// Field visitor to extract structured data from tracing events
struct FieldVisitor<'a> {
    fields: &'a mut HashMap<String, String>,
}

impl<'a> FieldVisitor<'a> {
    fn new(fields: &'a mut HashMap<String, String>) -> Self {
        Self { fields }
    }
}

impl<'a> tracing::field::Visit for FieldVisitor<'a> {
    fn record_f64(&mut self, field: &tracing::field::Field, value: f64) {
        self.fields.insert(field.name().to_string(), value.to_string());
    }

    fn record_i64(&mut self, field: &tracing::field::Field, value: i64) {
        self.fields.insert(field.name().to_string(), value.to_string());
    }

    fn record_u64(&mut self, field: &tracing::field::Field, value: u64) {
        self.fields.insert(field.name().to_string(), value.to_string());
    }

    fn record_bool(&mut self, field: &tracing::field::Field, value: bool) {
        self.fields.insert(field.name().to_string(), value.to_string());
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        self.fields.insert(field.name().to_string(), value.to_string());
    }

    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        let formatted = format!("{value:?}");
        // Clean up quoted strings for better readability
        let clean_value = if formatted.starts_with('"') && formatted.ends_with('"') && formatted.len() > 1 {
            formatted[1..formatted.len()-1].to_string()
        } else {
            formatted
        };
        self.fields.insert(field.name().to_string(), clean_value);
    }

    fn record_error(&mut self, field: &tracing::field::Field, value: &(dyn std::error::Error + 'static)) {
        self.fields.insert(field.name().to_string(), value.to_string());
    }
}


/// Initialize log capture and return the broadcast sender
pub fn init_log_capture() -> broadcast::Sender<LogEvent> {
    let (layer, _receiver) = LogCaptureLayer::new(); // Use default buffer size
    
    
    // The layer would be added to the tracing subscriber in main.rs
    // For now, just return the sender
    layer.sender()
}

/// Setup function to add log capture to an existing tracing subscriber
pub fn setup_log_capture_with_subscriber() -> (LogCaptureLayer, broadcast::Sender<LogEvent>) {
    let (layer, _receiver) = LogCaptureLayer::new(); // Use default buffer size (200 events)
    let sender = layer.sender();
    
    // Store the sender globally for access from web handlers
    let _ = GLOBAL_LOG_BROADCASTER.set(sender.clone());
    
    (layer, sender)
}

/// Get the global log broadcaster if it has been initialized
pub fn get_global_log_broadcaster() -> Option<broadcast::Sender<LogEvent>> {
    GLOBAL_LOG_BROADCASTER.get().cloned()
}