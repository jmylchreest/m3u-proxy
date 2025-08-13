//! Centralized error handling for the M3U Proxy application
//!
//! This module provides a comprehensive error handling system that unifies
//! error types across all application layers and provides consistent error
//! reporting and debugging capabilities.
//!
//! # Error Categories
//!
//! - **Database Errors**: SQLite operations, migrations, connection issues
//! - **Repository Errors**: Data access layer failures
//! - **Source Errors**: External stream source connectivity and parsing
//! - **Validation Errors**: Input validation and business rule violations
//! - **Web Errors**: HTTP request/response handling issues
//!
//! # Usage
//!
//! ```rust
//! use m3u_proxy::errors::{AppError, AppResult};
//!
//! async fn example_function() -> AppResult<String> {
//!     // Function can return any error type that converts to AppError
//!     Ok("success".to_string())
//! }
//! ```

pub mod types;

pub use types::*;

/// Convenience type alias for Results using AppError
pub type AppResult<T> = Result<T, AppError>;

/// Convenience type alias for Repository Results
pub type RepositoryResult<T> = Result<T, RepositoryError>;

/// Convenience type alias for Source Results  
pub type SourceResult<T> = Result<T, SourceError>;

/// Convenience type alias for Web Results
pub type WebResult<T> = Result<T, WebError>;