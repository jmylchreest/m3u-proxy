use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{error, info, warn};

use crate::config::{CircuitBreakerConfig, CircuitBreakerProfileConfig};
use crate::utils::circuit_breaker::{
    CircuitBreaker, ConcreteCircuitBreaker, create_circuit_breaker_from_profile,
};

/// Manages circuit breakers and supports runtime configuration updates
pub struct CircuitBreakerManager {
    /// Currently active circuit breakers indexed by service name
    active_breakers: Arc<RwLock<HashMap<String, Arc<ConcreteCircuitBreaker>>>>,
    /// Current configuration
    current_config: Arc<RwLock<CircuitBreakerConfig>>,
}

impl CircuitBreakerManager {
    /// Create a new circuit breaker manager with initial configuration
    pub fn new(initial_config: CircuitBreakerConfig) -> Self {
        Self {
            active_breakers: Arc::new(RwLock::new(HashMap::new())),
            current_config: Arc::new(RwLock::new(initial_config)),
        }
    }

    /// Get the configuration profile for a service
    pub async fn get_service_profile(&self, service_name: &str) -> CircuitBreakerProfileConfig {
        let config = self.current_config.read().await;
        config
            .profiles
            .get(service_name)
            .unwrap_or(&config.global)
            .clone()
    }

    /// Get or create a circuit breaker for a service
    pub async fn get_circuit_breaker(
        &self,
        service_name: &str,
    ) -> Result<Arc<ConcreteCircuitBreaker>, String> {
        // Check if we already have an active circuit breaker for this service
        {
            let breakers = self.active_breakers.read().await;
            if let Some(breaker) = breakers.get(service_name) {
                return Ok(breaker.clone());
            }
        }

        // Create a new circuit breaker
        let config = self.current_config.read().await;
        let profile = config.profiles.get(service_name).unwrap_or(&config.global);

        info!(
            "Creating circuit breaker for service '{}' with profile: {:?}",
            service_name, profile
        );
        let breaker = create_circuit_breaker_from_profile(profile)?;
        info!("Circuit breaker created for service '{}'", service_name);

        // Store the new circuit breaker
        {
            let mut breakers = self.active_breakers.write().await;
            breakers.insert(service_name.to_string(), breaker.clone());
        }

        Ok(breaker)
    }

    /// Update configuration and recreate affected circuit breakers
    pub async fn update_configuration(
        &self,
        new_config: CircuitBreakerConfig,
    ) -> Result<Vec<String>, String> {
        let mut updated_services = Vec::new();

        // Update the stored configuration
        {
            let mut config = self.current_config.write().await;
            *config = new_config;
        }

        // Get list of currently active services that need to be recreated
        let active_service_names: Vec<String>;
        {
            let breakers = self.active_breakers.read().await;
            active_service_names = breakers.keys().cloned().collect();
        }

        // Recreate circuit breakers for all active services
        {
            let mut breakers = self.active_breakers.write().await;
            let config = self.current_config.read().await;

            for service_name in &active_service_names {
                let profile = config.profiles.get(service_name).unwrap_or(&config.global);

                match create_circuit_breaker_from_profile(profile) {
                    Ok(new_breaker) => {
                        breakers.insert(service_name.clone(), new_breaker);
                        updated_services.push(service_name.clone());
                        info!(
                            "Updated circuit breaker for service '{}' with new configuration",
                            service_name
                        );
                    }
                    Err(e) => {
                        warn!(
                            "Failed to update circuit breaker for service '{}': {}",
                            service_name, e
                        );
                        // Keep the old circuit breaker in case of error
                    }
                }
            }
        }

        info!(
            "Configuration update completed. Updated {} services: {:?}",
            updated_services.len(),
            updated_services
        );

        Ok(updated_services)
    }

    /// Get current configuration
    pub async fn get_current_config(&self) -> CircuitBreakerConfig {
        let config = self.current_config.read().await;
        config.clone()
    }

    /// Get statistics for all active circuit breakers
    pub async fn get_all_stats(
        &self,
    ) -> HashMap<String, crate::utils::circuit_breaker::CircuitBreakerStats> {
        let mut stats = HashMap::new();
        let breakers = self.active_breakers.read().await;

        for (service_name, breaker) in breakers.iter() {
            let breaker_stats = breaker.stats().await;
            stats.insert(service_name.clone(), breaker_stats);
        }

        stats
    }

    /// Force a specific circuit breaker open (for testing/emergency)
    pub async fn force_circuit_open(&self, service_name: &str) -> Result<(), String> {
        let breakers = self.active_breakers.read().await;
        match breakers.get(service_name) {
            Some(breaker) => {
                breaker.force_open().await;
                info!(
                    "Manually forced circuit breaker open for service '{}'",
                    service_name
                );
                Ok(())
            }
            None => Err(format!(
                "No active circuit breaker found for service '{}'",
                service_name
            )),
        }
    }

    /// Force a specific circuit breaker closed (for testing/recovery)
    pub async fn force_circuit_closed(&self, service_name: &str) -> Result<(), String> {
        let breakers = self.active_breakers.read().await;
        match breakers.get(service_name) {
            Some(breaker) => {
                breaker.force_closed().await;
                info!(
                    "Manually forced circuit breaker closed for service '{}'",
                    service_name
                );
                Ok(())
            }
            None => Err(format!(
                "No active circuit breaker found for service '{}'",
                service_name
            )),
        }
    }

    /// List all active circuit breaker services
    pub async fn list_active_services(&self) -> Vec<String> {
        let breakers = self.active_breakers.read().await;
        breakers.keys().cloned().collect()
    }

    /// Update a specific service's circuit breaker profile
    pub async fn update_service_profile(
        &self,
        service_name: &str,
        profile: CircuitBreakerProfileConfig,
    ) -> Result<(), String> {
        // Update the configuration
        {
            let mut config = self.current_config.write().await;
            config
                .profiles
                .insert(service_name.to_string(), profile.clone());
        }

        // Recreate the circuit breaker if it exists
        {
            let mut breakers = self.active_breakers.write().await;
            if breakers.contains_key(service_name) {
                match create_circuit_breaker_from_profile(&profile) {
                    Ok(new_breaker) => {
                        breakers.insert(service_name.to_string(), new_breaker);
                        info!(
                            "Updated circuit breaker profile for service '{}': {:?}",
                            service_name, profile
                        );
                        Ok(())
                    }
                    Err(e) => {
                        error!(
                            "Failed to create circuit breaker with new profile for service '{}': {}",
                            service_name, e
                        );
                        Err(e)
                    }
                }
            } else {
                info!(
                    "Service '{}' profile updated but no active circuit breaker to recreate",
                    service_name
                );
                Ok(())
            }
        }
    }
}

impl Clone for CircuitBreakerManager {
    fn clone(&self) -> Self {
        Self {
            active_breakers: self.active_breakers.clone(),
            current_config: self.current_config.clone(),
        }
    }
}
