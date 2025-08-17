//! Runtime Settings Management
//!
//! This module provides a centralized store for runtime settings that can be
//! changed without restarting the service. Settings changes are applied immediately
//! and affect the actual server behavior.

use std::sync::Arc;
use std::collections::HashMap;
use tokio::sync::RwLock;
use tracing::{info, warn, error};
use tracing_subscriber::reload::Handle;

/// Runtime settings that can be changed without service restart
#[derive(Debug, Clone)]
pub struct RuntimeSettings {
    /// Current log level (TRACE, DEBUG, INFO, WARN, ERROR)
    pub log_level: String,
    /// Maximum number of concurrent connections (if configurable)
    pub max_connections: Option<u32>,
    /// Request timeout in seconds
    pub request_timeout_seconds: Option<u32>,
    /// Enable/disable request logging
    pub enable_request_logging: bool,
    /// Enable/disable metrics collection
    pub enable_metrics: bool,
}

impl Default for RuntimeSettings {
    fn default() -> Self {
        Self {
            log_level: "INFO".to_string(),
            max_connections: Some(1000),
            request_timeout_seconds: Some(30),
            enable_request_logging: true,
            enable_metrics: true,
        }
    }
}

/// Runtime settings store with atomic updates
#[derive(Clone)]
pub struct RuntimeSettingsStore {
    settings: Arc<RwLock<RuntimeSettings>>,
    // Optional handle for reloading tracing subscriber log level
    tracing_reload_handle: Option<Arc<RwLock<Handle<tracing_subscriber::EnvFilter, tracing_subscriber::Registry>>>>,
    /// Runtime configuration flags for middleware
    pub runtime_flags: Arc<RwLock<RuntimeFlags>>,
}

/// Runtime flags that can be checked by middleware and services
#[derive(Debug, Clone)]
pub struct RuntimeFlags {
    /// Whether request logging is currently enabled
    pub request_logging_enabled: bool,
    /// Whether metrics collection is currently enabled  
    pub metrics_enabled: bool,
    /// Current request timeout in seconds
    pub request_timeout_seconds: u32,
    /// Current max connections limit
    pub max_connections: u32,
    /// Runtime feature flags
    pub feature_flags: HashMap<String, bool>,
    /// Runtime feature configuration
    pub feature_config: HashMap<String, HashMap<String, serde_json::Value>>,
}

impl Default for RuntimeFlags {
    fn default() -> Self {
        Self {
            request_logging_enabled: true,
            metrics_enabled: true,
            request_timeout_seconds: 30,
            max_connections: 1000,
            feature_flags: HashMap::new(),
            feature_config: HashMap::new(),
        }
    }
}

impl RuntimeSettingsStore {
    /// Create a new runtime settings store with default values
    pub fn new() -> Self {
        Self {
            settings: Arc::new(RwLock::new(RuntimeSettings::default())),
            tracing_reload_handle: None,
            runtime_flags: Arc::new(RwLock::new(RuntimeFlags::default())),
        }
    }
    
    /// Get current runtime flags for middleware to check
    pub async fn get_flags(&self) -> RuntimeFlags {
        self.runtime_flags.read().await.clone()
    }

    /// Create a new runtime settings store with tracing reload capability
    pub fn with_tracing_reload(
        handle: Handle<tracing_subscriber::EnvFilter, tracing_subscriber::Registry>
    ) -> Self {
        Self {
            settings: Arc::new(RwLock::new(RuntimeSettings::default())),
            tracing_reload_handle: Some(Arc::new(RwLock::new(handle))),
            runtime_flags: Arc::new(RwLock::new(RuntimeFlags::default())),
        }
    }

    /// Get current settings (read-only copy)
    pub async fn get(&self) -> RuntimeSettings {
        self.settings.read().await.clone()
    }

    /// Update log level and apply change to tracing subscriber
    pub async fn update_log_level(&self, new_level: &str) -> bool {
        let new_level_upper = new_level.to_uppercase();
        
        // Validate log level
        if !["TRACE", "DEBUG", "INFO", "WARN", "ERROR"].contains(&new_level_upper.as_str()) {
            error!("Invalid log level: {}", new_level);
            return false;
        }

        // Update stored setting
        {
            let mut settings = self.settings.write().await;
            settings.log_level = new_level_upper.clone();
        }

        // Apply to tracing subscriber if handle is available
        if let Some(reload_handle) = &self.tracing_reload_handle {
            if let Ok(handle) = reload_handle.try_read() {
                // Create new filter with the log level, including tower_http for trace level
                let filter_directive = if new_level_upper == "TRACE" {
                    format!("m3u_proxy={},tower_http=trace", new_level_upper.to_lowercase())
                } else {
                    format!("m3u_proxy={}", new_level_upper.to_lowercase())
                };
                
                match tracing_subscriber::EnvFilter::try_new(&filter_directive) {
                    Ok(new_filter) => {
                        if let Err(e) = handle.reload(new_filter) {
                            error!("Failed to reload tracing filter: {}", e);
                            return false;
                        }
                        info!("Successfully changed log level to: {}", new_level_upper);
                        true
                    }
                    Err(e) => {
                        error!("Failed to create new tracing filter: {}", e);
                        false
                    }
                }
            } else {
                warn!("Could not acquire tracing reload handle lock");
                false
            }
        } else {
            // No tracing handle available, just log the change
            info!("Log level setting updated to: {} (tracing reload not available)", new_level_upper);
            true
        }
    }

