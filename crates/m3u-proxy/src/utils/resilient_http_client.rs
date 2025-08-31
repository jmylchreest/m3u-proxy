use crate::utils::{CircuitBreaker, CircuitBreakerConfig, CircuitBreakerType, create_circuit_breaker};
use reqwest::Client;
use std::time::Duration;
use tracing::{warn, debug, error};

/// HTTP client with circuit breaker protection for external service calls
pub struct ResilientHttpClient {
    client: Client,
    circuit_breaker: std::sync::Arc<crate::utils::ConcreteCircuitBreaker>,
    base_timeout: Duration,
}

impl ResilientHttpClient {
    /// Create a new resilient HTTP client with circuit breaker protection
    pub fn new(timeout: Duration) -> Self {
        Self::new_with_default_config(timeout)
    }

    /// Create a new resilient HTTP client with default circuit breaker configuration
    pub fn new_with_default_config(timeout: Duration) -> Self {
        let client = Client::builder()
            .timeout(timeout)
            .build()
            .expect("Failed to create HTTP client");

        // Configure circuit breaker for HTTP operations
        let cb_config = CircuitBreakerConfig {
            failure_threshold: 5,                      // Open after 5 failures
            timeout: Duration::from_secs(10),          // 10 second operation timeout
            reset_timeout: Duration::from_secs(60),    // Wait 60s before half-open
            success_threshold: 2,                      // Need 2 successes to close
        };

        let circuit_breaker = create_circuit_breaker(
            CircuitBreakerType::Simple,
            cb_config,
        );

        Self {
            client,
            circuit_breaker,
            base_timeout: timeout,
        }
    }

    /// Create a new resilient HTTP client with configuration-based circuit breaker
    pub fn new_with_config(timeout: Duration, app_config: &crate::config::Config) -> Self {
        let client = Client::builder()
            .timeout(timeout)
            .build()
            .expect("Failed to create HTTP client");

        // Get circuit breaker from configuration
        let circuit_breaker = crate::utils::create_circuit_breaker_for_service(
            "http_client",
            app_config,
        ).unwrap_or_else(|e| {
            tracing::warn!("Failed to create HTTP client circuit breaker from config: {}. Using defaults.", e);
            // Fallback to defaults
            let cb_config = CircuitBreakerConfig {
                failure_threshold: 5,
                timeout: Duration::from_secs(10),
                reset_timeout: Duration::from_secs(60),
                success_threshold: 2,
            };
            create_circuit_breaker(CircuitBreakerType::Simple, cb_config)
        });

        Self {
            client,
            circuit_breaker,
            base_timeout: timeout,
        }
    }

    /// Fetch a logo URL with circuit breaker protection
    pub async fn fetch_logo(&self, logo_url: &str) -> Result<bytes::Bytes, String> {
        debug!("Attempting to fetch logo: {}", logo_url);

        let url = logo_url.to_string();
        let client = self.client.clone();

        let cb_result = self.circuit_breaker.execute(|| async {
            let response = client
                .get(&url)
                .send()
                .await
                .map_err(|e| format!("HTTP request failed: {}", e))?;

            if !response.status().is_success() {
                return Err(format!("HTTP error: {}", response.status()));
            }

            let content_length = response.content_length().unwrap_or(0);
            if content_length > 5_000_000 { // 5MB limit
                return Err("Logo file too large".to_string());
            }

            let bytes = response
                .bytes()
                .await
                .map_err(|e| format!("Failed to read response body: {}", e))?;

            Ok(bytes)
        }).await;

        match cb_result.result {
            Ok(bytes) => {
                debug!("Successfully fetched logo: {} ({} bytes, took {:?}, CB state: {:?})", 
                       url, bytes.len(), cb_result.execution_time, cb_result.state);
                Ok(bytes)
            },
            Err(crate::utils::circuit_breaker::CircuitBreakerError::CircuitOpen) => {
                warn!("Logo fetch blocked by HTTP circuit breaker: {} (state: {:?})", url, cb_result.state);
                Err("Circuit breaker open - logo service unavailable".to_string())
            },
            Err(crate::utils::circuit_breaker::CircuitBreakerError::ServiceError(e)) => {
                error!("Logo fetch failed: {} - {} (CB state: {:?}, took {:?})", url, e, cb_result.state, cb_result.execution_time);
                Err(format!("Logo fetch error: {}", e))
            },
            Err(crate::utils::circuit_breaker::CircuitBreakerError::Timeout) => {
                error!("Logo fetch timed out: {} (CB state: {:?}, took {:?})", url, cb_result.state, cb_result.execution_time);
                Err("Logo fetch timeout".to_string())
            },
        }
    }

    /// Generic HTTP GET with circuit breaker protection
    pub async fn get(&self, url: &str) -> Result<reqwest::Response, String> {
        let url_string = url.to_string();
        let client = self.client.clone();

        let cb_result = self.circuit_breaker.execute(|| async {
            client
                .get(&url_string)
                .send()
                .await
                .map_err(|e| format!("HTTP GET failed: {}", e))
        }).await;

        match cb_result.result {
            Ok(response) => {
                debug!("HTTP GET successful: {} (status: {}, CB state: {:?}, took {:?})", 
                       url_string, response.status(), cb_result.state, cb_result.execution_time);
                Ok(response)
            },
            Err(crate::utils::circuit_breaker::CircuitBreakerError::CircuitOpen) => {
                warn!("HTTP GET blocked by circuit breaker: {} (state: {:?})", url_string, cb_result.state);
                Err("Circuit breaker open".to_string())
            },
            Err(crate::utils::circuit_breaker::CircuitBreakerError::ServiceError(e)) => {
                error!("HTTP GET failed: {} - {} (CB state: {:?}, took {:?})", url_string, e, cb_result.state, cb_result.execution_time);
                Err(e)
            },
            Err(crate::utils::circuit_breaker::CircuitBreakerError::Timeout) => {
                error!("HTTP GET timed out: {} (CB state: {:?}, took {:?})", url_string, cb_result.state, cb_result.execution_time);
                Err("HTTP request timeout".to_string())
            },
        }
    }

    /// Check if the HTTP client is available (circuit breaker not open)
    pub async fn is_available(&self) -> bool {
        self.circuit_breaker.is_available().await
    }

    /// Get circuit breaker statistics
    pub async fn stats(&self) -> crate::utils::circuit_breaker::CircuitBreakerStats {
        self.circuit_breaker.stats().await
    }

    /// Force circuit breaker open (for testing)
    pub async fn force_circuit_open(&self) {
        self.circuit_breaker.force_open().await;
    }
}

impl Clone for ResilientHttpClient {
    fn clone(&self) -> Self {
        Self::new(self.base_timeout)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio;

    #[tokio::test]
    async fn test_circuit_breaker_availability() {
        let client = ResilientHttpClient::new(Duration::from_secs(5));
        
        // Should be available initially
        assert!(client.is_available().await);
        
        // Force circuit open
        client.force_circuit_open().await;
        
        // Should not be available when circuit is open
        assert!(!client.is_available().await);
    }

    #[tokio::test]
    async fn test_logo_fetch_with_bad_url() {
        let client = ResilientHttpClient::new(Duration::from_secs(1));
        
        let result = client.fetch_logo("http://nonexistent.example.com/logo.png").await;
        assert!(result.is_err());
        
        // Circuit breaker should still track failures
        let stats = client.stats().await;
        assert!(stats.failed_calls > 0);
    }
}