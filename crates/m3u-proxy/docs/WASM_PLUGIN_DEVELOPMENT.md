# 🚀 **WASM Plugin Development Guide**

**m3u-proxy Advanced Strategy Development with WebAssembly**

This guide shows you how to design, develop, build, and deploy WASM plugins for the m3u-proxy streaming architecture. WASM plugins enable advanced processing strategies like chunking, memory-efficient spilling, and custom data transformations.

---

## 📋 **Table of Contents**

1. [Overview](#overview)
2. [Architecture](#architecture)
3. [Development Setup](#development-setup)
4. [Creating Your First Plugin](#creating-your-first-plugin)
5. [Advanced Strategies](#advanced-strategies)
6. [Host Interface Reference](#host-interface-reference)
7. [Build & Deploy](#build--deploy)
8. [Testing](#testing)
9. [Examples](#examples)
10. [Best Practices](#best-practices)

---

## 🎯 **Overview**

### What are WASM Plugins?

WASM plugins in m3u-proxy are WebAssembly modules that implement advanced processing strategies for different pipeline stages:

- **Source Loading**: Chunked loading, memory-efficient data retrieval
- **Data Mapping**: Custom transformations, logo processing  
- **Filtering**: Complex rule processing, pattern matching
- **File Spill**: Large dataset handling with temp file coordination

### Why Use WASM Plugins?

✅ **Memory Efficiency**: Handle datasets larger than available RAM  
✅ **Sandboxed Execution**: Safe, isolated processing environment  
✅ **Language Flexibility**: Write plugins in Rust, C++, AssemblyScript, etc.  
✅ **Performance**: Near-native execution speed  
✅ **Hot Reload**: Update strategies without restarting the service  

---

## 🏗️ **Architecture**

### Plugin Communication Flow

```
┌─────────────────┐    ┌──────────────────┐    ┌─────────────────┐
│   m3u-proxy     │    │   WASM Plugin    │    │  Host Interface │
│   (Host)        │    │   (Guest)        │    │   (Bridge)      │
├─────────────────┤    ├──────────────────┤    ├─────────────────┤
│ • Pipeline      │◄──►│ • Strategy Logic │◄──►│ • Memory Stats  │
│ • Config        │    │ • Data Processing│    │ • File I/O      │
│ • Orchestration │    │ • Temp Files     │    │ • Database      │
│ • Results       │    │ • Completion     │    │ • Logging       │
└─────────────────┘    └──────────────────┘    └─────────────────┘
```

### Data Flow

1. **Host** → Serializes input data → **Plugin**
2. **Plugin** → Processes chunks, monitors memory → **Host Interface**
3. **Host Interface** → Provides services (files, DB, logging) → **Plugin**
4. **Plugin** → Returns serialized results → **Host**

---

## ⚙️ **Development Setup**

### Prerequisites

```bash
# Install Rust with WASM target
rustup target add wasm32-wasi

# Install WASM tools
cargo install wasm-pack
cargo install wasmtime-cli

# Optional: WASM optimization tools
cargo install wasm-opt
```

### Project Structure

```
my-plugin/
├── Cargo.toml                 # Plugin manifest
├── src/
│   ├── lib.rs                # Plugin entry point
│   ├── strategy.rs           # Strategy implementation
│   └── host_interface.rs     # Host communication
├── examples/
│   └── test_plugin.rs        # Testing examples
└── build.sh                  # Build script
```

---

## 🔧 **Creating Your First Plugin**

### 1. Create Plugin Project

```bash
cargo new --lib my-chunked-loader
cd my-chunked-loader
```

### 2. Configure Cargo.toml

```toml
[package]
name = "my-chunked-loader"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[dependencies]
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
uuid = { version = "1.0", features = ["v4", "serde"] }

# m3u-proxy integration (in real deployment)
# m3u-proxy = { path = "../../crates/m3u-proxy" }

[target.'cfg(target_arch = "wasm32")'.dependencies]
wasm-bindgen = "0.2"
```

### 3. Implement Plugin Interface

```rust
// src/lib.rs
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

// Import host interface bindings
extern "C" {
    fn host_write_temp_file(id_ptr: *const u8, id_len: usize, data_ptr: *const u8, data_len: usize) -> i32;
    fn host_read_temp_file(id_ptr: *const u8, id_len: usize, out_ptr: *mut *mut u8, out_len: *mut usize) -> i32;
    fn host_get_memory_usage() -> u64;
    fn host_log(level: u32, msg_ptr: *const u8, msg_len: usize);
}

// Plugin data structures
#[derive(Debug, Serialize, Deserialize)]
pub struct Channel {
    pub id: Uuid,
    pub channel_name: String,
    pub source_id: Uuid,
    // ... other fields
}

#[derive(Debug, Serialize, Deserialize)]
pub struct StageChunk<T> {
    pub data: Vec<T>,
    pub chunk_id: usize,
    pub is_final_chunk: bool,
    pub total_chunks: Option<usize>,
    pub total_items: Option<usize>,
}

// Plugin state
static mut PLUGIN_STATE: Option<ChunkedSourceLoader> = None;

pub struct ChunkedSourceLoader {
    memory_threshold_mb: usize,
    accumulated_channels: Vec<Channel>,
    spilled_files: Vec<String>,
    chunks_processed: usize,
}

impl ChunkedSourceLoader {
    pub fn new(memory_threshold_mb: usize) -> Self {
        Self {
            memory_threshold_mb,
            accumulated_channels: Vec::new(),
            spilled_files: Vec::new(),
            chunks_processed: 0,
        }
    }

    fn should_spill(&self) -> bool {
        let memory_usage = unsafe { host_get_memory_usage() } / (1024 * 1024); // Convert to MB
        memory_usage as usize >= self.memory_threshold_mb
    }

    fn write_temp_file(&self, file_id: &str, data: &[u8]) -> Result<(), String> {
        let result = unsafe {
            host_write_temp_file(
                file_id.as_ptr(),
                file_id.len(),
                data.as_ptr(),
                data.len()
            )
        };
        if result == 0 { Ok(()) } else { Err("Write failed".to_string()) }
    }

    fn log(&self, message: &str) {
        unsafe {
            host_log(1, message.as_ptr(), message.len()); // 1 = Info level
        }
    }
}

// Plugin exports - called by m3u-proxy host
#[no_mangle]
pub extern "C" fn plugin_init(memory_threshold_mb: usize) -> i32 {
    unsafe {
        PLUGIN_STATE = Some(ChunkedSourceLoader::new(memory_threshold_mb));
    }
    0 // Success
}

#[no_mangle]
pub extern "C" fn plugin_process_chunk(
    chunk_data_ptr: *const u8,
    chunk_data_len: usize,
    chunk_metadata_ptr: *const u8,
    chunk_metadata_len: usize,
    out_ptr: *mut *mut u8,
    out_len: *mut usize,
) -> i32 {
    unsafe {
        if let Some(ref mut state) = PLUGIN_STATE {
            // Deserialize input
            let chunk_data = std::slice::from_raw_parts(chunk_data_ptr, chunk_data_len);
            let metadata_data = std::slice::from_raw_parts(chunk_metadata_ptr, chunk_metadata_len);
            
            match process_chunk_impl(state, chunk_data, metadata_data) {
                Ok(output) => {
                    // Allocate output buffer
                    let output_ptr = libc::malloc(output.len()) as *mut u8;
                    std::ptr::copy_nonoverlapping(output.as_ptr(), output_ptr, output.len());
                    *out_ptr = output_ptr;
                    *out_len = output.len();
                    0 // Success
                }
                Err(_) => -1 // Error
            }
        } else {
            -1 // Plugin not initialized
        }
    }
}

fn process_chunk_impl(
    state: &mut ChunkedSourceLoader,
    chunk_data: &[u8],
    metadata_data: &[u8],
) -> Result<Vec<u8>, String> {
    // Deserialize source IDs
    let source_ids: Vec<Uuid> = serde_json::from_slice(chunk_data)
        .map_err(|e| format!("Failed to deserialize chunk: {}", e))?;
    
    // Deserialize metadata
    let metadata: StageChunk<()> = serde_json::from_slice(metadata_data)
        .map_err(|e| format!("Failed to deserialize metadata: {}", e))?;
    
    state.log(&format!("Processing chunk {} with {} sources", metadata.chunk_id, source_ids.len()));
    state.chunks_processed += 1;

    // Simulate loading channels (in real plugin: make host database calls)
    let mut new_channels = Vec::new();
    for source_id in source_ids {
        // In real implementation: call host_database_query(source_id)
        // For demo, create mock channel
        new_channels.push(Channel {
            id: Uuid::new_v4(),
            channel_name: format!("Channel from source {}", source_id),
            source_id,
        });
    }

    state.accumulated_channels.extend(new_channels);

    // Check if we should spill
    if state.should_spill() {
        let file_id = format!("spill_{}", state.chunks_processed);
        let spill_data = serde_json::to_vec(&state.accumulated_channels)
            .map_err(|e| format!("Serialization failed: {}", e))?;
        
        state.write_temp_file(&file_id, &spill_data)?;
        state.spilled_files.push(file_id);
        state.accumulated_channels.clear();
        
        state.log(&format!("Spilled chunk {} to temp file", metadata.chunk_id));
    }

    // Return empty for accumulating strategy
    let empty_result: Vec<Channel> = Vec::new();
    serde_json::to_vec(&empty_result)
        .map_err(|e| format!("Failed to serialize output: {}", e))
}

#[no_mangle]
pub extern "C" fn plugin_finalize(
    out_ptr: *mut *mut u8,
    out_len: *mut usize,
) -> i32 {
    unsafe {
        if let Some(ref mut state) = PLUGIN_STATE {
            match finalize_impl(state) {
                Ok(output) => {
                    let output_ptr = libc::malloc(output.len()) as *mut u8;
                    std::ptr::copy_nonoverlapping(output.as_ptr(), output_ptr, output.len());
                    *out_ptr = output_ptr;
                    *out_len = output.len();
                    0
                }
                Err(_) => -1
            }
        } else {
            -1
        }
    }
}

fn finalize_impl(state: &mut ChunkedSourceLoader) -> Result<Vec<u8>, String> {
    state.log(&format!("Finalizing: {} spilled files, {} in memory", 
                      state.spilled_files.len(), 
                      state.accumulated_channels.len()));

    // TODO: Load from spilled files and combine with memory
    // For demo, just return accumulated channels
    serde_json::to_vec(&state.accumulated_channels)
        .map_err(|e| format!("Failed to serialize final result: {}", e))
}

#[no_mangle]
pub extern "C" fn plugin_cleanup() -> i32 {
    unsafe {
        PLUGIN_STATE = None;
    }
    0
}
```

---

## 🚀 **Advanced Strategies**

### Chunked Processing Strategy

```rust
// Advanced chunking with memory pressure monitoring
impl ChunkedSourceLoader {
    fn process_with_memory_monitoring(&mut self, sources: Vec<Uuid>) -> Result<Vec<Channel>, String> {
        let chunk_size = self.calculate_optimal_chunk_size();
        
        for chunk in sources.chunks(chunk_size) {
            // Process chunk
            let channels = self.load_channels_for_sources(chunk)?;
            self.accumulated_channels.extend(channels);
            
            // Monitor memory and spill if needed
            if self.memory_pressure_detected() {
                self.emergency_spill()?;
            }
        }
        
        Ok(self.finalize_processing()?)
    }
    
    fn calculate_optimal_chunk_size(&self) -> usize {
        let available_memory = self.get_available_memory();
        let estimated_channel_size = 2048; // bytes per channel
        (available_memory / estimated_channel_size).min(1000).max(10)
    }
    
    fn memory_pressure_detected(&self) -> bool {
        let memory_usage = unsafe { host_get_memory_usage() };
        let threshold = (self.memory_threshold_mb as u64) * 1024 * 1024;
        memory_usage > threshold
    }
}
```

### File Spill Strategy

```rust
impl FileSpillStrategy {
    async fn intelligent_spill(&mut self, data: &[Channel]) -> Result<String, String> {
        // Compress data before spilling
        let compressed = self.compress_channels(data)?;
        
        // Generate unique file ID
        let file_id = format!("spill_{}_{}", 
                             self.strategy_id, 
                             chrono::Utc::now().timestamp_millis());
        
        // Write with metadata
        let metadata = SpillMetadata {
            channel_count: data.len(),
            compression_ratio: data.len() as f64 / compressed.len() as f64,
            spill_reason: self.get_spill_reason(),
            timestamp: chrono::Utc::now(),
        };
        
        self.write_temp_file_with_metadata(&file_id, &compressed, &metadata)?;
        self.register_spill_file(file_id.clone())?;
        
        Ok(file_id)
    }
    
    fn compress_channels(&self, channels: &[Channel]) -> Result<Vec<u8>, String> {
        // Implement compression (e.g., zstd, lz4)
        // For demo, just use JSON
        serde_json::to_vec(channels)
            .map_err(|e| format!("Compression failed: {}", e))
    }
}
```

---

## 🔌 **Host Interface Reference**

### Memory Management

```rust
// Get current memory usage in bytes
extern "C" fn host_get_memory_usage() -> u64;

// Get memory pressure level (0=Optimal, 1=Moderate, 2=High, 3=Critical, 4=Emergency)
extern "C" fn host_get_memory_pressure() -> u32;

// Request garbage collection
extern "C" fn host_gc_request() -> i32;
```

### File Operations

```rust
// Write temporary file
extern "C" fn host_write_temp_file(
    id_ptr: *const u8, id_len: usize,           // File ID
    data_ptr: *const u8, data_len: usize       // Data to write
) -> i32;

// Read temporary file
extern "C" fn host_read_temp_file(
    id_ptr: *const u8, id_len: usize,          // File ID
    out_ptr: *mut *mut u8, out_len: *mut usize // Output buffer
) -> i32;

// Delete temporary file
extern "C" fn host_delete_temp_file(
    id_ptr: *const u8, id_len: usize           // File ID
) -> i32;
```

### Database Access

```rust
// Query channels for source
extern "C" fn host_database_query_source(
    source_id_ptr: *const u8,                  // UUID bytes (16 bytes)
    out_ptr: *mut *mut u8, out_len: *mut usize // Serialized channels
) -> i32;

// Execute custom SQL query
extern "C" fn host_database_query_sql(
    sql_ptr: *const u8, sql_len: usize,
    params_ptr: *const u8, params_len: usize,  // Serialized parameters
    out_ptr: *mut *mut u8, out_len: *mut usize // Serialized results
) -> i32;
```

### Logging

```rust
// Log message with level
extern "C" fn host_log(
    level: u32,                                 // 0=Debug, 1=Info, 2=Warn, 3=Error
    msg_ptr: *const u8, msg_len: usize
);

// Performance metrics
extern "C" fn host_record_metric(
    name_ptr: *const u8, name_len: usize,
    value: f64,
    tags_ptr: *const u8, tags_len: usize       // Serialized tags
);
```

### Configuration

```rust
// Get configuration value
extern "C" fn host_get_config(
    key_ptr: *const u8, key_len: usize,
    out_ptr: *mut *mut u8, out_len: *mut usize // Config value
) -> i32;

// Set plugin configuration
extern "C" fn host_set_plugin_config(
    config_ptr: *const u8, config_len: usize   // Serialized config
) -> i32;
```

---

## 🛠️ **Build & Deploy**

### 1. Build Script

```bash
#!/bin/bash
# build.sh

set -e

echo "Building WASM plugin..."

# Build for WASM target
cargo build --target wasm32-wasi --release

# Optimize WASM binary
wasm-opt target/wasm32-wasi/release/my_chunked_loader.wasm \
    -O3 --enable-bulk-memory --enable-sign-ext \
    -o dist/my_chunked_loader.wasm

# Generate plugin manifest
cat > dist/plugin.toml << EOF
[plugin]
name = "my-chunked-loader"
version = "0.1.0"
author = "Your Name"
description = "Chunked source loading with memory spilling"

[capabilities]
stages = ["source_loading"]
memory_efficient = true
supports_streaming = true
max_memory_mb = 512

[host_interface]
version = "1.0"
required_functions = [
    "host_get_memory_usage",
    "host_write_temp_file",
    "host_read_temp_file",
    "host_database_query_source",
    "host_log"
]
EOF

echo "Plugin built successfully!"
echo "  Binary: dist/my_chunked_loader.wasm"
echo "  Manifest: dist/plugin.toml"
```

### 2. Deploy Plugin

```bash
# Copy to m3u-proxy plugins directory
cp dist/my_chunked_loader.wasm /opt/m3u-proxy/plugins/
cp dist/plugin.toml /opt/m3u-proxy/plugins/my_chunked_loader.toml

# Restart m3u-proxy to load plugin
systemctl restart m3u-proxy
```

### 3. Configuration

```toml
# /etc/m3u-proxy/plugins.toml
[plugins.my_chunked_loader]
enabled = true
memory_threshold_mb = 256
chunk_size = 1000
strategy_priority = 10

[plugins.my_chunked_loader.config]
database_connection_pool_size = 5
temp_file_retention_hours = 24
compression_enabled = true
```

---

## 🧪 **Testing**

### Unit Testing

```rust
// tests/plugin_tests.rs
#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_chunked_processing() {
        let mut loader = ChunkedSourceLoader::new(100);
        
        let sources = vec![
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
        ];
        
        // Test chunk processing
        let chunk = StageChunk {
            data: sources,
            chunk_id: 0,
            is_final_chunk: false,
            total_chunks: Some(1),
            total_items: Some(3),
        };
        
        let chunk_data = serde_json::to_vec(&chunk.data).unwrap();
        let metadata_data = serde_json::to_vec(&chunk).unwrap();
        
        let result = process_chunk_impl(&mut loader, &chunk_data, &metadata_data);
        assert!(result.is_ok());
    }
}
```

### Integration Testing

```bash
# Test with wasmtime
wasmtime --invoke plugin_init dist/my_chunked_loader.wasm 256

# Test chunk processing
echo '["01234567-89ab-cdef-0123-456789abcdef"]' | \
    wasmtime --invoke plugin_process_chunk dist/my_chunked_loader.wasm

# Test finalization
wasmtime --invoke plugin_finalize dist/my_chunked_loader.wasm
```

### Performance Testing

```bash
# Benchmark memory usage
hyperfine --warmup 3 \
    'wasmtime --invoke plugin_process_chunk dist/my_chunked_loader.wasm < test_data.json'

# Memory profiling
valgrind --tool=massif wasmtime dist/my_chunked_loader.wasm
```

---

## 📝 **Examples**

### Example 1: Simple Chunked Loader

See the complete implementation above in the "Creating Your First Plugin" section.

### Example 2: File Spill Data Mapper

```rust
// File spill strategy for data mapping
pub struct FileSpillDataMapper {
    memory_threshold: usize,
    spill_count: usize,
    temp_files: Vec<String>,
}

impl FileSpillDataMapper {
    fn apply_mapping_with_spill(&mut self, channels: Vec<Channel>) -> Result<Vec<Channel>, String> {
        let mut mapped_channels = Vec::new();
        
        for chunk in channels.chunks(100) {
            // Apply data mapping transformations
            let mapped_chunk = self.apply_transformations(chunk)?;
            mapped_channels.extend(mapped_chunk);
            
            // Check memory and spill if needed
            if self.should_spill() {
                self.spill_mapped_channels(&mapped_channels)?;
                mapped_channels.clear();
            }
        }
        
        // Load and combine all spilled data
        self.load_and_combine_spilled().await
    }
}
```

### Example 3: Custom Filter Plugin

```rust
// Advanced filtering with pattern matching
pub struct AdvancedFilterPlugin {
    compiled_patterns: Vec<regex::Regex>,
    filter_stats: FilterStatistics,
}

impl AdvancedFilterPlugin {
    fn filter_channels(&mut self, channels: Vec<Channel>) -> Result<Vec<Channel>, String> {
        let mut filtered = Vec::new();
        
        for channel in channels {
            if self.apply_complex_filters(&channel)? {
                filtered.push(channel);
                self.filter_stats.passed += 1;
            } else {
                self.filter_stats.filtered += 1;
            }
        }
        
        self.log_filter_statistics();
        Ok(filtered)
    }
}
```

---

## ✅ **Best Practices**

### Memory Management

✅ **Monitor Memory Usage**: Always check `host_get_memory_usage()` before large operations  
✅ **Spill Early**: Don't wait until memory is exhausted  
✅ **Clean Up**: Always clean up temp files in finalize()  
✅ **Estimate Sizes**: Calculate memory requirements before processing  

### Error Handling

✅ **Graceful Degradation**: Fallback to simpler strategies on errors  
✅ **Detailed Logging**: Log errors with context for debugging  
✅ **Validation**: Validate input data before processing  
✅ **Resource Cleanup**: Always clean up resources on errors  

### Performance

✅ **Minimize Allocations**: Reuse buffers when possible  
✅ **Batch Operations**: Group database queries and file operations  
✅ **Compression**: Compress spilled data to save space  
✅ **Profiling**: Measure and optimize hot paths  

### Security

✅ **Input Validation**: Validate all inputs from host  
✅ **Resource Limits**: Respect memory and time limits  
✅ **No Unsafe Operations**: Avoid unsafe code when possible  
✅ **Sandboxing**: Don't try to break out of WASM sandbox  

### Testing

✅ **Unit Tests**: Test plugin logic independently  
✅ **Integration Tests**: Test with real host interface  
✅ **Performance Tests**: Benchmark memory and speed  
✅ **Edge Cases**: Test with empty data, large datasets, errors  

---

## 🎯 **Summary**

You now have a complete guide for developing WASM plugins for m3u-proxy:

1. **✅ Development Environment**: Rust + WASM toolchain
2. **✅ Plugin Architecture**: Host interface, serialized boundaries
3. **✅ Advanced Strategies**: Chunking, file spill, memory management
4. **✅ Build & Deploy**: Optimization, configuration, deployment
5. **✅ Testing**: Unit, integration, and performance testing
6. **✅ Best Practices**: Memory, performance, security guidelines

### Next Steps

1. Start with the simple chunked loader example
2. Implement your specific strategy requirements
3. Add comprehensive error handling and logging
4. Optimize for your expected data sizes
5. Deploy and monitor in production

For more examples and advanced techniques, check the `src/proxy/wasm_examples.rs` module in the m3u-proxy codebase! 🚀