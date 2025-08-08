//! Database retry utilities for handling transient failures
//!
//! This module provides retry mechanisms specifically designed for database operations,
//! with exponential backoff and configurable retry policies.

use std::time::Duration;
use tokio::time::sleep;
use tracing::{debug, warn};
use crate::errors::{RepositoryError, RepositoryResult};

/// Configuration for database retry behavior
#[derive(Debug, Clone)]
pub struct RetryConfig {
    /// Maximum number of retry attempts
    pub max_attempts: u32,
    /// Initial delay before first retry
    pub initial_delay: Duration,
    /// Maximum delay between retries
    pub max_delay: Duration,
    /// Multiplier for exponential backoff
    pub backoff_multiplier: f64,
    /// Whether to add jitter to delays
    pub jitter: bool,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            initial_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(2),
            backoff_multiplier: 2.0,
            jitter: true,
        }
    }
}

impl RetryConfig {
    /// Create a conservative retry policy for read operations
    pub fn for_reads() -> Self {
        Self {
            max_attempts: 3,
            initial_delay: Duration::from_millis(50),
            max_delay: Duration::from_millis(500),
            backoff_multiplier: 1.5,
            jitter: true,
        }
    }

    /// Create a more aggressive retry policy for write operations
    pub fn for_writes() -> Self {
        Self {
            max_attempts: 5,
            initial_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(3),
            backoff_multiplier: 2.0,
            jitter: true,
        }
    }

    /// Create a minimal retry policy for critical operations
    pub fn for_critical() -> Self {
        Self {
            max_attempts: 7,
            initial_delay: Duration::from_millis(200),
            max_delay: Duration::from_secs(5),
            backoff_multiplier: 2.0,
            jitter: false, // More predictable for critical operations
        }
    }
}

/// Execute a database operation with retry logic
///
/// # Arguments
///
/// * `config` - Retry configuration
/// * `operation` - Async closure that performs the database operation
/// * `operation_name` - Human-readable name for logging
///
/// # Returns
///
/// The result of the successful operation, or the last error if all retries failed
pub async fn with_retry<T, F, Fut>(
    config: &RetryConfig,
    mut operation: F,
    operation_name: &str,
) -> RepositoryResult<T>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = RepositoryResult<T>>,
{
    let mut last_error = None;
    
    for attempt in 1..=config.max_attempts {
        match operation().await {
            Ok(result) => {
                if attempt > 1 {
                    debug!(
                        "Database operation '{}' succeeded on attempt {}/{}",
                        operation_name, attempt, config.max_attempts
                    );
                }
                return Ok(result);
            }
            Err(err) => {
                let should_retry = is_retryable_error(&err);
                
                if !should_retry {
                    debug!(
                        "Database operation '{}' failed with non-retryable error: {}",
                        operation_name, err
                    );
                    return Err(err);
                }
                
                last_error = Some(err);
                
                if attempt < config.max_attempts {
                    let delay = calculate_delay(config, attempt);
                    
                    warn!(
                        "Database operation '{}' failed on attempt {}/{}, retrying in {:?}: {}",
                        operation_name, attempt, config.max_attempts, delay, last_error.as_ref().unwrap()
                    );
                    
                    sleep(delay).await;
                } else {
                    warn!(
                        "Database operation '{}' failed after {} attempts: {}",
                        operation_name, config.max_attempts, last_error.as_ref().unwrap()
                    );
                }
            }
        }
    }
    
    Err(last_error.unwrap())
}

/// Determine if an error is worth retrying
fn is_retryable_error(error: &RepositoryError) -> bool {
    match error {
        RepositoryError::Database(sqlx_error) => {
            match sqlx_error {
                // SQLite database is locked
                sqlx::Error::Database(db_err) if db_err.code() == Some("5".into()) => true,
                // SQLite database is busy
                sqlx::Error::Database(db_err) if db_err.code() == Some("SQLITE_BUSY".into()) => true,
                // Connection pool timeout
                sqlx::Error::PoolTimedOut => true,
                // Connection closed unexpectedly
                sqlx::Error::PoolClosed => true,
                // Other database errors are generally not retryable
                _ => {
                    // Check error message for common retryable patterns
                    let error_msg = format!("{}", sqlx_error).to_lowercase();
                    error_msg.contains("database is locked") ||
                    error_msg.contains("database is busy") ||
                    error_msg.contains("connection reset") ||
                    error_msg.contains("timeout")
                }
            }
        },
        RepositoryError::ConnectionFailed { .. } => true,
        RepositoryError::QueryFailed { message, .. } => {
            let msg = message.to_lowercase();
            msg.contains("locked") || msg.contains("busy") || msg.contains("timeout")
        },
        // Other repository errors are typically not retryable
        _ => false,
    }
}

/// Calculate delay with exponential backoff and optional jitter
fn calculate_delay(config: &RetryConfig, attempt: u32) -> Duration {
    let exponential_delay = config.initial_delay.as_millis() as f64 
        * config.backoff_multiplier.powi((attempt - 1) as i32);
    
    let delay_ms = exponential_delay.min(config.max_delay.as_millis() as f64) as u64;
    
    let final_delay = if config.jitter {
        // Add up to 25% jitter to prevent thundering herd
        let jitter_range = (delay_ms / 4).max(10); // At least 10ms jitter
        let jitter = fastrand::u64(0..=jitter_range);
        delay_ms + jitter
    } else {
        delay_ms
    };
    
    Duration::from_millis(final_delay)
}

// Tests are disabled due to SQLx version compatibility issues with mock DatabaseError trait
// The retry functionality is tested through the retry wrapper integration tests.