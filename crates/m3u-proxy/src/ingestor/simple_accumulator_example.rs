//! Simple Accumulator Usage Examples
//!
//! Clean, straightforward examples showing the simplified accumulator API

use std::sync::Arc;
use anyhow::Result;

use super::ingestion_accumulator::{
    IngestionAccumulator, IngestionAccumulationStrategy, IngestionAccumulatorFactory
};
use super::state_manager::IngestionStateManager;
use crate::services::sandboxed_file::SandboxedFileManager;

/// Simple examples of accumulator usage
pub struct SimpleAccumulatorExamples;

impl SimpleAccumulatorExamples {
    /// Example 1: Default usage (hybrid strategy - recommended)
    pub fn example_default_usage(
        file_manager: Arc<dyn SandboxedFileManager>,
        state_manager: Arc<IngestionStateManager>,
    ) -> Result<()> {
        println!("=== Default Usage (Hybrid Strategy) ===");
        
        // Just use the default - hybrid strategy with sensible defaults
        let _accumulator: IngestionAccumulator<serde_json::Value> = 
            IngestionAccumulatorFactory::create(file_manager.clone(), Some(state_manager.clone()));
        
        println!("✓ Created accumulator with hybrid strategy (10MB → 50MB limits)");
        println!("  - Starts in memory for fast access");
        println!("  - Automatically spills to disk if memory usage exceeds 10MB");
        println!("  - Handles cleanup automatically");
        
        Ok(())
    }

    /// Example 2: Manual override for specific needs
    pub fn example_manual_override(
        file_manager: Arc<dyn SandboxedFileManager>,
        state_manager: Arc<IngestionStateManager>,
    ) -> Result<()> {
        println!("\n=== Manual Override Examples ===");
        
        // Force in-memory for small, fast sources
        let strategy_memory = IngestionAccumulationStrategy::InMemoryBuffer {
            max_buffer_mb: 25,
        };
        let _memory_accumulator: IngestionAccumulator<serde_json::Value> = 
            IngestionAccumulatorFactory::create_with_strategy(
                strategy_memory, 
                file_manager.clone(), 
                Some(state_manager.clone())
            );
        println!("✓ In-memory strategy: Fast access, 25MB limit");
        
        // Force file streaming for large sources
        let strategy_file = IngestionAccumulationStrategy::StreamToFile {
            stream_threshold_mb: 1,
        };
        let _file_accumulator: IngestionAccumulator<serde_json::Value> = 
            IngestionAccumulatorFactory::create_with_strategy(
                strategy_file, 
                file_manager.clone(), 
                Some(state_manager.clone())
            );
        println!("✓ File streaming strategy: Low memory usage, streams to disk at 1MB");
        
        // Custom hybrid with different thresholds
        let strategy_custom = IngestionAccumulationStrategy::HybridStreaming {
            memory_threshold_mb: 5,  // Spill earlier
            max_memory_mb: 25,       // Lower max memory
        };
        let _custom_accumulator: IngestionAccumulator<serde_json::Value> = 
            IngestionAccumulatorFactory::create_with_strategy(
                strategy_custom, 
                file_manager.clone(), 
                Some(state_manager.clone())
            );
        println!("✓ Custom hybrid strategy: 5MB → 25MB thresholds");
        
        // Streaming parser for large datasets with many small items
        let strategy_parser = IngestionAccumulationStrategy::StreamingParser {
            parse_batch_size: 1000,
            db_batch_size: 500,
        };
        let _parser_accumulator: IngestionAccumulator<serde_json::Value> = 
            IngestionAccumulatorFactory::create_with_strategy(
                strategy_parser, 
                file_manager.clone(), 
                Some(state_manager.clone())
            );
        println!("✓ Streaming parser strategy: 1000 parse batch, 500 DB batch");
        
        Ok(())
    }
    
    /// Example 3: Real-world usage pattern
    pub async fn example_real_world_usage(
        file_manager: Arc<dyn SandboxedFileManager>,
        state_manager: Arc<IngestionStateManager>,
        epg_url: &str,
    ) -> Result<()> {
        println!("\n=== Real-World Usage Pattern ===");
        
        // Create accumulator (uses hybrid by default)
        let mut accumulator: IngestionAccumulator<serde_json::Value> = 
            IngestionAccumulatorFactory::create(file_manager.clone(), Some(state_manager.clone()));
        
        // Simulate HTTP download with accumulation
        println!("Downloading EPG data...");
        let fake_chunks = vec![
            b"chunk1".to_vec(),
            b"chunk2".to_vec(), 
            b"chunk3".to_vec(),
        ];
        
        for chunk in fake_chunks {
            accumulator.accumulate_http_chunk(&chunk).await?;
        }
        
        // Get final data
        let final_data = accumulator.finalize_accumulation().await?;
        println!("✓ Downloaded {} bytes", final_data.len());
        
        // Check statistics
        let stats = accumulator.get_stats();
        println!("  - Total downloaded: {} bytes", stats.total_bytes_downloaded);
        println!("  - Memory usage: {:.1}MB", stats.current_buffer_size_mb);
        println!("  - Used file streaming: {}", stats.is_streaming_to_file);
        
        Ok(())
    }
}

/// Usage guide for the simplified API
pub fn print_usage_guide() {
    println!("
==========================================
      Simple Accumulator Usage Guide
==========================================

**Default Usage (Recommended):**
```rust
let accumulator = IngestionAccumulatorFactory::create(file_manager, state_manager);
// Uses hybrid strategy: 10MB memory → 50MB max, automatic file spilling
```

**Manual Override (When Needed):**
```rust
let strategy = IngestionAccumulationStrategy::InMemoryBuffer {{ max_buffer_mb: 100 }};
let accumulator = IngestionAccumulatorFactory::create_with_strategy(strategy, file_manager, state_manager);
```

**Available Strategies:**
• **HybridStreaming** (default): Start in memory, spill to disk when needed
• **InMemoryBuffer**: Keep everything in memory (fast, higher memory usage)
• **StreamToFile**: Stream to disk immediately (low memory, some I/O overhead)
• **StreamingParser**: Parse and batch during download (optimal for large datasets)

**When to Override:**
• **Small sources (<10MB)**: Use InMemoryBuffer for speed
• **Large sources (>100MB)**: Use StreamToFile for memory efficiency
• **Many small items**: Use StreamingParser for batch processing
• **Memory-constrained**: Use StreamToFile with low threshold

**The hybrid default handles 90% of use cases well!**
==========================================
");
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_simple_api() {
        // Tests for the simplified API
    }
}