//! Iterator registry for managing all active iterators in the pipeline
//!
//! This module provides centralized management of:
//! - Iterator lifecycle (creation, cloning, destruction)
//! - Consuming iterator enforcement (single instance per type)
//! - Immutable data source storage
//! - Iterator state tracking

use anyhow::{Result, anyhow};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

use super::immutable_sources::{
    ImmutableLogoEnrichedChannelSource, ImmutableLogoEnrichedEpgSource, ImmutableProxyConfigSource,
    ImmutableSourceManager,
};
use super::iterator_traits::PipelineIterator;
use super::iterator_types::*;

/// Iterator instance wrapper
pub enum IteratorInstance {
    /// Consuming iterator that consumes from a source
    Consuming {
        iterator: Box<dyn PipelineIterator<serde_json::Value>>,
        metadata: IteratorMetadata,
    },
    /// Cloning iterator that reads from immutable data
    Cloning {
        source_key: String,
        position: usize,
        buffer: Vec<serde_json::Value>,
        metadata: IteratorMetadata,
    },
}

impl std::fmt::Debug for IteratorInstance {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IteratorInstance::Consuming { metadata, .. } => f
                .debug_struct("Consuming")
                .field("metadata", metadata)
                .field("iterator", &"<dyn PipelineIterator>")
                .finish(),
            IteratorInstance::Cloning {
                source_key,
                position,
                buffer,
                metadata,
            } => f
                .debug_struct("Cloning")
                .field("source_key", source_key)
                .field("position", position)
                .field("buffer_size", &buffer.len())
                .field("metadata", metadata)
                .finish(),
        }
    }
}

/// Registry for managing all iterators in the pipeline
#[derive(Debug)]
pub struct IteratorRegistry {
    /// All active iterator instances
    iterators: RwLock<HashMap<u32, IteratorInstance>>,

    /// Track consuming instances to prevent duplicates
    consuming_instances: RwLock<HashMap<IteratorType, u32>>,

    /// Immutable data source manager
    source_manager: ImmutableSourceManager,

    /// Next available iterator ID
    next_id: AtomicU32,

    /// Next available dynamic iterator ID
    next_dynamic_id: AtomicU32,

    /// Iterator name-to-ID mapping for named lookups
    named_iterators: RwLock<HashMap<String, u32>>,
}

impl IteratorRegistry {
    /// Create a new iterator registry
    pub fn new() -> Self {
        Self {
            iterators: RwLock::new(HashMap::new()),
            consuming_instances: RwLock::new(HashMap::new()),
            source_manager: ImmutableSourceManager::new(),
            next_id: AtomicU32::new(10), // Start after well-known IDs
            next_dynamic_id: AtomicU32::new(1),
            named_iterators: RwLock::new(HashMap::new()),
        }
    }

    /// Register a consuming iterator
    pub async fn register_consuming_iterator(
        &self,
        id: u32,
        iterator_type: IteratorType,
        iterator: Box<dyn PipelineIterator<serde_json::Value>>,
    ) -> Result<()> {
        if !iterator_type.is_consuming() {
            return Err(anyhow!(
                "Iterator type {:?} is not a consuming iterator",
                iterator_type
            ));
        }

        let mut consuming_instances = self.consuming_instances.write().await;
        if consuming_instances.contains_key(&iterator_type) {
            return Err(anyhow!(
                "Consuming iterator {:?} already exists",
                iterator_type
            ));
        }

        let metadata = IteratorMetadata {
            id,
            iterator_type,
            is_consuming: true,
            parent_id: None,
            position: 0,
            chunk_size: 1500, // Default chunk size
        };

        let instance = IteratorInstance::Consuming { iterator, metadata };

        let mut iterators = self.iterators.write().await;
        iterators.insert(id, instance);
        consuming_instances.insert(iterator_type, id);

        info!(
            "Registered consuming iterator: {} (id={})",
            iterator_type.name(),
            id
        );
        Ok(())
    }

