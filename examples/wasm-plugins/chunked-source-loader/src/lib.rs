//! Chunked Source Loader WASM Plugin
//!
//! This plugin demonstrates how to implement a memory-efficient source loading strategy
//! that processes data in chunks and spills to temporary files when memory pressure is high.
//!
//! Features:
//! - Configurable chunk sizes and memory thresholds
//! - Automatic memory pressure detection and spilling
//! - Cross-stage temp file coordination
//! - Comprehensive logging and error handling

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

// Host interface function imports
extern "C" {
    fn host_write_temp_file(id_ptr: *const u8, id_len: usize, data_ptr: *const u8, data_len: usize) -> i32;
    fn host_read_temp_file(id_ptr: *const u8, id_len: usize, out_ptr: *mut *mut u8, out_len: *mut usize) -> i32;
    fn host_delete_temp_file(id_ptr: *const u8, id_len: usize) -> i32;
    fn host_get_memory_usage() -> u64;
    fn host_get_memory_pressure() -> u32;
    fn host_log(level: u32, msg_ptr: *const u8, msg_len: usize);
    fn host_database_query_source(source_id_ptr: *const u8, out_ptr: *mut *mut u8, out_len: *mut usize) -> i32;
}

/// Channel data structure (simplified)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Channel {
    pub id: Uuid,
    pub channel_name: String,
    pub source_id: Uuid,
    pub stream_url: String,
    pub tvg_id: Option<String>,
    pub tvg_name: Option<String>,
    pub tvg_logo: Option<String>,
    pub group_title: Option<String>,
}

/// Stage chunk with completion metadata
#[derive(Debug, Serialize, Deserialize)]
pub struct StageChunk<T> {
    pub data: Vec<T>,
    pub chunk_id: usize,
    pub is_final_chunk: bool,
    pub total_chunks: Option<usize>,
    pub total_items: Option<usize>,
}

/// Plugin configuration
#[derive(Debug, Serialize, Deserialize)]
pub struct PluginConfig {
    pub memory_threshold_mb: usize,
    pub chunk_size: usize,
    pub compression_enabled: bool,
    pub max_spill_files: usize,
}

impl Default for PluginConfig {
    fn default() -> Self {
        Self {
            memory_threshold_mb: 256,
            chunk_size: 1000,
            compression_enabled: true,
            max_spill_files: 100,
        }
    }
}

/// Plugin state
pub struct ChunkedSourceLoader {
    config: PluginConfig,
    accumulated_channels: Vec<Channel>,
    spilled_files: Vec<String>,
    chunks_processed: usize,
    total_channels_processed: usize,
    start_time: chrono::DateTime<chrono::Utc>,
}

impl ChunkedSourceLoader {
    pub fn new(config: PluginConfig) -> Self {
        Self {
            config,
            accumulated_channels: Vec::new(),
            spilled_files: Vec::new(),
            chunks_processed: 0,
            total_channels_processed: 0,
            start_time: chrono::Utc::now(),
        }
    }

    /// Check if we should spill based on memory usage and pressure
    fn should_spill(&self) -> bool {
        let memory_usage_bytes = unsafe { host_get_memory_usage() };
        let memory_usage_mb = memory_usage_bytes / (1024 * 1024);
        let memory_pressure = unsafe { host_get_memory_pressure() };

        // Spill if we exceed threshold or high memory pressure
        memory_usage_mb as usize >= self.config.memory_threshold_mb || memory_pressure >= 3
    }

    /// Spill accumulated channels to temporary file
    fn spill_to_temp_file(&mut self) -> Result<(), String> {
        if self.accumulated_channels.is_empty() {
            return Ok(());
        }

        let file_id = format!("chunked_source_spill_{}", self.chunks_processed);
        
        // Serialize channels
        let data = serde_json::to_vec(&self.accumulated_channels)
            .map_err(|e| format!("Serialization failed: {}", e))?;

        // Write to temp file via host interface
        let result = unsafe {
            host_write_temp_file(
                file_id.as_ptr(),
                file_id.len(),
                data.as_ptr(),
                data.len()
            )
        };

        if result != 0 {
            return Err(format!("Failed to write temp file: {}", file_id));
        }

        // Log spill operation
        let message = format!(
            "Spilled {} channels ({} KB) to temp file: {}",
            self.accumulated_channels.len(),
            data.len() / 1024,
            file_id
        );
        self.log_info(&message);

        // Track spilled file
        self.spilled_files.push(file_id);
        self.accumulated_channels.clear();

        Ok(())
    }

    /// Load channels from a spilled temp file
    fn load_from_temp_file(&self, file_id: &str) -> Result<Vec<Channel>, String> {
        let mut data_ptr: *mut u8 = std::ptr::null_mut();
        let mut data_len: usize = 0;

        let result = unsafe {
            host_read_temp_file(
                file_id.as_ptr(),
                file_id.len(),
                &mut data_ptr,
                &mut data_len
            )
        };

        if result != 0 {
            return Err(format!("Failed to read temp file: {}", file_id));
        }

        if data_ptr.is_null() || data_len == 0 {
            return Ok(Vec::new());
        }

        // Deserialize channels
        let data = unsafe { std::slice::from_raw_parts(data_ptr, data_len) };
        let channels: Vec<Channel> = serde_json::from_slice(data)
            .map_err(|e| format!("Deserialization failed: {}", e))?;

        // Free the allocated memory
        unsafe { libc::free(data_ptr as *mut _) };

        Ok(channels)
    }

