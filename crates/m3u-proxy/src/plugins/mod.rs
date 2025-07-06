//! Plugin system for extensible functionality
//!
//! This module provides a complete plugin ecosystem supporting different
//! types of plugins for various aspects of the m3u-proxy system:
//!
//! - **Pipeline Plugins**: Process data during generation pipeline stages
//! - **Relay Plugins**: Handle stream relaying and proxying
//! - **Proxy Plugins**: Add functionality like analytics, authentication, etc.
//! - **Shared**: Common plugin infrastructure and utilities

pub mod pipeline;
pub mod proxy;
pub mod relay;
pub mod shared;
pub mod wasm_host_factory;

// Re-export core plugin traits and types from shared
pub use shared::{Plugin, PluginCapabilities, PluginInfo, PluginRegistry, PluginType};
pub use wasm_host_factory::{WasmHostFunctionFactory, HostFunctionContext, HostFunctionContextBuilder};

/// Plugin system configuration
#[derive(Debug, Clone)]
pub struct PluginSystemConfig {
    /// Enable pipeline plugins
    pub enable_pipeline_plugins: bool,
    /// Enable relay plugins
    pub enable_relay_plugins: bool,
    /// Enable proxy plugins
    pub enable_proxy_plugins: bool,
    /// Plugin discovery configuration
    pub discovery_config: shared::PluginDiscoveryConfig,
    /// Maximum plugins per type
    pub max_plugins_per_type: usize,
    /// Plugin timeout in seconds
    pub plugin_timeout_seconds: u64,
}

impl Default for PluginSystemConfig {
    fn default() -> Self {
        Self {
            enable_pipeline_plugins: true,
            enable_relay_plugins: false,
            enable_proxy_plugins: false,
            discovery_config: shared::PluginDiscoveryConfig::default(),
            max_plugins_per_type: 10,
            plugin_timeout_seconds: 30,
        }
    }
}

/// Central plugin manager for all plugin types
pub struct PluginManager {
    config: PluginSystemConfig,
    pipeline_manager: Option<pipeline::PipelinePluginManager>,
    relay_manager: Option<relay::RelayPluginManager>,
    proxy_manager: Option<proxy::ProxyPluginManager>,
}

impl PluginManager {
    /// Create new plugin manager with configuration
    pub fn new(config: PluginSystemConfig) -> Self {
        Self {
            pipeline_manager: if config.enable_pipeline_plugins {
                Some(pipeline::PipelinePluginManager::new(
                    config.discovery_config.clone()
                ))
            } else {
                None
            },
            relay_manager: if config.enable_relay_plugins {
                Some(relay::RelayPluginManager::new())
            } else {
                None
            },
            proxy_manager: if config.enable_proxy_plugins {
                Some(proxy::ProxyPluginManager::new())
            } else {
                None
            },
            config,
        }
    }
    
    /// Initialize all enabled plugin managers
    pub async fn initialize(&mut self) -> anyhow::Result<()> {
        if let Some(ref mut pm) = self.pipeline_manager {
            pm.initialize().await?;
        }
        
        if let Some(ref mut rm) = self.relay_manager {
            rm.initialize().await?;
        }
        
        if let Some(ref mut pm) = self.proxy_manager {
            pm.initialize().await?;
        }
        
        Ok(())
    }
    
    /// Get pipeline plugin manager
    pub fn pipeline(&self) -> Option<&pipeline::PipelinePluginManager> {
        self.pipeline_manager.as_ref()
    }
    
    /// Get relay plugin manager
    pub fn relay(&self) -> Option<&relay::RelayPluginManager> {
        self.relay_manager.as_ref()
    }
    
    /// Get proxy plugin manager
    pub fn proxy(&self) -> Option<&proxy::ProxyPluginManager> {
        self.proxy_manager.as_ref()
    }
    
    /// Get overall plugin system health
    pub async fn health_check(&self) -> std::collections::HashMap<String, bool> {
        let mut health = std::collections::HashMap::new();
        
        if let Some(ref pm) = self.pipeline_manager {
            health.extend(pm.health_check().await);
        }
        
        if let Some(ref rm) = self.relay_manager {
            health.extend(rm.health_check().await);
        }
        
        if let Some(ref pm) = self.proxy_manager {
            health.extend(pm.health_check().await);
        }
        
        health
    }
}