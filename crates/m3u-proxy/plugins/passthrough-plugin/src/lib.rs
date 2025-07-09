//! Pass-through WASM Plugin - Reference Implementation
//!
//! This plugin serves as the reference implementation for the WASM plugin system.
//! It demonstrates how to properly consume data from orchestrator iterators and
//! perform clean passthrough processing for all pipeline stages.
//!
//! Pipeline Stages:
//! 1. Data Mapping: Consumes OrderedDataMappingIterator + channels → outputs mapped channels  
//! 2. Filtering: Consumes OrderedFilterIterator + channels → outputs filtered channels
//! 3. Logo Pre-fetch: Processes channels for logo caching → outputs channels with cached logo URLs
//! 4. Channel Numbering: Assigns sequential numbers → outputs numbered channels
//! 5. M3U Generation: Converts numbered channels → outputs M3U content
//! 6. EPG Processing: Consumes OrderedEpgAggregateIterator + final channel map → outputs XMLTV

use serde::{Deserialize, Serialize};
use uuid::Uuid;

// Host interface function imports (Standardized naming)
unsafe extern "C" {
    // System & Logging (consistent parameter naming)
    fn host_log_write(level: u32, message_ptr: u32, message_len: u32) -> i32;
    fn host_system_flush_memory() -> i32;
    fn host_progress_report(stage_ptr: u32, stage_len: u32, message_ptr: u32, message_len: u32) -> i32;
    fn host_memory_get_pressure() -> u32;
    
    // Service Operations
    fn host_logo_cache(url_ptr: u32, url_len: u32, result_ptr: u32, result_len: u32) -> i32;
    
    // Iterator Operations (Input) - consistent naming
    fn host_iterator_read_chunk(iterator_id: u32, output_ptr: u32, output_len: u32, requested_count: u32) -> i32;
    fn host_iterator_close(iterator_id: u32) -> i32;
    
    // Iterator Operations (Output) - consistent naming
    fn host_iterator_create(iterator_type: u32) -> u32;  // Create new iterator, return ID
    fn host_iterator_write_chunk(iterator_id: u32, data_ptr: u32, data_len: u32, items_count: u32) -> i32;  // Add data chunk
    fn host_iterator_finalize(iterator_id: u32) -> i32;  // Mark iterator as complete
    
    // File Operations - consistent parameter naming
    fn host_file_create(path_ptr: u32, path_len: u32) -> i32;
    fn host_file_write(path_ptr: u32, path_len: u32, data_ptr: u32, data_len: u32) -> i32;
    fn host_file_read(path_ptr: u32, path_len: u32, output_ptr: u32, output_len: u32) -> i32;
    fn host_file_delete(path_ptr: u32, path_len: u32) -> i32;
}

/// Log levels that match the host interface
const LOG_ERROR: u32 = 1;
const _LOG_WARN: u32 = 2;
const LOG_INFO: u32 = 3;
const LOG_DEBUG: u32 = 4;

/// Iterator types that match the host interface
const ITERATOR_TYPE_CHANNEL: u32 = 1;
const ITERATOR_TYPE_MAPPING_RULE: u32 = 2;
const ITERATOR_TYPE_FILTER_RULE: u32 = 3;
const ITERATOR_TYPE_EPG_ENTRY: u32 = 4;
const ITERATOR_TYPE_NUMBERED_CHANNEL: u32 = 5;

/// Sample logging frequency (1 in SAMPLE_LOG_FREQUENCY messages will be logged)
const SAMPLE_LOG_FREQUENCY: u32 = 750;

/// Simple counter for probabilistic logging
static mut LOG_COUNTER: u32 = 0;

/// Log message with probability 1/SAMPLE_LOG_FREQUENCY
fn maybe_log(level: u32, message: &str) {
    unsafe {
        LOG_COUNTER = LOG_COUNTER.wrapping_add(1);
        if LOG_COUNTER % SAMPLE_LOG_FREQUENCY == 0 {
            let msg_bytes = message.as_bytes();
            host_log_write(level, msg_bytes.as_ptr() as u32, msg_bytes.len() as u32);
        }
    }
}

/// Create a compact string representation of a channel for logging
fn format_channel_summary(channel: &Channel) -> String {
    format!(
        "{{id: {}, name: '{}', group: '{}', tvg_id: '{}'}}",
        &channel.id.to_string()[..8],
        channel.channel_name,
        channel.group_title.as_deref().unwrap_or("None"),
        channel.tvg_id.as_deref().unwrap_or("None")
    )
}

