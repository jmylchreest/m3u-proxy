//! Iterator registry for managing all active iterators in the pipeline
//!
//! This module provides centralized management of:
//! - Iterator lifecycle (creation, cloning, destruction)
//! - Singleton enforcement for consuming iterators
//! - Immutable data source storage
//! - Iterator state tracking

use std::collections::HashMap;
use std::sync::{Arc, atomic::{AtomicU32, Ordering}};
use std::any::Any;
use anyhow::{Result, anyhow};
use tokio::sync::RwLock;
use tracing::{info, warn, debug};

use super::iterator_types::*;
use super::iterator_traits::PipelineIterator;
use super::immutable_sources::{ImmutableSourceManager, ImmutableLogoEnrichedChannelSource, ImmutableProxyConfigSource, ImmutableLogoEnrichedEpgSource};
use super::plugin_output_iterator::{
    PluginOutputIterator,
    PluginChannelOutputIterator,
    PluginEpgOutputIterator,
    PluginMappingRuleOutputIterator,
};
use crate::models::*;

/// Iterator instance wrapper
pub enum IteratorInstance {
    /// Singleton iterator that consumes from a source
    Singleton {
        iterator: Box<dyn PipelineIterator<serde_json::Value>>,
        metadata: IteratorMetadata,
    },
    /// Multi-instance iterator that reads from immutable data
    MultiInstance {
        source_key: String,
        position: usize,
        buffer: Vec<serde_json::Value>,
        metadata: IteratorMetadata,
    },
}

impl std::fmt::Debug for IteratorInstance {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IteratorInstance::Singleton { metadata, .. } => {
                f.debug_struct("Singleton")
                    .field("metadata", metadata)
                    .field("iterator", &"<dyn PipelineIterator>")
                    .finish()
            }
            IteratorInstance::MultiInstance { source_key, position, buffer, metadata } => {
                f.debug_struct("MultiInstance")
                    .field("source_key", source_key)
                    .field("position", position)
                    .field("buffer_size", &buffer.len())
                    .field("metadata", metadata)
                    .finish()
            }
        }
    }
}


/// Registry for managing all iterators in the pipeline
#[derive(Debug)]
pub struct IteratorRegistry {
    /// All active iterator instances
    iterators: RwLock<HashMap<u32, IteratorInstance>>,
    
    /// Track singleton instances to prevent duplicates
    singletons: RwLock<HashMap<IteratorType, u32>>,
    
    /// Immutable data source manager
    source_manager: ImmutableSourceManager,
    
    /// Next available iterator ID
    next_id: AtomicU32,
    
    /// Next available dynamic iterator ID
    next_dynamic_id: AtomicU32,
    
    /// Plugin-created output iterators
    plugin_output_iterators: RwLock<HashMap<u32, Box<dyn Any + Send + Sync>>>,
}

impl IteratorRegistry {
    /// Create a new iterator registry
    pub fn new() -> Self {
        Self {
            iterators: RwLock::new(HashMap::new()),
            singletons: RwLock::new(HashMap::new()),
            source_manager: ImmutableSourceManager::new(),
            next_id: AtomicU32::new(10), // Start after well-known IDs
            next_dynamic_id: AtomicU32::new(1),
            plugin_output_iterators: RwLock::new(HashMap::new()),
        }
    }
    
    /// Register a singleton iterator
    pub async fn register_singleton(
        &self,
        id: u32,
        iterator_type: IteratorType,
        iterator: Box<dyn PipelineIterator<serde_json::Value>>,
    ) -> Result<()> {
        if !iterator_type.is_singleton() {
            return Err(anyhow!("Iterator type {:?} is not a singleton", iterator_type));
        }
        
        let mut singletons = self.singletons.write().await;
        if singletons.contains_key(&iterator_type) {
            return Err(anyhow!("Singleton iterator {:?} already exists", iterator_type));
        }
        
        let metadata = IteratorMetadata {
            id,
            iterator_type,
            is_singleton: true,
            parent_id: None,
            position: 0,
            chunk_size: 1500, // Default chunk size
        };
        
        let instance = IteratorInstance::Singleton { iterator, metadata };
        
        let mut iterators = self.iterators.write().await;
        iterators.insert(id, instance);
        singletons.insert(iterator_type, id);
        
        info!("Registered singleton iterator: {} (id={})", iterator_type.name(), id);
        Ok(())
    }
    
    
    /// Clone an iterator (only works for multi-instance types)
    pub async fn clone_iterator(&self, source_id: u32, new_id: u32) -> Result<()> {
        let (source_key, source_type) = {
            let iterators = self.iterators.read().await;
            let source = iterators.get(&source_id)
                .ok_or_else(|| anyhow!("Source iterator {} not found", source_id))?;
            
            match source {
                IteratorInstance::Singleton { metadata, .. } => {
                    return Err(anyhow!("Cannot clone singleton iterator {}", metadata.iterator_type.name()));
                },
                IteratorInstance::MultiInstance { source_key, metadata, .. } => {
                    (source_key.clone(), metadata.iterator_type)
                }
            }
        }; // Read lock dropped here
        
        // Create new instance from same source
        let metadata = IteratorMetadata {
            id: new_id,
            iterator_type: source_type,
            is_singleton: false,
            parent_id: Some(source_id),
            position: 0,
            chunk_size: 1500,
        };
        
        let instance = IteratorInstance::MultiInstance {
            source_key: source_key.clone(),
            position: 0,
            buffer: Vec::new(),
            metadata,
        };
        
        let mut iterators = self.iterators.write().await;
        iterators.insert(new_id, instance);
        
        info!("Cloned iterator {} -> {} from source '{}'", source_id, new_id, source_key);
        Ok(())
    }
    
