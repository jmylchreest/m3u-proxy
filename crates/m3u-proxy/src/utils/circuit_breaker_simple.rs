//! Simplified circuit breaker implementation
//!
//! This provides a cleaner alternative to the rssafecircuit adapter
//! with direct generic type support and simpler internal logic.

use std::sync::Arc;
use std::time::Instant;
use async_trait::async_trait;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

use crate::utils::circuit_breaker::{
    CircuitBreaker, CircuitBreakerResult, CircuitBreakerError, CircuitBreakerState, 
    CircuitBreakerStats, CircuitBreakerConfig
};

/// State tracking for the simple circuit breaker
#[derive(Debug, Clone)]
struct SimpleCircuitBreakerState {
    /// Current state of the circuit breaker
    state: CircuitBreakerState,
    /// Number of consecutive failures
    failure_count: u32,
    /// Number of consecutive successes (used in half-open state)  
    success_count: u32,
    /// When the circuit was last opened
    last_opened: Option<Instant>,
    /// Total statistics
    total_calls: u64,
    successful_calls: u64,
    failed_calls: u64,
    /// Last state change timestamp
    last_state_change: Option<Instant>,
}

impl Default for SimpleCircuitBreakerState {
    fn default() -> Self {
        Self {
            state: CircuitBreakerState::Closed,
            failure_count: 0,
            success_count: 0,
            last_opened: None,
            total_calls: 0,
            successful_calls: 0,
            failed_calls: 0,
            last_state_change: Some(Instant::now()),
        }
    }
}

/// Simple circuit breaker implementation with direct generic type support
#[derive(Debug)]
pub struct SimpleCircuitBreaker {
    config: CircuitBreakerConfig,
    state: Arc<RwLock<SimpleCircuitBreakerState>>,
}

impl SimpleCircuitBreaker {
    /// Create a new simple circuit breaker
    pub fn new(config: CircuitBreakerConfig) -> Self {
        info!("Creating SimpleCircuitBreaker with config: {:?}", config);
        Self {
            config,
            state: Arc::new(RwLock::new(SimpleCircuitBreakerState::default())),
        }
    }

    /// Check if we should allow the operation to proceed
    async fn should_allow_request(&self) -> bool {
        let mut state = self.state.write().await;
        
        match state.state {
            CircuitBreakerState::Closed => true,
            CircuitBreakerState::Open => {
                // Check if we should transition to half-open
                if let Some(last_opened) = state.last_opened {
                    if last_opened.elapsed() >= self.config.reset_timeout {
                        info!("Circuit breaker transitioning from Open to HalfOpen");
                        state.state = CircuitBreakerState::HalfOpen;
                        state.success_count = 0;
                        state.last_state_change = Some(Instant::now());
                        true
                    } else {
                        debug!("Circuit breaker still open, blocking request");
                        false
                    }
                } else {
                    false
                }
            }
            CircuitBreakerState::HalfOpen => true,
        }
    }

    /// Record the result of an operation and update state
    async fn record_result(&self, success: bool) {
        let mut state = self.state.write().await;
        
        state.total_calls += 1;
        
        if success {
            state.successful_calls += 1;
            state.failure_count = 0;
            state.success_count += 1;
            
            // Check if we should transition from half-open to closed
            if state.state == CircuitBreakerState::HalfOpen 
                && state.success_count >= self.config.success_threshold {
                info!("Circuit breaker transitioning from HalfOpen to Closed");
                state.state = CircuitBreakerState::Closed;
                state.success_count = 0;
                state.last_state_change = Some(Instant::now());
            }
        } else {
            state.failed_calls += 1;
            state.success_count = 0;
            state.failure_count += 1;
            
            // Check if we should open the circuit
            if state.failure_count >= self.config.failure_threshold {
                match state.state {
                    CircuitBreakerState::Closed => {
                        warn!("Circuit breaker opening due to {} consecutive failures", state.failure_count);
                        state.state = CircuitBreakerState::Open;
                        state.last_opened = Some(Instant::now());
                        state.last_state_change = Some(Instant::now());
                    }
                    CircuitBreakerState::HalfOpen => {
                        warn!("Circuit breaker returning to Open state from HalfOpen due to failure");
                        state.state = CircuitBreakerState::Open;
                        state.last_opened = Some(Instant::now());
                        state.last_state_change = Some(Instant::now());
                    }
                    CircuitBreakerState::Open => {
                        // Already open, just reset the timer
                        state.last_opened = Some(Instant::now());
                    }
                }
            }
        }
    }
}