/// Channel data structure (matches orchestrator)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Channel {
    pub id: Uuid,
    pub source_id: Uuid,
    pub tvg_id: Option<String>,
    pub tvg_name: Option<String>,
    pub tvg_logo: Option<String>,
    pub tvg_shift: Option<String>,
    pub group_title: Option<String>,
    pub channel_name: String,
    pub stream_url: String,
    pub created_at: String, // ISO timestamp
    pub updated_at: String, // ISO timestamp
}

/// Data mapping rule from orchestrator
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataMappingRule {
    pub rule_id: Uuid,
    pub source_field: String,
    pub target_field: String,
    pub transformation: String,
    pub priority: i32,
}

/// Filter rule from orchestrator
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilterRule {
    pub filter_id: Uuid,
    pub rule_type: String,
    pub condition: String,
    pub action: String,
    pub priority: i32,
}

/// EPG entry from orchestrator
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpgEntry {
    pub channel_id: String,
    pub program_id: String,
    pub title: String,
    pub description: Option<String>,
    pub start_time: String, // ISO timestamp
    pub end_time: String,   // ISO timestamp
    pub source_id: Uuid,
    pub priority: i32,
}

/// Numbered channel for channel numbering stage
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NumberedChannel {
    pub channel: Channel,
    pub assigned_number: i32,
    pub assignment_type: String,
}

/// Iterator result wrapper
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum IteratorResult<T> {
    Chunk(Vec<T>),
    Exhausted,
}

// Simple passthrough plugin - no state needed since all functions just pass data through

/// Initialize plugin with configuration
#[unsafe(no_mangle)]
pub extern "C" fn plugin_init(_config_ptr: *const u8, _config_len: usize) -> i32 {
    let message = "Pass-through plugin initialized";
    unsafe {
        host_log_write(LOG_INFO, message.as_ptr() as u32, message.len() as u32);
    }

    0 // Success
}

/// Get plugin information
#[unsafe(no_mangle)]
pub extern "C" fn plugin_get_info(out_ptr: *mut *mut u8, out_len: *mut usize) -> i32 {
    let info = serde_json::json!({
        "name": "passthrough-plugin",
        "version": "0.0.1",
        "description": "Reference implementation demonstrating complete orchestrator iterator integration across all pipeline stages",
        "author": "m3u-proxy developers",
        "license": "MIT",
        "supported_stages": [
            "data_mapping", 
            "filtering",
            "logo_prefetch",
            "channel_numbering",
            "m3u_generation",
            "epg_processing"
        ],
        "memory_efficiency": "low"
    });

    match serde_json::to_vec(&info) {
        Ok(data) => {
            // Allocate memory using Vec instead of libc::malloc for WASM compatibility
            let mut output_vec = data;
            let output_ptr = output_vec.as_mut_ptr();
            let output_len = output_vec.len();
            std::mem::forget(output_vec); // Transfer ownership to caller
            unsafe {
                *out_ptr = output_ptr;
                *out_len = output_len;
            }
            0 // Success
        },
        Err(_) => -1, // Serialization error
    }
}

/// Get plugin capabilities
#[unsafe(no_mangle)]
pub extern "C" fn plugin_get_capabilities(out_ptr: *mut *mut u8, out_len: *mut usize) -> i32 {
    let capabilities = serde_json::json!({
        "memory_efficiency": "low",
        "stage_types": [
            "data_mapping",
            "filtering", 
            "logo_prefetch",
            "channel_numbering",
            "m3u_generation",
            "epg_processing"
        ],
        "host_interface_version": "1.0"
    });

    match serde_json::to_vec(&capabilities) {
        Ok(data) => {
            // Allocate memory using Vec instead of libc::malloc for WASM compatibility
            let mut output_vec = data;
            let output_ptr = output_vec.as_mut_ptr();
            let output_len = output_vec.len();
            std::mem::forget(output_vec); // Transfer ownership to caller
            unsafe {
                *out_ptr = output_ptr;
                *out_len = output_len;
            }
            0
        },
        Err(_) => -1,
    }
}


