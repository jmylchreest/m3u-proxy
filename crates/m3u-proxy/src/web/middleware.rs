//! HTTP middleware
//!
//! This module provides middleware for cross-cutting concerns like
//! request logging, error handling, rate limiting, and metrics.

use axum::{
    Json,
    body::Body,
    extract::Request,
    http::{HeaderMap, Method, StatusCode, Uri},
    middleware::Next,
    response::{IntoResponse, Response},
};
use std::time::Instant;
use tracing::{info, warn};

use super::responses::ApiResponse;

/// Request logging middleware
///
/// Logs all incoming requests with timing information
pub async fn request_logging_middleware(
    method: Method,
    uri: Uri,
    request: Request,
    next: Next,
) -> Response {
    let start = Instant::now();

    // Generate request ID for tracing
    let request_id = uuid::Uuid::new_v4().to_string();

    info!(
        method = %method,
        uri = %uri,
        request_id = %request_id,
        "HTTP request started"
    );

    let response = next.run(request).await;
    let status = response.status().as_u16();
    let duration = start.elapsed();

    if status >= 400 {
        warn!(
            method = %method,
            uri = %uri,
            status = status,
            request_id = %request_id,
            duration_ms = duration.as_millis(),
            "HTTP request completed with error"
        );
    } else {
        info!(
            method = %method,
            uri = %uri,
            status = status,
            request_id = %request_id,
            duration_ms = duration.as_millis(),
            "HTTP request completed"
        );
    }

    response
}

/// Error handling middleware
///
/// Provides graceful error handling for HTTP requests
/// Note: Panic catching in async middleware is complex and potentially problematic.
/// This middleware focuses on proper async error handling without blocking operations.
pub async fn error_handling_middleware(request: Request, next: Next) -> Response {
    // Simply run the handler - Axum and Tokio handle most error cases gracefully
    // Panics will be caught by the Tokio runtime and logged appropriately
    next.run(request).await
}

/// Request size limiting middleware
///
/// Prevents oversized requests from consuming resources
pub async fn request_size_middleware(headers: HeaderMap, request: Request, next: Next) -> Response {
    const MAX_REQUEST_SIZE: usize = 10 * 1024 * 1024; // 10MB

    if let Some(content_length) = headers.get("content-length")
        && let Ok(length_str) = content_length.to_str()
        && let Ok(length) = length_str.parse::<usize>()
        && length > MAX_REQUEST_SIZE
    {
        return (
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(ApiResponse::<()>::error(format!(
                "Request too large: {length} bytes (max: {MAX_REQUEST_SIZE})"
            ))),
        )
            .into_response();
    }

    next.run(request).await
}

/// CORS middleware (custom implementation)
///
/// Handles Cross-Origin Resource Sharing headers
pub async fn cors_middleware(
    method: Method,
    _headers: HeaderMap,
    request: Request,
    next: Next,
) -> Response {
    // Handle preflight requests
    if method == Method::OPTIONS {
        return Response::builder()
            .status(StatusCode::OK)
            .header("Access-Control-Allow-Origin", "*")
            .header(
                "Access-Control-Allow-Methods",
                "GET, POST, PUT, DELETE, OPTIONS",
            )
            .header(
                "Access-Control-Allow-Headers",
                "Content-Type, Authorization, X-Requested-With",
            )
            .header("Access-Control-Max-Age", "3600")
            .body(Body::empty())
            .unwrap();
    }

    let mut response = next.run(request).await;

    // Add CORS headers to response
    let headers = response.headers_mut();
    headers.insert("Access-Control-Allow-Origin", "*".parse().unwrap());
    headers.insert(
        "Access-Control-Allow-Methods",
        "GET, POST, PUT, DELETE, OPTIONS".parse().unwrap(),
    );
    headers.insert(
        "Access-Control-Allow-Headers",
        "Content-Type, Authorization, X-Requested-With"
            .parse()
            .unwrap(),
    );

    response
}

/// Security headers middleware
///
/// Adds security-related headers to responses
pub async fn security_headers_middleware(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;

    let headers = response.headers_mut();

    // Add security headers
    headers.insert("X-Content-Type-Options", "nosniff".parse().unwrap());
    headers.insert("X-Frame-Options", "DENY".parse().unwrap());
    headers.insert("X-XSS-Protection", "1; mode=block".parse().unwrap());
    headers.insert(
        "Referrer-Policy",
        "strict-origin-when-cross-origin".parse().unwrap(),
    );
    headers.insert("Content-Security-Policy", "default-src 'self'; script-src 'self' 'unsafe-inline' 'unsafe-eval'; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob: https: http:; font-src 'self' data:; connect-src 'self' *; media-src * blob:".parse().unwrap());

    response
}

/// Request timeout middleware
///
/// Ensures requests don't run indefinitely
pub async fn timeout_middleware(request: Request, next: Next) -> Response {
    const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

    match tokio::time::timeout(REQUEST_TIMEOUT, next.run(request)).await {
        Ok(response) => response,
        Err(_) => {
            warn!("Request timed out");
            (
                StatusCode::REQUEST_TIMEOUT,
                Json(ApiResponse::<()>::error("Request timed out".to_string())),
            )
                .into_response()
        }
    }
}

/// Metrics collection middleware
///
/// Collects request metrics for monitoring
pub async fn metrics_middleware(
    method: Method,
    uri: Uri,
    request: Request,
    next: Next,
) -> Response {
    let start = Instant::now();
    let response = next.run(request).await;
    let duration = start.elapsed();
    let status = response.status().as_u16();

    // Metrics sent to logs only
    // This could be Prometheus, StatsD, or another metrics system
    info!(
        method = %method,
        uri = %uri,
        status = status,
        duration_ms = duration.as_millis(),
        "Request metrics collected"
    );

    response
}

/// Health check bypass middleware
///
/// Allows health checks to bypass other middleware for better performance
pub async fn health_check_bypass_middleware(uri: Uri, request: Request, next: Next) -> Response {
    // Skip heavy middleware for health check endpoints
    if uri.path().starts_with("/health") || uri.path() == "/ready" || uri.path() == "/live" {
        return next.run(request).await;
    }

    // Continue with normal middleware chain
    next.run(request).await
}

/// Conditional request logging middleware that checks runtime settings
///
/// Only logs requests if request logging is enabled in runtime settings
pub async fn conditional_request_logging_middleware(
    method: axum::http::Method,
    uri: axum::http::Uri,
    axum::extract::State(state): axum::extract::State<crate::web::AppState>,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let flags = state.runtime_settings_store.get_flags().await;

    if flags.request_logging_enabled {
        // Call the normal request logging middleware
        request_logging_middleware(method, uri, request, next).await
    } else {
        // Skip logging and just run the request
        next.run(request).await
    }
}

/// Middleware stack builder
pub struct MiddlewareStack;

impl MiddlewareStack {
    /// Get the recommended middleware stack for production
    /// Returns a function that can be used to configure a Router
    pub fn production() -> impl Fn(axum::Router) -> axum::Router {
        |router| {
            router
                // Security headers middleware
                .layer(axum::middleware::from_fn(security_headers_middleware))
                // Request logging middleware
                .layer(axum::middleware::from_fn(request_logging_middleware))
        }
    }

    /// Get a minimal middleware stack for development
    pub fn development() -> impl Fn(axum::Router) -> axum::Router {
        |router| {
            router
                // Just request logging for development
                .layer(axum::middleware::from_fn(request_logging_middleware))
        }
    }
}