    /// Clone an iterator (only works for cloning iterator types)
    pub async fn clone_iterator(&self, source_id: u32, new_id: u32) -> Result<()> {
        let (source_key, source_type) = {
            let iterators = self.iterators.read().await;
            let source = iterators
                .get(&source_id)
                .ok_or_else(|| anyhow!("Source iterator {} not found", source_id))?;

            match source {
                IteratorInstance::Consuming { metadata, .. } => {
                    return Err(anyhow!(
                        "Cannot clone consuming iterator {}",
                        metadata.iterator_type.name()
                    ));
                }
                IteratorInstance::Cloning {
                    source_key,
                    metadata,
                    ..
                } => (source_key.clone(), metadata.iterator_type),
            }
        }; // Read lock dropped here

        // Create new instance from same source
        let metadata = IteratorMetadata {
            id: new_id,
            iterator_type: source_type,
            is_consuming: false,
            parent_id: Some(source_id),
            position: 0,
            chunk_size: 1500,
        };

        let instance = IteratorInstance::Cloning {
            source_key: source_key.clone(),
            position: 0,
            buffer: Vec::new(),
            metadata,
        };

        let mut iterators = self.iterators.write().await;
        iterators.insert(new_id, instance);

        info!(
            "Cloned iterator {} -> {} from source '{}'",
            source_id, new_id, source_key
        );
        Ok(())
    }