/// Execute data mapping stage - complete implementation with output iterator
/// Returns an iterator ID that the host can use to pull processed data
#[unsafe(no_mangle)]
pub extern "C" fn plugin_execute_data_mapping(
    mapping_iterator_id: u32,
    channels_iterator_id: u32,
    result_iterator_id_ptr: *mut u32,  // Output: where to store the result iterator ID
) -> i32 {
    unsafe {
        // 1. Create output iterator
        let output_iterator_id = host_iterator_create(ITERATOR_TYPE_CHANNEL);
        if output_iterator_id == 0 {
            return -1; // Failed to create iterator
        }
        
        maybe_log(LOG_INFO, &format!("Data mapping: created output iterator {}", output_iterator_id));
        
        // 2. Process mapping rules (consume them)
        let mut chunk_buffer = vec![0u8; 1024 * 1024]; // 1MB buffer
        let mut rules_processed = 0;
        
        loop {
            let result = host_iterator_read_chunk(
                mapping_iterator_id,
                chunk_buffer.as_mut_ptr() as u32,
                chunk_buffer.len() as u32,
                50, // 50 rules at a time
            );
            
            if result <= 0 {
                break; // No more mapping rules
            }
            
            // In a real plugin, we'd parse and apply these rules
            // For passthrough, we just count them
            rules_processed += 1;
            maybe_log(LOG_DEBUG, &format!("Consumed mapping rules chunk {}", rules_processed));
        }
        
        // Close mapping rules iterator
        host_iterator_close(mapping_iterator_id);
        
        // 3. Process input channels and write to output iterator
        let mut total_processed = 0;
        
        loop {
            let result = host_iterator_read_chunk(
                channels_iterator_id,
                chunk_buffer.as_mut_ptr() as u32,
                chunk_buffer.len() as u32,
                100, // 100 channels at a time
            );
            
            if result <= 0 {
                break; // Iterator exhausted
            }
            
            // Deserialize input chunk
            let chunk_data = &chunk_buffer[..result as usize];
            let iterator_result = match serde_json::from_slice::<IteratorResult<Channel>>(chunk_data) {
                Ok(result) => result,
                Err(_) => return -1,
            };
            
            match iterator_result {
                IteratorResult::Chunk(channels) => {
                    let chunk_size = channels.len();
                    total_processed += chunk_size;
                    
                    // Apply data mapping (in passthrough mode, no changes)
                    let processed_channels = channels;
                    
                    // Log sample from this chunk
                    if !processed_channels.is_empty() {
                        maybe_log(LOG_DEBUG, &format!("Sample processed: {}", format_channel_summary(&processed_channels[0])));
                    }
                    
                    // Serialize processed chunk
                    let processed_data = match serde_json::to_vec(&processed_channels) {
                        Ok(data) => data,
                        Err(_) => return -1,
                    };
                    
                    // Push processed chunk to output iterator
                    let push_result = host_iterator_write_chunk(
                        output_iterator_id,
                        processed_data.as_ptr() as u32,
                        processed_data.len() as u32,
                        chunk_size as u32,
                    );
                    
                    if push_result != 0 {
                        return -1; // Failed to push chunk
                    }
                    
                    maybe_log(LOG_DEBUG, &format!("Pushed chunk of {} channels (total: {})", chunk_size, total_processed));
                },
                IteratorResult::Exhausted => {
                    break;
                }
            }
        }
        
        // Close input iterator
        host_iterator_close(channels_iterator_id);
        
        // 4. Finalize output iterator
        let finalize_result = host_iterator_finalize(output_iterator_id);
        if finalize_result != 0 {
            return -1; // Failed to finalize
        }
        
        maybe_log(LOG_INFO, &format!("Data mapping complete: {} channels processed, output iterator {}", 
                  total_processed, output_iterator_id));
        
        // 5. Return the output iterator ID
        *result_iterator_id_ptr = output_iterator_id;
        
        0 // Success
    }
}

