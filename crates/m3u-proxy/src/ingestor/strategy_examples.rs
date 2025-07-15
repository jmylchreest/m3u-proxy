//! Examples: Easy Accumulator Strategy Selection
//!
//! This demonstrates the flexible ways to choose and configure
//! accumulator strategies for different use cases.

use std::sync::Arc;
use anyhow::Result;

use super::ingestion_accumulator::{
    IngestionAccumulator, IngestionAccumulationStrategy, IngestionAccumulatorFactory,
    AccumulatorConfig, AccumulatorPreset, IngestionAccumulatorStats
};
use super::state_manager::IngestionStateManager;
use crate::services::sandboxed_file::SandboxedFileManager;

/// Examples of different ways to choose accumulator strategies
pub struct StrategySelectionExamples;

impl StrategySelectionExamples {
    /// Example 1: Using predefined presets (easiest)
    pub fn example_presets(
        file_manager: Arc<dyn SandboxedFileManager>,
        state_manager: Arc<IngestionStateManager>,
    ) -> Result<()> {
        println!("=== Predefined Presets (Easiest) ===");
        
        // For resource-constrained environments
        let memory_factory = IngestionAccumulatorFactory::with_preset(AccumulatorPreset::MemoryOptimized);
        let _memory_accumulator: IngestionAccumulator<serde_json::Value> = memory_factory
            .create_for_source("xmltv", file_manager.clone(), Some(state_manager.clone()));
        println!("✓ Memory-optimized factory created (5MB thresholds, file streaming)");
        
        // For high-performance environments
        let performance_factory = IngestionAccumulatorFactory::with_preset(AccumulatorPreset::PerformanceOptimized);
        let _performance_accumulator: IngestionAccumulator<serde_json::Value> = performance_factory
            .create_for_source("xmltv", file_manager.clone(), Some(state_manager.clone()));
        println!("✓ Performance-optimized factory created (200MB in-memory buffers)");
        
        // For typical environments
        let balanced_factory = IngestionAccumulatorFactory::with_preset(AccumulatorPreset::Balanced);
        let _balanced_accumulator: IngestionAccumulator<serde_json::Value> = balanced_factory
            .create_for_source("xmltv", file_manager.clone(), Some(state_manager.clone()));
        println!("✓ Balanced factory created (hybrid streaming, auto-strategy enabled)");
        
        Ok(())
    }

    /// Example 2: Using auto-strategy with size hints (smart)
    pub fn example_auto_strategy(
        file_manager: Arc<dyn SandboxedFileManager>,
        state_manager: Arc<IngestionStateManager>,
    ) -> Result<()> {
        println!("\n=== Auto-Strategy with Size Hints (Smart) ===");
        
        let factory = IngestionAccumulatorFactory::new(); // Uses balanced config with auto-strategy
        
        // Small EPG file - will auto-select in-memory
        let _small_accumulator: IngestionAccumulator<serde_json::Value> = factory
            .create_for_source_with_size_hint(
                "xmltv", 
                Some(3), // 3MB EPG file
                file_manager.clone(), 
                Some(state_manager.clone())
            );
        println!("✓ Small EPG (3MB) → auto-selected in-memory strategy");
        
        // Large EPG file - will auto-select file streaming
        let _large_accumulator: IngestionAccumulator<serde_json::Value> = factory
            .create_for_source_with_size_hint(
                "xmltv", 
                Some(150), // 150MB EPG file
                file_manager.clone(), 
                Some(state_manager.clone())
            );
        println!("✓ Large EPG (150MB) → auto-selected file streaming strategy");
        
        // No size hint - will use configured default for EPG
        let _default_accumulator: IngestionAccumulator<serde_json::Value> = factory
            .create_for_source("xmltv", file_manager.clone(), Some(state_manager.clone()));
        println!("✓ No size hint → used configured EPG strategy (hybrid streaming)");
        
        Ok(())
    }