    /// Load channels for a list of source IDs
    fn load_channels_for_sources(&mut self, source_ids: &[Uuid]) -> Result<Vec<Channel>, String> {
        let mut all_channels = Vec::new();

        for source_id in source_ids {
            // Query channels via host interface
            let mut data_ptr: *mut u8 = std::ptr::null_mut();
            let mut data_len: usize = 0;

            let result = unsafe {
                host_database_query_source(
                    source_id.as_bytes().as_ptr(),
                    &mut data_ptr,
                    &mut data_len
                )
            };

            if result == 0 && !data_ptr.is_null() && data_len > 0 {
                // Deserialize channels
                let data = unsafe { std::slice::from_raw_parts(data_ptr, data_len) };
                if let Ok(channels) = serde_json::from_slice::<Vec<Channel>>(data) {
                    all_channels.extend(channels);
                }
                
                // Free allocated memory
                unsafe { libc::free(data_ptr as *mut _) };
            }
        }

        self.total_channels_processed += all_channels.len();
        Ok(all_channels)
    }

    /// Clean up all temp files
    fn cleanup_temp_files(&self) {
        for file_id in &self.spilled_files {
            unsafe {
                host_delete_temp_file(file_id.as_ptr(), file_id.len());
            }
        }
    }

    /// Log info message
    fn log_info(&self, message: &str) {
        unsafe {
            host_log(1, message.as_ptr(), message.len()); // 1 = Info
        }
    }

    /// Log error message
    fn log_error(&self, message: &str) {
        unsafe {
            host_log(3, message.as_ptr(), message.len()); // 3 = Error
        }
    }

    /// Get performance statistics
    fn get_stats(&self) -> HashMap<String, serde_json::Value> {
        let mut stats = HashMap::new();
        let elapsed = chrono::Utc::now() - self.start_time;
        
        stats.insert("chunks_processed".to_string(), self.chunks_processed.into());
        stats.insert("spilled_files".to_string(), self.spilled_files.len().into());
        stats.insert("total_channels".to_string(), self.total_channels_processed.into());
        stats.insert("elapsed_ms".to_string(), elapsed.num_milliseconds().into());
        stats.insert("memory_threshold_mb".to_string(), self.config.memory_threshold_mb.into());
        
        if self.chunks_processed > 0 {
            let avg_channels_per_chunk = self.total_channels_processed / self.chunks_processed;
            stats.insert("avg_channels_per_chunk".to_string(), avg_channels_per_chunk.into());
        }

        stats
    }
}

// Global plugin state
static mut PLUGIN_STATE: Option<ChunkedSourceLoader> = None;

/// Initialize plugin with configuration
#[no_mangle]
pub extern "C" fn plugin_init(config_ptr: *const u8, config_len: usize) -> i32 {
    let config = if config_ptr.is_null() || config_len == 0 {
        PluginConfig::default()
    } else {
        let config_data = unsafe { std::slice::from_raw_parts(config_ptr, config_len) };
        match serde_json::from_slice::<PluginConfig>(config_data) {
            Ok(config) => config,
            Err(_) => {
                return -1; // Invalid config
            }
        }
    };

    unsafe {
        PLUGIN_STATE = Some(ChunkedSourceLoader::new(config));
    }

    // Log initialization
    let message = "Chunked Source Loader plugin initialized";
    unsafe {
        host_log(1, message.as_ptr(), message.len());
    }

    0 // Success
}

/// Process a chunk of source IDs
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
            match process_chunk_impl(state, chunk_data_ptr, chunk_data_len, chunk_metadata_ptr, chunk_metadata_len) {
                Ok(output) => {
                    // Allocate output buffer
                    let output_ptr = libc::malloc(output.len()) as *mut u8;
                    if output_ptr.is_null() {
                        return -2; // Memory allocation failed
                    }
                    std::ptr::copy_nonoverlapping(output.as_ptr(), output_ptr, output.len());
                    *out_ptr = output_ptr;
                    *out_len = output.len();
                    0 // Success
                }
                Err(err) => {
                    state.log_error(&err);
                    -1 // Processing error
                }
            }
        } else {
            -1 // Plugin not initialized
        }
    }
}

