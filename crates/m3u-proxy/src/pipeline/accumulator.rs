//! Accumulator pattern for efficiently converting consuming iterators to immutable sources
//!
//! This module provides utilities for accumulating data from consuming iterators
//! and converting them into immutable sources that can be shared across multiple
//! iterator instances.

use anyhow::Result;
use serde_json;
use std::sync::Arc;
use tracing::{debug, info};
use uuid::Uuid;

use super::immutable_sources::{
    ImmutableLogoEnrichedChannelSource, ImmutableLogoEnrichedEpgSource, ImmutableProxyConfigSource,
};
use super::iterator_traits::{IteratorResult, PipelineIterator};
use super::iterator_types::IteratorType;
use crate::models::*;
use crate::services::sandboxed_file::SandboxedFileManager;

/// Strategy for how to handle data accumulation
#[derive(Debug, Clone)]
pub enum AccumulationStrategy {
    /// Buffer all data in memory (good for small datasets)
    InMemory,
    /// Always use a temporary file for storage
    FileSpilled,
    /// Hybrid: start in-memory, spill to disk if threshold exceeded
    Hybrid {
        /// Memory threshold in MB before spilling to disk
        memory_threshold_mb: usize,
    },
}

impl AccumulationStrategy {
    /// Create a hybrid strategy with custom threshold
    pub fn hybrid_with_threshold(threshold_mb: usize) -> Self {
        Self::Hybrid {
            memory_threshold_mb: threshold_mb,
        }
    }

    /// Get the default low-memory hybrid strategy (50MB threshold)
    pub fn default_hybrid() -> Self {
        Self::Hybrid {
            memory_threshold_mb: 50,
        }
    }
}

/// Accumulator for converting consuming iterators to immutable sources
pub struct IteratorAccumulator<T> {
    /// Accumulated data (in memory)
    buffer: Vec<T>,

    /// Total items processed
    total_items: usize,

    /// Strategy being used
    strategy: AccumulationStrategy,

    /// Memory usage estimation
    estimated_memory_mb: f64,

    /// Whether accumulation is complete
    is_complete: bool,

    /// Spill file info if data has been spilled to disk
    spill_file_id: Option<String>,

    /// Whether data is currently spilled
    is_spilled: bool,

    /// Sandboxed file manager for temporary files
    file_manager: Arc<dyn SandboxedFileManager>,

    /// Items per file when spilling (to avoid huge files)
    #[allow(dead_code)]
    items_per_spill_file: usize,

    /// Current spill file index
    current_spill_index: usize,
}

