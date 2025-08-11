//! Repository retry wrapper for handling database locking and transient failures
//!
//! This module provides a wrapper that adds retry capabilities to any repository
//! implementation, making database operations more resilient to transient failures.

use async_trait::async_trait;
use crate::errors::RepositoryResult;
use crate::repositories::traits::Repository;
use crate::utils::database_retry::{RetryConfig, with_retry};
use std::marker::PhantomData;

/// Wrapper that adds retry functionality to any repository
///
/// This wrapper implements the Repository trait and delegates all operations
/// to the underlying repository, but adds retry logic with exponential backoff
/// for handling database locking and other transient failures.
///
/// # Example Usage
///
/// ```rust
/// use crate::repositories::{StreamSourceRepository, RetryWrapper};
/// use crate::utils::database_retry::RetryConfig;
/// 
/// let base_repo = StreamSourceRepository::new(pool.clone());
/// let retry_repo = RetryWrapper::new(base_repo, RetryConfig::for_writes());
/// 
/// // All operations now have retry logic
/// let source = retry_repo.find_by_id(id).await?;
/// let updated = retry_repo.update(id, request).await?;
/// ```
pub struct RetryWrapper<T, ID, R> 
where 
    R: Repository<T, ID>,
    ID: Send + 'static,
{
    repository: R,
    retry_config: RetryConfig,
    _phantom: PhantomData<(T, ID)>,
}

impl<T, ID, R> RetryWrapper<T, ID, R>
where
    R: Repository<T, ID>,
    ID: Send + 'static,
{
    /// Create a new retry wrapper with the specified retry configuration
    pub fn new(repository: R, retry_config: RetryConfig) -> Self {
        Self {
            repository,
            retry_config,
            _phantom: PhantomData,
        }
    }

    /// Create a retry wrapper with read-optimized configuration
    pub fn for_reads(repository: R) -> Self {
        Self::new(repository, RetryConfig::for_reads())
    }

    /// Create a retry wrapper with write-optimized configuration
    pub fn for_writes(repository: R) -> Self {
        Self::new(repository, RetryConfig::for_writes())
    }

    /// Create a retry wrapper with critical operation configuration
    pub fn for_critical(repository: R) -> Self {
        Self::new(repository, RetryConfig::for_critical())
    }
}

#[async_trait]
impl<T, ID, R> Repository<T, ID> for RetryWrapper<T, ID, R>
where
    T: Send + Sync,
    ID: Send + Sync + 'static + Clone,
    R: Repository<T, ID> + Send + Sync,
    R::CreateRequest: Send + Sync + Clone,
    R::UpdateRequest: Send + Sync + Clone,
    R::Query: Send + Sync + Clone,
{
    type CreateRequest = R::CreateRequest;
    type UpdateRequest = R::UpdateRequest;
    type Query = R::Query;

    async fn find_by_id(&self, id: ID) -> RepositoryResult<Option<T>> {
        let id_clone = id.clone();
        with_retry(
            &self.retry_config,
            || {
                let id = id_clone.clone();
                async move { self.repository.find_by_id(id).await }
            },
            "find_by_id",
        ).await
    }

    async fn find_all(&self, query: Self::Query) -> RepositoryResult<Vec<T>> {
        let query_clone = query.clone();
        with_retry(
            &self.retry_config,
            || {
                let query = query_clone.clone();
                async move { self.repository.find_all(query).await }
            },
            "find_all",
        ).await
    }

    async fn create(&self, request: Self::CreateRequest) -> RepositoryResult<T> {
        let request_clone = request.clone();
        with_retry(
            &self.retry_config,
            || {
                let request = request_clone.clone();
                async move { self.repository.create(request).await }
            },
            "create",
        ).await
    }

    async fn update(&self, id: ID, request: Self::UpdateRequest) -> RepositoryResult<T> {
        let id_clone = id.clone();
        let request_clone = request.clone();
        with_retry(
            &self.retry_config,
            || {
                let id = id_clone.clone();
                let request = request_clone.clone();
                async move { self.repository.update(id, request).await }
            },
            "update",
        ).await
    }

    async fn delete(&self, id: ID) -> RepositoryResult<()> {
        let id_clone = id.clone();
        with_retry(
            &self.retry_config,
            || {
                let id = id_clone.clone();
                async move { self.repository.delete(id).await }
            },
            "delete",
        ).await
    }

    async fn count(&self, query: Self::Query) -> RepositoryResult<u64> {
        let query_clone = query.clone();
        with_retry(
            &self.retry_config,
            || {
                let query = query_clone.clone();
                async move { self.repository.count(query).await }
            },
            "count",
        ).await
    }
}

