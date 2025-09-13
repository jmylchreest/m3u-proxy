//! Shared circuit breaker pool for memory efficiency
//!
//! This service provides a global pool of circuit breakers that can be shared
//! across multiple instances instead of creating individual instances per service.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info};

use crate::config::{CircuitBreakerConfig, CircuitBreakerProfileConfig};
use crate::utils::circuit_breaker::{
    CircuitBreaker, ConcreteCircuitBreaker, create_circuit_breaker_from_profile,
};

/// Pool key for circuit breaker sharing
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
struct PoolKey {
    implementation_type: String,
    failure_threshold: u32,
    operation_timeout: String,
    reset_timeout: String,
    success_threshold: u32,
}

impl From<&CircuitBreakerProfileConfig> for PoolKey {
    fn from(profile: &CircuitBreakerProfileConfig) -> Self {
        Self {
            implementation_type: profile.implementation_type.clone(),
            failure_threshold: profile.failure_threshold,
            operation_timeout: profile.operation_timeout.clone(),
            reset_timeout: profile.reset_timeout.clone(),
            success_threshold: profile.success_threshold,
        }
    }
}

/// Reference-counted circuit breaker pool entry
#[derive(Debug, Clone)]
struct PoolEntry {
    circuit_breaker: Arc<ConcreteCircuitBreaker>,
    reference_count: usize,
    services: Vec<String>,
}

/// Shared pool of circuit breakers for memory efficiency
pub struct CircuitBreakerPool {
    pool: Arc<RwLock<HashMap<PoolKey, PoolEntry>>>,
    config: Arc<RwLock<CircuitBreakerConfig>>,
}

impl CircuitBreakerPool {
    /// Create a new circuit breaker pool
    pub fn new(initial_config: CircuitBreakerConfig) -> Self {
        info!(
            "Creating CircuitBreakerPool with {} profiles",
            initial_config.profiles.len()
        );
        Self {
            pool: Arc::new(RwLock::new(HashMap::new())),
            config: Arc::new(RwLock::new(initial_config)),
        }
    }

    /// Get a circuit breaker for a service, creating or reusing from pool
    pub async fn get_circuit_breaker(
        &self,
        service_name: &str,
    ) -> Result<Arc<ConcreteCircuitBreaker>, String> {
        let config = self.config.read().await;
        let profile = config.profiles.get(service_name).unwrap_or(&config.global);

        let pool_key = PoolKey::from(profile);

        // Check if we already have this configuration in the pool
        {
            let mut pool = self.pool.write().await;

            if let Some(entry) = pool.get_mut(&pool_key) {
                // Reuse existing circuit breaker
                entry.reference_count += 1;
                if !entry.services.contains(&service_name.to_string()) {
                    entry.services.push(service_name.to_string());
                }
                debug!(
                    "Reusing circuit breaker for service '{}' (ref count: {})",
                    service_name, entry.reference_count
                );
                return Ok(entry.circuit_breaker.clone());
            }
        }

        // Create new circuit breaker
        let circuit_breaker = create_circuit_breaker_from_profile(profile)?;
        info!(
            "Created new pooled circuit breaker for service '{}' with profile: {:?}",
            service_name, profile
        );

        // Add to pool
        {
            let mut pool = self.pool.write().await;
            let entry = PoolEntry {
                circuit_breaker: circuit_breaker.clone(),
                reference_count: 1,
                services: vec![service_name.to_string()],
            };
            pool.insert(pool_key, entry);
        }

        Ok(circuit_breaker)
    }

    /// Release a circuit breaker reference
    pub async fn release_circuit_breaker(&self, service_name: &str) {
        let config = self.config.read().await;
        let profile = config.profiles.get(service_name).unwrap_or(&config.global);

        let pool_key = PoolKey::from(profile);

        let mut pool = self.pool.write().await;
        if let Some(entry) = pool.get_mut(&pool_key) {
            entry.reference_count = entry.reference_count.saturating_sub(1);
            entry.services.retain(|s| s != service_name);

            debug!(
                "Released circuit breaker reference for service '{}' (ref count: {})",
                service_name, entry.reference_count
            );

            // Remove from pool if no references remain
            if entry.reference_count == 0 {
                pool.remove(&pool_key);
                info!("Removed unused circuit breaker from pool");
            }
        }
    }

    /// Update configuration and refresh pool
    pub async fn update_configuration(
        &self,
        new_config: CircuitBreakerConfig,
    ) -> Result<Vec<String>, String> {
        let mut updated_services = Vec::new();

        // Update stored configuration
        {
            let mut config = self.config.write().await;
            *config = new_config;
        }

        // Clear the pool to force recreation with new configuration
        // This is simpler than trying to update individual entries
        {
            let mut pool = self.pool.write().await;
            let affected_services: Vec<String> = pool
                .values()
                .flat_map(|entry| entry.services.iter().cloned())
                .collect();

            pool.clear();
            updated_services.extend(affected_services);
        }

        info!(
            "Updated circuit breaker pool configuration. Cleared pool affecting {} services",
            updated_services.len()
        );

        Ok(updated_services)
    }

    /// Get pool statistics
    pub async fn get_pool_stats(&self) -> PoolStats {
        let pool = self.pool.read().await;
        let total_entries = pool.len();
        let total_references: usize = pool.values().map(|e| e.reference_count).sum();
        let total_services: usize = pool.values().map(|e| e.services.len()).sum();

        PoolStats {
            total_entries,
            total_references,
            total_services,
            memory_efficiency: if total_services > 0 {
                total_services as f64 / total_entries.max(1) as f64
            } else {
                1.0
            },
        }
    }

    /// Get all circuit breaker statistics
    pub async fn get_all_stats(
        &self,
    ) -> HashMap<String, crate::utils::circuit_breaker::CircuitBreakerStats> {
        let mut stats = HashMap::new();
        let pool = self.pool.read().await;

        for entry in pool.values() {
            let cb_stats = entry.circuit_breaker.stats().await;
            // Add stats for each service using this circuit breaker
            for service_name in &entry.services {
                stats.insert(service_name.clone(), cb_stats.clone());
            }
        }

        stats
    }

    /// List all active services
    pub async fn list_active_services(&self) -> Vec<String> {
        let pool = self.pool.read().await;
        pool.values()
            .flat_map(|entry| entry.services.iter().cloned())
            .collect()
    }
}

impl Clone for CircuitBreakerPool {
    fn clone(&self) -> Self {
        Self {
            pool: self.pool.clone(),
            config: self.config.clone(),
        }
    }
}

/// Statistics about the circuit breaker pool
#[derive(Debug, Clone)]
pub struct PoolStats {
    pub total_entries: usize,
    pub total_references: usize,
    pub total_services: usize,
    pub memory_efficiency: f64, // services per circuit breaker instance
}