impl<T> IteratorAccumulator<T>
where
    T: serde::Serialize + serde::de::DeserializeOwned + Clone + Send + Sync + 'static,
{
    /// Create a new accumulator with the specified strategy and file manager
    pub fn new(
        strategy: AccumulationStrategy,
        file_manager: Arc<dyn SandboxedFileManager>,
    ) -> Self {
        Self {
            buffer: Vec::new(),
            total_items: 0,
            strategy,
            estimated_memory_mb: 0.0,
            is_complete: false,
            spill_file_id: None,
            is_spilled: false,
            file_manager,
            items_per_spill_file: 10000, // 10k items per spill file
            current_spill_index: 0,
        }
    }

    /// Create with custom configuration
    pub fn with_config(
        strategy: AccumulationStrategy,
        file_manager: Arc<dyn SandboxedFileManager>,
        items_per_spill_file: usize,
    ) -> Self {
        Self {
            buffer: Vec::new(),
            total_items: 0,
            strategy,
            estimated_memory_mb: 0.0,
            is_complete: false,
            spill_file_id: None,
            is_spilled: false,
            file_manager,
            items_per_spill_file,
            current_spill_index: 0,
        }
    }

    /// Accumulate all data from a consuming iterator
    pub async fn accumulate_from_iterator(
        &mut self,
        mut iterator: Box<dyn PipelineIterator<T> + Send + Sync>,
    ) -> Result<()> {
        info!("Starting accumulation with strategy: {:?}", self.strategy);

        loop {
            match iterator.next_chunk().await? {
                IteratorResult::Chunk(chunk) => {
                    let chunk_size = chunk.len();
                    self.buffer.extend(chunk);
                    self.total_items += chunk_size;

                    // Update memory estimation (rough approximation)
                    self.estimated_memory_mb += chunk_size as f64 * 0.001; // ~1KB per item estimate

                    // Check if we need to spill to disk
                    match &self.strategy {
                        AccumulationStrategy::Hybrid {
                            memory_threshold_mb,
                        } => {
                            if self.estimated_memory_mb > *memory_threshold_mb as f64 {
                                if !self.is_spilled {
                                    info!(
                                        "Memory threshold ({} MB) exceeded, spilling to disk",
                                        memory_threshold_mb
                                    );
                                    self.spill_to_disk().await?;
                                } else if self.buffer.len() > 0 {
                                    // Already spilled, but buffer has new data - spill incrementally
                                    self.spill_to_disk().await?;
                                }
                            }
                        }
                        AccumulationStrategy::FileSpilled => {
                            // Always spill immediately
                            if !self.buffer.is_empty() {
                                self.spill_to_disk().await?;
                            }
                        }
                        AccumulationStrategy::InMemory => {
                            // Never spill
                        }
                    }

                    info!(
                        "Accumulated {} items, estimated memory: {:.1}MB",
                        self.total_items, self.estimated_memory_mb
                    );
                }
                IteratorResult::Exhausted => {
                    info!(
                        "Iterator exhausted. Total accumulated: {} items",
                        self.total_items
                    );
                    break;
                }
            }
        }

        self.is_complete = true;
        Ok(())
    }

    /// Create an immutable channel source from accumulated channel data
    pub async fn into_channel_source(
        mut self,
        source_type: IteratorType,
    ) -> Result<Arc<ImmutableLogoEnrichedChannelSource>>
    where
        T: Into<Channel>,
    {
        if !self.is_complete {
            return Err(anyhow::anyhow!("Accumulation not complete"));
        }

        // Load spilled data back if necessary
        if self.is_spilled {
            self.load_from_spill().await?;
        }

        let channels: Vec<Channel> = self.buffer.into_iter().map(|item| item.into()).collect();
        Ok(Arc::new(ImmutableLogoEnrichedChannelSource::new(
            channels,
            source_type,
        )))
    }

    /// Create an immutable EPG source from accumulated data
    pub async fn into_epg_source(
        mut self,
        source_type: IteratorType,
    ) -> Result<Arc<ImmutableLogoEnrichedEpgSource>> {
        if !self.is_complete {
            return Err(anyhow::anyhow!("Accumulation not complete"));
        }

        // Load spilled data back if necessary
        if self.is_spilled {
            self.load_from_spill().await?;
        }

        Ok(Arc::new(ImmutableLogoEnrichedEpgSource::new(
            self.buffer,
            source_type,
        )?))
    }

    /// Create an immutable config source from accumulated data
    pub async fn into_config_source(
        mut self,
        source_type: IteratorType,
        description: String,
    ) -> Result<Arc<ImmutableProxyConfigSource>> {
        if !self.is_complete {
            return Err(anyhow::anyhow!("Accumulation not complete"));
        }

        // Load spilled data back if necessary
        if self.is_spilled {
            self.load_from_spill().await?;
        }

        Ok(Arc::new(ImmutableProxyConfigSource::new(
            self.buffer,
            source_type,
            description,
        )?))
    }

    /// Get current statistics
    pub fn stats(&self) -> AccumulatorStats {
        AccumulatorStats {
            total_items: self.total_items,
            estimated_memory_mb: self.estimated_memory_mb,
            is_complete: self.is_complete,
            is_spilled: self.is_spilled,
            strategy: self.strategy.clone(),
        }
    }

    /// Get accumulated items (load from spill if necessary)
    pub async fn into_items(mut self) -> Result<Vec<T>> {
        if !self.is_complete {
            return Err(anyhow::anyhow!("Accumulation not complete"));
        }

        // Load spilled data back if necessary
        if self.is_spilled {
            self.load_from_spill().await?;
        }

        Ok(self.buffer)
    }

    /// Get statistics without consuming the accumulator
    pub fn get_stats(&self) -> AccumulatorStats {
        self.stats()
    }

    /// Spill current buffer to disk
    async fn spill_to_disk(&mut self) -> Result<()> {
        if self.buffer.is_empty() {
            return Ok(());
        }

        // Generate unique ID for this accumulator's spill files (lazy initialization)
        if self.spill_file_id.is_none() {
            self.spill_file_id = Some(Uuid::new_v4().to_string());
            debug!("Created spill file ID for first-time disk spilling");
        }

        let spill_id = self.spill_file_id.as_ref().unwrap();
        let file_name = format!("spill_{}_{:04}", spill_id, self.current_spill_index);

        info!(
            "Spilling {} items to file: {}",
            self.buffer.len(),
            file_name
        );

        // Serialize buffer to JSON lines format (one JSON object per line)
        let mut content = Vec::new();
        for item in &self.buffer {
            let json = serde_json::to_string(item)?;
            content.extend_from_slice(json.as_bytes());
            content.push(b'\n');
        }

        // Store in sandboxed file manager (will be auto-cleaned)
        self.file_manager
            .store_file(
                "accumulator_spill",
                &file_name,
                &content,
                "jsonl", // JSON lines format
            )
            .await?;

        self.current_spill_index += 1;
        self.is_spilled = true;

        // Clear in-memory buffer to free memory and shrink capacity
        self.buffer.clear();
        self.buffer.shrink_to_fit();
        self.estimated_memory_mb = 0.0;

        debug!("Spill complete, memory buffer cleared");
        Ok(())
    }

    /// Load all spilled data back into memory
    async fn load_from_spill(&mut self) -> Result<()> {
        if !self.is_spilled || self.spill_file_id.is_none() {
            return Ok(());
        }

        let spill_id = self.spill_file_id.as_ref().unwrap();
        let mut all_items = Vec::new();

        info!("Loading spilled data back into memory");

        // Load all spill files in order
        for index in 0..self.current_spill_index {
            let file_name = format!("spill_{}_{:04}", spill_id, index);

            // Read from sandboxed file manager
            let content = self
                .file_manager
                .read_file("accumulator_spill", &file_name, "jsonl")
                .await?;

            // Parse JSON lines
            let content_str = String::from_utf8(content)?;
            for line in content_str.lines() {
                if !line.trim().is_empty() {
                    let item: T = serde_json::from_str(line)?;
                    all_items.push(item);
                }
            }

            // Delete the spill file after reading
            self.file_manager
                .delete_file("accumulator_spill", &file_name, "jsonl")
                .await?;
        }

        // Add current buffer items (if any)
        all_items.extend(self.buffer.drain(..));

        self.buffer = all_items;
        self.is_spilled = false;
        self.current_spill_index = 0;

        info!("Loaded {} items from spill files", self.buffer.len());
        Ok(())
    }
}

