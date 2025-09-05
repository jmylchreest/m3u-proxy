use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use prometheus::TextEncoder;

use crate::web::AppState;

/// Prometheus metrics endpoint handler
/// 
/// This provides a `/metrics` endpoint that exposes OpenTelemetry metrics
/// in Prometheus format via the opentelemetry-prometheus bridge.
pub async fn prometheus_metrics(State(state): State<AppState>) -> Result<Response, StatusCode> {
    // The OpenTelemetry-Prometheus bridge automatically registers our OTEL metrics
    // with the Prometheus registry, so we can just gather and encode them
    let registry = &state.observability.prometheus_registry;
    
    // Encode and return the metrics
    let encoder = TextEncoder::new();
    let metric_families = registry.gather();
    
    match encoder.encode_to_string(&metric_families) {
        Ok(output) => {
            Ok((
                StatusCode::OK,
                [("content-type", "text/plain; version=0.0.4; charset=utf-8")],
                output,
            ).into_response())
        }
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}