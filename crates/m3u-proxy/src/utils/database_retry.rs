//! Database retry utilities for handling transient failures
//!
//! This module provides retry mechanisms specifically designed for database operations,
//! with exponential backoff and configurable retry policies.

use std::time::Duration;
use tokio::time::sleep;
use tracing::{debug, warn};
use crate::errors::{RepositoryError, RepositoryResult};
use crate::utils::jitter::generate_jitter_percent;

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
        RepositoryError::Database(sea_orm_error) => {
            match sea_orm_error {
                // Connection-related errors that are retryable
                sea_orm::DbErr::ConnectionAcquire(_) => true,
                sea_orm::DbErr::Conn(_) => true,
                sea_orm::DbErr::Exec(sea_orm::RuntimeErr::SqlxError(sqlx_err)) => {
                    // Handle SQLx errors within SeaORM
                    let error_msg = format!("{sqlx_err}").to_lowercase();
                    error_msg.contains("database is locked") ||
                    error_msg.contains("database is busy") ||
                    error_msg.contains("connection reset") ||
                    error_msg.contains("timeout") ||
                    error_msg.contains("pool timed out") ||
                    error_msg.contains("pool closed")
                }
                // Other SeaORM errors - check message for retryable patterns
                _ => {
                    let error_msg = format!("{sea_orm_error}").to_lowercase();
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
        let jitter = generate_jitter_percent(delay_ms, 25);
        delay_ms + jitter
    } else {
        delay_ms
    };
    
    Duration::from_millis(final_delay)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;
    use std::time::Instant;

    /// Test that successful operations complete without retry
    #[tokio::test]
    async fn test_successful_operation_no_retry() {
        let config = RetryConfig::default();
        let counter = Arc::new(AtomicU32::new(0));
        let counter_clone = counter.clone();
        
        let result = with_retry(
            &config,
            || {
                let counter = counter_clone.clone();
                async move {
                    counter.fetch_add(1, Ordering::SeqCst);
                    Ok::<i32, RepositoryError>(42)
                }
            },
            "test_operation"
        ).await;
        
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 42);
        assert_eq!(counter.load(Ordering::SeqCst), 1); // Only called once
    }

    /// Test that non-retryable errors are not retried
    #[tokio::test]
    async fn test_non_retryable_error_immediate_failure() {
        let config = RetryConfig::default();
        let counter = Arc::new(AtomicU32::new(0));
        let counter_clone = counter.clone();
        
        let result: Result<(), _> = with_retry(
            &config,
            || {
                let counter = counter_clone.clone();
                async move {
                    counter.fetch_add(1, Ordering::SeqCst);
                    Err(RepositoryError::SerializationFailed(
                        serde_json::Error::io(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            "test error"
                        ))
                    ))
                }
            },
            "test_non_retryable"
        ).await;
        
        assert!(result.is_err());
        assert_eq!(counter.load(Ordering::SeqCst), 1); // Only called once, no retries
    }

    /// Test retryable error scenarios with connection failures
    #[tokio::test]
    async fn test_retryable_connection_error() {
        let config = RetryConfig {
            max_attempts: 3,
            initial_delay: Duration::from_millis(1), // Fast for testing
            max_delay: Duration::from_millis(10),
            backoff_multiplier: 2.0,
            jitter: false, // Predictable timing for tests
        };
        
        let counter = Arc::new(AtomicU32::new(0));
        let counter_clone = counter.clone();
        
        let result = with_retry(
            &config,
            || {
                let counter = counter_clone.clone();
                async move {
                    let count = counter.fetch_add(1, Ordering::SeqCst);
                    if count < 2 {
                        // Fail first 2 attempts with retryable error
                        Err(RepositoryError::ConnectionFailed {
                            message: "database is locked".to_string()
                        })
                    } else {
                        Ok(42)
                    }
                }
            },
            "test_retry"
        ).await;
        
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 42);
        assert_eq!(counter.load(Ordering::SeqCst), 3); // Called 3 times
    }

    /// Test max attempts are respected
    #[tokio::test]
    async fn test_max_attempts_respected() {
        let config = RetryConfig {
            max_attempts: 2,
            initial_delay: Duration::from_millis(1),
            max_delay: Duration::from_millis(10),
            backoff_multiplier: 2.0,
            jitter: false,
        };
        
        let counter = Arc::new(AtomicU32::new(0));
        let counter_clone = counter.clone();
        
        let result: Result<(), _> = with_retry(
            &config,
            || {
                let counter = counter_clone.clone();
                async move {
                    counter.fetch_add(1, Ordering::SeqCst);
                    Err(RepositoryError::ConnectionFailed {
                        message: "always fails".to_string()
                    })
                }
            },
            "test_max_attempts"
        ).await;
        
        assert!(result.is_err());
        assert_eq!(counter.load(Ordering::SeqCst), 2); // Called exactly max_attempts times
    }

    /// Test retry configuration factory methods
    #[test]
    fn test_retry_config_factory_methods() {
        let read_config = RetryConfig::for_reads();
        assert_eq!(read_config.max_attempts, 3);
        assert_eq!(read_config.initial_delay, Duration::from_millis(50));
        assert_eq!(read_config.max_delay, Duration::from_millis(500));
        assert_eq!(read_config.backoff_multiplier, 1.5);
        assert!(read_config.jitter);

        let write_config = RetryConfig::for_writes();
        assert_eq!(write_config.max_attempts, 5);
        assert_eq!(write_config.initial_delay, Duration::from_millis(100));
        assert_eq!(write_config.max_delay, Duration::from_secs(3));
        assert_eq!(write_config.backoff_multiplier, 2.0);
        assert!(write_config.jitter);

        let critical_config = RetryConfig::for_critical();
        assert_eq!(critical_config.max_attempts, 7);
        assert_eq!(critical_config.initial_delay, Duration::from_millis(200));
        assert_eq!(critical_config.max_delay, Duration::from_secs(5));
        assert_eq!(critical_config.backoff_multiplier, 2.0);
        assert!(!critical_config.jitter); // No jitter for critical ops
    }

    /// Test exponential backoff calculation
    #[test]
    fn test_calculate_delay_exponential_backoff() {
        let config = RetryConfig {
            max_attempts: 5,
            initial_delay: Duration::from_millis(100),
            max_delay: Duration::from_millis(1000),
            backoff_multiplier: 2.0,
            jitter: false, // No jitter for predictable testing
        };

        let delay1 = calculate_delay(&config, 1);
        let delay2 = calculate_delay(&config, 2);
        let delay3 = calculate_delay(&config, 3);

        // First attempt: 100ms * 2^0 = 100ms
        assert_eq!(delay1, Duration::from_millis(100));
        
        // Second attempt: 100ms * 2^1 = 200ms
        assert_eq!(delay2, Duration::from_millis(200));
        
        // Third attempt: 100ms * 2^2 = 400ms
        assert_eq!(delay3, Duration::from_millis(400));
    }

    /// Test delay calculation with max_delay cap
    #[test]
    fn test_calculate_delay_max_cap() {
        let config = RetryConfig {
            max_attempts: 10,
            initial_delay: Duration::from_millis(100),
            max_delay: Duration::from_millis(500),
            backoff_multiplier: 2.0,
            jitter: false,
        };

        let delay5 = calculate_delay(&config, 5); // Would be 100 * 2^4 = 1600ms
        let delay6 = calculate_delay(&config, 6); // Would be 100 * 2^5 = 3200ms
        
        // Both should be capped at max_delay
        assert_eq!(delay5, Duration::from_millis(500));
        assert_eq!(delay6, Duration::from_millis(500));
    }

    /// Test jitter adds randomness within expected range
    #[test]
    fn test_calculate_delay_with_jitter() {
        let config = RetryConfig {
            max_attempts: 3,
            initial_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(1),
            backoff_multiplier: 2.0,
            jitter: true,
        };

        // Run multiple times to test jitter variation
        let mut delays = Vec::new();
        for _ in 0..10 {
            delays.push(calculate_delay(&config, 1));
        }

        // All delays should be >= base delay (100ms)
        assert!(delays.iter().all(|&d| d >= Duration::from_millis(100)));
        
        // All delays should be <= base delay + 25% jitter
        let max_expected = Duration::from_millis(125); // 100 + 25
        assert!(delays.iter().all(|&d| d <= max_expected));

        // With jitter, we should see some variation (not all identical)
        let all_same = delays.windows(2).all(|w| w[0] == w[1]);
        assert!(!all_same, "Jitter should produce different delays");
    }

    /// Test is_retryable_error function with various error types
    #[test]
    fn test_is_retryable_error_classification() {
        // Connection failures should be retryable
        let conn_error = RepositoryError::ConnectionFailed {
            message: "database is locked".to_string()
        };
        assert!(is_retryable_error(&conn_error));

        // Query failures with retryable messages should be retryable
        let query_error_locked = RepositoryError::QueryFailed {
            query: "SELECT * FROM test".to_string(),
            message: "database is locked".to_string(),
        };
        assert!(is_retryable_error(&query_error_locked));

        let query_error_busy = RepositoryError::QueryFailed {
            query: "SELECT * FROM test".to_string(),
            message: "database is busy".to_string(),
        };
        assert!(is_retryable_error(&query_error_busy));

        let query_error_timeout = RepositoryError::QueryFailed {
            query: "SELECT * FROM test".to_string(),
            message: "timeout occurred".to_string(),
        };
        assert!(is_retryable_error(&query_error_timeout));

        // Non-retryable errors
        let serialization_error = RepositoryError::SerializationFailed(
            serde_json::Error::io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "test"
            ))
        );
        assert!(!is_retryable_error(&serialization_error));

        let record_not_found = RepositoryError::RecordNotFound {
            table: "test".to_string(),
            field: "id".to_string(),
            value: "123".to_string(),
        };
        assert!(!is_retryable_error(&record_not_found));
    }

    /// Test timing behavior of retry operations
    #[tokio::test]
    async fn test_retry_timing_behavior() {
        let config = RetryConfig {
            max_attempts: 3,
            initial_delay: Duration::from_millis(50),
            max_delay: Duration::from_millis(200),
            backoff_multiplier: 2.0,
            jitter: false,
        };
        
        let counter = Arc::new(AtomicU32::new(0));
        let counter_clone = counter.clone();
        let start_time = Instant::now();
        
        let result = with_retry(
            &config,
            || {
                let counter = counter_clone.clone();
                async move {
                    let count = counter.fetch_add(1, Ordering::SeqCst);
                    if count < 2 {
                        Err(RepositoryError::ConnectionFailed {
                            message: "temporary failure".to_string()
                        })
                    } else {
                        Ok("success")
                    }
                }
            },
            "test_timing"
        ).await;
        
        let elapsed = start_time.elapsed();
        
        assert!(result.is_ok());
        // Should have waited at least: 50ms + 100ms = 150ms total
        // (first retry delay + second retry delay)
        assert!(elapsed >= Duration::from_millis(150));
        // But not too much longer (allowing for test execution overhead)
        assert!(elapsed < Duration::from_millis(300));
    }

    /// Property-based test for retry configuration validation
    #[cfg(test)]
    mod property_tests {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            #[test]
            fn test_delay_calculation_properties(
                initial_delay_ms in 1u64..1000,
                multiplier in 1.0f64..5.0,
                max_delay_ms in 100u64..10000,
                attempt in 1u32..10
            ) {
                let config = RetryConfig {
                    max_attempts: 10,
                    initial_delay: Duration::from_millis(initial_delay_ms),
                    max_delay: Duration::from_millis(max_delay_ms),
                    backoff_multiplier: multiplier,
                    jitter: false,
                };

                let delay = calculate_delay(&config, attempt);
                
                // Delay should never exceed max_delay
                prop_assert!(delay <= config.max_delay);
                
                // Delay should be at least initial_delay, unless capped by max_delay
                let expected_min_delay = config.initial_delay.min(config.max_delay);
                prop_assert!(delay >= expected_min_delay);
            }
        }
    }
}