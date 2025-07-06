//! Pass-through WASM Plugin - Reference Implementation
//!
//! This plugin serves as the reference implementation for the WASM plugin system.
//! It demonstrates how to properly consume data from orchestrator iterators and
//! perform clean passthrough processing for all pipeline stages.
//!
//! Pipeline Stages:
//! 1. Source Loading: Consumes OrderedChannelAggregateIterator → outputs channels
//! 2. Data Mapping: Consumes OrderedDataMappingIterator + channels → outputs mapped channels  
//! 3. Filtering: Consumes OrderedFilterIterator + channels → outputs filtered channels
//! 4. Logo Pre-fetch: Processes channels for logo caching → outputs channels with cached logo URLs
//! 5. Channel Numbering: Assigns sequential numbers → outputs numbered channels
//! 6. M3U Generation: Converts numbered channels → outputs M3U content
//! 7. EPG Processing: Consumes OrderedEpgAggregateIterator + final channel map → outputs XMLTV

use serde::{Deserialize, Serialize};
use uuid::Uuid;

// Host interface function imports
unsafe extern "C" {
    fn host_log(level: u32, msg_ptr: *const u8, msg_len: usize);
    fn host_get_memory_usage() -> u64;
    fn host_get_memory_pressure() -> u32;
    fn host_report_progress(stage_ptr: *const u8, stage_len: usize, processed: usize, total: usize);
    fn host_cache_logo(url_ptr: *const u8, url_len: usize, uuid_out_ptr: *mut *mut u8, uuid_out_len: *mut usize) -> i32;
    fn host_iterator_next_chunk(iterator_id: u32, out_ptr: *mut *mut u8, out_len: *mut usize) -> i32;
    fn host_iterator_close(iterator_id: u32) -> i32;
}

/// Log levels that match the host interface
const LOG_ERROR: u32 = 1;
const _LOG_WARN: u32 = 2;
const LOG_INFO: u32 = 3;
const LOG_DEBUG: u32 = 4;

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

/// Plugin state
pub struct PassthroughPlugin {
    stage: String,
    processed_items: usize,
    _start_time: u64, // Simple timestamp
}

impl PassthroughPlugin {
    pub fn new() -> Self {
        Self {
            stage: "uninitialized".to_string(),
            processed_items: 0,
            _start_time: 0,
        }
    }

    /// Log a message to the host
    fn log(&self, level: u32, message: &str) {
        unsafe {
            host_log(level, message.as_ptr(), message.len());
        }
    }

    /// Report progress to the host
    fn report_progress(&self, processed: usize, total: usize) {
        unsafe {
            host_report_progress(self.stage.as_ptr(), self.stage.len(), processed, total);
        }
    }

    /// Get memory usage from host
    fn get_memory_usage(&self) -> u64 {
        unsafe { host_get_memory_usage() }
    }

    /// Get memory pressure from host
    fn get_memory_pressure(&self) -> u32 {
        unsafe { host_get_memory_pressure() }
    }
}

// Global plugin state
static mut PLUGIN_STATE: Option<PassthroughPlugin> = None;

