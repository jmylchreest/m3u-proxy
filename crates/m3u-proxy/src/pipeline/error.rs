//! Pipeline Error Types
//!
//! Unified error handling for the pipeline system, replacing the previous
//! Box<dyn std::error::Error> pattern with more structured error types.

use std::fmt;

/// Main error type for pipeline operations
#[derive(Debug)]
pub enum PipelineError {
    /// Database operation failed
    Database(sqlx::Error),
    
    /// File system operation failed
    FileSystem(std::io::Error),
    
    /// Stage execution failed
    StageExecution { 
        stage: String, 
        message: String,
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },
    
    /// Configuration error
    Configuration(String),
    
    /// Progress tracking error
    Progress(String),
    
    /// Resource not found
    NotFound(String),
    
    /// Invalid input or state
    InvalidInput(String),
    
    /// External service error (logo downloads, etc.)
    ExternalService {
        service: String,
        message: String,
    },
    
    /// Serialization/deserialization error
    Serialization(String),
    
    /// Generic error for compatibility
    Generic(String),
}

impl fmt::Display for PipelineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PipelineError::Database(e) => write!(f, "Database error: {e}"),
            PipelineError::FileSystem(e) => write!(f, "File system error: {e}"),
            PipelineError::StageExecution { stage, message, .. } => {
                write!(f, "Stage '{stage}' execution failed: {message}")
            }
            PipelineError::Configuration(msg) => write!(f, "Configuration error: {msg}"),
            PipelineError::Progress(msg) => write!(f, "Progress tracking error: {msg}"),
            PipelineError::NotFound(msg) => write!(f, "Resource not found: {msg}"),
            PipelineError::InvalidInput(msg) => write!(f, "Invalid input: {msg}"),
            PipelineError::ExternalService { service, message } => {
                write!(f, "External service '{service}' error: {message}")
            }
            PipelineError::Serialization(msg) => write!(f, "Serialization error: {msg}"),
            PipelineError::Generic(msg) => write!(f, "Pipeline error: {msg}"),
        }
    }
}

impl std::error::Error for PipelineError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            PipelineError::Database(e) => Some(e),
            PipelineError::FileSystem(e) => Some(e),
            PipelineError::StageExecution { source: Some(e), .. } => Some(e.as_ref()),
            _ => None,
        }
    }
}

// Conversion implementations for common error types
impl From<sqlx::Error> for PipelineError {
    fn from(error: sqlx::Error) -> Self {
        PipelineError::Database(error)
    }
}

impl From<std::io::Error> for PipelineError {
    fn from(error: std::io::Error) -> Self {
        PipelineError::FileSystem(error)
    }
}

impl From<serde_json::Error> for PipelineError {
    fn from(error: serde_json::Error) -> Self {
        PipelineError::Serialization(error.to_string())
    }
}

impl From<anyhow::Error> for PipelineError {
    fn from(error: anyhow::Error) -> Self {
        PipelineError::Generic(error.to_string())
    }
}

// Note: sandboxed_file_manager error conversion would go here when needed

// Conversion from Box<dyn std::error::Error> for backwards compatibility during migration
impl From<Box<dyn std::error::Error + Send + Sync>> for PipelineError {
    fn from(error: Box<dyn std::error::Error + Send + Sync>) -> Self {
        PipelineError::Generic(error.to_string())
    }
}

impl From<Box<dyn std::error::Error>> for PipelineError {
    fn from(error: Box<dyn std::error::Error>) -> Self {
        PipelineError::Generic(error.to_string())
    }
}

// Helper methods for creating specific error types
impl PipelineError {
    /// Create a stage execution error
    pub fn stage_error(stage: &str, message: impl Into<String>) -> Self {
        PipelineError::StageExecution {
            stage: stage.to_string(),
            message: message.into(),
            source: None,
        }
    }
    
    /// Create a stage execution error with source
    pub fn stage_error_with_source(
        stage: &str, 
        message: impl Into<String>,
        source: Box<dyn std::error::Error + Send + Sync>
    ) -> Self {
        PipelineError::StageExecution {
            stage: stage.to_string(),
            message: message.into(),
            source: Some(source),
        }
    }
    
    /// Create a configuration error
    pub fn config_error(message: impl Into<String>) -> Self {
        PipelineError::Configuration(message.into())
    }
    
    /// Create an external service error
    pub fn external_service_error(service: &str, message: impl Into<String>) -> Self {
        PipelineError::ExternalService {
            service: service.to_string(),
            message: message.into(),
        }
    }
}