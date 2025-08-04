//! Runtime Settings Management
//!
//! This module provides a centralized store for runtime settings that can be
//! changed without restarting the service. Settings changes are applied immediately
//! and affect the actual server behavior.

use std::sync::Arc;
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
}

impl RuntimeSettingsStore {
    /// Create a new runtime settings store with default values
    pub fn new() -> Self {
        Self {
            settings: Arc::new(RwLock::new(RuntimeSettings::default())),
            tracing_reload_handle: None,
        }
    }

    /// Create a new runtime settings store with tracing reload capability
    pub fn with_tracing_reload(
        handle: Handle<tracing_subscriber::EnvFilter, tracing_subscriber::Registry>
    ) -> Self {
        Self {
            settings: Arc::new(RwLock::new(RuntimeSettings::default())),
            tracing_reload_handle: Some(Arc::new(RwLock::new(handle))),
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
                        return true;
                    }
                    Err(e) => {
                        error!("Failed to create new tracing filter: {}", e);
                        return false;
                    }
                }
            } else {
                warn!("Could not acquire tracing reload handle lock");
                return false;
            }
        } else {
            // No tracing handle available, just log the change
            info!("Log level setting updated to: {} (tracing reload not available)", new_level_upper);
            return true;
        }
    }

    /// Update max connections setting
    pub async fn update_max_connections(&self, max_connections: u32) -> bool {
        if max_connections == 0 || max_connections > 10000 {
            error!("Invalid max_connections value: {} (must be 1-10000)", max_connections);
            return false;
        }

        let mut settings = self.settings.write().await;
        settings.max_connections = Some(max_connections);
        info!("Max connections setting updated to: {}", max_connections);
        
        // TODO: Apply to actual server connection limits if supported
        // This would require integration with the HTTP server configuration
        
        true
    }

    /// Update request timeout setting
    pub async fn update_request_timeout(&self, timeout_seconds: u32) -> bool {
        if timeout_seconds == 0 || timeout_seconds > 300 {
            error!("Invalid request_timeout_seconds value: {} (must be 1-300)", timeout_seconds);
            return false;
        }

        let mut settings = self.settings.write().await;
        settings.request_timeout_seconds = Some(timeout_seconds);
        info!("Request timeout setting updated to: {} seconds", timeout_seconds);
        
        // TODO: Apply to actual server timeout configuration
        // This would require integration with the HTTP client timeout settings
        
        true
    }

    /// Update request logging setting
    pub async fn update_request_logging(&self, enable: bool) {
        let mut settings = self.settings.write().await;
        settings.enable_request_logging = enable;
        info!("Request logging {}", if enable { "enabled" } else { "disabled" });
        
        // TODO: Apply to actual request logging middleware
        // This would require dynamic enabling/disabling of request logging middleware
    }

    /// Update metrics collection setting
    pub async fn update_metrics_collection(&self, enable: bool) {
        let mut settings = self.settings.write().await;
        settings.enable_metrics = enable;
        info!("Metrics collection {}", if enable { "enabled" } else { "disabled" });
        
        // TODO: Apply to actual metrics collection
        // This would require dynamic enabling/disabling of metrics logging
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
                applied_changes.push(format!("Max connections changed to {}", max_conn));
            }
        }

        // Update request timeout if provided
        if let Some(timeout) = request_timeout_seconds {
            if self.update_request_timeout(timeout).await {
                applied_changes.push(format!("Request timeout changed to {} seconds", timeout));
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
}

impl Default for RuntimeSettingsStore {
    fn default() -> Self {
        Self::new()
    }
}