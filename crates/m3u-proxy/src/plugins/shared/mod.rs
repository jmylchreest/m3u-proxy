//! Shared plugin infrastructure and utilities
//!
//! This module contains common types, traits, and utilities used across
//! all plugin types in the m3u-proxy system.

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Core plugin types
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PluginType {
    /// Pipeline processing plugins (data transformation)
    Pipeline,
    /// Stream relay and proxying plugins
    Relay,
    /// Proxy functionality plugins (analytics, auth, etc.)
    Proxy,
}

/// Plugin information and metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginInfo {
    pub name: String,
    pub version: String,
    pub author: String,
    pub license: String,
    pub description: String,
    pub plugin_type: PluginType,
    pub supported_features: Vec<String>,
}

/// Plugin capabilities and configuration
#[derive(Debug, Clone)]
pub struct PluginCapabilities {
    /// Memory efficiency rating
    pub memory_efficiency: MemoryEfficiency,
    /// Whether plugin supports hot reloading
    pub supports_hot_reload: bool,
    /// Whether plugin requires exclusive access
    pub requires_exclusive_access: bool,
    /// Maximum concurrent instances
    pub max_concurrent_instances: usize,
    /// Estimated CPU usage
    pub estimated_cpu_usage: CpuUsage,
    /// Estimated memory usage in MB
    pub estimated_memory_usage_mb: usize,
}

/// Memory efficiency rating
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub enum MemoryEfficiency {
    High,
    Medium, 
    Low,
    Adaptive,
}

/// CPU usage estimation
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub enum CpuUsage {
    Low,
    Medium,
    High,
    Variable,
}

/// Core plugin trait implemented by all plugin types
#[async_trait]
pub trait Plugin: Send + Sync {
    /// Get plugin information
    fn info(&self) -> &PluginInfo;
    
    /// Get plugin capabilities
    fn capabilities(&self) -> &PluginCapabilities;
    
    /// Initialize plugin with configuration
    async fn initialize(&mut self, config: HashMap<String, String>) -> Result<()>;
    
    /// Shutdown plugin and cleanup resources
    async fn shutdown(&mut self) -> Result<()>;
    
    /// Get plugin health status
    fn health_check(&self) -> bool;
    
    /// Get plugin metrics/statistics
    fn get_metrics(&self) -> HashMap<String, f64>;
}

/// Plugin discovery configuration
#[derive(Debug, Clone)]
pub struct PluginDiscoveryConfig {
    /// Directory to search for plugins
    pub plugin_directory: String,
    /// Enable automatic discovery
    pub enable_auto_discovery: bool,
    /// File extensions to search for
    pub file_extensions: Vec<String>,
    /// Maximum discovery depth
    pub max_depth: usize,
    /// Include subdirectories
    pub include_subdirectories: bool,
}

impl Default for PluginDiscoveryConfig {
    fn default() -> Self {
        Self {
            plugin_directory: "./plugins".to_string(),
            enable_auto_discovery: true,
            file_extensions: vec!["wasm".to_string(), "so".to_string()],
            max_depth: 3,
            include_subdirectories: true,
        }
    }
}

/// Plugin registry for managing loaded plugins
pub struct PluginRegistry {
    plugins: HashMap<String, Box<dyn Plugin>>,
    plugin_types: HashMap<String, PluginType>,
}

impl PluginRegistry {
    /// Create new plugin registry
    pub fn new() -> Self {
        Self {
            plugins: HashMap::new(),
            plugin_types: HashMap::new(),
        }
    }
    
    /// Register a plugin
    pub fn register(&mut self, name: String, plugin: Box<dyn Plugin>) {
        let plugin_type = plugin.info().plugin_type.clone();
        self.plugin_types.insert(name.clone(), plugin_type);
        self.plugins.insert(name, plugin);
    }
    
    /// Unregister a plugin
    pub fn unregister(&mut self, name: &str) -> Option<Box<dyn Plugin>> {
        self.plugin_types.remove(name);
        self.plugins.remove(name)
    }
    
    /// Get plugin by name
    pub fn get(&self, name: &str) -> Option<&dyn Plugin> {
        self.plugins.get(name).map(|p| p.as_ref())
    }
    
    /// Get mutable plugin by name
    pub fn get_mut(&mut self, name: &str) -> bool {
        self.plugins.contains_key(name)
    }
    
    /// List all registered plugins
    pub fn list_plugins(&self) -> Vec<&str> {
        self.plugins.keys().map(|s| s.as_str()).collect()
    }
    
    /// Get plugins by type
    pub fn plugins_by_type(&self, plugin_type: PluginType) -> Vec<&str> {
        self.plugin_types
            .iter()
            .filter(|&(_, &ref t)| *t == plugin_type)
            .map(|(name, _)| name.as_str())
            .collect()
    }
    
    /// Get plugin count
    pub fn plugin_count(&self) -> usize {
        self.plugins.len()
    }
    
    /// Check if registry contains plugin
    pub fn contains(&self, name: &str) -> bool {
        self.plugins.contains_key(name)
    }
}

impl Default for PluginRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Plugin event types for hooks and notifications
#[derive(Debug, Clone)]
pub enum PluginEvent {
    /// Plugin was loaded
    PluginLoaded { name: String },
    /// Plugin was unloaded
    PluginUnloaded { name: String },
    /// Plugin initialization started
    InitializationStarted { name: String },
    /// Plugin initialization completed
    InitializationCompleted { name: String },
    /// Plugin initialization failed
    InitializationFailed { name: String, error: String },
    /// Plugin health check failed
    HealthCheckFailed { name: String },
    /// Plugin performance warning
    PerformanceWarning { name: String, metric: String, value: f64 },
}

/// Plugin event handler trait
#[async_trait]
pub trait PluginEventHandler: Send + Sync {
    /// Handle a plugin event
    async fn handle_event(&self, event: PluginEvent);
}

/// Simple logging event handler
pub struct LoggingEventHandler;

#[async_trait]
impl PluginEventHandler for LoggingEventHandler {
    async fn handle_event(&self, event: PluginEvent) {
        match event {
            PluginEvent::PluginLoaded { name } => {
                tracing::info!("Plugin loaded: {}", name);
            }
            PluginEvent::PluginUnloaded { name } => {
                tracing::info!("Plugin unloaded: {}", name);
            }
            PluginEvent::InitializationStarted { name } => {
                tracing::debug!("Plugin initialization started: {}", name);
            }
            PluginEvent::InitializationCompleted { name } => {
                tracing::info!("Plugin initialization completed: {}", name);
            }
            PluginEvent::InitializationFailed { name, error } => {
                tracing::error!("Plugin initialization failed: {} - {}", name, error);
            }
            PluginEvent::HealthCheckFailed { name } => {
                tracing::warn!("Plugin health check failed: {}", name);
            }
            PluginEvent::PerformanceWarning { name, metric, value } => {
                tracing::warn!("Plugin performance warning: {} - {} = {}", name, metric, value);
            }
        }
    }
}