    /// Update max connections setting (temporary, not persisted)
    pub async fn update_max_connections(&self, max_connections: u32) -> bool {
        if max_connections == 0 || max_connections > 10000 {
            error!("Invalid max_connections value: {} (must be 1-10000)", max_connections);
            return false;
        }

        let mut settings = self.settings.write().await;
        settings.max_connections = Some(max_connections);
        
        // Apply to runtime flags immediately (temporary application)
        let mut flags = self.runtime_flags.write().await;
        flags.max_connections = max_connections;
        
        info!("Max connections setting updated to: {} (temporary - resets on restart)", max_connections);
        
        true
    }

    /// Update request timeout setting (temporary, not persisted)
    pub async fn update_request_timeout(&self, timeout_seconds: u32) -> bool {
        if timeout_seconds == 0 || timeout_seconds > 300 {
            error!("Invalid request_timeout_seconds value: {} (must be 1-300)", timeout_seconds);
            return false;
        }

        let mut settings = self.settings.write().await;
        settings.request_timeout_seconds = Some(timeout_seconds);
        
        // Apply to runtime flags immediately (temporary application)
        let mut flags = self.runtime_flags.write().await;
        flags.request_timeout_seconds = timeout_seconds;
        
        info!("Request timeout updated to: {} seconds (temporary - resets on restart)", timeout_seconds);
        
        true
    }

    /// Update request logging setting (temporary, not persisted)
    pub async fn update_request_logging(&self, enable: bool) {
        let mut settings = self.settings.write().await;
        settings.enable_request_logging = enable;
        
        // Apply to runtime flags immediately (temporary application)
        let mut flags = self.runtime_flags.write().await;
        flags.request_logging_enabled = enable;
        
        info!("Request logging {} (temporary - resets on restart)", 
              if enable { "enabled" } else { "disabled" });
    }

    /// Update metrics collection setting (temporary, not persisted)
    pub async fn update_metrics_collection(&self, enable: bool) {
        let mut settings = self.settings.write().await;
        settings.enable_metrics = enable;
        
        // Apply to runtime flags immediately (temporary application)
        let mut flags = self.runtime_flags.write().await;
        flags.metrics_enabled = enable;
        
        info!("Metrics collection {} (temporary - resets on restart)", 
              if enable { "enabled" } else { "disabled" });
    }

    /// Bulk update multiple settings
    pub async fn update_multiple(
        &self,
        log_level: Option<&str>,
        max_connections: Option<u32>,
        request_timeout_seconds: Option<u32>,
        enable_request_logging: Option<bool>,
        enable_metrics: Option<bool>,
    ) -> Vec<String> {
        let mut applied_changes = Vec::new();

        // Update log level if provided
        if let Some(level) = log_level {
            if self.update_log_level(level).await {
                applied_changes.push(format!("Log level changed to {}", level.to_uppercase()));
            }
        }

        // Update max connections if provided
        if let Some(max_conn) = max_connections {
            if self.update_max_connections(max_conn).await {
                applied_changes.push(format!("Max connections changed to {max_conn}"));
            }
        }

        // Update request timeout if provided
        if let Some(timeout) = request_timeout_seconds {
            if self.update_request_timeout(timeout).await {
                applied_changes.push(format!("Request timeout changed to {timeout} seconds"));
            }
        }

        // Update request logging if provided
        if let Some(enable_logging) = enable_request_logging {
            self.update_request_logging(enable_logging).await;
            applied_changes.push(format!(
                "Request logging {}", 
                if enable_logging { "enabled" } else { "disabled" }
            ));
        }

        // Update metrics collection if provided
        if let Some(enable_metrics) = enable_metrics {
            self.update_metrics_collection(enable_metrics).await;
            applied_changes.push(format!(
                "Metrics collection {}", 
                if enable_metrics { "enabled" } else { "disabled" }
            ));
        }

        applied_changes
    }

    /// Update feature flags and configuration (temporary, not persisted)
    pub async fn update_feature_flags(
        &self,
        flags: HashMap<String, bool>,
        config: HashMap<String, HashMap<String, serde_json::Value>>,
    ) -> bool {
        let mut runtime_flags = self.runtime_flags.write().await;
        
        // Update the runtime feature flags and config
        runtime_flags.feature_flags = flags.clone();
        runtime_flags.feature_config = config.clone();
        
        info!(
            "Feature flags updated: {} flags, {} configurations (temporary - resets on restart)",
            flags.len(),
            config.len()
        );
        
        true
    }

    /// Get current feature flags (read-only copy)
    pub async fn get_feature_flags(&self) -> HashMap<String, bool> {
        self.runtime_flags.read().await.feature_flags.clone()
    }

    /// Get current feature configuration (read-only copy)
    pub async fn get_feature_config(&self) -> HashMap<String, HashMap<String, serde_json::Value>> {
        self.runtime_flags.read().await.feature_config.clone()
    }

    /// Check if a specific feature flag is enabled
    pub async fn is_feature_enabled(&self, feature_key: &str) -> bool {
        self.runtime_flags
            .read()
            .await
            .feature_flags
            .get(feature_key)
            .copied()
            .unwrap_or(false)
    }

    /// Get configuration for a specific feature
    pub async fn get_feature_configuration(&self, feature_key: &str) -> HashMap<String, serde_json::Value> {
        self.runtime_flags
            .read()
            .await
            .feature_config
            .get(feature_key)
            .cloned()
            .unwrap_or_default()
    }

    /// Initialize feature flags from config (called on startup)
    pub async fn initialize_feature_flags_from_config(&self, config: &crate::config::Config) {
        if let Some(features) = &config.features {
            let mut runtime_flags = self.runtime_flags.write().await;
            runtime_flags.feature_flags = features.flags.clone();
            runtime_flags.feature_config = features.config.clone();
            
            info!(
                "Initialized feature flags from config: {} flags, {} configurations",
                features.flags.len(),
                features.config.len()
            );
        }
    }
}

impl Default for RuntimeSettingsStore {
    fn default() -> Self {
        Self::new()
    }
}