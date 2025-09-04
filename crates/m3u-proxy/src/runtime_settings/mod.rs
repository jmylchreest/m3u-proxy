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
    /// Enable/disable request logging
    pub enable_request_logging: bool,
}

impl Default for RuntimeSettings {
    fn default() -> Self {
        Self {
            log_level: "INFO".to_string(),
            enable_request_logging: true,
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
    /// Runtime feature flags
    pub feature_flags: HashMap<String, bool>,
    /// Runtime feature configuration
    pub feature_config: HashMap<String, HashMap<String, serde_json::Value>>,
}

impl Default for RuntimeFlags {
    fn default() -> Self {
        Self {
            request_logging_enabled: false, // Default to false, override from config
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



    /// Bulk update multiple settings
    pub async fn update_multiple(
        &self,
        log_level: Option<&str>,
        enable_request_logging: Option<bool>,
    ) -> Vec<String> {
        let mut applied_changes = Vec::new();

        // Update log level if provided
        if let Some(level) = log_level
            && self.update_log_level(level).await {
                applied_changes.push(format!("Log level changed to {}", level.to_uppercase()));
            }


        // Update request logging if provided
        if let Some(enable_logging) = enable_request_logging {
            self.update_request_logging(enable_logging).await;
            applied_changes.push(format!(
                "Request logging {}", 
                if enable_logging { "enabled" } else { "disabled" }
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
        let mut runtime_flags = self.runtime_flags.write().await;
        
        // Start with known default features that should always be available
        let mut feature_flags = std::collections::HashMap::new();
        feature_flags.insert("debug-frontend".to_string(), false);
        feature_flags.insert("feature-cache".to_string(), false);
        
        // Override with config values if present
        if let Some(features) = &config.features {
            for (key, value) in &features.flags {
                feature_flags.insert(key.clone(), *value);
            }
            runtime_flags.feature_config = features.config.clone();
            
            info!(
                "Initialized feature flags: {} total ({} from config, {} defaults)",
                feature_flags.len(),
                features.flags.len(),
                feature_flags.len() - features.flags.len()
            );
        } else {
            info!(
                "Initialized feature flags: {} default features (no config provided)",
                feature_flags.len()
            );
        }
        
        runtime_flags.feature_flags = feature_flags;
    }
}

impl Default for RuntimeSettingsStore {
    fn default() -> Self {
        Self::new()
    }
}