    /// Get the next chunk from an iterator
    pub async fn next_chunk(&self, id: u32, chunk_size: usize) -> Result<Vec<serde_json::Value>> {
        let mut iterators = self.iterators.write().await;
        let iterator = iterators.get_mut(&id)
            .ok_or_else(|| anyhow!("Iterator {} not found", id))?;
        
        match iterator {
            IteratorInstance::Singleton { iterator, metadata } => {
                // Update requested chunk size
                metadata.chunk_size = chunk_size;
                
                // Get chunk from singleton iterator
                match iterator.next_chunk().await? {
                    crate::pipeline::IteratorResult::Chunk(data) => Ok(data),
                    crate::pipeline::IteratorResult::Exhausted => Ok(Vec::new()),
                }
            },
            IteratorInstance::MultiInstance { source_key, position, buffer, metadata } => {
                // Update requested chunk size
                metadata.chunk_size = chunk_size;
                
                // Get source data based on type - check all source types
                let source_data = if let Some(channel_source) = self.source_manager.get_channel_source(source_key) {
                    channel_source.as_json_values()
                } else if let Some(config_source) = self.source_manager.get_config_source(source_key) {
                    config_source.data().clone()
                } else if let Some(epg_source) = self.source_manager.get_epg_source(source_key) {
                    epg_source.data().clone()
                } else {
                    return Err(anyhow!("Source '{}' not found in any source manager", source_key));
                };
                
                // Fill buffer from source
                buffer.clear();
                let end = (*position + chunk_size).min(source_data.len());
                for i in *position..end {
                    buffer.push(source_data[i].clone());
                }
                
                *position = end;
                metadata.position = *position;
                
                Ok(buffer.clone())
            }
        }
    }
    
    /// Close an iterator and free resources
    pub async fn close_iterator(&self, id: u32) -> Result<()> {
        let mut iterators = self.iterators.write().await;
        if let Some(instance) = iterators.remove(&id) {
            // If it's a singleton, remove from singleton tracking
            if let IteratorInstance::Singleton { metadata, .. } = &instance {
                let mut singletons = self.singletons.write().await;
                singletons.remove(&metadata.iterator_type);
            }
            
            info!("Closed iterator {}", id);
            Ok(())
        } else {
            warn!("Attempted to close non-existent iterator {}", id);
            Ok(())
        }
    }
    
    /// Allocate a new dynamic iterator ID
    pub fn allocate_dynamic_id(&self) -> u32 {
        let id = self.next_dynamic_id.fetch_add(1, Ordering::SeqCst);
        well_known_ids::DYNAMIC_ITERATOR_FLAG | id
    }
    
    /// Register an immutable channel source
    pub fn register_channel_source(&self, key: String, source: std::sync::Arc<ImmutableLogoEnrichedChannelSource>) -> Result<()> {
        self.source_manager.register_channel_source(key, source)
    }
    
    /// Register an immutable proxy config source
    pub fn register_config_source(&self, key: String, source: std::sync::Arc<ImmutableProxyConfigSource>) -> Result<()> {
        self.source_manager.register_config_source(key, source)
    }
    
    /// Register an immutable EPG source
    pub fn register_epg_source(&self, key: String, source: std::sync::Arc<ImmutableLogoEnrichedEpgSource>) -> Result<()> {
        self.source_manager.register_epg_source(key, source)
    }
    
    /// Get an immutable channel source by key
    pub fn get_channel_source(&self, key: &str) -> Option<std::sync::Arc<ImmutableLogoEnrichedChannelSource>> {
        self.source_manager.get_channel_source(key)
    }
    