/// Execute data mapping stage with chunked processing (advanced example)
/// This shows how a real plugin would process data chunk by chunk
#[unsafe(no_mangle)]
pub extern "C" fn plugin_execute_data_mapping_chunked(
    mapping_iterator_id: u32,
    channels_iterator_id: u32,
    result_iterator_id_ptr: *mut u32,
) -> i32 {
    unsafe {
        let mut total_processed = 0;
        let mut chunk_buffer = vec![0u8; 1024 * 1024]; // 1MB buffer for chunks
        let mut processed_data = Vec::new();
        
        // First, consume the mapping rules
        loop {
            let result = host_iterator_read_chunk(
                mapping_iterator_id,
                chunk_buffer.as_mut_ptr() as u32,
                chunk_buffer.len() as u32,
                50, // 50 rules at a time
            );
            
            if result <= 0 {
                break; // No more mapping rules
            }
            
            // In a real plugin, we'd parse and store the mapping rules
            // For passthrough, we just acknowledge we consumed them
            maybe_log(LOG_DEBUG, "Consumed mapping rules chunk");
        }
        
        // Close the mapping iterator
        host_iterator_close(mapping_iterator_id);
        
        // Now process channels chunk by chunk
        loop {
            let result = host_iterator_read_chunk(
                channels_iterator_id,
                chunk_buffer.as_mut_ptr() as u32,
                chunk_buffer.len() as u32,
                100, // 100 channels at a time
            );
            
            if result <= 0 {
                break; // Iterator exhausted
            }
            
            // Deserialize chunk
            let chunk_data = &chunk_buffer[..result as usize];
            let iterator_result = match serde_json::from_slice::<IteratorResult<Channel>>(chunk_data) {
                Ok(result) => result,
                Err(_) => return -1,
            };
            
            match iterator_result {
                IteratorResult::Chunk(channels) => {
                    total_processed += channels.len();
                    
                    // Process channels (in passthrough mode, just copy)
                    for channel in channels {
                        // Apply mapping rules here in a real plugin
                        processed_data.push(channel);
                    }
                    
                    maybe_log(LOG_DEBUG, &format!("Processed chunk, total: {}", total_processed));
                },
                IteratorResult::Exhausted => {
                    break;
                }
            }
        }
        
        // Close input iterator
        host_iterator_close(channels_iterator_id);
        
        maybe_log(LOG_INFO, &format!("Data mapping complete: {} channels processed", total_processed));
        
        // In a real implementation, we'd create a new iterator with processed_data
        // and return its ID. For now, we'll return a placeholder ID.
        *result_iterator_id_ptr = 999; // Placeholder iterator ID
        
        0 // Success
    }
}

/// Execute filtering stage - chunked iterator version
#[unsafe(no_mangle)]
pub extern "C" fn plugin_execute_filtering(
    filter_iterator_id: u32,
    channels_iterator_id: u32,
    result_iterator_id_ptr: *mut u32,
) -> i32 {
    unsafe {
        maybe_log(LOG_INFO, "Filtering: passthrough mode - returning input iterator");
        
        // Passthrough: return the input iterator as result
        *result_iterator_id_ptr = channels_iterator_id;
        
        // Close the filter rules iterator
        host_iterator_close(filter_iterator_id);
        
        0 // Success
    }
}

/// Execute logo pre-fetch stage - pure passthrough
#[unsafe(no_mangle)]
pub extern "C" fn plugin_execute_logo_prefetch(
    channels_ptr: *const u8,
    channels_len: usize,
    out_ptr: *mut *mut u8,
    out_len: *mut usize,
) -> i32 {
    unsafe {
        // Deserialize input channels
        let channels_data = std::slice::from_raw_parts(channels_ptr, channels_len);
        let channels = match serde_json::from_slice::<Vec<Channel>>(channels_data) {
            Ok(channels) => channels,
            Err(_) => return -1,
        };

        // Log based on probability
        maybe_log(LOG_INFO, &format!("Logo prefetch: processing {} channels", channels.len()));
        
        // Pure passthrough - copy input to output
        let output = channels;
        
        // Log a sample channel
        if !output.is_empty() {
            maybe_log(LOG_DEBUG, &format!("Sample logo channel: {}", format_channel_summary(&output[0])));
        }

        // Serialize output
        let output_data = match serde_json::to_vec(&output) {
            Ok(data) => data,
            Err(_) => return -1,
        };

        // Allocate output memory
        let mut output_vec = output_data;
        let output_ptr = output_vec.as_mut_ptr();
        let output_len_val = output_vec.len();
        std::mem::forget(output_vec); // Transfer ownership to caller

        *out_ptr = output_ptr;
        *out_len = output_len_val;

        0 // Success
    }
}

/// Execute channel numbering stage - pure passthrough
#[unsafe(no_mangle)]
pub extern "C" fn plugin_execute_channel_numbering(
    channels_ptr: *const u8,
    channels_len: usize,
    out_ptr: *mut *mut u8,
    out_len: *mut usize,
) -> i32 {
    unsafe {
        // Deserialize input channels
        let channels_data = std::slice::from_raw_parts(channels_ptr, channels_len);
        let channels = match serde_json::from_slice::<Vec<Channel>>(channels_data) {
            Ok(channels) => channels,
            Err(_) => return -1,
        };

        // Log based on probability
        maybe_log(LOG_INFO, &format!("Channel numbering: processing {} channels", channels.len()));
        
        // Pure passthrough - copy input to output
        let output = channels;
        
        // Log a sample channel
        if !output.is_empty() {
            maybe_log(LOG_DEBUG, &format!("Sample numbered channel: {}", format_channel_summary(&output[0])));
        }

        // Serialize output
        let output_data = match serde_json::to_vec(&output) {
            Ok(data) => data,
            Err(_) => return -1,
        };

        // Allocate output memory
        let mut output_vec = output_data;
        let output_ptr = output_vec.as_mut_ptr();
        let output_len_val = output_vec.len();
        std::mem::forget(output_vec); // Transfer ownership to caller

        *out_ptr = output_ptr;
        *out_len = output_len_val;

        0 // Success
    }
}