fn process_chunk_impl(
    state: &mut ChunkedSourceLoader,
    chunk_data_ptr: *const u8,
    chunk_data_len: usize,
    chunk_metadata_ptr: *const u8,
    chunk_metadata_len: usize,
) -> Result<Vec<u8>, String> {
    // Deserialize source IDs
    let chunk_data = unsafe { std::slice::from_raw_parts(chunk_data_ptr, chunk_data_len) };
    let source_ids: Vec<Uuid> = serde_json::from_slice(chunk_data)
        .map_err(|e| format!("Failed to deserialize source IDs: {}", e))?;

    // Deserialize metadata
    let metadata_data = unsafe { std::slice::from_raw_parts(chunk_metadata_ptr, chunk_metadata_len) };
    let metadata: StageChunk<()> = serde_json::from_slice(metadata_data)
        .map_err(|e| format!("Failed to deserialize metadata: {}", e))?;

    let message = format!(
        "Processing chunk {} of {} with {} source IDs",
        metadata.chunk_id + 1,
        metadata.total_chunks.unwrap_or(0),
        source_ids.len()
    );
    state.log_info(&message);

    // Load channels for this chunk
    let channels = state.load_channels_for_sources(&source_ids)?;
    state.accumulated_channels.extend(channels);
    state.chunks_processed += 1;

    // Check if we should spill
    if state.should_spill() {
        state.spill_to_temp_file()?;
    }

    // For accumulating strategy, return empty result
    let empty_result: Vec<Channel> = Vec::new();
    serde_json::to_vec(&empty_result)
        .map_err(|e| format!("Failed to serialize output: {}", e))
}

/// Finalize processing and return all accumulated channels
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
                    if output_ptr.is_null() {
                        return -2; // Memory allocation failed
                    }
                    std::ptr::copy_nonoverlapping(output.as_ptr(), output_ptr, output.len());
                    *out_ptr = output_ptr;
                    *out_len = output.len();
                    0 // Success
                }
                Err(err) => {
                    state.log_error(&err);
                    -1 // Finalization error
                }
            }
        } else {
            -1 // Plugin not initialized
        }
    }
}

fn finalize_impl(state: &mut ChunkedSourceLoader) -> Result<Vec<u8>, String> {
    let message = format!(
        "Finalizing: {} spilled files, {} channels in memory, {} total channels processed",
        state.spilled_files.len(),
        state.accumulated_channels.len(),
        state.total_channels_processed
    );
    state.log_info(&message);

    // Combine all channels from memory and spilled files
    let mut all_channels = state.accumulated_channels.clone();

    // Load from spilled files
    for file_id in &state.spilled_files.clone() {
        match state.load_from_temp_file(file_id) {
            Ok(spilled_channels) => {
                all_channels.extend(spilled_channels);
            }
            Err(err) => {
                state.log_error(&format!("Failed to load spilled file {}: {}", file_id, err));
            }
        }
    }

    // Clean up temp files
    state.cleanup_temp_files();

    // Log final statistics
    let stats = state.get_stats();
    let stats_message = format!(
        "Chunked loading complete: {} total channels, {} chunks processed, {} spilled files, {}ms elapsed",
        all_channels.len(),
        stats.get("chunks_processed").unwrap_or(&0.into()),
        stats.get("spilled_files").unwrap_or(&0.into()),
        stats.get("elapsed_ms").unwrap_or(&0.into())
    );
    state.log_info(&stats_message);

    // Return all channels
    serde_json::to_vec(&all_channels)
        .map_err(|e| format!("Failed to serialize final result: {}", e))
}

/// Get plugin capabilities
#[no_mangle]
pub extern "C" fn plugin_get_capabilities(
    out_ptr: *mut *mut u8,
    out_len: *mut usize,
) -> i32 {
    let capabilities = serde_json::json!({
        "supports_streaming": true,
        "requires_all_data": false,
        "can_produce_early_output": false,
        "preferred_chunk_size": 1000,
        "memory_efficient": true,
        "stage_types": ["source_loading"],
        "host_interface_version": "1.0"
    });

    match serde_json::to_vec(&capabilities) {
        Ok(data) => unsafe {
            let output_ptr = libc::malloc(data.len()) as *mut u8;
            if output_ptr.is_null() {
                return -2;
            }
            std::ptr::copy_nonoverlapping(data.as_ptr(), output_ptr, data.len());
            *out_ptr = output_ptr;
            *out_len = data.len();
            0
        },
        Err(_) => -1
    }
}

/// Cleanup plugin resources
#[no_mangle]
pub extern "C" fn plugin_cleanup() -> i32 {
    unsafe {
        if let Some(ref state) = PLUGIN_STATE {
            state.cleanup_temp_files();
            state.log_info("Chunked Source Loader plugin cleaned up");
        }
        PLUGIN_STATE = None;
    }
    0
}

/// Get plugin information
#[no_mangle]
pub extern "C" fn plugin_get_info(
    out_ptr: *mut *mut u8,
    out_len: *mut usize,
) -> i32 {
    let info = serde_json::json!({
        "name": "chunked-source-loader",
        "version": "0.0.1",
        "description": "Memory-efficient chunked source loading with spilling",
        "author": "m3u-proxy developers",
        "license": "MIT"
    });

    match serde_json::to_vec(&info) {
        Ok(data) => unsafe {
            let output_ptr = libc::malloc(data.len()) as *mut u8;
            if output_ptr.is_null() {
                return -2;
            }
            std::ptr::copy_nonoverlapping(data.as_ptr(), output_ptr, data.len());
            *out_ptr = output_ptr;
            *out_len = data.len();
            0
        },
        Err(_) => -1
    }
}