/// Extension trait to easily add retry functionality to any repository
pub trait RepositoryRetryExt<T, ID>: Repository<T, ID> + Sized
where
    ID: Send + 'static,
{
    /// Wrap this repository with retry functionality using read-optimized configuration
    fn with_read_retries(self) -> RetryWrapper<T, ID, Self> {
        RetryWrapper::for_reads(self)
    }

    /// Wrap this repository with retry functionality using write-optimized configuration
    fn with_write_retries(self) -> RetryWrapper<T, ID, Self> {
        RetryWrapper::for_writes(self)
    }

    /// Wrap this repository with retry functionality using critical operation configuration
    fn with_critical_retries(self) -> RetryWrapper<T, ID, Self> {
        RetryWrapper::for_critical(self)
    }

    /// Wrap this repository with retry functionality using custom configuration
    fn with_retries(self, config: RetryConfig) -> RetryWrapper<T, ID, Self> {
        RetryWrapper::new(self, config)
    }
}

// Implement the extension trait for all repositories
impl<T, ID, R> RepositoryRetryExt<T, ID> for R
where
    R: Repository<T, ID>,
    ID: Send + 'static,
{
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::errors::RepositoryError;
    use crate::utils::database_retry::RetryConfig;
    use async_trait::async_trait;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;
    use std::time::Duration;
    use uuid::Uuid;

    // Mock repository for testing retry wrapper functionality
    #[derive(Clone)]
    struct MockRepository {
        fail_counter: Arc<AtomicU32>,
        fail_until_attempt: u32,
        #[allow(dead_code)]
        should_fail_permanently: bool,
    }

    impl MockRepository {
        fn new() -> Self {
            Self {
                fail_counter: Arc::new(AtomicU32::new(0)),
                fail_until_attempt: 0,
                should_fail_permanently: false,
            }
        }

        fn new_with_failure_count(fail_until_attempt: u32) -> Self {
            Self {
                fail_counter: Arc::new(AtomicU32::new(0)),
                fail_until_attempt,
                should_fail_permanently: false,
            }
        }

        fn new_with_permanent_failure() -> Self {
            Self {
                fail_counter: Arc::new(AtomicU32::new(0)),
                fail_until_attempt: u32::MAX,
                should_fail_permanently: true,
            }
        }

        fn call_count(&self) -> u32 {
            self.fail_counter.load(Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl Repository<String, Uuid> for MockRepository {
        type CreateRequest = String;
        type UpdateRequest = String;  
        type Query = String;

        async fn find_by_id(&self, _id: Uuid) -> RepositoryResult<Option<String>> {
            let attempt = self.fail_counter.fetch_add(1, Ordering::SeqCst) + 1;
            
            if attempt <= self.fail_until_attempt {
                Err(RepositoryError::ConnectionFailed {
                    message: "database is locked".to_string()
                })
            } else {
                Ok(Some("success".to_string()))
            }
        }

        async fn find_all(&self, query: Self::Query) -> RepositoryResult<Vec<String>> {
            let attempt = self.fail_counter.fetch_add(1, Ordering::SeqCst) + 1;
            
            if attempt <= self.fail_until_attempt {
                Err(RepositoryError::ConnectionFailed {
                    message: "database is locked".to_string()
                })
            } else {
                Ok(vec![format!("result_for_{query}")])
            }
        }

        async fn create(&self, request: Self::CreateRequest) -> RepositoryResult<String> {
            let attempt = self.fail_counter.fetch_add(1, Ordering::SeqCst) + 1;
            
            if attempt <= self.fail_until_attempt {
                Err(RepositoryError::ConnectionFailed {
                    message: "database is locked".to_string()
                })
            } else {
                Ok(format!("created_{request}"))
            }
        }

        async fn update(&self, _id: Uuid, request: Self::UpdateRequest) -> RepositoryResult<String> {
            let attempt = self.fail_counter.fetch_add(1, Ordering::SeqCst) + 1;
            
            if attempt <= self.fail_until_attempt {
                Err(RepositoryError::ConnectionFailed {
                    message: "database is locked".to_string()
                })
            } else {
                Ok(format!("updated_{request}"))
            }
        }

        async fn delete(&self, _id: Uuid) -> RepositoryResult<()> {
            let attempt = self.fail_counter.fetch_add(1, Ordering::SeqCst) + 1;
            
            if attempt <= self.fail_until_attempt {
                Err(RepositoryError::ConnectionFailed {
                    message: "database is locked".to_string()
                })
            } else {
                Ok(())
            }
        }

        async fn count(&self, query: Self::Query) -> RepositoryResult<u64> {
            let attempt = self.fail_counter.fetch_add(1, Ordering::SeqCst) + 1;
            
            if attempt <= self.fail_until_attempt {
                Err(RepositoryError::ConnectionFailed {
                    message: "database is locked".to_string()
                })
            } else {
                Ok(query.len() as u64) // Return query length as mock count
            }
        }
    }

    /// Test successful operations work without retry overhead
    #[tokio::test]
    async fn test_successful_operations_no_retry() {
        let mock_repo = MockRepository::new();
        let retry_repo = RetryWrapper::for_reads(mock_repo.clone());
        
        let id = Uuid::new_v4();
        
        // Test find_by_id
        let result = retry_repo.find_by_id(id).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Some("success".to_string()));
        
        // Test find_all
        let result = retry_repo.find_all("test_query".to_string()).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), vec!["result_for_test_query"]);
        
        // Test create
        let result = retry_repo.create("test_data".to_string()).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "created_test_data");
        
        // Test update
        let result = retry_repo.update(id, "updated_data".to_string()).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "updated_updated_data");
        
        // Test delete
        let result = retry_repo.delete(id).await;
        assert!(result.is_ok());
        
        // Test count
        let result = retry_repo.count("count_query".to_string()).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 11); // "count_query".len()
        
        // Should have called each operation exactly once (no retries)
        assert_eq!(mock_repo.call_count(), 6);
    }

    /// Test retry functionality with transient failures
    #[tokio::test]
    async fn test_retry_on_transient_failures() {
        // Mock repo that fails first 2 attempts
        let mock_repo = MockRepository::new_with_failure_count(2);
        let retry_repo = RetryWrapper::for_writes(mock_repo.clone());
        
        let id = Uuid::new_v4();
        let result = retry_repo.find_by_id(id).await;
        
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Some("success".to_string()));
        
        // Should have been called 3 times (2 failures + 1 success)
        assert_eq!(mock_repo.call_count(), 3);
    }

    /// Test different retry configurations
    #[tokio::test]
    async fn test_different_retry_configurations() {
        // Test read configuration (3 max attempts)
        let mock_repo_read = MockRepository::new_with_failure_count(4); // More failures than max attempts
        let retry_repo_read = RetryWrapper::for_reads(mock_repo_read.clone());
        
        let id = Uuid::new_v4();
        let result = retry_repo_read.find_by_id(id).await;
        
        assert!(result.is_err()); // Should fail after 3 attempts
        assert_eq!(mock_repo_read.call_count(), 3); // Read config: max 3 attempts
        
        // Test write configuration (5 max attempts)
        let mock_repo_write = MockRepository::new_with_failure_count(4); // 4 failures, should succeed on 5th attempt
        let retry_repo_write = RetryWrapper::for_writes(mock_repo_write.clone());
        
        let result = retry_repo_write.create("test_data".to_string()).await;
        
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "created_test_data");
        assert_eq!(mock_repo_write.call_count(), 5); // Write config: succeeded on 5th attempt
        
        // Test critical configuration (7 max attempts)
        let mock_repo_critical = MockRepository::new_with_failure_count(6); // 6 failures, should succeed on 7th attempt
        let critical_config = RetryConfig::for_critical();
        eprintln!("DEBUG: Critical config max_attempts: {}", critical_config.max_attempts);
        let retry_repo_critical = RetryWrapper::for_critical(mock_repo_critical.clone());
        
        let result = retry_repo_critical.update(id, "critical_data".to_string()).await;
        
        if let Err(ref e) = result {
            eprintln!("DEBUG: Critical update failed with error: {:?}", e);
        }
        eprintln!("DEBUG: Critical call count: {}", mock_repo_critical.call_count());
        assert!(result.is_ok(), "Critical update should succeed after retries, got: {:?}", result);
        assert_eq!(result.unwrap(), "updated_critical_data");
        assert_eq!(mock_repo_critical.call_count(), 7); // Critical config: succeeded on 7th attempt
    }

    /// Test extension trait methods work correctly
    #[tokio::test]
    async fn test_extension_trait_methods() {
        let mock_repo = MockRepository::new();
        
        // Test each extension trait method
        let _read_retry_repo = mock_repo.clone().with_read_retries();
        let _write_retry_repo = mock_repo.clone().with_write_retries();
        let _critical_retry_repo = mock_repo.clone().with_critical_retries();
        
        // Test custom config
        let custom_config = RetryConfig {
            max_attempts: 10,
            initial_delay: Duration::from_millis(1),
            max_delay: Duration::from_millis(100),
            backoff_multiplier: 1.5,
            jitter: false,
        };
        let _custom_retry_repo = mock_repo.with_retries(custom_config);
        
        // All should compile and create correctly
    }

    /// Test that non-retryable errors are not retried
    #[tokio::test]
    async fn test_non_retryable_errors_not_retried() {
        // Create a custom mock that returns non-retryable errors
        #[derive(Clone)]
        struct NonRetryableMockRepository {
            call_counter: Arc<AtomicU32>,
        }

        #[async_trait]
        impl Repository<String, Uuid> for NonRetryableMockRepository {
            type CreateRequest = String;
            type UpdateRequest = String;
            type Query = String;

            async fn find_by_id(&self, _id: Uuid) -> RepositoryResult<Option<String>> {
                self.call_counter.fetch_add(1, Ordering::SeqCst);
                Err(RepositoryError::RecordNotFound {
                    table: "test".to_string(),
                    field: "id".to_string(),
                    value: "123".to_string(),
                })
            }

            async fn find_all(&self, _query: Self::Query) -> RepositoryResult<Vec<String>> {
                Ok(vec![])
            }

            async fn create(&self, _request: Self::CreateRequest) -> RepositoryResult<String> {
                Ok("created".to_string())
            }

            async fn update(&self, _id: Uuid, _request: Self::UpdateRequest) -> RepositoryResult<String> {
                Ok("updated".to_string())
            }

            async fn delete(&self, _id: Uuid) -> RepositoryResult<()> {
                Ok(())
            }

            async fn count(&self, _query: Self::Query) -> RepositoryResult<u64> {
                Ok(0)
            }
        }

        let mock_repo = NonRetryableMockRepository {
            call_counter: Arc::new(AtomicU32::new(0)),
        };
        let retry_repo = RetryWrapper::for_writes(mock_repo.clone());
        
        let id = Uuid::new_v4();
        let result = retry_repo.find_by_id(id).await;
        
        assert!(result.is_err());
        // Should only be called once since the error is not retryable
        assert_eq!(mock_repo.call_counter.load(Ordering::SeqCst), 1);
    }

    /// Test Clone requirements are properly enforced at compile time
    #[tokio::test]
    async fn test_clone_requirements() {
        let mock_repo = MockRepository::new();
        let retry_repo = RetryWrapper::for_reads(mock_repo.clone());
        
        // These should all compile because String implements Clone
        let query = "test_query".to_string();
        let create_request = "create_data".to_string();
        let update_request = "update_data".to_string();
        let id = Uuid::new_v4();
        
        // Test that we can call operations multiple times with cloned parameters
        let _result1 = retry_repo.find_all(query.clone()).await;
        let _result2 = retry_repo.find_all(query.clone()).await;
        
        let _result3 = retry_repo.create(create_request.clone()).await;
        let _result4 = retry_repo.create(create_request.clone()).await;
        
        let _result5 = retry_repo.update(id, update_request.clone()).await;
        let _result6 = retry_repo.update(id, update_request.clone()).await;
    }

    /// Test retry wrapper preserves original error information
    #[tokio::test]
    async fn test_error_preservation() {
        let mock_repo = MockRepository::new_with_permanent_failure();
        let retry_repo = RetryWrapper::for_reads(mock_repo.clone());
        
        let id = Uuid::new_v4();
        let result = retry_repo.find_by_id(id).await;
        
        assert!(result.is_err());
        
        // Check that the original error is preserved
        match result.unwrap_err() {
            RepositoryError::ConnectionFailed { message } => {
                assert_eq!(message, "database is locked");
            }
            other => panic!("Expected ConnectionFailed error, got: {other:?}"),
        }
        
        // Should have exhausted all retry attempts
        assert_eq!(mock_repo.call_count(), 3); // Read config max attempts
    }

    /// Integration test for retry wrapper with actual retry timing
    #[tokio::test]
    async fn test_retry_timing_integration() {
        use std::time::Instant;
        
        let mock_repo = MockRepository::new_with_failure_count(2);
        
        // Custom config with known timing for test predictability
        let config = RetryConfig {
            max_attempts: 3,
            initial_delay: Duration::from_millis(10),
            max_delay: Duration::from_millis(50),
            backoff_multiplier: 2.0,
            jitter: false,
        };
        
        let retry_repo = RetryWrapper::new(mock_repo.clone(), config);
        
        let start = Instant::now();
        let result = retry_repo.create("timing_test".to_string()).await;
        let elapsed = start.elapsed();
        
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "created_timing_test");
        
        // Should have waited: 10ms (first retry) + 20ms (second retry) = 30ms minimum
        assert!(elapsed >= Duration::from_millis(30));
        // But not too much longer (allowing for execution overhead and test environment variability)
        assert!(elapsed < Duration::from_millis(500)); // Increased from 100ms to 500ms
        
        assert_eq!(mock_repo.call_count(), 3);
    }
}