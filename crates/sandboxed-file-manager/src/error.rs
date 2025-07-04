//! Error types for the sandboxed file manager.

use std::path::PathBuf;

/// Result type for sandboxed file operations.
pub type Result<T> = std::result::Result<T, SandboxedFileError>;

/// Errors that can occur during sandboxed file operations.
#[derive(Debug, thiserror::Error)]
pub enum SandboxedFileError {
    /// I/O operation failed
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Path validation failed - potential security issue
    #[error("Path validation failed: {path:?} - {reason}")]
    PathValidation { path: PathBuf, reason: String },

    /// File not found or expired
    #[error("File not found: {id}")]
    FileNotFound { id: String },

    /// Unsupported content type
    #[error("Unsupported content type: {content_type}")]
    UnsupportedContentType { content_type: String },

    /// Cleanup operation failed
    #[error("Cleanup failed: {reason}")]
    CleanupFailed { reason: String },

    /// Directory creation failed
    #[error("Failed to create directory: {path:?} - {source}")]
    DirectoryCreation {
        path: PathBuf,
        source: std::io::Error,
    },

    /// Permissions error
    #[error("Permission denied: {operation} on {path:?}")]
    Permission { operation: String, path: PathBuf },

    /// Configuration error
    #[error("Configuration error: {message}")]
    Configuration { message: String },
}