    /// Get the next chunk from an iterator
    pub async fn next_chunk(&self, id: u32, chunk_size: usize) -> Result<Vec<serde_json::Value>> {
        let mut iterators = self.iterators.write().await;
        let iterator = iterators
            .get_mut(&id)
            .ok_or_else(|| anyhow!("Iterator {} not found", id))?;

        match iterator {
            IteratorInstance::Consuming { iterator, metadata } => {
                // Update requested chunk size
                metadata.chunk_size = chunk_size;

                // Get chunk from consuming iterator
                match iterator.next_chunk().await? {
                    crate::pipeline::IteratorResult::Chunk(data) => Ok(data),
                    crate::pipeline::IteratorResult::Exhausted => Ok(Vec::new()),
                }
            }
            IteratorInstance::Cloning {
                source_key,
                position,
                buffer,
                metadata,
            } => {
                // Update requested chunk size
                metadata.chunk_size = chunk_size;

                // Get source data based on type - check all source types
                let source_data = if let Some(channel_source) =
                    self.source_manager.get_channel_source(source_key)
                {
                    channel_source.as_json_values()
                } else if let Some(config_source) =
                    self.source_manager.get_config_source(source_key)
                {
                    config_source.data().clone()
                } else if let Some(epg_source) = self.source_manager.get_epg_source(source_key) {
                    epg_source.data().clone()
                } else {
                    return Err(anyhow!(
                        "Source '{}' not found in any source manager",
                        source_key
                    ));
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
            // If it's a consuming iterator, remove from consuming instances tracking
            if let IteratorInstance::Consuming { metadata, .. } = &instance {
                let mut consuming_instances = self.consuming_instances.write().await;
                consuming_instances.remove(&metadata.iterator_type);
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
    pub fn register_channel_source(
        &self,
        key: String,
        source: std::sync::Arc<ImmutableLogoEnrichedChannelSource>,
    ) -> Result<()> {
        self.source_manager.register_channel_source(key, source)
    }

    /// Register an immutable proxy config source
    pub fn register_config_source(
        &self,
        key: String,
        source: std::sync::Arc<ImmutableProxyConfigSource>,
    ) -> Result<()> {
        self.source_manager.register_config_source(key, source)
    }

    /// Register an immutable EPG source
    pub fn register_epg_source(
        &self,
        key: String,
        source: std::sync::Arc<ImmutableLogoEnrichedEpgSource>,
    ) -> Result<()> {
        self.source_manager.register_epg_source(key, source)
    }

    /// Get an immutable channel source by key
    pub fn get_channel_source(
        &self,
        key: &str,
    ) -> Option<std::sync::Arc<ImmutableLogoEnrichedChannelSource>> {
        self.source_manager.get_channel_source(key)
    }

    /// Get an immutable config source by key
    pub fn get_config_source(
        &self,
        key: &str,
    ) -> Option<std::sync::Arc<ImmutableProxyConfigSource>> {
        self.source_manager.get_config_source(key)
    }

    /// Get an immutable EPG source by key
    pub fn get_epg_source(
        &self,
        key: &str,
    ) -> Option<std::sync::Arc<ImmutableLogoEnrichedEpgSource>> {
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
        // Register consuming iterators with well-known IDs
        if let Some(iterator) = channel_iterator {
            self.register_consuming_iterator(
                well_known_ids::CHANNEL_ITERATOR_ID,
                IteratorType::ChannelSource,
                iterator,
            )
            .await?;
        }

        if let Some(iterator) = epg_iterator {
            self.register_consuming_iterator(
                well_known_ids::EPG_ITERATOR_ID,
                IteratorType::EpgSource,
                iterator,
            )
            .await?;
        }

        // Register immutable sources for config data
        if let Some(rules) = data_mapping_rules {
            let source = std::sync::Arc::new(ImmutableProxyConfigSource::new(
                rules,
                IteratorType::ConfigSnapshot(ConfigType::DataMappingRules),
                "Data Mapping Rules".to_string(),
            )?);
            self.register_config_source("data_mapping_rules".to_string(), source)?;
        }

        if let Some(rules) = filter_rules {
            let source = std::sync::Arc::new(ImmutableProxyConfigSource::new(
                rules,
                IteratorType::ConfigSnapshot(ConfigType::FilterRules),
                "Filter Rules".to_string(),
            )?);
            self.register_config_source("filter_rules".to_string(), source)?;
        }

        info!("Populated iterator registry with well-known iterators");
        Ok(())
    }

    /// Create a cloning iterator from an immutable source using well-known ID
    pub async fn create_cloning_iterator(
        &self,
        iterator_type: IteratorType,
        source_key: &str,
    ) -> Result<u32> {
        if iterator_type.is_consuming() {
            return Err(anyhow!(
                "Cannot create cloning iterator for consuming type: {:?}",
                iterator_type
            ));
        }

        // Check if source exists
        let source_exists = match iterator_type {
            IteratorType::ConfigSnapshot(_) => {
                self.source_manager.get_config_source(source_key).is_some()
            }
            IteratorType::LogoChannels => {
                self.source_manager.get_channel_source(source_key).is_some()
            }
            IteratorType::LogoEpg => self.source_manager.get_epg_source(source_key).is_some(),
            _ => false,
        };

        if !source_exists {
            return Err(anyhow!(
                "Source '{}' not found for iterator type {:?}",
                source_key,
                iterator_type
            ));
        }

        // Allocate dynamic ID for the instance
        let id = self.allocate_dynamic_id();

        // Create the iterator instance
        let metadata = IteratorMetadata {
            id,
            iterator_type,
            is_consuming: false,
            parent_id: None,
            position: 0,
            chunk_size: 1500,
        };

        let instance = IteratorInstance::Cloning {
            source_key: source_key.to_string(),
            position: 0,
            buffer: Vec::new(),
            metadata,
        };

        let mut iterators = self.iterators.write().await;
        iterators.insert(id, instance);

        debug!(
            "Created multi-instance iterator {} for type {:?} from source '{}'",
            id, iterator_type, source_key
        );
        Ok(id)
    }

    /// Register logo-enriched channels as an immutable source
    pub fn register_logo_channels(&self, channels: Vec<crate::models::Channel>) -> Result<()> {
        let source = std::sync::Arc::new(ImmutableLogoEnrichedChannelSource::new(
            channels,
            IteratorType::LogoChannels,
        ));
        self.register_channel_source("logo_channels".to_string(), source)
    }

    /// Register logo-enriched EPG as an immutable source
    pub fn register_logo_epg<T: serde::Serialize>(&self, epg_items: Vec<T>) -> Result<()> {
        let source = std::sync::Arc::new(ImmutableLogoEnrichedEpgSource::new(
            epg_items,
            IteratorType::LogoEpg,
        )?);
        self.register_epg_source("logo_epg".to_string(), source)
    }

    /// Clean up all resources (called at pipeline end)
    pub async fn cleanup(&self) {
        let mut iterators = self.iterators.write().await;
        let mut consuming_instances = self.consuming_instances.write().await;
        let mut named_iterators = self.named_iterators.write().await;

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
        consuming_instances.clear();
        named_iterators.clear();

        info!(
            "Iterator registry cleanup: {} iterators, {} sources freed",
            iter_count, source_stats.total_sources
        );
    }

    /// Get registry statistics
    pub async fn stats(&self) -> RegistryStats {
        let iterators = self.iterators.read().await;
        let consuming_instances = self.consuming_instances.read().await;
        let source_stats = self.source_manager.stats();

        let consuming_count = iterators
            .values()
            .filter(|i| matches!(i, IteratorInstance::Consuming { .. }))
            .count();

        let cloning_count = iterators
            .values()
            .filter(|i| matches!(i, IteratorInstance::Cloning { .. }))
            .count();

        RegistryStats {
            total_iterators: iterators.len(),
            singleton_iterators: consuming_count,
            multi_instance_iterators: cloning_count,
            immutable_sources: source_stats.total_sources,
            tracked_singletons: consuming_instances.len(),
        }
    }

    /// Get iterator ID by well-known name
    pub async fn get_iterator_by_name(&self, name: &str, stage_name: &str) -> Result<u32> {
        // Check if we have a named iterator
        {
            let named_iterators = self.named_iterators.read().await;
            if let Some(&iterator_id) = named_iterators.get(name) {
                return Ok(iterator_id);
            }
        }

        // If not found, try to create it based on well-known names
        match name {
            "data_mapping_rules" => {
                // Look for existing config source
                let source_key = format!("data_mapping_rules_{}", stage_name);
                if self.source_manager.get_config_source(&source_key).is_some() {
                    // Create cloning iterator for this config source
                    let iterator_id = self
                        .create_cloning_iterator(
                            IteratorType::ConfigSnapshot(ConfigType::DataMappingRules),
                            &source_key,
                        )
                        .await?;

                    // Cache the name mapping
                    let mut named_iterators = self.named_iterators.write().await;
                    named_iterators.insert(name.to_string(), iterator_id);

                    Ok(iterator_id)
                } else {
                    Err(anyhow!(
                        "No data mapping rules available for stage '{}'",
                        stage_name
                    ))
                }
            }
            "input_channels" => {
                // Input channels are typically handled via plugin output iterators
                // or channel sources, not config snapshots. This case is handled
                // by pre-registering the iterator in the plugin execution logic.
                Err(anyhow!(
                    "Input channels iterator should be pre-registered for stage '{}'",
                    stage_name
                ))
            }
            "filter_rules" => {
                let source_key = "filter_rules";
                if self.source_manager.get_config_source(source_key).is_some() {
                    let iterator_id = self
                        .create_cloning_iterator(
                            IteratorType::ConfigSnapshot(ConfigType::FilterRules),
                            source_key,
                        )
                        .await?;

                    let mut named_iterators = self.named_iterators.write().await;
                    named_iterators.insert(name.to_string(), iterator_id);

                    Ok(iterator_id)
                } else {
                    Err(anyhow!("No filter rules available"))
                }
            }
            _ => Err(anyhow!("Unknown iterator name: '{}'", name)),
        }
    }

    /// Register a named iterator (for custom cases)
    pub async fn register_named_iterator(&self, name: String, iterator_id: u32) -> Result<()> {
        let mut named_iterators = self.named_iterators.write().await;
        named_iterators.insert(name, iterator_id);
        Ok(())
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