/// Initialize plugin with configuration
#[unsafe(no_mangle)]
pub extern "C" fn plugin_init(_config_ptr: *const u8, _config_len: usize) -> i32 {
    unsafe {
        PLUGIN_STATE = Some(PassthroughPlugin::new());
    }

    let message = "Pass-through plugin initialized";
    unsafe {
        host_log(LOG_INFO, message.as_ptr(), message.len());
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
            "source_loading",
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
        Ok(data) => unsafe {
            let output_ptr = libc::malloc(data.len()) as *mut u8;
            if output_ptr.is_null() {
                return -2; // Memory allocation failed
            }
            std::ptr::copy_nonoverlapping(data.as_ptr(), output_ptr, data.len());
            *out_ptr = output_ptr;
            *out_len = data.len();
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
            "source_loading",
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
        Err(_) => -1,
    }
}

/// Execute source loading stage - consume from OrderedChannelAggregateIterator
#[unsafe(no_mangle)]
pub extern "C" fn plugin_execute_source_loading(
    iterator_id: u32,
    out_ptr: *mut *mut u8,
    out_len: *mut usize,
) -> i32 {
    unsafe {
        if let Some(ref mut state) = PLUGIN_STATE {
            state.stage = "source_loading".to_string();
            state.log(LOG_INFO, "Executing source loading stage with orchestrator iterator");

            let mut all_channels = Vec::new();
            let mut total_chunks = 0;

            // Consume all chunks from the orchestrator iterator
            loop {
                let mut chunk_ptr: *mut u8 = std::ptr::null_mut();
                let mut chunk_len: usize = 0;
                
                let result = host_iterator_next_chunk(iterator_id, &mut chunk_ptr, &mut chunk_len);
                
                if result != 0 {
                    state.log(LOG_ERROR, "Failed to get next chunk from iterator");
                    return -1;
                }
                
                if chunk_ptr.is_null() || chunk_len == 0 {
                    state.log(LOG_DEBUG, "Iterator exhausted");
                    break;
                }
                
                // Deserialize chunk
                let chunk_data = std::slice::from_raw_parts(chunk_ptr, chunk_len);
                match serde_json::from_slice::<IteratorResult<Channel>>(chunk_data) {
                    Ok(IteratorResult::Chunk(channels)) => {
                        state.log(LOG_DEBUG, &format!("Received chunk with {} channels", channels.len()));
                        all_channels.extend(channels);
                        total_chunks += 1;
                    }
                    Ok(IteratorResult::Exhausted) => {
                        state.log(LOG_DEBUG, "Iterator reports exhausted");
                        break;
                    }
                    Err(_) => {
                        state.log(LOG_ERROR, "Failed to deserialize iterator chunk");
                        return -1;
                    }
                }
                
                // Free the chunk memory
                libc::free(chunk_ptr as *mut _);
            }

            state.processed_items = all_channels.len();
            state.log(
                LOG_INFO,
                &format!("Source loading completed: {} channels from {} chunks", 
                        all_channels.len(), total_chunks),
            );

            // Passthrough: return channels unchanged
            match serde_json::to_vec(&all_channels) {
                Ok(output) => {
                    let output_ptr = libc::malloc(output.len()) as *mut u8;
                    if output_ptr.is_null() {
                        return -2;
                    }
                    std::ptr::copy_nonoverlapping(output.as_ptr(), output_ptr, output.len());
                    *out_ptr = output_ptr;
                    *out_len = output.len();
                    0
                }
                Err(_) => {
                    state.log(LOG_ERROR, "Failed to serialize channels");
                    -1
                }
            }
        } else {
            -1 // Plugin not initialized
        }
    }
}

/// Execute data mapping stage - consume mapping rules and apply to channels
#[unsafe(no_mangle)]
pub extern "C" fn plugin_execute_data_mapping(
    mapping_iterator_id: u32,
    channels_ptr: *const u8,
    channels_len: usize,
    out_ptr: *mut *mut u8,
    out_len: *mut usize,
) -> i32 {
    unsafe {
        if let Some(ref mut state) = PLUGIN_STATE {
            state.stage = "data_mapping".to_string();
            state.log(LOG_INFO, "Executing data mapping stage with orchestrator iterator");

            // Deserialize input channels
            let channels_data = std::slice::from_raw_parts(channels_ptr, channels_len);
            let channels = match serde_json::from_slice::<Vec<Channel>>(channels_data) {
                Ok(channels) => channels,
                Err(_) => {
                    state.log(LOG_ERROR, "Failed to deserialize input channels");
                    return -1;
                }
            };

            let mut mapping_rules = Vec::new();
            let mut total_chunks = 0;

            // Consume all mapping rules from the orchestrator iterator
            loop {
                let mut chunk_ptr: *mut u8 = std::ptr::null_mut();
                let mut chunk_len: usize = 0;
                
                let result = host_iterator_next_chunk(mapping_iterator_id, &mut chunk_ptr, &mut chunk_len);
                
                if result != 0 {
                    state.log(LOG_ERROR, "Failed to get next chunk from mapping iterator");
                    return -1;
                }
                
                if chunk_ptr.is_null() || chunk_len == 0 {
                    state.log(LOG_DEBUG, "Mapping iterator exhausted");
                    break;
                }
                
                // Deserialize chunk
                let chunk_data = std::slice::from_raw_parts(chunk_ptr, chunk_len);
                match serde_json::from_slice::<IteratorResult<DataMappingRule>>(chunk_data) {
                    Ok(IteratorResult::Chunk(rules)) => {
                        state.log(LOG_DEBUG, &format!("Received chunk with {} mapping rules", rules.len()));
                        mapping_rules.extend(rules);
                        total_chunks += 1;
                    }
                    Ok(IteratorResult::Exhausted) => {
                        state.log(LOG_DEBUG, "Mapping iterator reports exhausted");
                        break;
                    }
                    Err(_) => {
                        state.log(LOG_ERROR, "Failed to deserialize mapping iterator chunk");
                        return -1;
                    }
                }
                
                // Free the chunk memory
                libc::free(chunk_ptr as *mut _);
            }

            state.log(
                LOG_INFO,
                &format!("Data mapping stage: {} channels, {} mapping rules from {} chunks", 
                        channels.len(), mapping_rules.len(), total_chunks),
            );

            // Passthrough: In a real implementation, we would apply the mapping rules
            // For passthrough, we just log the rules and return channels unchanged
            for rule in &mapping_rules {
                state.log(LOG_DEBUG, &format!("Mapping rule: {} -> {} ({})", 
                         rule.source_field, rule.target_field, rule.transformation));
            }

            state.processed_items = channels.len();

            // Serialize result
            match serde_json::to_vec(&channels) {
                Ok(output) => {
                    let output_ptr = libc::malloc(output.len()) as *mut u8;
                    if output_ptr.is_null() {
                        return -2;
                    }
                    std::ptr::copy_nonoverlapping(output.as_ptr(), output_ptr, output.len());
                    *out_ptr = output_ptr;
                    *out_len = output.len();
                    0
                }
                Err(_) => {
                    state.log(LOG_ERROR, "Failed to serialize mapped channels");
                    -1
                }
            }
        } else {
            -1 // Plugin not initialized
        }
    }
}

/// Execute filtering stage - consume filter rules and apply to channels
#[unsafe(no_mangle)]
pub extern "C" fn plugin_execute_filtering(
    filter_iterator_id: u32,
    channels_ptr: *const u8,
    channels_len: usize,
    out_ptr: *mut *mut u8,
    out_len: *mut usize,
) -> i32 {
    unsafe {
        if let Some(ref mut state) = PLUGIN_STATE {
            state.stage = "filtering".to_string();
            state.log(LOG_INFO, "Executing filtering stage with orchestrator iterator");

            // Deserialize input channels
            let channels_data = std::slice::from_raw_parts(channels_ptr, channels_len);
            let channels = match serde_json::from_slice::<Vec<Channel>>(channels_data) {
                Ok(channels) => channels,
                Err(_) => {
                    state.log(LOG_ERROR, "Failed to deserialize input channels");
                    return -1;
                }
            };

            let mut filter_rules = Vec::new();
            let mut total_chunks = 0;

            // Consume all filter rules from the orchestrator iterator
            loop {
                let mut chunk_ptr: *mut u8 = std::ptr::null_mut();
                let mut chunk_len: usize = 0;
                
                let result = host_iterator_next_chunk(filter_iterator_id, &mut chunk_ptr, &mut chunk_len);
                
                if result != 0 {
                    state.log(LOG_ERROR, "Failed to get next chunk from filter iterator");
                    return -1;
                }
                
                if chunk_ptr.is_null() || chunk_len == 0 {
                    state.log(LOG_DEBUG, "Filter iterator exhausted");
                    break;
                }
                
                // Deserialize chunk
                let chunk_data = std::slice::from_raw_parts(chunk_ptr, chunk_len);
                match serde_json::from_slice::<IteratorResult<FilterRule>>(chunk_data) {
                    Ok(IteratorResult::Chunk(rules)) => {
                        state.log(LOG_DEBUG, &format!("Received chunk with {} filter rules", rules.len()));
                        filter_rules.extend(rules);
                        total_chunks += 1;
                    }
                    Ok(IteratorResult::Exhausted) => {
                        state.log(LOG_DEBUG, "Filter iterator reports exhausted");
                        break;
                    }
                    Err(_) => {
                        state.log(LOG_ERROR, "Failed to deserialize filter iterator chunk");
                        return -1;
                    }
                }
                
                // Free the chunk memory
                libc::free(chunk_ptr as *mut _);
            }

            state.log(
                LOG_INFO,
                &format!("Filtering stage: {} channels, {} filter rules from {} chunks", 
                        channels.len(), filter_rules.len(), total_chunks),
            );

            // Passthrough: In a real implementation, we would apply the filter rules
            // For passthrough, we just log the rules and return channels unchanged
            for rule in &filter_rules {
                state.log(LOG_DEBUG, &format!("Filter rule: {} {} ({})", 
                         rule.rule_type, rule.condition, rule.action));
            }

            state.processed_items = channels.len();

            // Serialize result
            match serde_json::to_vec(&channels) {
                Ok(output) => {
                    let output_ptr = libc::malloc(output.len()) as *mut u8;
                    if output_ptr.is_null() {
                        return -2;
                    }
                    std::ptr::copy_nonoverlapping(output.as_ptr(), output_ptr, output.len());
                    *out_ptr = output_ptr;
                    *out_len = output.len();
                    0
                }
                Err(_) => {
                    state.log(LOG_ERROR, "Failed to serialize filtered channels");
                    -1
                }
            }
        } else {
            -1 // Plugin not initialized
        }
    }
}

/// Execute logo pre-fetch stage - cache logos and update channel URLs
#[unsafe(no_mangle)]
pub extern "C" fn plugin_execute_logo_prefetch(
    channels_ptr: *const u8,
    channels_len: usize,
    out_ptr: *mut *mut u8,
    out_len: *mut usize,
) -> i32 {
    unsafe {
        if let Some(ref mut state) = PLUGIN_STATE {
            state.stage = "logo_prefetch".to_string();
            state.log(LOG_INFO, "Executing logo pre-fetch stage");

            // Deserialize input channels
            let channels_data = std::slice::from_raw_parts(channels_ptr, channels_len);
            let mut channels = match serde_json::from_slice::<Vec<Channel>>(channels_data) {
                Ok(channels) => channels,
                Err(_) => {
                    state.log(LOG_ERROR, "Failed to deserialize input channels");
                    return -1;
                }
            };

            let mut logos_cached = 0;

            // Process each channel for logo caching
            for channel in &mut channels {
                if let Some(logo_url) = channel.tvg_logo.clone() {
                    if !logo_url.is_empty() {
                        // Cache the logo and get UUID
                        let mut uuid_ptr: *mut u8 = std::ptr::null_mut();
                        let mut uuid_len: usize = 0;
                        
                        let result = host_cache_logo(
                            logo_url.as_ptr(),
                            logo_url.len(),
                            &mut uuid_ptr,
                            &mut uuid_len
                        );
                        
                        if result == 0 && !uuid_ptr.is_null() && uuid_len > 0 {
                            // Get the cached logo serving URL (format: "URL|UUID")
                            let response_data = std::slice::from_raw_parts(uuid_ptr, uuid_len);
                            if let Ok(response_str) = std::str::from_utf8(response_data) {
                                // Parse the response format: "serving_url|uuid"
                                let parts: Vec<&str> = response_str.split('|').collect();
                                if parts.len() == 2 {
                                    let serving_url = parts[0];
                                    let cached_uuid = parts[1];
                                    
                                    // Update channel logo URL to use serving URL
                                    channel.tvg_logo = Some(serving_url.to_string());
                                    logos_cached += 1;
                                    state.log(LOG_DEBUG, &format!("Cached logo for channel {}: {} -> {} (UUID: {})", 
                                             channel.channel_name, logo_url, serving_url, cached_uuid));
                                } else {
                                    state.log(LOG_DEBUG, &format!("Invalid response format from host_cache_logo: {}", response_str));
                                }
                            }
                            
                            // Free the UUID memory
                            libc::free(uuid_ptr as *mut _);
                        } else {
                            state.log(LOG_DEBUG, &format!("Failed to cache logo for channel {}: {}", 
                                     channel.channel_name, logo_url));
                        }
                    }
                }
            }

            state.log(
                LOG_INFO,
                &format!("Logo pre-fetch completed: {} channels, {} logos cached", 
                        channels.len(), logos_cached),
            );

            state.processed_items = channels.len();

            // Serialize result
            match serde_json::to_vec(&channels) {
                Ok(output) => {
                    let output_ptr = libc::malloc(output.len()) as *mut u8;
                    if output_ptr.is_null() {
                        return -2;
                    }
                    std::ptr::copy_nonoverlapping(output.as_ptr(), output_ptr, output.len());
                    *out_ptr = output_ptr;
                    *out_len = output.len();
                    0
                }
                Err(_) => {
                    state.log(LOG_ERROR, "Failed to serialize channels with cached logos");
                    -1
                }
            }
        } else {
            -1 // Plugin not initialized
        }
    }
}

/// Execute channel numbering stage - convert channels to numbered channels
#[unsafe(no_mangle)]
pub extern "C" fn plugin_execute_channel_numbering(
    channels_ptr: *const u8,
    channels_len: usize,
    out_ptr: *mut *mut u8,
    out_len: *mut usize,
) -> i32 {
    unsafe {
        if let Some(ref mut state) = PLUGIN_STATE {
            state.stage = "channel_numbering".to_string();
            state.log(LOG_INFO, "Executing channel numbering stage");

            let channels_data = std::slice::from_raw_parts(channels_ptr, channels_len);
            match serde_json::from_slice::<Vec<Channel>>(channels_data) {
                Ok(channels) => {
                    let channel_count = channels.len();
                    let numbered_channels: Vec<NumberedChannel> = channels
                        .into_iter()
                        .enumerate()
                        .map(|(i, channel)| {
                            state.report_progress(i + 1, channel_count);
                            NumberedChannel {
                                channel,
                                assigned_number: i as i32 + 1,
                                assignment_type: "sequential".to_string(),
                            }
                        })
                        .collect();

                    state.log(
                        LOG_INFO,
                        &format!(
                            "Channel numbering completed: {} channels",
                            numbered_channels.len()
                        ),
                    );

                    match serde_json::to_vec(&numbered_channels) {
                        Ok(output) => {
                            let output_ptr = libc::malloc(output.len()) as *mut u8;
                            if output_ptr.is_null() {
                                return -2;
                            }
                            std::ptr::copy_nonoverlapping(
                                output.as_ptr(),
                                output_ptr,
                                output.len(),
                            );
                            *out_ptr = output_ptr;
                            *out_len = output.len();
                            0
                        }
                        Err(_) => {
                            state.log(LOG_ERROR, "Failed to serialize numbered channels");
                            -1
                        }
                    }
                }
                Err(_) => {
                    state.log(LOG_ERROR, "Failed to deserialize channels");
                    -1
                }
            }
        } else {
            -1
        }
    }
}

/// Execute M3U generation stage - convert numbered channels to M3U content
#[unsafe(no_mangle)]
pub extern "C" fn plugin_execute_m3u_generation(
    numbered_channels_ptr: *const u8,
    numbered_channels_len: usize,
    out_ptr: *mut *mut u8,
    out_len: *mut usize,
) -> i32 {
    unsafe {
        if let Some(ref mut state) = PLUGIN_STATE {
            state.stage = "m3u_generation".to_string();
            state.log(LOG_INFO, "Executing M3U generation stage");

            let channels_data =
                std::slice::from_raw_parts(numbered_channels_ptr, numbered_channels_len);
            match serde_json::from_slice::<Vec<NumberedChannel>>(channels_data) {
                Ok(numbered_channels) => {
                    let mut m3u_content = String::from("#EXTM3U\n");

                    for (i, numbered_channel) in numbered_channels.iter().enumerate() {
                        state.report_progress(i + 1, numbered_channels.len());

                        m3u_content.push_str(&format!(
                            "#EXTINF:-1 tvg-id=\"{}\" tvg-name=\"{}\" tvg-logo=\"{}\" group-title=\"{}\",{}\n{}\n",
                            numbered_channel.channel.tvg_id.as_deref().unwrap_or(""),
                            numbered_channel.channel.tvg_name.as_deref().unwrap_or(""),
                            numbered_channel.channel.tvg_logo.as_deref().unwrap_or(""),
                            numbered_channel.channel.group_title.as_deref().unwrap_or(""),
                            numbered_channel.channel.channel_name,
                            numbered_channel.channel.stream_url
                        ));
                    }

                    state.log(
                        LOG_INFO,
                        &format!(
                            "M3U generation completed: {} lines",
                            m3u_content.lines().count()
                        ),
                    );

                    let output = m3u_content.into_bytes();
                    let output_ptr = libc::malloc(output.len()) as *mut u8;
                    if output_ptr.is_null() {
                        return -2;
                    }
                    std::ptr::copy_nonoverlapping(output.as_ptr(), output_ptr, output.len());
                    *out_ptr = output_ptr;
                    *out_len = output.len();
                    0
                }
                Err(_) => {
                    state.log(LOG_ERROR, "Failed to deserialize numbered channels");
                    -1
                }
            }
        } else {
            -1
        }
    }
}

/// Execute EPG processing stage - consume EPG data and filter by final channel map
#[unsafe(no_mangle)]
pub extern "C" fn plugin_execute_epg_processing(
    epg_iterator_id: u32,
    final_channel_map_ptr: *const u8,
    final_channel_map_len: usize,
    out_ptr: *mut *mut u8,
    out_len: *mut usize,
) -> i32 {
    unsafe {
        if let Some(ref mut state) = PLUGIN_STATE {
            state.stage = "epg_processing".to_string();
            state.log(LOG_INFO, "Executing EPG processing stage with orchestrator iterator");

            // Deserialize final channel map
            let channel_map_data = std::slice::from_raw_parts(final_channel_map_ptr, final_channel_map_len);
            let final_channels = match serde_json::from_slice::<Vec<NumberedChannel>>(channel_map_data) {
                Ok(channels) => channels,
                Err(_) => {
                    state.log(LOG_ERROR, "Failed to deserialize final channel map");
                    return -1;
                }
            };

            // Create channel ID lookup set for filtering EPG data
            let mut channel_ids = std::collections::HashSet::new();
            for numbered_channel in &final_channels {
                if let Some(ref tvg_id) = numbered_channel.channel.tvg_id {
                    channel_ids.insert(tvg_id.clone());
                }
            }

            let mut all_epg_entries = Vec::new();
            let mut total_chunks = 0;
            let mut filtered_entries = 0;

            // Consume all EPG data from the orchestrator iterator
            loop {
                let mut chunk_ptr: *mut u8 = std::ptr::null_mut();
                let mut chunk_len: usize = 0;
                
                let result = host_iterator_next_chunk(epg_iterator_id, &mut chunk_ptr, &mut chunk_len);
                
                if result != 0 {
                    state.log(LOG_ERROR, "Failed to get next chunk from EPG iterator");
                    return -1;
                }
                
                if chunk_ptr.is_null() || chunk_len == 0 {
                    state.log(LOG_DEBUG, "EPG iterator exhausted");
                    break;
                }
                
                // Deserialize chunk
                let chunk_data = std::slice::from_raw_parts(chunk_ptr, chunk_len);
                match serde_json::from_slice::<IteratorResult<EpgEntry>>(chunk_data) {
                    Ok(IteratorResult::Chunk(epg_entries)) => {
                        state.log(LOG_DEBUG, &format!("Received chunk with {} EPG entries", epg_entries.len()));
                        
                        // Filter EPG entries to only include channels in final channel map
                        for entry in epg_entries {
                            if channel_ids.contains(&entry.channel_id) {
                                all_epg_entries.push(entry);
                                filtered_entries += 1;
                            }
                        }
                        total_chunks += 1;
                    }
                    Ok(IteratorResult::Exhausted) => {
                        state.log(LOG_DEBUG, "EPG iterator reports exhausted");
                        break;
                    }
                    Err(_) => {
                        state.log(LOG_ERROR, "Failed to deserialize EPG iterator chunk");
                        return -1;
                    }
                }
                
                // Free the chunk memory
                libc::free(chunk_ptr as *mut _);
            }

            state.log(
                LOG_INFO,
                &format!("EPG processing: {} final channels, {} filtered EPG entries from {} chunks", 
                        final_channels.len(), filtered_entries, total_chunks),
            );

            // Generate XMLTV content
            let mut xmltv_content = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
            xmltv_content.push_str("<!DOCTYPE tv SYSTEM \"xmltv.dtd\">\n");
            xmltv_content.push_str("<tv generator-info-name=\"passthrough-plugin\">\n");

            // Add channel definitions
            for numbered_channel in &final_channels {
                if let Some(ref tvg_id) = numbered_channel.channel.tvg_id {
                    xmltv_content.push_str(&format!(
                        "  <channel id=\"{}\">\n    <display-name>{}</display-name>\n  </channel>\n",
                        tvg_id, numbered_channel.channel.channel_name
                    ));
                }
            }

            // Add programme entries (simplified for passthrough)
            for entry in &all_epg_entries {
                xmltv_content.push_str(&format!(
                    "  <programme start=\"{}\" stop=\"{}\" channel=\"{}\">\n    <title>{}</title>\n",
                    entry.start_time, entry.end_time, entry.channel_id, entry.title
                ));
                if let Some(ref desc) = entry.description {
                    xmltv_content.push_str(&format!("    <desc>{}</desc>\n", desc));
                }
                xmltv_content.push_str("  </programme>\n");
            }

            xmltv_content.push_str("</tv>\n");

            state.log(
                LOG_INFO,
                &format!(
                    "EPG processing completed: {} channels, {} programmes, {} lines",
                    final_channels.len(),
                    all_epg_entries.len(),
                    xmltv_content.lines().count()
                ),
            );

            state.processed_items = all_epg_entries.len();

            // Serialize result
            let output = xmltv_content.into_bytes();
            let output_ptr = libc::malloc(output.len()) as *mut u8;
            if output_ptr.is_null() {
                return -2;
            }
            std::ptr::copy_nonoverlapping(output.as_ptr(), output_ptr, output.len());
            *out_ptr = output_ptr;
            *out_len = output.len();
            0
        } else {
            -1 // Plugin not initialized
        }
    }
}

/// Helper function for pass-through stages that don't modify data
fn execute_passthrough_stage(
    stage_name: &str,
    input_ptr: *const u8,
    input_len: usize,
    out_ptr: *mut *mut u8,
    out_len: *mut usize,
) -> i32 {
    unsafe {
        if let Some(ref mut state) = PLUGIN_STATE {
            state.stage = stage_name.to_string();
            state.log(
                LOG_INFO,
                &format!("Executing {} stage (pass-through)", stage_name),
            );

            // Just copy input to output
            let input_data = std::slice::from_raw_parts(input_ptr, input_len);
            let output_ptr_raw = libc::malloc(input_len) as *mut u8;
            if output_ptr_raw.is_null() {
                return -2;
            }

            std::ptr::copy_nonoverlapping(input_data.as_ptr(), output_ptr_raw, input_len);
            *out_ptr = output_ptr_raw;
            *out_len = input_len;

            state.log(
                LOG_INFO,
                &format!("{} stage completed (pass-through)", stage_name),
            );
            0
        } else {
            -1
        }
    }
}

/// Cleanup plugin resources
#[unsafe(no_mangle)]
pub extern "C" fn plugin_cleanup() -> i32 {
    unsafe {
        if let Some(ref state) = PLUGIN_STATE {
            state.log(LOG_INFO, "Pass-through plugin cleaned up");
        }
        PLUGIN_STATE = None;
    }
    0
}

/// Get plugin statistics
#[unsafe(no_mangle)]
pub extern "C" fn plugin_get_stats(out_ptr: *mut *mut u8, out_len: *mut usize) -> i32 {
    unsafe {
        if let Some(ref state) = PLUGIN_STATE {
            let stats = serde_json::json!({
                "stage": state.stage,
                "processed_items": state.processed_items,
                "memory_usage": state.get_memory_usage(),
                "memory_pressure": state.get_memory_pressure(),
                "plugin_type": "passthrough"
            });

            match serde_json::to_vec(&stats) {
                Ok(data) => {
                    let output_ptr = libc::malloc(data.len()) as *mut u8;
                    if output_ptr.is_null() {
                        return -2;
                    }
                    std::ptr::copy_nonoverlapping(data.as_ptr(), output_ptr, data.len());
                    *out_ptr = output_ptr;
                    *out_len = data.len();
                    0
                }
                Err(_) => -1,
            }
        } else {
            -1
        }
    }
}
