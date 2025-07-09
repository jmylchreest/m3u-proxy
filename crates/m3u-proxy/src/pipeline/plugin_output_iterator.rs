//! Plugin output iterator implementation
//!
//! This module provides iterators that can be populated by WASM plugins
//! and then consumed by the host application.

use std::sync::{Arc, Mutex};
use std::collections::VecDeque;
use anyhow::Result;
use serde_json;
use async_trait::async_trait;

use super::iterator_traits::{PipelineIterator, IteratorResult};
use crate::models::*;
use crate::models::data_mapping::DataMappingRule;
use crate::pipeline::orchestrator::EpgEntry;

/// Iterator that can be populated by plugins through host functions
pub struct PluginOutputIterator<T> {
    /// Queue of chunks that have been written by the plugin
    chunks: Arc<Mutex<VecDeque<Vec<T>>>>,
    
    /// Whether the iterator has been finalized (no more data will be added)
    finalized: Arc<Mutex<bool>>,
    
    /// Current position in the current chunk
    current_chunk: Option<Vec<T>>,
    current_position: usize,
    
    /// Total items processed
    total_items: usize,
}

impl<T> PluginOutputIterator<T> 
where 
    T: serde::de::DeserializeOwned + Clone + Send + Sync + 'static
{
    /// Create a new plugin output iterator
    pub fn new() -> Self {
        Self {
            chunks: Arc::new(Mutex::new(VecDeque::new())),
            finalized: Arc::new(Mutex::new(false)),
            current_chunk: None,
            current_position: 0,
            total_items: 0,
        }
    }
    
    /// Push a chunk of data to the iterator (called by host functions)
    pub fn push_chunk(&self, data: &[u8]) -> Result<()> {
        // Deserialize the data
        let items: Vec<T> = serde_json::from_slice(data)?;
        
        // Add to queue
        let mut chunks = self.chunks.lock().unwrap();
        chunks.push_back(items);
        
        Ok(())
    }
    
    /// Mark the iterator as finalized (no more data will be added)
    pub fn finalize(&self) {
        let mut finalized = self.finalized.lock().unwrap();
        *finalized = true;
    }
    
    /// Check if the iterator is finalized
    pub fn is_finalized(&self) -> bool {
        *self.finalized.lock().unwrap()
    }
    
    /// Get the total number of items processed
    pub fn total_items(&self) -> usize {
        self.total_items
    }
}

#[async_trait]
impl<T> PipelineIterator<T> for PluginOutputIterator<T>
where 
    T: serde::de::DeserializeOwned + Clone + Send + Sync + 'static
{
    async fn next_chunk(&mut self) -> Result<IteratorResult<T>> {
        // If we have data in current chunk, return it
        if let Some(ref chunk) = self.current_chunk {
            if self.current_position < chunk.len() {
                let remaining = chunk[self.current_position..].to_vec();
                self.current_position = chunk.len();
                return Ok(IteratorResult::Chunk(remaining));
            }
        }
        
        // Try to get next chunk from queue
        {
            let mut chunks = self.chunks.lock().unwrap();
            if let Some(chunk) = chunks.pop_front() {
                self.total_items += chunk.len();
                self.current_chunk = Some(chunk.clone());
                self.current_position = 0;
                return Ok(IteratorResult::Chunk(chunk));
            }
        }
        
        // No more chunks available
        if self.is_finalized() {
            Ok(IteratorResult::Exhausted)
        } else {
            // Plugin hasn't finished yet, but no data available right now
            // In a real implementation, we might want to wait or yield
            Ok(IteratorResult::Exhausted)
        }
    }
    
    fn reset(&mut self) -> Result<()> {
        // Reset current position
        self.current_position = 0;
        
        // Cannot reset if chunks have been consumed
        // In a real implementation, we'd need to store all chunks
        Err(anyhow::anyhow!("Plugin output iterators cannot be reset after consumption"))
    }
    
    async fn next_chunk_with_size(&mut self, _chunk_size: usize) -> Result<IteratorResult<T>> {
        // Plugin output iterators don't support dynamic chunk sizing
        // as they return whatever the plugin provides
        self.next_chunk().await
    }
    
    async fn set_buffer_size(&mut self, _buffer_size: usize) -> Result<()> {
        // Plugin output iterators manage their own buffer internally
        Ok(())
    }
    
    fn get_current_buffer_size(&self) -> usize {
        let chunks = self.chunks.lock().unwrap();
        chunks.iter().map(|chunk| chunk.len()).sum()
    }
    
    fn get_chunk_size(&self) -> usize {
        // Plugin output iterators don't have a fixed chunk size
        1000 // Default fallback
    }
    
    fn is_exhausted(&self) -> bool {
        let chunks = self.chunks.lock().unwrap();
        self.is_finalized() && chunks.is_empty() && self.current_chunk.is_none()
    }
    
    async fn close(&mut self) -> Result<()> {
        self.finalize();
        Ok(())
    }
}

/// Channel-specific output iterator
pub type PluginChannelOutputIterator = PluginOutputIterator<Channel>;

/// EPG-specific output iterator
pub type PluginEpgOutputIterator = PluginOutputIterator<EpgEntry>;

/// Mapping rule-specific output iterator
pub type PluginMappingRuleOutputIterator = PluginOutputIterator<DataMappingRule>;

/// Factory for creating typed output iterators
pub struct PluginOutputIteratorFactory;

impl PluginOutputIteratorFactory {
    /// Create a channel output iterator
    pub fn create_channel_iterator() -> Box<dyn PipelineIterator<Channel> + Send + Sync> {
        Box::new(PluginChannelOutputIterator::new())
    }
    
    /// Create an EPG output iterator
    pub fn create_epg_iterator() -> Box<dyn PipelineIterator<EpgEntry> + Send + Sync> {
        Box::new(PluginEpgOutputIterator::new())
    }
    
    /// Create a mapping rule output iterator
    pub fn create_mapping_rule_iterator() -> Box<dyn PipelineIterator<DataMappingRule> + Send + Sync> {
        Box::new(PluginMappingRuleOutputIterator::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;
    
    #[tokio::test]
    async fn test_plugin_output_iterator() {
        let mut iterator = PluginChannelOutputIterator::new();
        
        // Create test channel data
        let test_channels = vec![
            Channel {
                id: Uuid::new_v4(),
                source_id: Uuid::new_v4(),
                tvg_id: Some("test1".to_string()),
                tvg_name: Some("Test Channel 1".to_string()),
                tvg_logo: None,
                tvg_shift: None,
                group_title: Some("Test Group".to_string()),
                channel_name: "Test Channel 1".to_string(),
                stream_url: "http://test1.com".to_string(),
                created_at: "2023-01-01T00:00:00Z".to_string(),
                updated_at: "2023-01-01T00:00:00Z".to_string(),
            }
        ];
        
        // Serialize and push to iterator
        let serialized = serde_json::to_vec(&test_channels).unwrap();
        iterator.push_chunk(&serialized).unwrap();
        iterator.finalize();
        
        // Read from iterator
        match iterator.next_chunk().await.unwrap() {
            IteratorResult::Chunk(channels) => {
                assert_eq!(channels.len(), 1);
                assert_eq!(channels[0].channel_name, "Test Channel 1");
            },
            IteratorResult::Exhausted => panic!("Expected chunk, got exhausted"),
        }
        
        // Should be exhausted now
        match iterator.next_chunk().await.unwrap() {
            IteratorResult::Exhausted => {},
            _ => panic!("Expected exhausted, got chunk"),
        }
    }
}