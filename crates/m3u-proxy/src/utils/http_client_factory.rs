//! HTTP Client Factory
//!
//! This module provides a centralized factory for creating HTTP clients
//! with appropriate circuit breaker protection based on service names.
//! This decouples services from circuit breaker management.

use std::time::Duration;
use crate::utils::StandardHttpClient;
use crate::services::CircuitBreakerManager;
use std::sync::Arc;

/// Factory for creating HTTP clients with appropriate circuit breaker protection
#[derive(Clone)]
pub struct HttpClientFactory {
    circuit_breaker_manager: Option<Arc<CircuitBreakerManager>>,
    default_connect_timeout: Duration,
    user_agent: String,
}

impl HttpClientFactory {
    /// Create a new HTTP client factory
    /// Automatically generates a standard user agent format
    pub fn new(
        circuit_breaker_manager: Option<Arc<CircuitBreakerManager>>,
        default_connect_timeout: Duration,
    ) -> Self {
        Self {
            circuit_breaker_manager,
            default_connect_timeout,
            user_agent: format!("{}/{}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION")),
        }
    }

    /// Create an HTTP client for a specific service
    /// 
    /// The service name determines which circuit breaker profile to use:
    /// - "source_m3u" -> uses source_m3u circuit breaker profile
    /// - "source_xc_stream" -> uses source_xc_stream circuit breaker profile
    /// - "source_xc_xmltv" -> uses source_xc_xmltv circuit breaker profile
    /// - "source_xmltv" -> uses source_xmltv circuit breaker profile  
    /// - "logo_fetch" -> uses logo_fetch circuit breaker profile
    pub async fn create_client_for_service(&self, service_name: &str) -> StandardHttpClient {
        match &self.circuit_breaker_manager {
            Some(cb_manager) => {
                // Get the service profile to access acceptable status codes
                let profile = cb_manager.get_service_profile(service_name).await;
                let acceptable_status_codes = profile.acceptable_status_codes;

                match cb_manager.get_circuit_breaker(service_name).await {
                    Ok(circuit_breaker) => {
                        tracing::debug!("Created circuit breaker-protected HTTP client for service: {} with acceptable codes: {:?}", service_name, acceptable_status_codes);
                        StandardHttpClient::new(
                            self.default_connect_timeout,
                            Some(circuit_breaker),
                            &self.user_agent,
                            acceptable_status_codes,
                        )
                    },
                    Err(e) => {
                        tracing::warn!("Failed to get circuit breaker for {}: {}. Creating client without circuit breaker.", service_name, e);
                        StandardHttpClient::new(
                            self.default_connect_timeout,
                            None,
                            &self.user_agent,
                            acceptable_status_codes,
                        )
                    }
                }
            }
            None => {
                tracing::debug!("Creating HTTP client for service: {} (no circuit breaker manager)", service_name);
                // Use default acceptable status codes when no circuit breaker manager
                let default_acceptable_codes = vec!["2xx".to_string(), "3xx".to_string()];
                StandardHttpClient::new(
                    self.default_connect_timeout,
                    None,
                    &self.user_agent,
                    default_acceptable_codes,
                )
            }
        }
    }

    /// Create a basic HTTP client without circuit breaker protection
    /// Use this for internal services or when circuit breaker protection isn't needed
    pub fn create_basic_client(&self) -> StandardHttpClient {
        let default_acceptable_codes = vec!["2xx".to_string(), "3xx".to_string()];
        StandardHttpClient::new(
            self.default_connect_timeout,
            None,
            &self.user_agent,
            default_acceptable_codes,
        )
    }

    /// Check if circuit breaker manager is available
    pub fn has_circuit_breaker_support(&self) -> bool {
        self.circuit_breaker_manager.is_some()
    }
}

