use std::future::Future;
use async_trait::async_trait;
use tracing::warn;

use super::circuit_breaker::{
    CircuitBreaker, CircuitBreakerError, CircuitBreakerResult, 
    CircuitBreakerState, CircuitBreakerStats
};

/// No-Op circuit breaker that always allows operations through
/// WARNING: This is for reference/testing only - provides no protection
#[derive(Debug)]
pub struct NoOpCircuitBreaker;

impl Default for NoOpCircuitBreaker {
    fn default() -> Self {
        Self::new()
    }
}

impl NoOpCircuitBreaker {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl CircuitBreaker for NoOpCircuitBreaker {
    async fn execute<T, F, Fut>(&self, mut operation: F) -> CircuitBreakerResult<T>
    where
        F: FnMut() -> Fut + Send,
        Fut: Future<Output = Result<T, String>> + Send,
        T: Send,
    {
        let start_time = std::time::Instant::now();
        
        // Execute operation without any circuit breaker logic
        match operation().await {
            Ok(result) => CircuitBreakerResult {
                result: Ok(result),
                state: CircuitBreakerState::Closed,
                execution_time: start_time.elapsed(),
            },
            Err(error) => CircuitBreakerResult {
                result: Err(CircuitBreakerError::ServiceError(error)),
                state: CircuitBreakerState::Closed,
                execution_time: start_time.elapsed(),
            },
        }
    }

    async fn state(&self) -> CircuitBreakerState {
        CircuitBreakerState::Closed // Always report as closed
    }

    async fn is_available(&self) -> bool {
        true // Always available
    }

    async fn force_open(&self) {
        warn!("Attempted to force open NoOp circuit breaker - operation ignored");
    }

    async fn force_closed(&self) {
        // Already always closed, no-op
    }

    async fn stats(&self) -> CircuitBreakerStats {
        CircuitBreakerStats {
            total_calls: 0,
            successful_calls: 0,
            failed_calls: 0,
            state: CircuitBreakerState::Closed,
            failure_rate: 0.0,
            last_state_change: None,
        }
    }
}