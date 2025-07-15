# WebAssembly Plugin Architecture for Strategy Extensions

## Overview

The WebAssembly (WASM) plugin system allows users to extend the adaptive pipeline with custom processing strategies without recompiling the main application. This enables:

- **Community Contributions**: Users can share custom strategies
- **Domain-Specific Optimizations**: Industry-specific processing logic  
- **Safe Extensibility**: Sandboxed execution prevents system damage
- **Cross-Language Support**: Plugins can be written in Rust, C++, Go, etc.

## Architecture Design

### 1. WASM Runtime Integration

```rust
// Use wasmtime for robust WASM execution
use wasmtime::{Engine, Module, Store, Instance, Func, Memory};

pub struct WasmRuntime {
    engine: Engine,
    modules: HashMap<String, Module>,
    instances: HashMap<String, WasmInstance>,
}

pub struct WasmInstance {
    store: Store<WasmState>,
    instance: Instance,
    memory: Memory,
    exports: WasmExports,
}

pub struct WasmExports {
    // Strategy interface functions
    init: Func,
    execute_stage: Func,
    check_memory_pressure: Func,
    cleanup: Func,
    get_strategy_info: Func,
}
```

### 2. Plugin Interface Definition

```rust
// Host interface that plugins can call
#[derive(Clone)]
pub struct PluginHostInterface {
    pub log: fn(level: u32, message: &str),
    pub get_memory_usage: fn() -> u64,
    pub read_channels: fn(ptr: *const u8, len: usize) -> Vec<Channel>,
    pub write_channels: fn(channels: &[Channel]) -> (*mut u8, usize),
    pub get_config: fn(key: &str) -> Option<String>,
}

// WASM plugin trait (implemented by host)
#[async_trait]
pub trait WasmStageStrategy: StageStrategy {
    async fn load_from_file(path: &Path, host_interface: PluginHostInterface) -> Result<Self>
    where
        Self: Sized;
    
    fn get_plugin_info(&self) -> PluginInfo;
    fn get_memory_stats(&self) -> WasmMemoryStats;
    fn reload(&mut self) -> Result<()>;
}

#[derive(Debug, Clone)]
pub struct PluginInfo {
    pub name: String,
    pub version: String,
    pub author: String,
    pub description: String,
    pub supported_stages: Vec<String>,
    pub memory_requirements: PluginMemoryRequirements,
}

#[derive(Debug, Clone)]
pub struct PluginMemoryRequirements {
    pub min_heap_mb: usize,
    pub max_heap_mb: usize,
    pub supports_streaming: bool,
    pub supports_compression: bool,
}
```

### 3. Plugin Types and Examples

#### A. Memory Optimization Plugins

**Compression Strategy Plugin**
```rust
// Plugin that uses zstd compression for intermediate data
impl WasmStageStrategy for ZstdCompressionStrategy {
    async fn execute_data_mapping(&self, context: &StageContext, channels: Vec<Channel>) -> Result<Vec<Channel>> {
        // 1. Serialize channels to bytes
        let serialized = bincode::serialize(&channels)?;
        
        // 2. Compress using zstd in WASM
        let compressed = self.call_wasm_function("compress_data", &serialized)?;
        
        // 3. Process compressed data in WASM
        let processed_compressed = self.call_wasm_function("process_channels", &compressed)?;
        
        // 4. Decompress results
        let decompressed = self.call_wasm_function("decompress_data", &processed_compressed)?;
        
        // 5. Deserialize back to channels
        Ok(bincode::deserialize(&decompressed)?)
    }
}
```

**Custom Deduplication Plugin**
```wasm
;; WebAssembly text format example for channel deduplication
(module
  (import "host" "log" (func $log (param i32 i32)))
  (import "host" "memory" (memory 1))
  
  (export "deduplicate_channels" (func $deduplicate))
  
  (func $deduplicate (param $channels_ptr i32) (param $channels_len i32) (result i32)
    ;; Custom deduplication logic here
    ;; Uses advanced algorithms like bloom filters or hash tables
    ;; Returns pointer to deduplicated channel array
  )
)
```

#### B. External Integration Plugins

