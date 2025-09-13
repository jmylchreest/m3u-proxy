use async_trait::async_trait;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;
use tracing::warn;

/// Generic result for circuit breaker operations
#[derive(Debug, Clone)]
pub struct CircuitBreakerResult<T> {
    pub result: Result<T, CircuitBreakerError>,
    pub state: CircuitBreakerState,
    pub execution_time: Duration,
}

#[derive(Debug, Clone)]
pub enum CircuitBreakerError {
    /// Circuit breaker is open, operation blocked
    CircuitOpen,
    /// Operation failed due to underlying service error
    ServiceError(String),
    /// Operation timed out
    Timeout,
}

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub enum CircuitBreakerState {
    Closed,
    Open,
    HalfOpen,
}

/// Generic circuit breaker trait that different implementations can provide
#[async_trait]
pub trait CircuitBreaker: Send + Sync {
    /// Execute an async operation through the circuit breaker
    async fn execute<T, F, Fut>(&self, operation: F) -> CircuitBreakerResult<T>
    where
        F: FnMut() -> Fut + Send,
        Fut: Future<Output = Result<T, String>> + Send,
        T: Send;

    /// Get current circuit breaker state
    async fn state(&self) -> CircuitBreakerState;

    /// Check if operations are currently allowed
    async fn is_available(&self) -> bool;

    /// Force circuit breaker to open state (for testing)
    async fn force_open(&self);

    /// Force circuit breaker to closed state (for testing)
    async fn force_closed(&self);

    /// Get circuit breaker statistics
    async fn stats(&self) -> CircuitBreakerStats;
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct CircuitBreakerStats {
    pub total_calls: u64,
    pub successful_calls: u64,
    pub failed_calls: u64,
    pub state: CircuitBreakerState,
    pub failure_rate: f64,
    #[serde(skip)]
    pub last_state_change: Option<std::time::Instant>,
}

/// Configuration for circuit breakers
#[derive(Debug, Clone)]
pub struct CircuitBreakerConfig {
    pub failure_threshold: u32,
    pub timeout: Duration,
    pub reset_timeout: Duration,
    pub success_threshold: u32,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 3,
            timeout: Duration::from_secs(5),
            reset_timeout: Duration::from_secs(30),
            success_threshold: 2,
        }
    }
}

/// Factory for creating different circuit breaker implementations
pub enum CircuitBreakerType {
    /// Simplified implementation with direct generic support (recommended)
    Simple,
    /// WARNING: NoOp circuit breaker always passes operations through - DO NOT USE IN PRODUCTION
    #[allow(dead_code)]
    NoOp,
}

/// Concrete circuit breaker implementation that wraps different types
#[derive(Debug)]
pub enum ConcreteCircuitBreaker {
    Simple(crate::utils::circuit_breaker_simple::SimpleCircuitBreaker),
    NoOp(crate::utils::circuit_breaker_noop::NoOpCircuitBreaker),
}

#[async_trait]
impl CircuitBreaker for ConcreteCircuitBreaker {
    async fn execute<T, F, Fut>(&self, operation: F) -> CircuitBreakerResult<T>
    where
        F: FnMut() -> Fut + Send,
        Fut: Future<Output = Result<T, String>> + Send,
        T: Send,
    {
        match self {
            ConcreteCircuitBreaker::Simple(cb) => cb.execute(operation).await,
            ConcreteCircuitBreaker::NoOp(cb) => cb.execute(operation).await,
        }
    }

    async fn state(&self) -> CircuitBreakerState {
        match self {
            ConcreteCircuitBreaker::Simple(cb) => cb.state().await,
            ConcreteCircuitBreaker::NoOp(cb) => cb.state().await,
        }
    }

    async fn is_available(&self) -> bool {
        match self {
            ConcreteCircuitBreaker::Simple(cb) => cb.is_available().await,
            ConcreteCircuitBreaker::NoOp(cb) => cb.is_available().await,
        }
    }

    async fn force_open(&self) {
        match self {
            ConcreteCircuitBreaker::Simple(cb) => cb.force_open().await,
            ConcreteCircuitBreaker::NoOp(cb) => cb.force_open().await,
        }
    }

    async fn force_closed(&self) {
        match self {
            ConcreteCircuitBreaker::Simple(cb) => cb.force_closed().await,
            ConcreteCircuitBreaker::NoOp(cb) => cb.force_closed().await,
        }
    }

    async fn stats(&self) -> CircuitBreakerStats {
        match self {
            ConcreteCircuitBreaker::Simple(cb) => cb.stats().await,
            ConcreteCircuitBreaker::NoOp(cb) => cb.stats().await,
        }
    }
}

