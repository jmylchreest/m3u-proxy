//! Iterator type system for distinguishing between singleton and multi-instance iterators
//!
//! This module defines the core types for managing different iterator behaviors:
//! - Singleton iterators: Consume data from sources (database, upstream iterators)
//! - Multi-instance iterators: Read from immutable sources without consuming

use std::sync::Arc;
use std::any::Any;

/// Types of iterators in the pipeline system
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IteratorType {
    // Singleton iterators (consuming from sources)
    ChannelSource,       // From database
    EpgSource,          // From database
    MappedChannels,     // From data mapping plugin
    MappedEpg,          // From data mapping plugin
    FilteredChannels,   // From filter plugin
    
    // Multi-instance iterators (reading from immutable sources)
    ConfigSnapshot(ConfigType),  // Configuration data
    LogoChannels,               // From logo plugin (immutable output)
    LogoEpg,                   // From logo plugin (immutable output)
    
    // Plugin output iterator types
    Channel,           // For PluginChannelOutputIterator
    EpgEntry,         // For PluginEpgOutputIterator
    DataMappingRule,  // For PluginMappingRuleOutputIterator
}

/// Types of configuration that can be accessed via iterators
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConfigType {
    DataMappingRules,
    FilterRules,
}

/// Metadata about an iterator instance
#[derive(Debug, Clone)]
pub struct IteratorMetadata {
    /// Unique identifier for this iterator instance
    pub id: u32,
    
    /// Type of iterator (determines behavior)
    pub iterator_type: IteratorType,
    
    /// Whether this is a singleton (only one instance allowed)
    pub is_singleton: bool,
    
    /// Parent iterator ID if this was cloned
    pub parent_id: Option<u32>,
    
    /// Current position (for multi-instance iterators)
    pub position: usize,
    
    /// Chunk size for this iterator
    pub chunk_size: usize,
}

impl IteratorType {
    /// Check if this iterator type is a singleton
    pub fn is_singleton(&self) -> bool {
        match self {
            // Consuming iterators are singletons
            IteratorType::ChannelSource |
            IteratorType::EpgSource |
            IteratorType::MappedChannels |
            IteratorType::MappedEpg |
            IteratorType::FilteredChannels => true,
            
            // Config and output iterators can have multiple instances
            IteratorType::ConfigSnapshot(_) |
            IteratorType::LogoChannels |
            IteratorType::LogoEpg |
            IteratorType::Channel |
            IteratorType::EpgEntry |
            IteratorType::DataMappingRule => false,
        }
    }
    
    /// Get a human-readable name for this iterator type
    pub fn name(&self) -> &'static str {
        match self {
            IteratorType::ChannelSource => "channel_source",
            IteratorType::EpgSource => "epg_source",
            IteratorType::MappedChannels => "mapped_channels",
            IteratorType::MappedEpg => "mapped_epg",
            IteratorType::FilteredChannels => "filtered_channels",
            IteratorType::ConfigSnapshot(ConfigType::DataMappingRules) => "data_mapping_rules",
            IteratorType::ConfigSnapshot(ConfigType::FilterRules) => "filter_rules",
            IteratorType::LogoChannels => "logo_channels",
            IteratorType::LogoEpg => "logo_epg",
            IteratorType::Channel => "channel",
            IteratorType::EpgEntry => "epg_entry",
            IteratorType::DataMappingRule => "data_mapping_rule",
        }
    }
}

/// Well-known iterator IDs used by plugins
pub mod well_known_ids {
    pub const CHANNEL_ITERATOR_ID: u32 = 1;        // Singleton
    pub const EPG_ITERATOR_ID: u32 = 2;            // Singleton
    pub const DATA_MAPPING_RULES_ID: u32 = 3;      // Multi-instance
    pub const FILTER_RULES_ID: u32 = 4;            // Multi-instance
    pub const MAPPED_CHANNELS_ID: u32 = 5;         // Singleton
    pub const MAPPED_EPG_ID: u32 = 6;              // Singleton
    pub const FILTERED_CHANNELS_ID: u32 = 7;       // Singleton
    pub const LOGO_CHANNELS_ID: u32 = 8;           // Multi-instance
    pub const LOGO_EPG_ID: u32 = 9;                // Multi-instance
    
    /// Flag for dynamically allocated iterator IDs
    pub const DYNAMIC_ITERATOR_FLAG: u32 = 0x80000000;
    
    /// Check if an iterator ID is dynamically allocated
    pub fn is_dynamic_id(id: u32) -> bool {
        (id & DYNAMIC_ITERATOR_FLAG) != 0
    }
}

/// Trait for immutable data sources that can spawn multiple iterators
pub trait ImmutableSource: Send + Sync {
    /// The type of data this source provides
    type Item: Send + Sync;
    
    /// Get the underlying data
    fn data(&self) -> Arc<dyn Any + Send + Sync>;
    
    /// Get the total number of items available
    fn len(&self) -> usize;
    
    /// Check if the source is empty
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}