**Cloud Storage Spill Plugin**
```rust
// Plugin that spills data to cloud storage instead of local files
impl WasmStageStrategy for CloudStorageSpillStrategy {
    async fn execute_source_loading(&self, context: &StageContext, source_ids: Vec<Uuid>) -> Result<Vec<Channel>> {
        // Stream data directly to cloud storage (S3, GCS, etc.)
        // Use cloud-native pagination and filtering
        // Return minimal metadata, actual data stays in cloud
    }
}
```

**Database Integration Plugin**
```rust
// Plugin that uses specialized databases for temporary storage
impl WasmStageStrategy for RedisSpillStrategy {
    async fn execute_filtering(&self, context: &StageContext, channels: Vec<Channel>) -> Result<Vec<Channel>> {
        // Use Redis streams for filtering pipeline
        // Leverage Redis's memory-efficient data structures
        // Support distributed filtering across Redis cluster
    }
}
```

#### C. Domain-Specific Plugins

**Sports Channel Optimizer**
```rust
// Plugin optimized for sports content with specific metadata
impl WasmStageStrategy for SportsChannelStrategy {
    async fn execute_data_mapping(&self, context: &StageContext, channels: Vec<Channel>) -> Result<Vec<Channel>> {
        // Sports-specific logo resolution
        // League/team metadata enhancement  
        // Schedule-aware channel prioritization
        // Geographic content filtering
    }
}
```

### 4. Plugin Configuration System

```toml
[proxy_generation.wasm_plugins]
enabled = true
plugin_directory = "./plugins"
max_memory_per_plugin = "64MB"
timeout_seconds = 30
enable_hot_reload = true

[proxy_generation.wasm_plugins.security]
allow_network_access = false
allow_file_system_access = false
allowed_host_functions = [
    "log", "get_memory_usage", "read_channels", "write_channels"
]

[proxy_generation.wasm_plugins.strategies]
# Map stages to preferred plugin strategies
source_loading = ["cloud_spill", "inmemory_full"]
data_mapping = ["zstd_compression", "sports_optimizer", "parallel_mapping"]
filtering = ["redis_spill", "bitmask_filter", "inmemory_filter"]

[proxy_generation.wasm_plugins.fallbacks]
# Fallback to native strategies if plugins fail
enable_fallbacks = true
fallback_timeout_ms = 5000
max_plugin_failures = 3
```

### 5. Plugin Development Kit (PDK)

```rust
// Plugin Development Kit crate: m3u-proxy-plugin-sdk
pub mod prelude {
    pub use crate::{
        StageStrategy, StageContext, StageInput, StageOutput,
        MemoryPressureLevel, PluginInfo, PluginHostInterface,
        Channel, NumberedChannel, GenerationStats,
    };
    
    // Helper macros for plugin development
    pub use crate::macros::{
        export_plugin, log_info, log_warn, log_error,
        read_host_memory, write_host_memory,
    };
}

// Macro to simplify plugin exports
#[macro_export]
macro_rules! export_plugin {
    ($plugin_type:ty, $plugin_name:expr) => {
        #[no_mangle]
        pub extern "C" fn init() -> *mut c_void {
            // Plugin initialization
        }
        
        #[no_mangle] 
        pub extern "C" fn get_plugin_info() -> *const PluginInfo {
            // Return plugin metadata
        }
        
        #[no_mangle]
        pub extern "C" fn execute_stage(
            stage: *const c_char,
            input_ptr: *const u8,
            input_len: usize,
            context_ptr: *const u8,
            context_len: usize,
        ) -> PluginResult {
            // Stage execution wrapper
        }
    };
}
```

### 6. Plugin Example: Custom Compression Strategy

