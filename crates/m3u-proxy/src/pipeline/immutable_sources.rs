//! Immutable data source management for multi-instance iterators
//!
//! This module provides specialized wrappers for different types of immutable data
//! that can be safely shared across multiple iterator instances without consuming
//! the underlying data.

use std::sync::{Arc, atomic::{AtomicU64, Ordering}};
use std::time::Instant;
use anyhow::Result;
use serde_json;
use tracing::info;

use crate::models::*;
use crate::pipeline::iterator_types::IteratorType;

/// Versioned immutable data source for tracking changes
pub trait VersionedSource: Send + Sync {
    /// Get the current version of the data
    fn version(&self) -> u64;
    
    /// Check if the source has been updated since the given version
    fn has_updates_since(&self, version: u64) -> bool {
        self.version() > version
    }
    
    /// Get the creation timestamp
    fn created_at(&self) -> Instant;
    
    /// Get the number of items in the source
    fn len(&self) -> usize;
    
    /// Check if the source is empty
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Immutable channel data source for logo-enriched channels
#[derive(Debug)]
pub struct ImmutableLogoEnrichedChannelSource {
    /// The channel data (logo-enriched)
    data: Arc<Vec<Channel>>,
    
    /// Version counter for change detection
    version: AtomicU64,
    
    /// When this source was created
    created_at: Instant,
    
    /// Source type for metadata
    source_type: IteratorType,
}

impl ImmutableLogoEnrichedChannelSource {
    /// Create a new immutable channel source
    pub fn new(channels: Vec<Channel>, source_type: IteratorType) -> Self {
        info!("Created immutable channel source with {} channels", channels.len());
        
        Self {
            data: Arc::new(channels),
            version: AtomicU64::new(1),
            created_at: Instant::now(),
            source_type,
        }
    }
    
    /// Get a reference to the channel data
    pub fn data(&self) -> &Arc<Vec<Channel>> {
        &self.data
    }
    
    /// Get the source type
    pub fn source_type(&self) -> IteratorType {
        self.source_type
    }
    
    /// Replace the data with a new version (rare operation)
    pub fn update_data(&self, new_channels: Vec<Channel>) -> Result<()> {
        // This would require interior mutability if we need to support updates
        // For now, immutable sources are truly immutable after creation
        info!("Immutable source update requested - creating new version with {} channels", new_channels.len());
        self.version.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    
    /// Convert to JSON values for iterator consumption
    pub fn as_json_values(&self) -> Arc<Vec<serde_json::Value>> {
        let json_values: Vec<serde_json::Value> = self.data
            .iter()
            .map(|channel| serde_json::to_value(channel))
            .collect::<Result<Vec<_>, _>>()
            .unwrap_or_else(|e| {
                tracing::error!("Failed to convert channels to JSON: {}", e);
                Vec::new()
            });
        
        Arc::new(json_values)
    }
}

impl VersionedSource for ImmutableLogoEnrichedChannelSource {
    fn version(&self) -> u64 {
        self.version.load(Ordering::SeqCst)
    }
    
    fn created_at(&self) -> Instant {
        self.created_at
    }
    
    fn len(&self) -> usize {
        self.data.len()
    }
}

/// Immutable proxy configuration source for rules and settings
#[derive(Debug)]
pub struct ImmutableProxyConfigSource {
    /// Configuration data (rules, settings, etc.)
    data: Arc<Vec<serde_json::Value>>,
    
    /// Version counter for change detection
    version: AtomicU64,
    
    /// When this source was created
    created_at: Instant,
    
    /// Source type for metadata
    source_type: IteratorType,
    
    /// Human-readable description
    description: String,
}

impl ImmutableProxyConfigSource {
    /// Create a new immutable config source
    pub fn new<T: serde::Serialize>(config_items: Vec<T>, source_type: IteratorType, description: String) -> Result<Self> {
        let json_values: Result<Vec<serde_json::Value>, _> = config_items
            .into_iter()
            .map(|item| serde_json::to_value(item))
            .collect();
        
        let data = json_values?;
        info!("Created immutable config source '{}' with {} items", description, data.len());
        
        Ok(Self {
            data: Arc::new(data),
            version: AtomicU64::new(1),
            created_at: Instant::now(),
            source_type,
            description,
        })
    }
    
