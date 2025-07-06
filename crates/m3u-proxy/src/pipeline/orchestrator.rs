//! Orchestrator iterators for the complete generator pipeline
//!
//! This module provides ordered, multi-source iterators that handle the full
//! proxy generation pipeline: channels, EPG data, filters, and data mapping.

use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;
use uuid::Uuid;

use crate::database::Database;
use crate::models::*;
use crate::pipeline::iterator_traits::PluginIterator;
use crate::pipeline::generic_iterator::{DataLoader, OrderedMultiSourceIterator, SingleSourceLoader, OrderedSingleSourceIterator};
use crate::pipeline::rolling_buffer_iterator::{ActiveDataLoader, BufferConfig, RollingBufferIterator};

/// Data loader for channels
pub struct ChannelLoader;

/// Data loader for channels using UUID source IDs
pub struct UuidChannelLoader;

/// Active data loader for channels (rolling buffer support)
pub struct ActiveChannelLoader;

#[async_trait]
impl DataLoader<Channel, ProxySource> for ChannelLoader {
    async fn load_chunk(&self, database: &Arc<Database>, source: &ProxySource, offset: usize, limit: usize) -> Result<Vec<Channel>> {
        database.get_channels_for_source_paginated(source.source_id, offset, limit).await
    }
    
    fn get_source_id(&self, source: &ProxySource) -> String {
        source.source_id.to_string()
    }
    
    fn get_source_priority(&self, source: &ProxySource) -> i32 {
        source.priority_order
    }
    
    fn get_type_name(&self) -> &'static str {
        "channel"
    }
}

#[async_trait]
impl DataLoader<Channel, uuid::Uuid> for UuidChannelLoader {
    async fn load_chunk(&self, database: &Arc<Database>, source_id: &uuid::Uuid, offset: usize, limit: usize) -> Result<Vec<Channel>> {
        database.get_channels_for_source_paginated(*source_id, offset, limit).await
    }
    
    fn get_source_id(&self, source_id: &uuid::Uuid) -> String {
        source_id.to_string()
    }
    
    fn get_source_priority(&self, _source_id: &uuid::Uuid) -> i32 {
        0 // Default priority for UUID-based loading
    }
    
    fn get_type_name(&self) -> &'static str {
        "channel"
    }
}

#[async_trait]
impl ActiveDataLoader<Channel, ProxySource> for ActiveChannelLoader {
    async fn load_chunk_from_active_source(&self, database: &Arc<Database>, source: &ProxySource, offset: usize, limit: usize) -> Result<Vec<Channel>> {
        database.get_channels_for_active_source_paginated(source.source_id, offset, limit).await
    }
    
    fn get_source_id(&self, source: &ProxySource) -> String {
        source.source_id.to_string()
    }
    
    fn get_source_priority(&self, source: &ProxySource) -> i32 {
        source.priority_order
    }
    
    fn get_type_name(&self) -> &'static str {
        "channel"
    }
}

/// Ordered channel aggregate iterator that streams channels from multiple sources
/// in the order specified by the proxy configuration
pub type OrderedChannelAggregateIterator = OrderedMultiSourceIterator<Channel, ProxySource, ChannelLoader>;

/// Rolling buffer channel iterator for sophisticated buffer management
pub type RollingBufferChannelIterator = RollingBufferIterator<Channel, ProxySource, ActiveChannelLoader>;

// The PluginIterator trait is automatically implemented by OrderedMultiSourceIterator

/// EPG data structure for streaming
#[derive(Debug, Clone)]
pub struct EpgEntry {
    pub channel_id: String,
    pub program_id: String,
    pub title: String,
    pub description: Option<String>,
    pub start_time: chrono::DateTime<chrono::Utc>,
    pub end_time: chrono::DateTime<chrono::Utc>,
    pub source_id: Uuid,
    pub priority: i32,
}

/// Data loader for EPG entries
pub struct EpgLoader;

#[async_trait]
impl DataLoader<EpgEntry, ProxyEpgSourceConfig> for EpgLoader {
    async fn load_chunk(&self, _database: &Arc<Database>, _source: &ProxyEpgSourceConfig, _offset: usize, _limit: usize) -> Result<Vec<EpgEntry>> {
        // TODO: Implement actual EPG data fetching from database
        // For now, return empty to maintain structure
        Ok(Vec::new())
    }
    
