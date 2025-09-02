//! Error type definitions for the M3U Proxy application
//!
//! This module defines all error types used throughout the application,
//! providing a hierarchical error system that makes debugging and error
//! handling more straightforward.

use thiserror::Error;

/// Top-level application error type
///
/// This enum represents all possible errors that can occur in the application.
/// It uses `thiserror` to provide automatic error trait implementations and
/// proper error chaining.
#[derive(Error, Debug)]
pub enum AppError {
    /// Database-related errors (SeaORM)
    #[error("Database error: {0}")]
    Database(#[from] sea_orm::DbErr),
    
    /// Repository layer errors
    #[error("Repository error: {0}")]
    Repository(#[from] RepositoryError),
    
    /// Source handling errors
    #[error("Source error: {0}")]
    Source(#[from] SourceError),
    
    /// Web layer errors
    #[error("Web error: {0}")]
    Web(#[from] WebError),
    
    /// Validation errors
    #[error("Validation error: {message}")]
    Validation { message: String },
    
    /// Resource not found errors
    #[error("Not found: {resource} with id {id}")]
    NotFound { resource: String, id: String },
    
    /// Permission denied errors
    #[error("Permission denied: {action} on {resource}")]
    PermissionDenied { action: String, resource: String },
    
    /// Configuration errors
    #[error("Configuration error: {message}")]
    Configuration { message: String },
    
    /// External service errors
    #[error("External service error: {service} - {message}")]
    ExternalService { service: String, message: String },
    
    /// Operation already in progress errors
    #[error("Operation already in progress: {operation_type} on {resource}")]
    OperationInProgress { operation_type: String, resource: String },
    
    /// Generic internal errors
    #[error("Internal error: {message}")]
    Internal { message: String },
    
    /// HTTP client errors  
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
}

/// Repository layer specific errors
#[derive(Error, Debug)]
pub enum RepositoryError {
    /// Database connection failures
    #[error("Database connection failed: {message}")]
    ConnectionFailed { message: String },
    
    /// SQL query execution failures
    #[error("Query failed: {query} - {message}")]
    QueryFailed { query: String, message: String },
    
    /// Database errors from SeaORM
    #[error("Database error: {0}")]
    Database(#[from] sea_orm::DbErr),
    
    /// UUID parsing errors
    #[error("UUID parsing error: {0}")]
    UuidParse(#[from] anyhow::Error),
    
    /// DateTime parsing errors  
    #[error("DateTime parsing error: {0}")]
    DateTimeParse(#[from] crate::utils::datetime::DateTimeError),
    
    /// Chrono parsing errors
    #[error("Chrono parsing error: {0}")]
    ChronoParse(#[from] chrono::ParseError),
    
    /// Data serialization/deserialization failures
    #[error("Serialization failed: {0}")]
    SerializationFailed(#[from] serde_json::Error),
    
    /// Constraint violations (unique, foreign key, etc.)
    #[error("Constraint violation: {constraint} - {message}")]
    ConstraintViolation { constraint: String, message: String },
    
    /// Record not found
    #[error("Record not found: {table} with {field} = {value}")]
    RecordNotFound { table: String, field: String, value: String },
    
    /// Generic not found (for compatibility)
    #[error("Not found: {resource} with id {id}")]
    NotFound { resource: String, id: String },
    
    /// Migration failures
    #[error("Migration failed: {version} - {message}")]
    MigrationFailed { version: String, message: String },
}

/// Source handling specific errors
#[derive(Error, Debug)]
pub enum SourceError {
    /// Network connection timeouts
    #[error("Connection timeout: {url}")]
    Timeout { url: String },
    
    /// Authentication failures
    #[error("Authentication failed: {source_type} - {message}")]
    AuthenticationFailed { source_type: String, message: String },
    
    /// Invalid source configuration
    #[error("Invalid configuration: {field} - {message}")]
    InvalidConfig { field: String, message: String },
    
    /// Parsing errors for source data
    #[error("Parse error: {source_type} - {message}")]
    ParseError { source_type: String, message: String },
    
    /// Unsupported source features
    #[error("Unsupported feature: {feature} for {source_type}")]
    UnsupportedFeature { feature: String, source_type: String },
    
    /// Rate limiting errors
    #[error("Rate limited: {source_name} - retry after {retry_after} seconds")]
    RateLimited { source_name: String, retry_after: u64 },
    
    /// HTTP errors from external sources
    #[error("HTTP error: {status} - {message}")]
    Http { status: u16, message: String },
}

/// Web layer specific errors
#[derive(Error, Debug)]
pub enum WebError {
    /// Invalid request format
    #[error("Invalid request: {field} - {message}")]
    InvalidRequest { field: String, message: String },
    
    /// Missing required headers
    #[error("Missing header: {header}")]
    MissingHeader { header: String },
    
    /// Invalid authentication token
    #[error("Invalid authentication: {message}")]
    InvalidAuth { message: String },
    
    /// Request payload too large
    #[error("Payload too large: {size} bytes (max: {max_size})")]
    PayloadTooLarge { size: usize, max_size: usize },
    
    /// Unsupported content type
    #[error("Unsupported content type: {content_type}")]
    UnsupportedContentType { content_type: String },
    
    /// JSON parsing errors
    #[error("JSON parse error: {0}")]
    JsonParse(#[from] serde_json::Error),
}

/// Convenience methods for creating common error types
impl AppError {
    /// Create a validation error with a custom message
    pub fn validation<S: Into<String>>(message: S) -> Self {
        Self::Validation {
            message: message.into(),
        }
    }
    
    
    /// Create a configuration error
    pub fn configuration<S: Into<String>>(message: S) -> Self {
        Self::Configuration {
            message: message.into(),
        }
    }
    
    /// Create an external service error
    pub fn external_service<S: Into<String>, M: Into<String>>(service: S, message: M) -> Self {
        Self::ExternalService {
            service: service.into(),
            message: message.into(),
        }
    }
    
    /// Create an operation in progress error
    pub fn operation_in_progress<O: Into<String>, R: Into<String>>(operation_type: O, resource: R) -> Self {
        Self::OperationInProgress {
            operation_type: operation_type.into(),
            resource: resource.into(),
        }
    }
    
    /// Create an internal error
    pub fn internal<S: Into<String>>(message: S) -> Self {
        Self::Internal {
            message: message.into(),
        }
    }
    
    /// Create a source error
    pub fn source_error<S: Into<String>>(message: S) -> Self {
        Self::Source(SourceError::InvalidConfig {
            field: "general".to_string(),
            message: message.into(),
        })
    }
}

impl RepositoryError {
}

impl SourceError {
    
    /// Create an authentication failed error
    pub fn auth_failed<S: Into<String>, M: Into<String>>(source_type: S, message: M) -> Self {
        Self::AuthenticationFailed {
            source_type: source_type.into(),
            message: message.into(),
        }
    }
    
}

impl WebError {
}