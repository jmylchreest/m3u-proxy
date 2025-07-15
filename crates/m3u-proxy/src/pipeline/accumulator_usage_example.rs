//! Example usage patterns for efficient accumulator-based immutable source management
//!
//! This demonstrates how to handle the transition from consuming iterators to 
//! immutable sources for different scenarios.

use std::sync::Arc;
use anyhow::Result;
use tracing::info;

use super::accumulator::{AccumulatorFactory, ChannelAccumulator, AccumulationStrategy, IteratorAccumulator};
use super::immutable_sources::{ImmutableLogoEnrichedChannelSource, ImmutableSourceManager};
use super::iterator_registry::IteratorRegistry;
use super::iterator_traits::{PipelineIterator, IteratorResult};
use super::iterator_types::{IteratorType, well_known_ids};
use crate::models::*;
use crate::services::sandboxed_file::SandboxedFileManager;

/// Example: Efficient logo plugin processing with accumulator pattern
pub struct LogoPluginProcessor {
    registry: Arc<IteratorRegistry>,
    source_manager: Arc<ImmutableSourceManager>,
}

impl LogoPluginProcessor {
    pub fn new(registry: Arc<IteratorRegistry>, source_manager: Arc<ImmutableSourceManager>) -> Self {
        Self { registry, source_manager }
    }
    
    /// Process logos efficiently - demonstrates the accumulator pattern
    pub async fn process_logo_enrichment(&self) -> Result<()> {
        // Step 1: Get the consuming channel iterator (from data mapping or filtering stage)
        let consuming_channel_iterator = self.get_consuming_channel_iterator().await?;
        
        // Step 2: Create accumulator with hybrid strategy
        let file_manager = self.get_file_manager().await?;
        let mut accumulator = AccumulatorFactory::create_channel_accumulator(file_manager);
        
        info!("Starting logo enrichment with accumulator");
        
        // Step 3: Process through logo plugin while accumulating
        let logo_enriched_iterator = self.run_logo_plugin(consuming_channel_iterator).await?;
        
        // Step 4: Accumulate all logo-enriched results
        accumulator.accumulate_channels(logo_enriched_iterator).await?;
        
        // Step 5: Convert accumulated data to immutable source
        let immutable_source = accumulator.into_channel_source(IteratorType::LogoChannels).await?;
        
        // Step 6: Register the immutable source for multi-instance access
        self.registry.register_channel_source("logo_channels".to_string(), immutable_source)?;
        
        info!("Logo enrichment complete - immutable source registered");
        Ok(())
    }
    
    /// Alternative approach: Streaming accumulation for large datasets
    pub async fn process_large_logo_enrichment(&self) -> Result<()> {
        // For very large datasets, use streaming accumulation with disk spilling
        let file_manager = self.get_file_manager().await?;
        let mut accumulator = ChannelAccumulator::new(AccumulationStrategy::hybrid_with_threshold(512), file_manager);
        
        // Process in batches to manage memory
        let batch_size = 1000;
        let mut batch_count = 0;
        
        loop {
            let batch_iterator = self.get_channel_batch(batch_count, batch_size).await?;
            
            if self.is_batch_empty(&batch_iterator).await? {
                break; // No more data
            }
            
            let logo_enriched_batch = self.run_logo_plugin(batch_iterator).await?;
            accumulator.accumulate_channels(logo_enriched_batch).await?;
            
            batch_count += 1;
            
            // Log progress
            let stats = accumulator.enrichment_stats();
            info!("Processed batch {}: {} channels, {:.1}% logo-enriched", 
                  batch_count, stats.total_channels, stats.enrichment_percentage);
        }
        
        // Convert to immutable source
        let immutable_source = accumulator.into_channel_source(IteratorType::LogoChannels).await?;
        self.registry.register_channel_source("logo_channels".to_string(), immutable_source)?;
        
        Ok(())
    }
    
    // Mock implementations for example
    async fn get_consuming_channel_iterator(&self) -> Result<Box<dyn PipelineIterator<serde_json::Value> + Send + Sync>> {
        todo!("Get iterator from previous stage")
    }
    
    async fn estimate_channel_count(&self) -> Result<usize> {
        // Could query database for count, or use previous stage statistics
        Ok(5000) // Example estimate
    }
    
    async fn run_logo_plugin(&self, input: Box<dyn PipelineIterator<serde_json::Value> + Send + Sync>) -> Result<Box<dyn PipelineIterator<serde_json::Value> + Send + Sync>> {
        todo!("Run logo enrichment plugin")
    }
    
    async fn get_channel_batch(&self, batch: usize, size: usize) -> Result<Box<dyn PluginIterator<serde_json::Value> + Send + Sync>> {
        todo!("Get batch of channels")
    }
    
    async fn is_batch_empty(&self, iterator: &Box<dyn PluginIterator<serde_json::Value> + Send + Sync>) -> Result<bool> {
        todo!("Check if batch has data")
    }
    
    async fn get_file_manager(&self) -> Result<Arc<dyn SandboxedFileManager>> {
        todo!("Get sandboxed file manager")
    }
}

/// Example: Multi-source accumulation for complex pipelines
pub struct ComplexPipelineProcessor {
    registry: Arc<IteratorRegistry>,
}

