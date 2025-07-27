//! HTTP response types and utilities
//!
//! This module provides standardized response types and error handling
//! for the web layer, ensuring consistent API responses across all endpoints.

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use utoipa::ToSchema;

use crate::errors::{AppError, AppResult};

/// Standard API response wrapper
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ApiResponse<T> {
    /// Whether the operation was successful
    pub success: bool,
    /// Response data (present on success)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
    /// Error message (present on failure)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Additional error details
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<HashMap<String, String>>,
    /// Request timestamp
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

impl<T> ApiResponse<T>
where
    T: Serialize,
{
    /// Create a successful response
    pub fn success(data: T) -> Self {
        Self {
            success: true,
            data: Some(data),
            error: None,
            details: None,
            timestamp: chrono::Utc::now(),
        }
    }

    /// Create an error response
    pub fn error(message: String) -> ApiResponse<()> {
        ApiResponse {
            success: false,
            data: None,
            error: Some(message),
            details: None,
            timestamp: chrono::Utc::now(),
        }
    }

    /// Create an error response with details
    pub fn error_with_details(message: String, details: HashMap<String, String>) -> ApiResponse<()> {
        ApiResponse {
            success: false,
            data: None,
            error: Some(message),
            details: Some(details),
            timestamp: chrono::Utc::now(),
        }
    }
}

/// Convert AppResult to HTTP response
impl<T> IntoResponse for ApiResponse<T>
where
    T: Serialize,
{
    fn into_response(self) -> Response {
        let status = if self.success {
            StatusCode::OK
        } else {
            StatusCode::INTERNAL_SERVER_ERROR
        };

        (status, Json(self)).into_response()
    }
}

/// Paginated response wrapper
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PaginatedResponse<T> {
    /// The actual data items
    pub items: Vec<T>,
    /// Total number of items (across all pages)
    pub total: u64,
    /// Current page number (1-based)
    pub page: u32,
    /// Number of items per page
    pub per_page: u32,
    /// Total number of pages
    pub total_pages: u32,
    /// Whether there is a next page
    pub has_next: bool,
    /// Whether there is a previous page
    pub has_previous: bool,
}

impl<T> PaginatedResponse<T> {
    /// Create a new paginated response
    pub fn new(items: Vec<T>, total: u64, page: u32, per_page: u32) -> Self {
        let total_pages = if per_page > 0 {
            (total as f64 / per_page as f64).ceil() as u32
        } else {
            1
        };

        Self {
            items,
            total,
            page,
            per_page,
            total_pages,
            has_next: page < total_pages,
            has_previous: page > 1,
        }
    }
}

/// Helper function to convert AppResult to HTTP response
pub fn handle_result<T>(result: AppResult<T>) -> impl IntoResponse
where
    T: Serialize,
{
    match result {
        Ok(data) => (StatusCode::OK, Json(ApiResponse::success(data))).into_response(),
        Err(error) => handle_error(error).into_response(),
    }
}

/// Convert AppError to appropriate HTTP response
pub fn handle_error(error: AppError) -> impl IntoResponse {
    let (status, message, details) = match &error {
        AppError::Validation { message } => (
            StatusCode::BAD_REQUEST,
            message.clone(),
            None,
        ),
        AppError::NotFound { resource, id } => (
            StatusCode::NOT_FOUND,
            format!("{} with id '{}' not found", resource, id),
            None,
        ),
        AppError::PermissionDenied { action, resource } => (
            StatusCode::FORBIDDEN,
            format!("Permission denied: {} on {}", action, resource),
            None,
        ),
        AppError::Configuration { message } => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Configuration error: {}", message),
            None,
        ),
        AppError::ExternalService { service, message } => (
            StatusCode::BAD_GATEWAY,
            format!("External service error ({}): {}", service, message),
            None,
        ),
        AppError::Http(_) => (
            StatusCode::BAD_GATEWAY,
            "External service communication failed".to_string(),
            None,
        ),
        AppError::Database(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Database operation failed".to_string(),
            None,
        ),
        AppError::Repository(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Data access failed".to_string(),
            None,
        ),
        AppError::Source(_) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            "Source operation failed".to_string(),
            None,
        ),
        AppError::Web(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Web request processing failed".to_string(),
            None,
        ),
        AppError::Internal { message } => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Internal error: {}", message),
            None,
        ),
    };

    let response = if let Some(details) = details {
        ApiResponse::<()>::error_with_details(message, details)
    } else {
        ApiResponse::<()>::error(message)
    };

    (status, Json(response)).into_response()
}

/// Success response helpers
pub fn ok<T: Serialize>(data: T) -> impl IntoResponse {
    (StatusCode::OK, Json(ApiResponse::success(data)))
}

pub fn created<T: Serialize>(data: T) -> impl IntoResponse {
    (StatusCode::CREATED, Json(ApiResponse::success(data)))
}

pub fn no_content() -> impl IntoResponse {
    StatusCode::NO_CONTENT
}

/// Error response helpers
pub fn bad_request(message: &str) -> impl IntoResponse {
    (
        StatusCode::BAD_REQUEST,
        Json(ApiResponse::<()>::error(message.to_string())),
    )
}

pub fn not_found(resource: &str, id: &str) -> impl IntoResponse {
    (
        StatusCode::NOT_FOUND,
        Json(ApiResponse::<()>::error(format!("{} with id '{}' not found", resource, id))),
    )
}

pub fn internal_error(message: &str) -> impl IntoResponse {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ApiResponse::<()>::error(message.to_string())),
    )
}

/// Validation error response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationErrorResponse {
    pub field: String,
    pub message: String,
}

pub fn validation_error(errors: Vec<ValidationErrorResponse>) -> impl IntoResponse {
    let mut details = HashMap::new();
    for error in &errors {
        details.insert(error.field.clone(), error.message.clone());
    }

    (
        StatusCode::BAD_REQUEST,
        Json(ApiResponse::<()>::error_with_details(
            "Validation failed".to_string(),
            details,
        )),
    )
}

/// Database health status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseHealth {
    pub status: String,
    pub connection_pool_size: u32,
    pub active_connections: u32,
}