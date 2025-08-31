use std::future::Future;
use std::sync::Arc;
use async_trait::async_trait;
use tracing::{info, error, debug};

use super::circuit_breaker::{
    CircuitBreaker, CircuitBreakerConfig, CircuitBreakerResult, 
    CircuitBreakerState, CircuitBreakerStats, CircuitBreakerError
};

/// Adapter for rssafecircuit crate with state tracking and logging
#[derive(Debug)]
pub struct RsSafeCircuitBreakerAdapter {
    inner: Arc<tokio::sync::Mutex<rssafecircuit::CircuitBreaker>>,
    config: CircuitBreakerConfig,
}

impl RsSafeCircuitBreakerAdapter {
    pub fn new(config: CircuitBreakerConfig) -> Self {
        // Map our config to rssafecircuit parameters
        let max_failures = config.failure_threshold;
        let timeout_secs = config.reset_timeout.as_secs();
        let pause_time_ms = config.timeout.as_millis() as u64;
        
        let circuit_breaker = rssafecircuit::CircuitBreaker::new(
            max_failures,
            timeout_secs,
            pause_time_ms,
        );
        
        // Set up logging callbacks for state transitions
        circuit_breaker.set_on_open(|| {
            info!("RsSafe circuit breaker transitioned to open state");
        });
        
        circuit_breaker.set_on_close(|| {
            info!("RsSafe circuit breaker transitioned to closed state");
        });
        
        circuit_breaker.set_on_half_open(|| {
            info!("RsSafe circuit breaker transitioned to half-open state");
        });
        
        Self {
            inner: Arc::new(tokio::sync::Mutex::new(circuit_breaker)),
            config,
        }
    }
}

#[async_trait]
impl CircuitBreaker for RsSafeCircuitBreakerAdapter {
    async fn execute<T, F, Fut>(&self, operation: F) -> CircuitBreakerResult<T>
    where
        F: FnMut() -> Fut + Send,
        Fut: Future<Output = Result<T, String>> + Send,
        T: Send,
    {
        let start_time = std::time::Instant::now();
        let initial_state = self.state().await;
        
        // Check if circuit breaker allows execution
        {
            let breaker = self.inner.lock().await;
            match breaker.state {
                rssafecircuit::CircuitBreakerState::Open => {
                    // Check if timeout has passed for half-open transition
                    if std::time::Instant::now() <= breaker.open_timeout {
                        debug!("RsSafe circuit breaker blocked operation (state: Open)");
                        return CircuitBreakerResult {
                            result: Err(CircuitBreakerError::CircuitOpen),
                            state: CircuitBreakerState::Open,
                            execution_time: start_time.elapsed(),
                        };
                    }
                    // If timeout passed, it will transition to half-open in the actual execute call
                }
                _ => {} // Closed or HalfOpen states allow execution
            }
        }
        
        // Execute the operation and let rssafecircuit handle the state management
        let mut breaker = self.inner.lock().await;
        
        // Store the result in a shared location since rssafecircuit only handles String results
        let result_holder = Arc::new(tokio::sync::Mutex::new(None));
        let result_holder_clone = result_holder.clone();
        
        // Wrap the operation in an Arc<Mutex<>> to handle the FnMut constraint and make it Send
        let operation_cell = Arc::new(tokio::sync::Mutex::new(Some(operation)));
        let operation_cell_clone = operation_cell.clone();
        
        let operation_result = breaker.execute(move || {
            let holder = result_holder_clone.clone();
            let op_cell = operation_cell_clone.clone();
            async move {
                // Take the operation from the cell (can only be done once)
                let op = op_cell.lock().await.take();
                match op {
                    Some(mut op) => {
                        match op().await {
                            Ok(value) => {
                                // Store the actual result
                                *holder.lock().await = Some(Ok(value));
                                Ok("success".to_string())
                            }
                            Err(e) => {
                                // Store the error
                                *holder.lock().await = Some(Err(e.clone()));
                                Err(e)
                            }
                        }
                    }
                    None => {
                        // This shouldn't happen with proper circuit breaker usage
                        Err("Operation already consumed".to_string())
                    }
                }
            }
        }).await;
        
        let execution_time = start_time.elapsed();
        let final_state = self.state().await;
        
        // Log state transitions
        if initial_state != final_state {
            info!("RsSafe circuit breaker state changed: {:?} -> {:?}", initial_state, final_state);
        }
        
        match operation_result {
            Ok(_) => {
                // Get the actual result from our holder
                let stored_result = result_holder.lock().await.take();
                match stored_result {
                    Some(Ok(result)) => {
                        debug!("RsSafe circuit breaker operation succeeded (state: {:?}, took {:?})", final_state, execution_time);
                        CircuitBreakerResult {
                            result: Ok(result),
                            state: final_state,
                            execution_time,
                        }
                    }
                    Some(Err(e)) => {
                        error!("RsSafe circuit breaker operation failed: {} (state: {:?}, took {:?})", e, final_state, execution_time);
                        CircuitBreakerResult {
                            result: Err(CircuitBreakerError::ServiceError(e)),
                            state: final_state,
                            execution_time,
                        }
                    }
                    None => {
                        error!("RsSafe circuit breaker: no result stored - this should not happen");
                        CircuitBreakerResult {
                            result: Err(CircuitBreakerError::ServiceError("Internal error: no result stored".to_string())),
                            state: final_state,
                            execution_time,
                        }
                    }
                }
            }
            Err(e) => {
                if e.contains("Circuit breaker is open") {
                    debug!("RsSafe circuit breaker blocked operation (state: {:?})", final_state);
                    CircuitBreakerResult {
                        result: Err(CircuitBreakerError::CircuitOpen),
                        state: final_state,
                        execution_time,
                    }
                } else {
                    error!("RsSafe circuit breaker operation failed: {} (state: {:?}, took {:?})", e, final_state, execution_time);
                    CircuitBreakerResult {
                        result: Err(CircuitBreakerError::ServiceError(e)),
                        state: final_state,
                        execution_time,
                    }
                }
            }
        }
    }