/// Statistics about accumulation progress
#[derive(Debug, Clone)]
pub struct AccumulatorStats {
    pub total_items: usize,
    pub estimated_memory_mb: f64,
    pub is_complete: bool,
    pub is_spilled: bool,
    pub strategy: AccumulationStrategy,
}

/// Specialized accumulator for channels with logo enrichment tracking
pub struct ChannelAccumulator {
    inner: IteratorAccumulator<serde_json::Value>,
    logo_enriched_count: usize,
    original_count: usize,
}

impl ChannelAccumulator {
    pub fn new(
        strategy: AccumulationStrategy,
        file_manager: Arc<dyn SandboxedFileManager>,
    ) -> Self {
        Self {
            inner: IteratorAccumulator::new(strategy, file_manager),
            logo_enriched_count: 0,
            original_count: 0,
        }
    }

    /// Accumulate channels and track logo enrichment
    pub async fn accumulate_channels(
        &mut self,
        mut iterator: Box<dyn PipelineIterator<serde_json::Value> + Send + Sync>,
    ) -> Result<()> {
        info!("Starting channel accumulation with logo tracking");

        loop {
            match iterator.next_chunk().await? {
                IteratorResult::Chunk(chunk) => {
                    // Track logo enrichment statistics
                    for item in &chunk {
                        if let Ok(channel) = serde_json::from_value::<Channel>(item.clone()) {
                            self.original_count += 1;
                            if channel.tvg_logo.is_some() {
                                self.logo_enriched_count += 1;
                            }
                        }
                    }

                    self.inner.buffer.extend(chunk);
                    self.inner.total_items = self.original_count;
                    self.inner.estimated_memory_mb = self.original_count as f64 * 0.002; // ~2KB per channel
                }
                IteratorResult::Exhausted => {
                    info!(
                        "Channel accumulation complete: {} total, {} logo-enriched ({:.1}%)",
                        self.original_count,
                        self.logo_enriched_count,
                        (self.logo_enriched_count as f64 / self.original_count as f64) * 100.0
                    );
                    break;
                }
            }
        }

        self.inner.is_complete = true;
        Ok(())
    }