    fn get_source_id(&self, source: &ProxyEpgSourceConfig) -> String {
        source.epg_source.id.to_string()
    }
    
    fn get_source_priority(&self, source: &ProxyEpgSourceConfig) -> i32 {
        source.priority_order
    }
    
    fn get_type_name(&self) -> &'static str {
        "EPG"
    }
}

/// Ordered EPG aggregate iterator that streams EPG data from multiple sources
/// in priority order with deduplication
pub type OrderedEpgAggregateIterator = OrderedMultiSourceIterator<EpgEntry, ProxyEpgSourceConfig, EpgLoader>;

// The PluginIterator trait is automatically implemented by OrderedMultiSourceIterator

/// Data mapping rule entry for streaming
#[derive(Debug, Clone)]
pub struct DataMappingRule {
    pub rule_id: Uuid,
    pub source_field: String,
    pub target_field: String,
    pub transformation: String,
    pub priority: i32,
}

/// Data loader for data mapping rules
pub struct DataMappingLoader;

#[async_trait]
impl SingleSourceLoader<DataMappingRule> for DataMappingLoader {
    async fn load_chunk(&self, _database: &Arc<Database>, _source_id: Uuid, _offset: usize, _limit: usize) -> Result<Vec<DataMappingRule>> {
        // TODO: Implement actual data mapping rule fetching from database
        // For now, return empty to maintain structure
        Ok(Vec::new())
    }
    
    fn get_type_name(&self) -> &'static str {
        "data mapping rule"
    }
}

/// Ordered data mapping iterator that streams mapping rules in specified order
pub type OrderedDataMappingIterator = OrderedSingleSourceIterator<DataMappingRule, DataMappingLoader>;

// The PluginIterator trait is automatically implemented by OrderedSingleSourceIterator

/// Filter rule entry for streaming
#[derive(Debug, Clone)]
pub struct FilterRule {
    pub filter_id: Uuid,
    pub rule_type: String,
    pub condition: String,
    pub action: String,
    pub priority: i32,
}

/// Data loader for filter rules
pub struct FilterLoader;

#[async_trait]
impl DataLoader<FilterRule, ProxyFilterConfig> for FilterLoader {
    async fn load_chunk(&self, _database: &Arc<Database>, _source: &ProxyFilterConfig, _offset: usize, _limit: usize) -> Result<Vec<FilterRule>> {
        // TODO: Implement actual filter rule fetching from database
        // For now, return empty to maintain structure
        Ok(Vec::new())
    }
    
    fn get_source_id(&self, source: &ProxyFilterConfig) -> String {
        source.filter.id.to_string()
    }
    
    fn get_source_priority(&self, source: &ProxyFilterConfig) -> i32 {
        source.priority_order
    }
    
    fn get_type_name(&self) -> &'static str {
        "filter rule"
    }
}

/// Ordered filter iterator that streams filter rules in priority order
pub type OrderedFilterIterator = OrderedMultiSourceIterator<FilterRule, ProxyFilterConfig, FilterLoader>;

// The PluginIterator trait is automatically implemented by OrderedMultiSourceIterator

/// Factory for creating orchestrator iterators for the complete pipeline
pub struct OrchestratorIteratorFactory;

impl OrchestratorIteratorFactory {
    /// Filter proxy source configs to only include active ones
    /// Note: This filtering should normally be done at the config resolver level,
    /// but this provides an additional safety check.
    pub fn filter_active_source_configs(source_configs: Vec<ProxySourceConfig>) -> Vec<ProxySourceConfig> {
        let total_sources = source_configs.len();
        let active_sources: Vec<ProxySourceConfig> = source_configs
            .into_iter()
            .filter(|source_config| source_config.source.is_active)
            .collect();
        
        if active_sources.len() != total_sources {
            tracing::info!(
                "Filtered {} source configs to {} active sources for orchestrator",
                total_sources,
                active_sources.len()
            );
        }
        
        active_sources
    }

    /// Convert ProxySourceConfig to ProxySource for the factory methods
    pub fn convert_to_proxy_sources(
        proxy_id: uuid::Uuid,
        source_configs: Vec<ProxySourceConfig>,
    ) -> Vec<ProxySource> {
        source_configs
            .into_iter()
            .map(|config| ProxySource {
                proxy_id,
                source_id: config.source.id,
                priority_order: config.priority_order,
                created_at: chrono::Utc::now(),
            })
            .collect()
    }