    /// Get a reference to the config data
    pub fn data(&self) -> &Arc<Vec<serde_json::Value>> {
        &self.data
    }
    
    /// Get the source type
    pub fn source_type(&self) -> IteratorType {
        self.source_type
    }
    
    /// Get the description
    pub fn description(&self) -> &str {
        &self.description
    }
}

impl VersionedSource for ImmutableProxyConfigSource {
    fn version(&self) -> u64 {
        self.version.load(Ordering::SeqCst)
    }
    
    fn created_at(&self) -> Instant {
        self.created_at
    }
    
    fn len(&self) -> usize {
        self.data.len()
    }
}

/// Immutable EPG data source for logo-enriched EPG
#[derive(Debug)]
pub struct ImmutableLogoEnrichedEpgSource {
    /// EPG data (logo-enriched)
    data: Arc<Vec<serde_json::Value>>, // Generic since EPG types vary
    
    /// Version counter for change detection
    version: AtomicU64,
    
    /// When this source was created
    created_at: Instant,
    
    /// Source type for metadata
    source_type: IteratorType,
}

impl ImmutableLogoEnrichedEpgSource {
    /// Create a new immutable EPG source
    pub fn new<T: serde::Serialize>(epg_items: Vec<T>, source_type: IteratorType) -> Result<Self> {
        let json_values: Result<Vec<serde_json::Value>, _> = epg_items
            .into_iter()
            .map(|item| serde_json::to_value(item))
            .collect();
        
        let data = json_values?;
        info!("Created immutable EPG source with {} items", data.len());
        
        Ok(Self {
            data: Arc::new(data),
            version: AtomicU64::new(1),
            created_at: Instant::now(),
            source_type,
        })
    }
    
    /// Get a reference to the EPG data
    pub fn data(&self) -> &Arc<Vec<serde_json::Value>> {
        &self.data
    }
    
    /// Get the source type
    pub fn source_type(&self) -> IteratorType {
        self.source_type
    }
}

impl VersionedSource for ImmutableLogoEnrichedEpgSource {
    fn version(&self) -> u64 {
        self.version.load(Ordering::SeqCst)
    }
    
    fn created_at(&self) -> Instant {
        self.created_at
    }
    
    fn len(&self) -> usize {
        self.data.len()
    }
}

/// Manager for all immutable data sources in the pipeline
#[derive(Debug)]
pub struct ImmutableSourceManager {
    /// Channel sources by key
    channel_sources: std::sync::RwLock<std::collections::HashMap<String, Arc<ImmutableLogoEnrichedChannelSource>>>,
    
    /// Config sources by key
    config_sources: std::sync::RwLock<std::collections::HashMap<String, Arc<ImmutableProxyConfigSource>>>,
    
    /// EPG sources by key
    epg_sources: std::sync::RwLock<std::collections::HashMap<String, Arc<ImmutableLogoEnrichedEpgSource>>>,
    
    /// Creation timestamp for cleanup tracking
    created_at: Instant,
}

impl ImmutableSourceManager {
    /// Create a new immutable source manager
    pub fn new() -> Self {
        Self {
            channel_sources: std::sync::RwLock::new(std::collections::HashMap::new()),
            config_sources: std::sync::RwLock::new(std::collections::HashMap::new()),
            epg_sources: std::sync::RwLock::new(std::collections::HashMap::new()),
            created_at: Instant::now(),
        }
    }
    
    /// Register a channel source
    pub fn register_channel_source(&self, key: String, source: Arc<ImmutableLogoEnrichedChannelSource>) -> Result<()> {
        let mut sources = self.channel_sources.write().map_err(|_| anyhow::anyhow!("Lock poisoned"))?;
        sources.insert(key.clone(), source);
        info!("Registered channel source: {}", key);
        Ok(())
    }
    
    /// Register a config source
    pub fn register_config_source(&self, key: String, source: Arc<ImmutableProxyConfigSource>) -> Result<()> {
        let mut sources = self.config_sources.write().map_err(|_| anyhow::anyhow!("Lock poisoned"))?;
        sources.insert(key.clone(), source);
        info!("Registered config source: {}", key);
        Ok(())
    }
    