    /// Example 3: Custom configuration (flexible)
    pub fn example_custom_config(
        file_manager: Arc<dyn SandboxedFileManager>,
        state_manager: Arc<IngestionStateManager>,
    ) -> Result<()> {
        println!("\n=== Custom Configuration (Flexible) ===");
        
        // Create custom configuration
        let custom_config = AccumulatorConfig {
            epg_strategy: IngestionAccumulationStrategy::StreamToFile {
                stream_threshold_mb: 1, // Very aggressive file streaming
            },
            m3u_strategy: IngestionAccumulationStrategy::InMemoryBuffer {
                max_buffer_mb: 75, // Large M3U buffers
            },
            xtream_strategy: IngestionAccumulationStrategy::HybridStreaming {
                memory_threshold_mb: 20,
                max_memory_mb: 100,
            },
            default_strategy: IngestionAccumulationStrategy::StreamingParser {
                parse_batch_size: 2000,
                db_batch_size: 1000,
            },
            enable_auto_strategy: false, // Disable auto-strategy for predictable behavior
            auto_strategy_memory_limit_mb: 0,
        };
        
        let factory = IngestionAccumulatorFactory::with_config(custom_config);
        
        let _epg_accumulator: IngestionAccumulator<serde_json::Value> = factory
            .create_for_source("xmltv", file_manager.clone(), Some(state_manager.clone()));
        println!("✓ EPG uses custom strategy (stream to file at 1MB)");
        
        let _m3u_accumulator: IngestionAccumulator<serde_json::Value> = factory
            .create_for_source("m3u", file_manager.clone(), Some(state_manager.clone()));
        println!("✓ M3U uses custom strategy (75MB in-memory buffer)");
        
        Ok(())
    }

    /// Example 4: Explicit strategy override (maximum control)
    pub fn example_explicit_strategy(
        file_manager: Arc<dyn SandboxedFileManager>,
        state_manager: Arc<IngestionStateManager>,
    ) -> Result<()> {
        println!("\n=== Explicit Strategy Override (Maximum Control) ===");
        
        // Override any configuration - useful for special cases
        let explicit_strategy = IngestionAccumulationStrategy::StreamingParser {
            parse_batch_size: 100,   // Very small batches for testing
            db_batch_size: 50,       // Very small DB batches
        };
        
        let _accumulator: IngestionAccumulator<serde_json::Value> = 
            IngestionAccumulatorFactory::create_with_strategy(
                explicit_strategy,
                file_manager.clone(),
                Some(state_manager.clone())
            );
        
        println!("✓ Explicit strategy override (streaming parser with small batches)");
        
        Ok(())
    }

    /// Example 5: Runtime strategy switching
    pub fn example_runtime_switching(
        file_manager: Arc<dyn SandboxedFileManager>,
        state_manager: Arc<IngestionStateManager>,
    ) -> Result<()> {
        println!("\n=== Runtime Strategy Switching ===");
        
        let mut factory = IngestionAccumulatorFactory::new();
        
        // Start with balanced configuration
        let _balanced_accumulator: IngestionAccumulator<serde_json::Value> = factory
            .create_for_source("xmltv", file_manager.clone(), Some(state_manager.clone()));
        println!("✓ Started with balanced configuration");
        
        // Switch to memory-optimized during low-memory conditions
        factory.update_config(AccumulatorConfig::memory_optimized());
        let _memory_accumulator: IngestionAccumulator<serde_json::Value> = factory
            .create_for_source("xmltv", file_manager.clone(), Some(state_manager.clone()));
        println!("✓ Switched to memory-optimized configuration");
        
        // Switch to performance-optimized during off-peak hours
        factory.update_config(AccumulatorConfig::performance_optimized());
        let _performance_accumulator: IngestionAccumulator<serde_json::Value> = factory
            .create_for_source("xmltv", file_manager.clone(), Some(state_manager.clone()));
        println!("✓ Switched to performance-optimized configuration");
        
        Ok(())
    }