/// Execute M3U generation stage - pure passthrough
#[unsafe(no_mangle)]
pub extern "C" fn plugin_execute_m3u_generation(
    numbered_channels_ptr: *const u8,
    numbered_channels_len: usize,
    out_ptr: *mut *mut u8,
    out_len: *mut usize,
) -> i32 {
    unsafe {
        // Deserialize input channels
        let channels_data = std::slice::from_raw_parts(numbered_channels_ptr, numbered_channels_len);
        let channels = match serde_json::from_slice::<Vec<NumberedChannel>>(channels_data) {
            Ok(channels) => channels,
            Err(_) => return -1,
        };

        // Log based on probability
        maybe_log(LOG_INFO, &format!("M3U generation: processing {} channels", channels.len()));
        
        // Pure passthrough - copy input to output
        let output = channels;
        
        // Log a sample channel
        if !output.is_empty() {
            maybe_log(LOG_DEBUG, &format!("Sample M3U channel: {}", format_channel_summary(&output[0].channel)));
        }

        // Serialize output
        let output_data = match serde_json::to_vec(&output) {
            Ok(data) => data,
            Err(_) => return -1,
        };

        // Allocate output memory
        let mut output_vec = output_data;
        let output_ptr = output_vec.as_mut_ptr();
        let output_len_val = output_vec.len();
        std::mem::forget(output_vec); // Transfer ownership to caller

        *out_ptr = output_ptr;
        *out_len = output_len_val;

        0 // Success
    }
}

/// Execute EPG processing stage - pure passthrough
#[unsafe(no_mangle)]
pub extern "C" fn plugin_execute_epg_processing(
    epg_iterator_id: u32,
    final_channel_map_ptr: *const u8,
    final_channel_map_len: usize,
    out_ptr: *mut *mut u8,
    out_len: *mut usize,
) -> i32 {
    unsafe {
        // Deserialize final channel map
        let channel_map_data = std::slice::from_raw_parts(final_channel_map_ptr, final_channel_map_len);
        let channels = match serde_json::from_slice::<Vec<NumberedChannel>>(channel_map_data) {
            Ok(channels) => channels,
            Err(_) => return -1,
        };

        // Log based on probability
        maybe_log(LOG_INFO, &format!("EPG processing: processing {} channels", channels.len()));
        
        // Pure passthrough - copy input to output
        let output = channels;
        
        // Log a sample channel
        if !output.is_empty() {
            maybe_log(LOG_DEBUG, &format!("Sample EPG channel: {}", format_channel_summary(&output[0].channel)));
        }

        // Serialize output
        let output_data = match serde_json::to_vec(&output) {
            Ok(data) => data,
            Err(_) => return -1,
        };

        // Allocate output memory
        let mut output_vec = output_data;
        let output_ptr = output_vec.as_mut_ptr();
        let output_len_val = output_vec.len();
        std::mem::forget(output_vec); // Transfer ownership to caller

        *out_ptr = output_ptr;
        *out_len = output_len_val;

        0 // Success
    }
}


/// Cleanup plugin resources
#[unsafe(no_mangle)]
pub extern "C" fn plugin_cleanup() -> i32 {
    let message = "Pass-through plugin cleaned up";
    unsafe {
        host_log_write(LOG_INFO, message.as_ptr() as u32, message.len() as u32);
    }
    0
}

/// Get plugin statistics
#[unsafe(no_mangle)]
pub extern "C" fn plugin_get_stats(out_ptr: *mut *mut u8, out_len: *mut usize) -> i32 {
    let stats = serde_json::json!({
        "stage": "passthrough",
        "processed_items": 0,
        "plugin_type": "passthrough"
    });

    match serde_json::to_vec(&stats) {
        Ok(data) => {
            // Allocate memory using Vec instead of libc::malloc for WASM compatibility
            let mut output_vec = data;
            let output_ptr = output_vec.as_mut_ptr();
            let output_len = output_vec.len();
            std::mem::forget(output_vec); // Transfer ownership to caller
            unsafe {
                *out_ptr = output_ptr;
                *out_len = output_len;
            }
            0
        }
        Err(_) => -1,
    }
}