    /// Get an immutable config source by key
    pub fn get_config_source(&self, key: &str) -> Option<std::sync::Arc<ImmutableProxyConfigSource>> {
        self.source_manager.get_config_source(key)
    }
    
    /// Get an immutable EPG source by key
    pub fn get_epg_source(&self, key: &str) -> Option<std::sync::Arc<ImmutableLogoEnrichedEpgSource>> {
        self.source_manager.get_epg_source(key)
    }
    
    /// Helper method to populate registry with well-known iterators from pipeline context
    pub async fn populate_well_known_iterators(
        &self,
        channel_iterator: Option<Box<dyn PipelineIterator<serde_json::Value> + Send + Sync>>,
        epg_iterator: Option<Box<dyn PipelineIterator<serde_json::Value> + Send + Sync>>,
        data_mapping_rules: Option<Vec<serde_json::Value>>,
        filter_rules: Option<Vec<serde_json::Value>>,
    ) -> Result<()> {
        // Register singleton iterators with well-known IDs
        if let Some(iterator) = channel_iterator {
            self.register_singleton(
                well_known_ids::CHANNEL_ITERATOR_ID,
                IteratorType::ChannelSource,
                iterator
            ).await?;
        }
        
        if let Some(iterator) = epg_iterator {
            self.register_singleton(
                well_known_ids::EPG_ITERATOR_ID,
                IteratorType::EpgSource,
                iterator
            ).await?;
        }
        
        // Register immutable sources for config data
        if let Some(rules) = data_mapping_rules {
            let source = std::sync::Arc::new(ImmutableProxyConfigSource::new(
                rules,
                IteratorType::ConfigSnapshot(ConfigType::DataMappingRules),
                "Data Mapping Rules".to_string()
            )?);
            self.register_config_source("data_mapping_rules".to_string(), source)?;
        }
        
        if let Some(rules) = filter_rules {
            let source = std::sync::Arc::new(ImmutableProxyConfigSource::new(
                rules,
                IteratorType::ConfigSnapshot(ConfigType::FilterRules),
                "Filter Rules".to_string()
            )?);
            self.register_config_source("filter_rules".to_string(), source)?;
        }
        
        info!("Populated iterator registry with well-known iterators");
        Ok(())
    }
    
    /// Create a multi-instance iterator from an immutable source using well-known ID
    pub async fn create_multi_instance_iterator(
        &self,
        iterator_type: IteratorType,
        source_key: &str,
    ) -> Result<u32> {
        if iterator_type.is_singleton() {
            return Err(anyhow!("Cannot create multi-instance iterator for singleton type: {:?}", iterator_type));
        }
        
        // Check if source exists
        let source_exists = match iterator_type {
            IteratorType::ConfigSnapshot(_) => self.source_manager.get_config_source(source_key).is_some(),
            IteratorType::LogoChannels => self.source_manager.get_channel_source(source_key).is_some(),
            IteratorType::LogoEpg => self.source_manager.get_epg_source(source_key).is_some(),
            _ => false,
        };
        
        if !source_exists {
            return Err(anyhow!("Source '{}' not found for iterator type {:?}", source_key, iterator_type));
        }
        
        // Allocate dynamic ID for the instance
        let id = self.allocate_dynamic_id();
        
        // Create the iterator instance
        let metadata = IteratorMetadata {
            id,
            iterator_type,
            is_singleton: false,
            parent_id: None,
            position: 0,
            chunk_size: 1500,
        };
        
        let instance = IteratorInstance::MultiInstance {
            source_key: source_key.to_string(),
            position: 0,
            buffer: Vec::new(),
            metadata,
        };
        
        let mut iterators = self.iterators.write().await;
        iterators.insert(id, instance);
        
        info!("Created multi-instance iterator {} for type {:?} from source '{}'", id, iterator_type, source_key);
        Ok(id)
    }
    
    /// Register logo-enriched channels as an immutable source
    pub fn register_logo_channels(&self, channels: Vec<crate::models::Channel>) -> Result<()> {
        let source = std::sync::Arc::new(ImmutableLogoEnrichedChannelSource::new(channels, IteratorType::LogoChannels));
        self.register_channel_source("logo_channels".to_string(), source)
    }
    
    /// Register logo-enriched EPG as an immutable source
    pub fn register_logo_epg<T: serde::Serialize>(&self, epg_items: Vec<T>) -> Result<()> {
        let source = std::sync::Arc::new(ImmutableLogoEnrichedEpgSource::new(epg_items, IteratorType::LogoEpg)?);
        self.register_epg_source("logo_epg".to_string(), source)
    }
    