```rust
// plugins/zstd_compression/src/lib.rs
use m3u_proxy_plugin_sdk::prelude::*;

pub struct ZstdCompressionStrategy {
    compression_level: i32,
    dictionary: Option<Vec<u8>>,
}

impl StageStrategy for ZstdCompressionStrategy {
    async fn execute_data_mapping(
        &self, 
        context: &StageContext, 
        channels: Vec<Channel>
    ) -> Result<Vec<Channel>> {
        log_info!("Starting zstd compression data mapping");
        
        // Serialize channels
        let serialized = bincode::serialize(&channels)
            .map_err(|e| anyhow::anyhow!("Serialization failed: {}", e))?;
        
        // Compress with zstd
        let compressed = zstd::block::compress(&serialized, self.compression_level)
            .map_err(|e| anyhow::anyhow!("Compression failed: {}", e))?;
        
        log_info!("Compressed {} bytes to {} bytes (ratio: {:.2})", 
            serialized.len(), compressed.len(), 
            compressed.len() as f64 / serialized.len() as f64);
        
        // Apply data mapping on compressed data representation
        // (This is a simplified example - real implementation would be more complex)
        let processed_channels = self.apply_mapping_compressed(compressed, context).await?;
        
        log_info!("Zstd compression data mapping completed");
        Ok(processed_channels)
    }
    
    fn can_handle_memory_pressure(&self, level: MemoryPressureLevel) -> bool {
        // Compression helps with all memory pressure levels
        true
    }
    
    fn strategy_name(&self) -> &str {
        "zstd_compression"
    }
    
    fn estimated_memory_usage(&self, input_size: usize) -> Option<usize> {
        // Compression reduces memory usage significantly
        Some(input_size / 4) // Estimate 4:1 compression ratio
    }
}

impl ZstdCompressionStrategy {
    async fn apply_mapping_compressed(
        &self, 
        compressed_data: Vec<u8>, 
        context: &StageContext
    ) -> Result<Vec<Channel>> {
        // Decompress, apply mapping, recompress if needed
        let decompressed = zstd::block::decompress(&compressed_data, 1024 * 1024)
            .map_err(|e| anyhow::anyhow!("Decompression failed: {}", e))?;
        
        let channels: Vec<Channel> = bincode::deserialize(&decompressed)?;
        
        // Apply standard data mapping (could also be custom logic)
        // In real implementation, this would use the host's data mapping service
        Ok(channels) // Simplified
    }
}

// Export the plugin
export_plugin!(ZstdCompressionStrategy, "zstd_compression");
```

### 7. Plugin Management System

```rust
pub struct PluginManager {
    runtime: WasmRuntime,
    loaded_plugins: HashMap<String, Box<dyn WasmStageStrategy>>,
    plugin_configs: HashMap<String, PluginConfig>,
    health_monitor: PluginHealthMonitor,
}

impl PluginManager {
    pub async fn load_plugins_from_directory(&mut self, dir: &Path) -> Result<()> {
        for entry in std::fs::read_dir(dir)? {
            let path = entry?.path();
            if path.extension() == Some(std::ffi::OsStr::new("wasm")) {
                self.load_plugin_from_file(&path).await?;
            }
        }
        Ok(())
    }
    
    pub async fn hot_reload_plugin(&mut self, plugin_name: &str) -> Result<()> {
        // Safely reload plugin without disrupting active processing
    }
    
    pub fn get_plugin_for_stage(&self, stage: &str, memory_pressure: MemoryPressureLevel) -> Option<&dyn StageStrategy> {
        // Select best plugin for current conditions
    }
}

pub struct PluginHealthMonitor {
    failure_counts: HashMap<String, usize>,
    last_health_check: HashMap<String, std::time::Instant>,
    performance_metrics: HashMap<String, PluginPerformanceMetrics>,
}
```

### 8. Security Considerations

- **Sandboxing**: WASM provides natural sandboxing
- **Resource Limits**: Memory and CPU time limits per plugin
- **Capability-Based Security**: Only expose necessary host functions
- **Code Signing**: Verify plugin authenticity
- **Network Isolation**: Plugins cannot make direct network calls
- **File System Isolation**: Restricted file access

### 9. Benefits Summary

**For Users:**
- Extend functionality without code changes
- Community-driven strategy ecosystem
- Safe experimentation with custom logic

**For Developers:**
- Plugin SDK simplifies development
- Hot-reload for rapid iteration
- Cross-language support

**For Operations:**
- Graceful fallbacks if plugins fail
- Performance monitoring and metrics
- Resource usage control

This WebAssembly plugin architecture transforms the adaptive pipeline into a truly extensible platform where the community can contribute specialized strategies for different use cases and constraints.