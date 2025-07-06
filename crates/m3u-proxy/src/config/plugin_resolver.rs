//! Plugin resolver for configuration-driven plugin selection
//!
//! This module provides a clean interface for resolving which plugin to use
//! for each pipeline stage based on the configuration, with graceful fallbacks.

use std::collections::HashMap;
use tracing::{debug, warn};

use super::{Config, PipelineConfig, PipelineStrategiesConfig};

/// Resolves plugin names for pipeline stages based on configuration
pub struct PluginResolver {
    /// The selected pipeline strategy name
    strategy_name: String,
    /// Map of stage name -> plugin name for the selected strategy
    stage_plugins: HashMap<String, String>,
    /// Per-plugin configuration
    plugin_configs: HashMap<String, HashMap<String, toml::Value>>,
}

impl PluginResolver {
    /// Create a new plugin resolver from configuration
    pub fn from_config(config: &Config) -> Self {
        // Get the pipeline configuration
        let default_pipeline_config = PipelineConfig::default();
        let pipeline_config = config.pipeline.as_ref().unwrap_or(&default_pipeline_config);
        let strategy_name = pipeline_config.strategy.clone();
        
        // Get the pipeline strategies
        let default_strategies = PipelineStrategiesConfig::default();
        let strategies = config.pipeline_strategies.as_ref()
            .unwrap_or(&default_strategies);
        
        // Resolve the stage -> plugin mappings for the selected strategy
        let stage_plugins = strategies.strategies.get(&strategy_name)
            .cloned()
            .unwrap_or_else(|| {
                warn!(
                    "Pipeline strategy '{}' not found, falling back to native implementations", 
                    strategy_name
                );
                HashMap::new()
            });
            
        // Get plugin configurations
        let plugin_configs = pipeline_config.plugin_configs.clone().unwrap_or_default();
        
        debug!(
            "Initialized plugin resolver: strategy='{}', {} stage mappings, {} plugin configs",
            strategy_name,
            stage_plugins.len(),
            plugin_configs.len()
        );
        
        Self {
            strategy_name,
            stage_plugins,
            plugin_configs,
        }
    }
    
    /// Get the plugin name for a specific stage, returns None if should use native
    pub fn get_plugin_for_stage(&self, stage: &str) -> Option<&str> {
        let plugin_name = self.stage_plugins.get(stage)?;
        debug!("Resolved stage '{}' to plugin '{}'", stage, plugin_name);
        Some(plugin_name)
    }
    
    /// Get the configuration for a specific plugin
    pub fn get_plugin_config(&self, plugin_name: &str) -> Option<&HashMap<String, toml::Value>> {
        self.plugin_configs.get(plugin_name)
    }
    
    /// Get the current strategy name
    pub fn get_strategy_name(&self) -> &str {
        &self.strategy_name
    }
    
    /// Check if a stage should use a plugin (vs native fallback)
    pub fn should_use_plugin(&self, stage: &str) -> bool {
        self.stage_plugins.contains_key(stage)
    }
    
    /// Get all configured stages for the current strategy
    pub fn get_configured_stages(&self) -> Vec<&str> {
        self.stage_plugins.keys().map(|s| s.as_str()).collect()
    }
    
    /// Log the current plugin resolution configuration
    pub fn log_configuration(&self) {
        debug!("Plugin resolution configuration:");
        debug!("  Strategy: {}", self.strategy_name);
        
        if self.stage_plugins.is_empty() {
            debug!("  All stages will use native implementations");
        } else {
            debug!("  Stage -> Plugin mappings:");
            for (stage, plugin) in &self.stage_plugins {
                debug!("    {} -> {}", stage, plugin);
            }
        }
        
        if !self.plugin_configs.is_empty() {
            debug!("  Plugin configurations:");
            for (plugin, config) in &self.plugin_configs {
                debug!("    {}: {} settings", plugin, config.len());
            }
        }
    }
}

/// Helper function to create a plugin resolver from config
pub fn create_plugin_resolver(config: &Config) -> PluginResolver {
    let resolver = PluginResolver::from_config(config);
    resolver.log_configuration();
    resolver
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{PipelineConfig, PipelineStrategiesConfig};
    
    fn create_test_config() -> Config {
        let mut strategies = HashMap::new();
        
        // Create a test strategy
        let mut test_strategy = HashMap::new();
        test_strategy.insert("source_loading".to_string(), "test_loader".to_string());
        test_strategy.insert("filtering".to_string(), "test_filter".to_string());
        strategies.insert("test".to_string(), test_strategy);
        
        // Create plugin configs
        let mut plugin_configs = HashMap::new();
        let mut loader_config = HashMap::new();
        loader_config.insert("chunk_size".to_string(), toml::Value::Integer(1000));
        plugin_configs.insert("test_loader".to_string(), loader_config);
        
        let mut config = Config::default();
        config.pipeline_strategies = Some(PipelineStrategiesConfig { strategies });
        config.pipeline = Some(PipelineConfig {
            strategy: "test".to_string(),
            plugin_configs: Some(plugin_configs),
        });
        
        config
    }
    
    #[test]
    fn test_plugin_resolver_basic() {
        let config = create_test_config();
        let resolver = PluginResolver::from_config(&config);
        
        assert_eq!(resolver.get_strategy_name(), "test");
        assert_eq!(resolver.get_plugin_for_stage("source_loading"), Some("test_loader"));
        assert_eq!(resolver.get_plugin_for_stage("filtering"), Some("test_filter"));
        assert_eq!(resolver.get_plugin_for_stage("unknown_stage"), None);
        
        assert!(resolver.should_use_plugin("source_loading"));
        assert!(!resolver.should_use_plugin("unknown_stage"));
    }
    
    #[test]
    fn test_plugin_config_resolution() {
        let config = create_test_config();
        let resolver = PluginResolver::from_config(&config);
        
        let loader_config = resolver.get_plugin_config("test_loader").unwrap();
        assert_eq!(loader_config.get("chunk_size"), Some(&toml::Value::Integer(1000)));
        
        assert!(resolver.get_plugin_config("unknown_plugin").is_none());
    }
    
    #[test]
    fn test_missing_strategy_fallback() {
        let mut config = Config::default();
        config.pipeline = Some(PipelineConfig {
            strategy: "nonexistent".to_string(),
            plugin_configs: None,
        });
        
        let resolver = PluginResolver::from_config(&config);
        
        // Should gracefully handle missing strategy
        assert_eq!(resolver.get_strategy_name(), "nonexistent");
        assert_eq!(resolver.get_plugin_for_stage("source_loading"), None);
        assert!(!resolver.should_use_plugin("source_loading"));
    }
}