    /// Clean up all resources (called at pipeline end)
    pub async fn cleanup(&self) {
        let mut iterators = self.iterators.write().await;
        let mut singletons = self.singletons.write().await;
        
        let iter_count = iterators.len();
        let source_stats = self.source_manager.cleanup().unwrap_or_else(|e| {
            warn!("Failed to get source cleanup stats: {}", e);
            super::immutable_sources::ImmutableSourceStats {
                channel_sources: 0,
                config_sources: 0,
                epg_sources: 0,
                total_sources: 0,
                manager_age: std::time::Duration::from_secs(0),
            }
        });
        
        iterators.clear();
        singletons.clear();
        
        info!("Iterator registry cleanup: {} iterators, {} sources freed", iter_count, source_stats.total_sources);
    }
    
    /// Get registry statistics
    pub async fn stats(&self) -> RegistryStats {
        let iterators = self.iterators.read().await;
        let singletons = self.singletons.read().await;
        let source_stats = self.source_manager.stats();
        
        let singleton_count = iterators.values()
            .filter(|i| matches!(i, IteratorInstance::Singleton { .. }))
            .count();
            
        let multi_instance_count = iterators.values()
            .filter(|i| matches!(i, IteratorInstance::MultiInstance { .. }))
            .count();
        
        RegistryStats {
            total_iterators: iterators.len(),
            singleton_iterators: singleton_count,
            multi_instance_iterators: multi_instance_count,
            immutable_sources: source_stats.total_sources,
            tracked_singletons: singletons.len(),
        }
    }
    
    /// Create a new plugin output iterator
    pub async fn create_plugin_output_iterator(&self, iterator_type: IteratorType) -> Result<u32> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        
        let iterator: Box<dyn Any + Send + Sync> = match iterator_type {
            IteratorType::Channel => Box::new(PluginChannelOutputIterator::new()),
            IteratorType::EpgEntry => Box::new(PluginEpgOutputIterator::new()),
            IteratorType::DataMappingRule => Box::new(PluginMappingRuleOutputIterator::new()),
            _ => return Err(anyhow!("Unsupported iterator type for plugin output: {:?}", iterator_type)),
        };
        
        let mut output_iterators = self.plugin_output_iterators.write().await;
        output_iterators.insert(id, iterator);
        
        info!("Created plugin output iterator {} for type {:?}", id, iterator_type);
        Ok(id)
    }
    
    /// Push data to a plugin output iterator
    pub async fn push_to_plugin_iterator(&self, iterator_id: u32, data: &[u8]) -> Result<()> {
        let output_iterators = self.plugin_output_iterators.read().await;
        
        if let Some(iterator_box) = output_iterators.get(&iterator_id) {
            // Try to downcast to the appropriate type
            if let Some(channel_iter) = iterator_box.downcast_ref::<PluginChannelOutputIterator>() {
                channel_iter.push_chunk(data)?;
            } else if let Some(epg_iter) = iterator_box.downcast_ref::<PluginEpgOutputIterator>() {
                epg_iter.push_chunk(data)?;
            } else if let Some(mapping_iter) = iterator_box.downcast_ref::<PluginMappingRuleOutputIterator>() {
                mapping_iter.push_chunk(data)?;
            } else {
                return Err(anyhow!("Unknown iterator type for ID {}", iterator_id));
            }
            
            Ok(())
        } else {
            Err(anyhow!("Plugin output iterator {} not found", iterator_id))
        }
    }
    
    /// Finalize a plugin output iterator
    pub async fn finalize_plugin_iterator(&self, iterator_id: u32) -> Result<()> {
        let output_iterators = self.plugin_output_iterators.read().await;
        
        if let Some(iterator_box) = output_iterators.get(&iterator_id) {
            // Try to downcast to the appropriate type
            if let Some(channel_iter) = iterator_box.downcast_ref::<PluginChannelOutputIterator>() {
                channel_iter.finalize();
            } else if let Some(epg_iter) = iterator_box.downcast_ref::<PluginEpgOutputIterator>() {
                epg_iter.finalize();
            } else if let Some(mapping_iter) = iterator_box.downcast_ref::<PluginMappingRuleOutputIterator>() {
                mapping_iter.finalize();
            } else {
                return Err(anyhow!("Unknown iterator type for ID {}", iterator_id));
            }
            
            info!("Finalized plugin output iterator {}", iterator_id);
            Ok(())
        } else {
            Err(anyhow!("Plugin output iterator {} not found", iterator_id))
        }
    }
}

#[derive(Debug)]
pub struct RegistryStats {
    pub total_iterators: usize,
    pub singleton_iterators: usize,
    pub multi_instance_iterators: usize,
    pub immutable_sources: usize,
    pub tracked_singletons: usize,
}

impl Default for IteratorRegistry {
    fn default() -> Self {
        Self::new()
    }
}