    /// Example 6: Strategy selection based on system conditions
    pub fn example_conditional_strategy(
        available_memory_mb: usize,
        cpu_usage_percent: f64,
        file_manager: Arc<dyn SandboxedFileManager>,
        state_manager: Arc<IngestionStateManager>,
    ) -> Result<()> {
        println!("\n=== Conditional Strategy Selection ===");
        
        let factory = if available_memory_mb < 100 {
            println!("Low memory detected ({} MB) → using memory-optimized preset", available_memory_mb);
            IngestionAccumulatorFactory::with_preset(AccumulatorPreset::MemoryOptimized)
        } else if available_memory_mb > 500 && cpu_usage_percent < 50.0 {
            println!("High memory ({} MB) and low CPU ({:.1}%) → using performance-optimized preset", 
                     available_memory_mb, cpu_usage_percent);
            IngestionAccumulatorFactory::with_preset(AccumulatorPreset::PerformanceOptimized)
        } else {
            println!("Normal conditions → using balanced preset");
            IngestionAccumulatorFactory::with_preset(AccumulatorPreset::Balanced)
        };
        
        let _accumulator: IngestionAccumulator<serde_json::Value> = factory
            .create_for_source("xmltv", file_manager.clone(), Some(state_manager.clone()));
        println!("✓ Strategy selected based on system conditions");
        
        Ok(())
    }
}

/// Usage summary showing all the ways to choose strategies
pub fn print_strategy_selection_summary() {
    println!("
==========================================
   Accumulator Strategy Selection Guide
==========================================

1. **Presets** (Easiest - 3 lines of code):
   ```rust
   let factory = IngestionAccumulatorFactory::with_preset(AccumulatorPreset::MemoryOptimized);
   let accumulator = factory.create_for_source(\"xmltv\", file_manager, state_manager);
   ```

2. **Auto-Strategy** (Smart - considers content size):
   ```rust
   let factory = IngestionAccumulatorFactory::new(); // Auto-strategy enabled by default
   let accumulator = factory.create_for_source_with_size_hint(\"xmltv\", Some(150), file_manager, state_manager);
   // → Automatically chooses file streaming for 150MB file
   ```

3. **Custom Config** (Flexible - set per source type):
   ```rust
   let config = AccumulatorConfig {{
       epg_strategy: IngestionAccumulationStrategy::StreamToFile {{ stream_threshold_mb: 1 }},
       m3u_strategy: IngestionAccumulationStrategy::InMemoryBuffer {{ max_buffer_mb: 75 }},
       // ...
   }};
   let factory = IngestionAccumulatorFactory::with_config(config);
   ```

4. **Explicit Override** (Maximum control):
   ```rust
   let strategy = IngestionAccumulationStrategy::StreamingParser {{ parse_batch_size: 100, db_batch_size: 50 }};
   let accumulator = IngestionAccumulatorFactory::create_with_strategy(strategy, file_manager, state_manager);
   ```

5. **Runtime Switching**:
   ```rust
   factory.update_config(AccumulatorConfig::memory_optimized()); // Switch during runtime
   ```

==========================================
           Available Strategies
==========================================

• **InMemoryBuffer**: Keep all data in memory (fast, high memory usage)
• **StreamToFile**: Stream to temp file immediately (low memory, some I/O overhead)  
• **HybridStreaming**: Start in memory, switch to file when needed (balanced)
• **StreamingParser**: Parse and batch during download (optimal for large datasets)

==========================================
              Presets
==========================================

• **MemoryOptimized**: 5MB thresholds, aggressive file streaming
• **PerformanceOptimized**: 200MB+ buffers, keep everything in memory
• **Balanced**: Hybrid strategies with auto-selection (default)

Choose based on your environment and use case!
");
}

#[cfg(test)]
mod tests {
    use super::*;
    
    // Tests would go here to verify strategy selection logic
}