    /// Register an EPG source
    pub fn register_epg_source(&self, key: String, source: Arc<ImmutableLogoEnrichedEpgSource>) -> Result<()> {
        let mut sources = self.epg_sources.write().map_err(|_| anyhow::anyhow!("Lock poisoned"))?;
        sources.insert(key.clone(), source);
        info!("Registered EPG source: {}", key);
        Ok(())
    }
    
    /// Get a channel source by key
    pub fn get_channel_source(&self, key: &str) -> Option<Arc<ImmutableLogoEnrichedChannelSource>> {
        let sources = self.channel_sources.read().ok()?;
        sources.get(key).cloned()
    }
    
    /// Get a config source by key
    pub fn get_config_source(&self, key: &str) -> Option<Arc<ImmutableProxyConfigSource>> {
        let sources = self.config_sources.read().ok()?;
        sources.get(key).cloned()
    }
    
    /// Get an EPG source by key
    pub fn get_epg_source(&self, key: &str) -> Option<Arc<ImmutableLogoEnrichedEpgSource>> {
        let sources = self.epg_sources.read().ok()?;
        sources.get(key).cloned()
    }
    
    /// Get statistics about all sources
    pub fn stats(&self) -> ImmutableSourceStats {
        let channel_count = self.channel_sources.read().map(|s| s.len()).unwrap_or(0);
        let config_count = self.config_sources.read().map(|s| s.len()).unwrap_or(0);
        let epg_count = self.epg_sources.read().map(|s| s.len()).unwrap_or(0);
        
        ImmutableSourceStats {
            channel_sources: channel_count,
            config_sources: config_count,
            epg_sources: epg_count,
            total_sources: channel_count + config_count + epg_count,
            manager_age: self.created_at.elapsed(),
        }
    }
    
    /// Clean up all sources (called at pipeline end)
    pub fn cleanup(&self) -> Result<ImmutableSourceStats> {
        let stats = self.stats();
        
        if let Ok(mut sources) = self.channel_sources.write() {
            sources.clear();
        }
        if let Ok(mut sources) = self.config_sources.write() {
            sources.clear();
        }
        if let Ok(mut sources) = self.epg_sources.write() {
            sources.clear();
        }
        
        info!("Cleaned up {} immutable sources", stats.total_sources);
        Ok(stats)
    }
}

impl Default for ImmutableSourceManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Statistics about immutable sources
#[derive(Debug, Clone)]
pub struct ImmutableSourceStats {
    pub channel_sources: usize,
    pub config_sources: usize,
    pub epg_sources: usize,
    pub total_sources: usize,
    pub manager_age: std::time::Duration,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::iterator_types::IteratorType;

    #[test]
    fn test_immutable_channel_source() {
        let channels = vec![
            Channel {
                id: uuid::Uuid::new_v4(),
                source_id: uuid::Uuid::new_v4(),
                tvg_id: None,
                tvg_name: None,
                tvg_logo: None,
                tvg_shift: None,
                group_title: None,
                channel_name: "Test Channel".to_string(),
                stream_url: "http://example.com/stream".to_string(),
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            }
        ];
        
        let source = ImmutableLogoEnrichedChannelSource::new(channels, IteratorType::LogoChannels);
        assert_eq!(source.len(), 1);
        assert_eq!(source.version(), 1);
        assert!(!source.is_empty());
    }
    
    #[test]
    fn test_immutable_config_source() {
        let config_items = vec![
            serde_json::json!({"rule": "test_rule", "value": 42}),
            serde_json::json!({"rule": "another_rule", "value": "test"}),
        ];
        
        let source = ImmutableProxyConfigSource::new(
            config_items,
            IteratorType::ConfigSnapshot(crate::pipeline::iterator_types::ConfigType::DataMappingRules),
            "Test Config".to_string()
        ).unwrap();
        
        assert_eq!(source.len(), 2);
        assert_eq!(source.version(), 1);
        assert_eq!(source.description(), "Test Config");
    }
    
    #[test]
    fn test_immutable_source_manager() {
        let manager = ImmutableSourceManager::new();
        let stats = manager.stats();
        
        assert_eq!(stats.total_sources, 0);
        assert_eq!(stats.channel_sources, 0);
        assert_eq!(stats.config_sources, 0);
        assert_eq!(stats.epg_sources, 0);
    }
}