    /// Create ordered channel aggregate iterator from proxy configuration
    pub fn create_channel_iterator(
        database: Arc<Database>,
        proxy_sources: Vec<ProxySource>, // Should be pre-sorted by priority_order
        chunk_size: usize,
    ) -> Box<dyn PluginIterator<Channel>> {
        Box::new(OrderedMultiSourceIterator::new(database, proxy_sources, ChannelLoader {}, chunk_size))
    }

    /// Create rolling buffer channel iterator for sophisticated buffer management
    pub fn create_rolling_buffer_channel_iterator(
        database: Arc<Database>,
        proxy_sources: Vec<ProxySource>, // Should be pre-sorted by priority_order and filtered to active
        buffer_config: BufferConfig,
    ) -> Box<dyn PluginIterator<Channel>> {
        // Note: Active filtering should have been done before creating ProxySource objects
        Box::new(RollingBufferIterator::new(database, proxy_sources, ActiveChannelLoader {}, buffer_config))
    }

    /// Create rolling buffer channel iterator from source configs with active filtering
    pub fn create_rolling_buffer_channel_iterator_from_configs(
        database: Arc<Database>,
        proxy_id: uuid::Uuid,
        source_configs: Vec<ProxySourceConfig>,
        buffer_config: BufferConfig,
    ) -> Box<dyn PluginIterator<Channel>> {
        // Filter to only active sources and convert to ProxySource
        let active_configs = Self::filter_active_source_configs(source_configs);
        let proxy_sources = Self::convert_to_proxy_sources(proxy_id, active_configs);
        Box::new(RollingBufferIterator::new(database, proxy_sources, ActiveChannelLoader {}, buffer_config))
    }

    /// Create rolling buffer channel iterator from source configs with cascading buffer integration
    pub fn create_rolling_buffer_channel_iterator_from_configs_with_cascade(
        database: Arc<Database>,
        proxy_id: uuid::Uuid,
        source_configs: Vec<ProxySourceConfig>,
        buffer_config: BufferConfig,
        chunk_manager: Option<Arc<crate::pipeline::chunk_manager::ChunkSizeManager>>,
        stage_name: String,
    ) -> Box<dyn PluginIterator<Channel>> {
        // Filter to only active sources and convert to ProxySource
        let active_configs = Self::filter_active_source_configs(source_configs);
        let proxy_sources = Self::convert_to_proxy_sources(proxy_id, active_configs);
        Box::new(RollingBufferIterator::new_with_chunk_manager(
            database, 
            proxy_sources, 
            ActiveChannelLoader {}, 
            buffer_config,
            chunk_manager,
            stage_name,
        ))
    }

    /// Create channel iterator with active source filtering (legacy compatibility)
    pub fn create_active_channel_iterator(
        database: Arc<Database>,
        proxy_sources: Vec<ProxySource>, // Should be pre-sorted by priority_order
        chunk_size: usize,
    ) -> Box<dyn PluginIterator<Channel>> {
        // Note: Active filtering should have been done before creating ProxySource objects
        Box::new(OrderedMultiSourceIterator::new(database, proxy_sources, ChannelLoader {}, chunk_size))
    }
    
    /// Create ordered EPG aggregate iterator from proxy configuration
    pub fn create_epg_iterator(
        database: Arc<Database>,
        epg_sources: Vec<ProxyEpgSourceConfig>, // Should be pre-sorted by priority_order
        chunk_size: usize,
    ) -> Box<dyn PluginIterator<EpgEntry>> {
        Box::new(OrderedMultiSourceIterator::new(database, epg_sources, EpgLoader {}, chunk_size))
    }
    
    /// Create ordered data mapping iterator
    pub fn create_data_mapping_iterator(
        database: Arc<Database>,
        proxy_id: Uuid,
        chunk_size: usize,
    ) -> Box<dyn PluginIterator<DataMappingRule>> {
        Box::new(OrderedSingleSourceIterator::new(database, DataMappingLoader {}, proxy_id, chunk_size))
    }
    
    /// Create ordered filter iterator from proxy configuration
    pub fn create_filter_iterator(
        database: Arc<Database>,
        proxy_filters: Vec<ProxyFilterConfig>, // Should be pre-sorted by priority_order
        chunk_size: usize,
    ) -> Box<dyn PluginIterator<FilterRule>> {
        Box::new(OrderedMultiSourceIterator::new(database, proxy_filters, FilterLoader {}, chunk_size))
    }
}