impl ComplexPipelineProcessor {
    /// Demonstrates accumulating from multiple consuming sources
    pub async fn process_multi_source_data(&self) -> Result<()> {
        // Scenario: Combine data from multiple plugin outputs that can't be re-queried
        
        // Source 1: Logo-enriched channels (accumulate)
        let logo_iterator = self.get_logo_plugin_output().await?;
        let file_manager = self.get_file_manager().await?;
        let mut channel_accumulator = AccumulatorFactory::create_channel_accumulator(file_manager.clone());
        channel_accumulator.accumulate_channels(logo_iterator).await?;
        
        // Source 2: Additional metadata from another plugin (accumulate)
        let metadata_iterator = self.get_metadata_plugin_output().await?;
        let mut metadata_accumulator = IteratorAccumulator::new(
            AccumulationStrategy::default_hybrid(),
            file_manager.clone(),
        );
        // metadata_accumulator.accumulate_from_iterator(metadata_iterator).await?;
        
        // Convert both to immutable sources
        let channel_source = channel_accumulator.into_channel_source(IteratorType::LogoChannels).await?;
        // let metadata_source = metadata_accumulator.into_config_source(IteratorType::ConfigSnapshot(ConfigType::DataMappingRules), "Metadata".to_string())?;
        
        // Register both for multi-instance access
        self.registry.register_channel_source("logo_channels".to_string(), channel_source)?;
        // self.registry.register_config_source("metadata".to_string(), metadata_source)?;
        
        // Now multiple consuming plugins can independently access the data
        info!("Multi-source immutable sources registered");
        Ok(())
    }
    
    async fn get_logo_plugin_output(&self) -> Result<Box<dyn PluginIterator<serde_json::Value> + Send + Sync>> {
        todo!("Get consuming iterator from logo plugin")
    }
    
    async fn get_metadata_plugin_output(&self) -> Result<Box<dyn PluginIterator<serde_json::Value> + Send + Sync>> {
        todo!("Get consuming iterator from metadata plugin")
    }
    
    async fn get_file_manager(&self) -> Result<Arc<dyn SandboxedFileManager>> {
        todo!("Get sandboxed file manager")
    }
}

/// Example: Performance-optimized accumulation strategies
pub struct PerformanceOptimizedProcessor;

impl PerformanceOptimizedProcessor {
    /// Choose accumulation strategy based on data characteristics
    pub fn choose_strategy(estimated_items: usize, avg_item_size_kb: f64) -> AccumulationStrategy {
        let estimated_memory_mb = (estimated_items as f64 * avg_item_size_kb) / 1024.0;
        
        match estimated_memory_mb {
            mem if mem < 50.0 => {
                info!("Using in-memory strategy for {:.1}MB dataset", mem);
                AccumulationStrategy::InMemory
            },
            mem if mem < 500.0 => {
                info!("Using hybrid strategy for {:.1}MB dataset", mem);
                AccumulationStrategy::hybrid_with_threshold(256)
            },
            mem => {
                info!("Using file-spilled strategy for {:.1}MB dataset", mem);
                AccumulationStrategy::FileSpilled
            }
        }
    }
    
    /// Demonstrate efficient memory management during accumulation
    pub async fn efficient_accumulation_example() -> Result<()> {
        // Large channel dataset - use streaming with memory management
        let strategy = Self::choose_strategy(100_000, 2.0); // 100k channels, ~2KB each
        let file_manager = create_mock_file_manager().await?;
        let mut accumulator = ChannelAccumulator::new(strategy, file_manager);
        
        // Process with memory monitoring
        let mock_iterator = create_mock_large_iterator().await?;
        accumulator.accumulate_channels(mock_iterator).await?;
        
        // Check final statistics
        let stats = accumulator.enrichment_stats();
        info!("Accumulation complete: {} channels, {:.1}% enriched", 
              stats.total_channels, stats.enrichment_percentage);
        
        Ok(())
    }
}

// Mock functions for example
async fn create_mock_large_iterator() -> Result<Box<dyn PluginIterator<serde_json::Value> + Send + Sync>> {
    todo!("Create mock iterator for example")
}

async fn create_mock_file_manager() -> Result<Arc<dyn SandboxedFileManager>> {
    todo!("Create mock file manager")
}

/// Usage patterns summary:
/// 
/// 1. **Small Datasets (<50MB)**: Use `AccumulationStrategy::InMemory`
///    - Fast, simple, good for most config data and small channel lists
/// 
/// 2. **Medium Datasets (50-500MB)**: Use `AccumulationStrategy::Hybrid`
///    - Start in memory, spill to disk if needed
///    - Good for typical channel datasets with logo enrichment
/// 
/// 3. **Large Datasets (>500MB)**: Use `AccumulationStrategy::FileSpilled`
///    - Always use temporary files for storage
///    - Good for massive EPG datasets or very large channel lists
/// 
/// 4. **Streaming Processing**: Use batch accumulation with progress monitoring
///    - Process in chunks to manage memory pressure
///    - Monitor enrichment statistics during processing
/// 
/// 5. **Multi-Source Scenarios**: Accumulate from multiple consuming sources
///    - Each source gets its own accumulator
///    - Convert all to immutable sources for shared access
///    - Enable complex multi-plugin workflows