// Implement CircuitBreaker for Arc<ConcreteCircuitBreaker> to make it easier to use
#[async_trait]
impl CircuitBreaker for Arc<ConcreteCircuitBreaker> {
    async fn execute<T, F, Fut>(&self, operation: F) -> CircuitBreakerResult<T>
    where
        F: FnMut() -> Fut + Send,
        Fut: Future<Output = Result<T, String>> + Send,
        T: Send,
    {
        self.as_ref().execute(operation).await
    }

    async fn state(&self) -> CircuitBreakerState {
        self.as_ref().state().await
    }

    async fn is_available(&self) -> bool {
        self.as_ref().is_available().await
    }

    async fn force_open(&self) {
        self.as_ref().force_open().await
    }

    async fn force_closed(&self) {
        self.as_ref().force_closed().await
    }

    async fn stats(&self) -> CircuitBreakerStats {
        self.as_ref().stats().await
    }
}

/// Parse a duration string (e.g., "5s", "30s", "1m") into a Duration
fn parse_duration(duration_str: &str) -> Result<Duration, String> {
    if duration_str.is_empty() {
        return Err("Duration string cannot be empty".to_string());
    }

    let (number_part, unit_part) = if let Some(pos) = duration_str.find(|c: char| c.is_alphabetic())
    {
        (&duration_str[..pos], &duration_str[pos..])
    } else {
        return Err(format!("Invalid duration format: {}", duration_str));
    };

    let number: u64 = number_part
        .parse()
        .map_err(|_| format!("Invalid number in duration: {}", number_part))?;

    let duration = match unit_part.to_lowercase().as_str() {
        "s" | "sec" | "secs" | "second" | "seconds" => Duration::from_secs(number),
        "m" | "min" | "mins" | "minute" | "minutes" => Duration::from_secs(number * 60),
        "h" | "hour" | "hours" => Duration::from_secs(number * 3600),
        _ => return Err(format!("Unsupported time unit: {}", unit_part)),
    };

    Ok(duration)
}

/// Create a circuit breaker from config profile
pub fn create_circuit_breaker_from_profile(
    profile: &crate::config::CircuitBreakerProfileConfig,
) -> Result<std::sync::Arc<ConcreteCircuitBreaker>, String> {
    let cb_type = match profile.implementation_type.as_str() {
        "simple" => CircuitBreakerType::Simple,
        "noop" => CircuitBreakerType::NoOp,
        _ => {
            return Err(format!(
                "Unsupported circuit breaker type: {} (supported: simple, noop)",
                profile.implementation_type
            ));
        }
    };

    let config = CircuitBreakerConfig {
        failure_threshold: profile.failure_threshold,
        timeout: parse_duration(&profile.operation_timeout)?,
        reset_timeout: parse_duration(&profile.reset_timeout)?,
        success_threshold: profile.success_threshold,
    };

    Ok(create_circuit_breaker(cb_type, config))
}

/// Create a circuit breaker for a specific service with profile support
pub fn create_circuit_breaker_for_service(
    service_name: &str,
    app_config: &crate::config::Config,
) -> Result<std::sync::Arc<ConcreteCircuitBreaker>, String> {
    let default_cb_config = crate::config::CircuitBreakerConfig::default();
    let cb_config = app_config
        .circuitbreaker
        .as_ref()
        .unwrap_or(&default_cb_config);

    // Try to get service-specific profile, fallback to global
    let profile = cb_config
        .profiles
        .get(service_name)
        .unwrap_or(&cb_config.global);

    create_circuit_breaker_from_profile(profile)
}

/// Factory function to create different circuit breaker implementations
pub fn create_circuit_breaker(
    cb_type: CircuitBreakerType,
    config: CircuitBreakerConfig,
) -> std::sync::Arc<ConcreteCircuitBreaker> {
    use crate::utils::{
        circuit_breaker_noop::NoOpCircuitBreaker, circuit_breaker_simple::SimpleCircuitBreaker,
    };

    let cb = match cb_type {
        CircuitBreakerType::Simple => {
            ConcreteCircuitBreaker::Simple(SimpleCircuitBreaker::new(config))
        }
        CircuitBreakerType::NoOp => {
            warn!("CREATING NOOP CIRCUIT BREAKER - THIS SHOULD NOT BE USED IN PRODUCTION!");
            ConcreteCircuitBreaker::NoOp(NoOpCircuitBreaker::new())
        }
    };

    std::sync::Arc::new(cb)
}
