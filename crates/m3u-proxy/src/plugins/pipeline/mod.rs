//! Pipeline plugins for processing data during generation stages
//!
//! Pipeline plugins extend the data processing pipeline with custom logic
//! for transforming channels, EPG data, filtering, and other operations.

pub mod wasm;

use anyhow::Result;
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;

use crate::models::*;
use crate::pipeline::{ChunkSizeManager, PluginIterator, IteratorResult};
use super::shared::{Plugin, PluginCapabilities, PluginDiscoveryConfig, PluginInfo, PluginRegistry};

/// Pipeline-specific plugin trait
#[async_trait]
pub trait PipelinePlugin: Plugin {
    /// Stages this plugin can handle
    fn supported_stages(&self) -> Vec<String>;
    
    /// Execute plugin for a specific stage
    async fn execute_stage(
        &mut self,
        stage: &str,
        context: PipelineStageContext,
    ) -> Result<PipelineStageResult>;
    
    /// Check if plugin can handle specific stage
    fn can_handle_stage(&self, stage: &str) -> bool {
        self.supported_stages().contains(&stage.to_string())
    }
    
    /// Get preferred chunk size for optimal performance
    fn preferred_chunk_size(&self) -> usize {
        1000 // Default
    }
    
    /// Get memory efficiency for this plugin
    fn memory_efficiency(&self) -> super::shared::MemoryEfficiency {
        super::shared::MemoryEfficiency::Medium
    }
}

/// Context provided to pipeline plugins during execution
pub struct PipelineStageContext {
    /// Current stage being processed
    pub stage: String,
    /// Proxy configuration
    pub proxy_config: ResolvedProxyConfig,
    /// Base URL for the service
    pub base_url: String,
    /// Available iterators for data access
    pub iterators: PipelineIterators,
    /// Chunk size manager for dynamic sizing
    pub chunk_manager: Arc<ChunkSizeManager>,
    /// Stage-specific configuration
    pub stage_config: HashMap<String, String>,
}

/// Available iterators for pipeline stage execution
pub struct PipelineIterators {
    pub channel_iterator: Option<Box<dyn PluginIterator<Channel> + Send + Sync>>,
    pub epg_iterator: Option<Box<dyn PluginIterator<crate::pipeline::orchestrator::EpgEntry> + Send + Sync>>,
    pub data_mapping_iterator: Option<Box<dyn PluginIterator<crate::pipeline::orchestrator::DataMappingRule> + Send + Sync>>,
    pub filter_iterator: Option<Box<dyn PluginIterator<crate::pipeline::orchestrator::FilterRule> + Send + Sync>>,
}

/// Result returned by pipeline plugins
#[derive(Debug)]
pub enum PipelineStageResult {
    /// Processed channels
    Channels(Vec<Channel>),
    /// Processed numbered channels
    NumberedChannels(Vec<NumberedChannel>),
    /// Generated M3U content
    M3uContent(String),
    /// Processing completed with no output
    Completed,
    /// Processing failed
    Failed(String),
}

/// Pipeline plugin manager
pub struct PipelinePluginManager {
    registry: PluginRegistry,
    wasm_manager: Option<wasm::WasmPluginManager>,
    discovery_config: PluginDiscoveryConfig,
    chunk_manager: Arc<ChunkSizeManager>,
}

impl PipelinePluginManager {
    /// Create new pipeline plugin manager
    pub fn new(discovery_config: PluginDiscoveryConfig) -> Self {
        Self {
            registry: PluginRegistry::new(),
            wasm_manager: None,
            discovery_config,
            chunk_manager: Arc::new(ChunkSizeManager::default()),
        }
    }
    
    /// Initialize plugin manager and discover plugins
    pub async fn initialize(&mut self) -> Result<()> {
        // Initialize WASM plugin manager
        if self.discovery_config.file_extensions.contains(&"wasm".to_string()) {
            let wasm_config = wasm::WasmPluginConfig {
                enabled: true,
                plugin_directory: self.discovery_config.plugin_directory.clone(),
                ..Default::default()
            };
            
            let host_interface = crate::proxy::wasm_host_interface::WasmHostInterface;
            let mut wasm_manager = wasm::WasmPluginManager::new(wasm_config, host_interface);
            wasm_manager.load_plugins().await?;
            
            self.wasm_manager = Some(wasm_manager);
        }
        
        tracing::info!("Pipeline plugin manager initialized");
        Ok(())
    }
    
    /// Get plugin for specific stage
    pub fn get_plugin_for_stage(&self, stage: &str) -> Option<&dyn PipelinePlugin> {
        // First check WASM plugins
        if let Some(ref wasm_manager) = self.wasm_manager {
            if let Some(plugin) = wasm_manager.get_plugin_for_stage(stage) {
                return Some(plugin);
            }
        }
        
        // Then check registry
        for plugin_name in self.registry.list_plugins() {
            if let Some(plugin) = self.registry.get(plugin_name) {
                // Try to downcast to PipelinePlugin
                // Note: In a real implementation, you'd need proper trait object handling
                // This is simplified for the reorganization example
            }
        }
        
        None
    }
    
    /// Execute stage with appropriate plugin
    pub async fn execute_stage(
        &mut self,
        stage: &str,
        context: PipelineStageContext,
    ) -> Result<PipelineStageResult> {
        if let Some(plugin) = self.get_plugin_for_stage(stage) {
            // Note: This would need proper mutable access in real implementation
            tracing::info!("Executing stage '{}' with plugin", stage);
            // plugin.execute_stage(stage, context).await
            Ok(PipelineStageResult::Completed)
        } else {
            Err(anyhow::anyhow!("No plugin available for stage: {}", stage))
        }
    }
    
    /// Get health status of all pipeline plugins
    pub async fn health_check(&self) -> HashMap<String, bool> {
        let mut health = HashMap::new();
        
        // Check WASM plugins
        if let Some(ref wasm_manager) = self.wasm_manager {
            health.extend(wasm_manager.health_check().await);
        }
        
        // Check registry plugins
        for plugin_name in self.registry.list_plugins() {
            if let Some(plugin) = self.registry.get(plugin_name) {
                health.insert(plugin_name.to_string(), plugin.health_check());
            }
        }
        
        health
    }
    
    /// Get shared chunk manager
    pub fn chunk_manager(&self) -> Arc<ChunkSizeManager> {
        self.chunk_manager.clone()
    }
}