    /// Convert to immutable channel source
    pub async fn into_channel_source(
        mut self,
        source_type: IteratorType,
    ) -> Result<Arc<ImmutableLogoEnrichedChannelSource>> {
        if !self.inner.is_complete {
            return Err(anyhow::anyhow!("Accumulation not complete"));
        }

        // Load spilled data back if necessary
        if self.inner.is_spilled {
            self.inner.load_from_spill().await?;
        }

        // Convert JSON values back to channels
        let channels: Result<Vec<Channel>, _> = self
            .inner
            .buffer
            .into_iter()
            .map(|json| serde_json::from_value(json))
            .collect();

        let channels = channels?;
        info!(
            "Created immutable channel source with {} channels ({} logo-enriched)",
            channels.len(),
            self.logo_enriched_count
        );

        Ok(Arc::new(ImmutableLogoEnrichedChannelSource::new(
            channels,
            source_type,
        )))
    }

    /// Get enrichment statistics
    pub fn enrichment_stats(&self) -> ChannelEnrichmentStats {
        ChannelEnrichmentStats {
            total_channels: self.original_count,
            logo_enriched_channels: self.logo_enriched_count,
            enrichment_percentage: if self.original_count > 0 {
                (self.logo_enriched_count as f64 / self.original_count as f64) * 100.0
            } else {
                0.0
            },
        }
    }

    /// Get accumulated channels (load from spill if necessary)
    pub async fn get_channels(&mut self) -> Result<Vec<Channel>> {
        if !self.inner.is_complete {
            return Err(anyhow::anyhow!("Accumulation not complete"));
        }

        // Load spilled data back if necessary
        if self.inner.is_spilled {
            self.inner.load_from_spill().await?;
        }

        // Convert JSON values back to channels
        let channels: Result<Vec<Channel>, _> = self
            .inner
            .buffer
            .iter()
            .map(|json| serde_json::from_value(json.clone()))
            .collect();

        channels.map_err(|e| anyhow::anyhow!("Failed to deserialize channels: {}", e))
    }

    /// Get accumulated data without consuming the accumulator
    pub async fn get_accumulated_data(&mut self) -> Result<Vec<Channel>> {
        self.get_channels().await
    }

    /// Get logo enriched count
    pub fn get_logo_enriched_count(&self) -> usize {
        self.logo_enriched_count
    }

    /// Get accumulated statistics
    pub fn get_stats(&self) -> AccumulatorStats {
        self.inner.get_stats()
    }
}

/// Statistics about channel logo enrichment
#[derive(Debug, Clone)]
pub struct ChannelEnrichmentStats {
    pub total_channels: usize,
    pub logo_enriched_channels: usize,
    pub enrichment_percentage: f64,
}

/// Factory for creating accumulators with appropriate strategies
pub struct AccumulatorFactory;