#[async_trait]
impl CircuitBreaker for SimpleCircuitBreaker {
    async fn execute<T, F, Fut>(&self, mut operation: F) -> CircuitBreakerResult<T>
    where
        F: FnMut() -> Fut + Send,
        Fut: std::future::Future<Output = Result<T, String>> + Send,
        T: Send,
    {
        let start_time = Instant::now();
        let initial_state = self.state().await;
        
        // Check if we should allow the request
        if !self.should_allow_request().await {
            return CircuitBreakerResult {
                result: Err(CircuitBreakerError::CircuitOpen),
                state: initial_state,
                execution_time: start_time.elapsed(),
            };
        }
        
        // Execute the operation with timeout
        let result = tokio::time::timeout(self.config.timeout, operation()).await;
        
        let execution_time = start_time.elapsed();
        let final_state;
        
        match result {
            Ok(Ok(value)) => {
                // Success
                self.record_result(true).await;
                final_state = self.state().await;
                CircuitBreakerResult {
                    result: Ok(value),
                    state: final_state,
                    execution_time,
                }
            }
            Ok(Err(error)) => {
                // Operation failed
                self.record_result(false).await;
                final_state = self.state().await;
                CircuitBreakerResult {
                    result: Err(CircuitBreakerError::ServiceError(error)),
                    state: final_state,
                    execution_time,
                }
            }
            Err(_) => {
                // Timeout
                self.record_result(false).await;
                final_state = self.state().await;
                CircuitBreakerResult {
                    result: Err(CircuitBreakerError::Timeout),
                    state: final_state,
                    execution_time,
                }
            }
        }
    }
    
    async fn state(&self) -> CircuitBreakerState {
        self.state.read().await.state
    }
    
    async fn is_available(&self) -> bool {
        match self.state().await {
            CircuitBreakerState::Closed | CircuitBreakerState::HalfOpen => true,
            CircuitBreakerState::Open => {
                // Check if enough time has passed for half-open
                let state = self.state.read().await;
                if let Some(last_opened) = state.last_opened {
                    last_opened.elapsed() >= self.config.reset_timeout
                } else {
                    false
                }
            }
        }
    }
    
    async fn force_open(&self) {
        let mut state = self.state.write().await;
        info!("Manually forcing circuit breaker to Open state");
        state.state = CircuitBreakerState::Open;
        state.last_opened = Some(Instant::now());
        state.last_state_change = Some(Instant::now());
    }
    
    async fn force_closed(&self) {
        let mut state = self.state.write().await;
        info!("Manually forcing circuit breaker to Closed state");
        state.state = CircuitBreakerState::Closed;
        state.failure_count = 0;
        state.success_count = 0;
        state.last_state_change = Some(Instant::now());
    }
    
    async fn stats(&self) -> CircuitBreakerStats {
        let state = self.state.read().await;
        let failure_rate = if state.total_calls > 0 {
            state.failed_calls as f64 / state.total_calls as f64
        } else {
            0.0
        };
        
        CircuitBreakerStats {
            total_calls: state.total_calls,
            successful_calls: state.successful_calls,
            failed_calls: state.failed_calls,
            state: state.state,
            failure_rate,
            last_state_change: state.last_state_change,
        }
    }
}