    async fn state(&self) -> CircuitBreakerState {
        let breaker = self.inner.lock().await;
        match breaker.state {
            rssafecircuit::CircuitBreakerState::Closed => CircuitBreakerState::Closed,
            rssafecircuit::CircuitBreakerState::Open => CircuitBreakerState::Open,
            rssafecircuit::CircuitBreakerState::HalfOpen => CircuitBreakerState::HalfOpen,
        }
    }

    async fn is_available(&self) -> bool {
        let state = self.state().await;
        match state {
            CircuitBreakerState::Closed | CircuitBreakerState::HalfOpen => true,
            CircuitBreakerState::Open => false,
        }
    }

    async fn force_open(&self) {
        info!("RsSafe circuit breaker manually forced to open state");
        let mut breaker = self.inner.lock().await;
        breaker.trip();
    }

    async fn force_closed(&self) {
        info!("RsSafe circuit breaker manually forced to closed state");
        let mut breaker = self.inner.lock().await;
        breaker.reset();
    }

    async fn stats(&self) -> CircuitBreakerStats {
        let breaker = self.inner.lock().await;
        let state = match breaker.state {
            rssafecircuit::CircuitBreakerState::Closed => CircuitBreakerState::Closed,
            rssafecircuit::CircuitBreakerState::Open => CircuitBreakerState::Open,
            rssafecircuit::CircuitBreakerState::HalfOpen => CircuitBreakerState::HalfOpen,
        };
        
        // rssafecircuit doesn't expose detailed stats, so we provide basic info
        CircuitBreakerStats {
            total_calls: 0, // Not tracked by rssafecircuit
            successful_calls: 0, // Not tracked by rssafecircuit
            failed_calls: 0, // Not tracked by rssafecircuit
            state,
            failure_rate: 0.0, // Not tracked by rssafecircuit
            last_state_change: None, // Not tracked by rssafecircuit
        }
    }
}