impl AccumulatorFactory {
    /// Create a channel accumulator with hybrid strategy for optimal memory management
    pub fn create_channel_accumulator(
        file_manager: Arc<dyn SandboxedFileManager>,
    ) -> ChannelAccumulator {
        // Always use hybrid strategy for better memory management
        let strategy = AccumulationStrategy::default_hybrid(); // 50MB threshold

        ChannelAccumulator::new(strategy, file_manager)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::iterator_types::IteratorType;
    use crate::services::sandboxed_file::{FileInfo, SandboxedFileManager};
    use anyhow::Result;
    use async_trait::async_trait;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Mutex;

    // Mock implementation for testing
    #[derive(Debug)]
    struct MockSandboxedFileManager {
        files: Mutex<HashMap<String, Vec<u8>>>,
    }

    impl MockSandboxedFileManager {
        fn new() -> Self {
            Self {
                files: Mutex::new(HashMap::new()),
            }
        }
    }

    #[async_trait]
    impl SandboxedFileManager for MockSandboxedFileManager {
        async fn store_file(
            &self,
            category: &str,
            file_id: &str,
            content: &[u8],
            extension: &str,
        ) -> Result<()> {
            let key = format!("{}/{}.{}", category, file_id, extension);
            let mut files = self.files.lock().unwrap();
            files.insert(key, content.to_vec());
            Ok(())
        }

        async fn store_linked_file(
            &self,
            category: &str,
            file_id: &str,
            content: &[u8],
            extension: &str,
        ) -> Result<()> {
            self.store_file(category, file_id, content, extension).await
        }

        async fn read_file(
            &self,
            category: &str,
            file_id: &str,
            extension: &str,
        ) -> Result<Vec<u8>> {
            let key = format!("{}/{}.{}", category, file_id, extension);
            let files = self.files.lock().unwrap();
            files
                .get(&key)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("File not found: {}", key))
        }

        async fn file_exists(
            &self,
            category: &str,
            file_id: &str,
            extension: &str,
        ) -> Result<bool> {
            let key = format!("{}/{}.{}", category, file_id, extension);
            let files = self.files.lock().unwrap();
            Ok(files.contains_key(&key))
        }

        async fn get_file_path(
            &self,
            _category: &str,
            _file_id: &str,
            _extension: &str,
        ) -> Result<Option<PathBuf>> {
            Ok(None) // Mock implementation
        }

        async fn delete_file(&self, category: &str, file_id: &str, extension: &str) -> Result<()> {
            let key = format!("{}/{}.{}", category, file_id, extension);
            let mut files = self.files.lock().unwrap();
            files.remove(&key);
            Ok(())
        }

        async fn list_files(&self, _category: &str) -> Result<Vec<FileInfo>> {
            Ok(vec![]) // Mock implementation
        }
    }

    #[tokio::test]
    async fn test_accumulator_basic() {
        let file_manager = Arc::new(MockSandboxedFileManager::new());
        let mut accumulator = IteratorAccumulator::<serde_json::Value>::new(
            AccumulationStrategy::InMemory,
            file_manager,
        );

        // Create a mock iterator that would normally consume data
        // In a real scenario, this would be a consuming iterator from a plugin

        let test_data = vec![
            serde_json::json!({"test": "data1"}),
            serde_json::json!({"test": "data2"}),
        ];

        accumulator.buffer.extend(test_data);
        accumulator.total_items = 2;
        accumulator.is_complete = true;

        let stats = accumulator.stats();
        assert_eq!(stats.total_items, 2);
        assert!(stats.is_complete);
    }

    #[test]
    fn test_accumulator_factory() {
        let file_manager = Arc::new(MockSandboxedFileManager::new());

        let accumulator = IteratorAccumulator::<serde_json::Value>::new(
            AccumulationStrategy::default_hybrid(),
            file_manager.clone(),
        );
        assert!(matches!(
            accumulator.strategy,
            AccumulationStrategy::Hybrid { .. }
        ));
    }
}
