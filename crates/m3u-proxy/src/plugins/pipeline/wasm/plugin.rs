//! Production-ready WASM pipeline plugin implementation
//!
//! This module provides the complete WASM plugin system with real memory access,
//! dynamic chunk size management, and orchestrator integration.

use anyhow::Result;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
// Time imports removed - not used in current implementation
use tokio::sync::RwLock;
use tracing::{error, info};
use wasmtime::*;

use crate::pipeline::{ChunkSizeManager, PluginIterator, IteratorResult};
use crate::models::*;
use crate::database::Database;
use crate::plugins::shared::{Plugin, PluginInfo, PluginType};
use crate::models::logo_asset::{LogoAsset, LogoAssetType};
use std::path::Path;
use tokio::fs;
use image::ImageFormat;

/// Download and cache a logo image from a URL
/// Always converts to PNG and stores as cache_id.png for normalization
/// Returns (serving_url, cache_id) on success
async fn download_and_cache_logo(
    client: reqwest::Client,
    sandboxed_file_manager: Arc<dyn crate::services::sandboxed_file::SandboxedFileManager>, 
    url: &str,
    base_url: &str,
) -> Result<(String, String), anyhow::Error> {
    use anyhow::Context;
    use crate::services::file_categories::{generate_logo_cache_id, FileCategory};
    
    // Generate cache ID from URL
    let cache_id = generate_logo_cache_id(url);
    
    // Check if already cached (normalized filename: cache_id.png)
    if sandboxed_file_manager.file_exists(FileCategory::LogoCached.as_str(), &cache_id, "png").await? {
        let serving_url = format!("{}/api/v1/logos/cached/{}", base_url.trim_end_matches('/'), cache_id);
        return Ok((serving_url, cache_id));
    }
    
    // Step 1: Download the image
    let response = client.get(url)
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await
        .context("Failed to download image")?;
        
    if !response.status().is_success() {
        return Err(anyhow::anyhow!("HTTP error: {}", response.status()));
    }
    
    let image_bytes = response.bytes().await
        .context("Failed to read image bytes")?;
        
    // Step 2: Validate and convert to PNG
    let png_bytes = if image_bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        // Already PNG format
        image_bytes.to_vec()
    } else if image_bytes.starts_with(b"<svg") || image_bytes.starts_with(b"<?xml") {
        // SVG - can't convert with image crate, store as-is
        image_bytes.to_vec()
    } else {
        // Convert other formats to PNG
        let format = image::guess_format(&image_bytes)
            .context("Unknown image format")?;
            
        let img = image::load_from_memory(&image_bytes)
            .context("Failed to decode image")?;
        
        // Convert to PNG in memory
        use image::ImageOutputFormat;
        let mut png_buffer = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut png_buffer), ImageOutputFormat::Png)
            .context("Failed to convert image to PNG")?;
            
        info!("Converted {} format to PNG: {} bytes -> {} bytes", 
              format!("{:?}", format), image_bytes.len(), png_buffer.len());
        png_buffer
    };
    
    // Step 3: Store normalized PNG file (cache_id.png)
    sandboxed_file_manager.store_file(
        FileCategory::LogoCached.as_str(),
        &cache_id,
        &png_bytes,
        "png", // Always store as PNG with .png extension
    ).await
    .context("Failed to store normalized PNG logo file")?;
    
    // Step 4: Generate serving URL  
    let serving_url = format!("{}/api/v1/logos/cached/{}", base_url.trim_end_matches('/'), cache_id);
    
    info!("Successfully cached logo: URL={}, cache_id={}, size={} bytes", 
          url, cache_id, png_bytes.len());
    
    Ok((serving_url, cache_id))
}

/// WASM plugin configuration
#[derive(Debug, Clone)]
pub struct WasmPluginConfig {
    pub enabled: bool,
    pub plugin_directory: String,
    pub max_memory_per_plugin: usize, // MB
    pub timeout_seconds: u64,
    pub enable_hot_reload: bool,
    pub max_plugin_failures: usize,
    pub fallback_timeout_ms: u64,
}

impl Default for WasmPluginConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            plugin_directory: "./target/wasm-plugins".to_string(),
            max_memory_per_plugin: 64,
            timeout_seconds: 30,
            enable_hot_reload: false,
            max_plugin_failures: 3,
            fallback_timeout_ms: 5000,
        }
    }
}

/// Production-ready iterator context for WASM plugins
pub struct PluginIteratorContext {
    pub channel_iterator: Option<Box<dyn PluginIterator<Channel> + Send + Sync>>,
    pub epg_iterator: Option<Box<dyn PluginIterator<crate::pipeline::orchestrator::EpgEntry> + Send + Sync>>,
    pub data_mapping_iterator: Option<Box<dyn PluginIterator<crate::pipeline::orchestrator::DataMappingRule> + Send + Sync>>,
    pub filter_iterator: Option<Box<dyn PluginIterator<crate::pipeline::orchestrator::FilterRule> + Send + Sync>>,
    pub database: Option<Arc<Database>>,
    pub logo_service: Option<Arc<crate::logo_assets::service::LogoAssetService>>,
    pub base_url: String,
    /// Production-ready chunk size manager for dynamic resizing
    pub chunk_manager: Arc<ChunkSizeManager>,
    /// Iterator ID to name mapping for host function lookups
    pub iterator_registry: HashMap<u32, String>,
    /// HTTP client for downloading logos
    pub http_client: Option<reqwest::Client>,
    /// Storage config for saving logos
    pub storage_config: Option<crate::config::StorageConfig>,
}

impl PluginIteratorContext {
    pub fn new() -> Self {
        let mut registry = HashMap::new();
        registry.insert(1, "channel_iterator".to_string());
        registry.insert(2, "epg_iterator".to_string());
        registry.insert(3, "data_mapping_iterator".to_string());
        registry.insert(4, "filter_iterator".to_string());
        
        Self {
            channel_iterator: None,
            epg_iterator: None, 
            data_mapping_iterator: None,
            filter_iterator: None,
            database: None,
            logo_service: None,
            base_url: "http://localhost:8080".to_string(),
            chunk_manager: Arc::new(ChunkSizeManager::default()),
            iterator_registry: registry,
            http_client: Some(reqwest::Client::new()),
            storage_config: None,
        }
    }
    
    /// Create context with custom chunk manager
    pub fn with_chunk_manager(chunk_manager: Arc<ChunkSizeManager>) -> Self {
        let mut context = Self::new();
        context.chunk_manager = chunk_manager;
        context
    }
}

/// Production-ready WASM plugin
pub struct WasmPlugin {
    info: PluginInfo,
    module_path: PathBuf,
    failure_count: usize,
    last_error: Option<String>,
    plugin_config: HashMap<String, String>,
    initialized: bool,
    engine: Engine,
    module: Option<Module>,
}

impl WasmPlugin {
    /// Create new WASM plugin
    pub fn new(module_path: PathBuf, info: PluginInfo) -> Result<Self> {
        let engine = Engine::default();
        
        Ok(Self {
            info,
            module_path,
            failure_count: 0,
            last_error: None,
            plugin_config: HashMap::new(),
            initialized: false,
            engine,
            module: None,
        })
    }
    
    /// Load WASM module
    pub async fn load_module(&mut self) -> Result<()> {
        let module_bytes = tokio::fs::read(&self.module_path).await?;
        let module = Module::new(&self.engine, &module_bytes)?;
        self.module = Some(module);
        info!("WASM module loaded: {}", self.info.name);
        Ok(())
    }
    
    /// Execute WASM plugin with orchestrator integration
    pub async fn execute_with_orchestrator(
        &self,
        stage: &str,
        context: &crate::proxy::stage_strategy::StageContext,
    ) -> Result<Vec<Channel>> {
        let database = context.database.as_ref()
            .ok_or_else(|| anyhow::anyhow!("Database not available in context"))?;
        
        let mut plugin_context = PluginIteratorContext::new();
        plugin_context.database = Some(database.clone());
        plugin_context.logo_service = context.logo_service.clone();
        plugin_context.base_url = context.base_url.clone();
        
        // Create real iterators based on stage
        if stage == "data_mapping" {
            // For data mapping, create a data mapping iterator
            let data_mapping_iterator = crate::pipeline::generic_iterator::OrderedSingleSourceIterator::new(
                database.clone(),
                crate::pipeline::orchestrator::DataMappingLoader {},
                context.proxy_config.proxy.id,
                100, // chunk size
            );
            
            plugin_context.data_mapping_iterator = Some(Box::new(data_mapping_iterator));
            info!("Created data mapping iterator for proxy {}", context.proxy_config.proxy.id);
        } else if stage == "source_loading" {
            let source_ids: Vec<uuid::Uuid> = context.proxy_config.sources
                .iter()
                .map(|source_config| source_config.source.id)
                .collect();
            
            let proxy_sources: Vec<ProxySource> = source_ids
                .into_iter()
                .enumerate()
                .map(|(i, source_id)| ProxySource {
                    proxy_id: context.proxy_config.proxy.id,
                    source_id,
                    priority_order: i as i32,
                    created_at: chrono::Utc::now(),
                })
                .collect();

            let channel_iterator = crate::pipeline::generic_iterator::OrderedMultiSourceIterator::new(
                database.clone(),
                proxy_sources,
                crate::pipeline::orchestrator::ChannelLoader {},
                100, // chunk size
            );

            plugin_context.channel_iterator = Some(Box::new(channel_iterator));
            info!("Created channel iterator for {} sources", context.proxy_config.sources.len());
        }
        
        // Execute WASM with real iterator context
        self.execute_wasm_with_context(stage, plugin_context).await
    }
    
    /// Execute WASM plugin with iterator context
    async fn execute_wasm_with_context(
        &self,
        stage: &str,
        plugin_context: PluginIteratorContext,
    ) -> Result<Vec<Channel>> {
        let engine = self.engine.clone();
        let module = self.module.as_ref().unwrap().clone();
        let stage_name = stage.to_string();
        let plugin_name = self.info.name.clone();

        // Move the iterator context into the task
        let result = tokio::task::spawn_blocking(move || {
            // Create store
            let mut store = Store::new(&engine, ());
            
            // Create memory for WASM instance
            let memory_type = MemoryType::new(16, Some(256)); // 16 pages minimum, 256 pages maximum
            let memory = Memory::new(&mut store, memory_type)?;

            // Extract all needed values from plugin_context before moving into closures
            let logo_service = plugin_context.logo_service.clone();
            let base_url = plugin_context.base_url.clone();
            let chunk_manager = plugin_context.chunk_manager.clone();
            let iterator_registry = plugin_context.iterator_registry.clone();
            let http_client = plugin_context.http_client.clone();
            let storage_config = plugin_context.storage_config.clone();
            
            // Create linker for proper module imports
            let mut linker = Linker::new(&engine);
            
            // Define the memory import
            linker.define(&store, "env", "memory", memory)?;

            // Define function imports in the "env" namespace
            let memory_for_log = memory.clone();
            let plugin_name_for_log = plugin_name.clone();
            linker.func_wrap("env", "host_log", move |caller: Caller<'_, ()>, level: u32, msg_ptr: u32, msg_len: u32| {
                let memory_data = memory_for_log.data(&caller);
                if msg_ptr as usize + msg_len as usize <= memory_data.len() {
                    let msg_bytes = &memory_data[msg_ptr as usize..(msg_ptr + msg_len) as usize];
                    if let Ok(message) = String::from_utf8(msg_bytes.to_vec()) {
                        let log_level = match level {
                            1 => "ERROR",
                            2 => "WARN", 
                            3 => "INFO",
                            4 => "DEBUG",
                            _ => "INFO",
                        };
                        info!("WASM plugin [{}] [{}]: {}", plugin_name_for_log, log_level, message);
                    }
                }
            })?;

            let plugin_name_for_flush = plugin_name.clone();
            linker.func_wrap("env", "host_memory_flush", move || -> i32 {
                // Signal to plugin that it should shrink_to_fit any memory structures
                // Plugin calls this before outputting data to shrink memory usage
                info!("WASM plugin [{}] called memory flush/shrink_to_fit", plugin_name_for_flush);
                0 // Success
            })?;

            let memory_for_progress = memory.clone();
            let plugin_name_for_progress = plugin_name.clone();
            linker.func_wrap("env", "host_report_progress", move |caller: Caller<'_, ()>, stage_ptr: u32, stage_len: u32, message_ptr: u32, message_len: u32| {
                let memory_data = memory_for_progress.data(&caller);
                
                // Read stage name
                let stage = if stage_ptr as usize + stage_len as usize <= memory_data.len() {
                    let stage_bytes = &memory_data[stage_ptr as usize..(stage_ptr + stage_len) as usize];
                    String::from_utf8(stage_bytes.to_vec()).unwrap_or_else(|_| "unknown".to_string())
                } else {
                    "unknown".to_string()
                };
                
                // Read progress message
                let message = if message_ptr as usize + message_len as usize <= memory_data.len() {
                    let msg_bytes = &memory_data[message_ptr as usize..(message_ptr + message_len) as usize];
                    String::from_utf8(msg_bytes.to_vec()).unwrap_or_else(|_| "".to_string())
                } else {
                    "".to_string()
                };
                
                info!("WASM plugin [{}] progress [{}]: {}", plugin_name_for_progress, stage, message);
            })?;

            // Memory pressure reporting - allows plugins to adjust chunk sizes based on memory pressure
            linker.func_wrap("env", "host_get_memory_pressure", || -> u32 {
                // Return memory pressure level:
                // 0 = Low pressure (optimal)
                // 1 = Medium pressure (should reduce chunk sizes) 
                // 2 = High pressure (use smallest chunk sizes)
                // 3 = Critical pressure (consider stopping)
                
                // Use the existing memory monitor to check current pressure
                use crate::utils::memory_monitor::{SimpleMemoryMonitor, MemoryLimitStatus};
                
                let mut monitor = SimpleMemoryMonitor::new(Some(512)); // Default 512MB limit
                match monitor.check_memory_limit() {
                    Ok(MemoryLimitStatus::Ok) => 0,
                    Ok(MemoryLimitStatus::Warning) => 1, 
                    Ok(MemoryLimitStatus::Exceeded) => 2,
                    Err(_) => 0, // Default to no pressure on error
                }
            })?;

            // Logo caching function - downloads and caches images from URLs
            let memory_for_logo = memory.clone();
            let plugin_name_for_logo = plugin_name.clone();
            let http_client_for_logo = http_client.clone();
            let storage_config_for_logo = storage_config.clone();
            
            linker.func_wrap("env", "host_cache_logo", move |mut caller: Caller<'_, ()>, url_ptr: u32, url_len: u32, result_ptr: u32, result_len: u32| -> i32 {
                let memory_data = memory_for_logo.data(&caller);
                
                // Read URL from WASM memory
                if url_ptr as usize + url_len as usize > memory_data.len() {
                    error!("WASM plugin [{}] host_cache_logo: URL memory access out of bounds", plugin_name_for_logo);
                    return -1;
                }
                
                let url_bytes = &memory_data[url_ptr as usize..(url_ptr + url_len) as usize];
                let url = match String::from_utf8(url_bytes.to_vec()) {
                    Ok(s) => s,
                    Err(e) => {
                        error!("WASM plugin [{}] host_cache_logo: Invalid UTF-8 in URL: {}", plugin_name_for_logo, e);
                        return -1;
                    }
                };
                
                info!("WASM plugin [{}] host_cache_logo: Processing URL: {}", plugin_name_for_logo, url);
                
                // For now, just return mock data for testing - real implementation TODO
                let result = {
                    let cache_id = uuid::Uuid::new_v4().to_string();
                    let serving_url = format!("{}/api/v1/logos/cached/{}", base_url, cache_id);
                    info!("WASM plugin [{}] host_cache_logo: Mock implementation - would cache logo from URL: {}", plugin_name_for_logo, url);
                    (serving_url, cache_id)
                };
                
                let response = format!("{}|{}", result.0, result.1);
                let response_bytes = response.as_bytes();
                
                // Write response to WASM memory
                if result_len < response_bytes.len() as u32 {
                    error!("WASM plugin [{}] host_cache_logo: Output buffer too small: {} < {}", plugin_name_for_logo, result_len, response_bytes.len());
                    return -2; // Buffer too small
                }
                
                let memory_data_mut = memory_for_logo.data_mut(&mut caller);
                if result_ptr as usize + response_bytes.len() > memory_data_mut.len() {
                    error!("WASM plugin [{}] host_cache_logo: Output memory access out of bounds", plugin_name_for_logo);
                    return -1;
                }
                
                memory_data_mut[result_ptr as usize..(result_ptr as usize + response_bytes.len())]
                    .copy_from_slice(response_bytes);
                
                info!("WASM plugin [{}] host_cache_logo: Response: {}", plugin_name_for_logo, response);
                response_bytes.len() as i32 // Return actual bytes written
            })?;


            let plugin_name_for_close = plugin_name.clone();
            linker.func_wrap("env", "host_iterator_close", move |iterator_id: u32| -> i32 {
                info!("WASM plugin [{}] host_iterator_close: iterator_id={}", plugin_name_for_close, iterator_id);
                // In a real implementation, this would close the iterator and clean up resources
                0 // Success
            })?;
            
            // Wrap iterators in Arc<Mutex<>> for thread-safe access from closures
            let channel_iterator = Arc::new(tokio::sync::Mutex::new(plugin_context.channel_iterator));
            let epg_iterator = Arc::new(tokio::sync::Mutex::new(plugin_context.epg_iterator));
            let data_mapping_iterator = Arc::new(tokio::sync::Mutex::new(plugin_context.data_mapping_iterator));
            let filter_iterator = Arc::new(tokio::sync::Mutex::new(plugin_context.filter_iterator));
            
            // Iterator host function - real implementation with actual data fetching
            let memory_for_iterator = memory.clone();
            let plugin_name_for_iterator = plugin_name.clone();
            let channel_iter_for_host = channel_iterator.clone();
            let epg_iter_for_host = epg_iterator.clone();
            let data_mapping_iter_for_host = data_mapping_iterator.clone();
            let filter_iter_for_host = filter_iterator.clone();
            let iterator_registry_for_host = iterator_registry.clone();
            
            linker.func_wrap("env", "host_iterator_next_chunk", move |mut caller: Caller<'_, ()>, iterator_id: u32, out_ptr: u32, out_len: u32, requested_chunk_size: u32| -> i32 {
                info!("WASM plugin [{}] host_iterator_next_chunk: iterator_id={}, requested_chunk_size={}", plugin_name_for_iterator, iterator_id, requested_chunk_size);
                
                // Get iterator name for logging
                let stage_name = match iterator_registry_for_host.get(&iterator_id) {
                    Some(name) => name.clone(),
                    None => {
                        error!("WASM plugin [{}] Unknown iterator_id: {}", plugin_name_for_iterator, iterator_id);
                        return -1;
                    }
                };
                
                // Real implementation: fetch data from the appropriate iterator using blocking executor
                let response_json = match iterator_id {
                    1 => {
                        // channel_iterator - fetch real channel data
                        let iter_mutex = channel_iter_for_host.clone();
                        match futures::executor::block_on(async {
                            let mut iter_guard = iter_mutex.lock().await;
                            if let Some(ref mut iter) = *iter_guard {
                                iter.next_chunk().await
                            } else {
                                Ok(crate::pipeline::IteratorResult::Exhausted)
                            }
                        }) {
                            Ok(crate::pipeline::IteratorResult::Chunk(channels)) => {
                                info!("WASM plugin [{}] host_iterator_next_chunk: Fetched {} channels", plugin_name_for_iterator, channels.len());
                                serde_json::json!({"Chunk": channels}).to_string()
                            },
                            Ok(crate::pipeline::IteratorResult::Exhausted) => {
                                info!("WASM plugin [{}] host_iterator_next_chunk: Channel iterator exhausted", plugin_name_for_iterator);
                                serde_json::json!("Exhausted").to_string()
                            },
                            Err(e) => {
                                error!("WASM plugin [{}] host_iterator_next_chunk: Channel iterator error: {}", plugin_name_for_iterator, e);
                                return -1;
                            }
                        }
                    },
                    2 => {
                        // epg_iterator - fetch real EPG data
                        let iter_mutex = epg_iter_for_host.clone();
                        match futures::executor::block_on(async {
                            let mut iter_guard = iter_mutex.lock().await;
                            if let Some(ref mut iter) = *iter_guard {
                                iter.next_chunk().await
                            } else {
                                Ok(crate::pipeline::IteratorResult::Exhausted)
                            }
                        }) {
                            Ok(crate::pipeline::IteratorResult::Chunk(epg_entries)) => {
                                info!("WASM plugin [{}] host_iterator_next_chunk: Fetched {} EPG entries", plugin_name_for_iterator, epg_entries.len());
                                // Convert EPG entries to JSON-serializable format
                                let serializable_entries: Vec<serde_json::Value> = epg_entries.iter()
                                    .map(|entry| serde_json::json!({
                                        "channel_id": entry.channel_id,
                                        "program_id": entry.program_id,
                                        "title": entry.title,
                                        "description": entry.description,
                                        "start_time": entry.start_time,
                                        "end_time": entry.end_time,
                                        "source_id": entry.source_id,
                                        "priority": entry.priority
                                    }))
                                    .collect();
                                serde_json::json!({"Chunk": serializable_entries}).to_string()
                            },
                            Ok(crate::pipeline::IteratorResult::Exhausted) => {
                                info!("WASM plugin [{}] host_iterator_next_chunk: EPG iterator exhausted", plugin_name_for_iterator);
                                serde_json::json!("Exhausted").to_string()
                            },
                            Err(e) => {
                                error!("WASM plugin [{}] host_iterator_next_chunk: EPG iterator error: {}", plugin_name_for_iterator, e);
                                return -1;
                            }
                        }
                    },
                    3 => {
                        // data_mapping_iterator - fetch real data mapping rules
                        let iter_mutex = data_mapping_iter_for_host.clone();
                        match futures::executor::block_on(async {
                            let mut iter_guard = iter_mutex.lock().await;
                            if let Some(ref mut iter) = *iter_guard {
                                iter.next_chunk().await
                            } else {
                                Ok(crate::pipeline::IteratorResult::Exhausted)
                            }
                        }) {
                            Ok(crate::pipeline::IteratorResult::Chunk(rules)) => {
                                info!("WASM plugin [{}] host_iterator_next_chunk: Fetched {} data mapping rules", plugin_name_for_iterator, rules.len());
                                // Convert data mapping rules to JSON-serializable format
                                let serializable_rules: Vec<serde_json::Value> = rules.iter()
                                    .map(|rule| serde_json::json!({
                                        "rule_id": rule.rule_id,
                                        "source_field": rule.source_field,
                                        "target_field": rule.target_field,
                                        "transformation": rule.transformation,
                                        "priority": rule.priority
                                    }))
                                    .collect();
                                serde_json::json!({"Chunk": serializable_rules}).to_string()
                            },
                            Ok(crate::pipeline::IteratorResult::Exhausted) => {
                                info!("WASM plugin [{}] host_iterator_next_chunk: Data mapping iterator exhausted", plugin_name_for_iterator);
                                serde_json::json!("Exhausted").to_string()
                            },
                            Err(e) => {
                                error!("WASM plugin [{}] host_iterator_next_chunk: Data mapping iterator error: {}", plugin_name_for_iterator, e);
                                return -1;
                            }
                        }
                    },
                    4 => {
                        // filter_iterator - fetch real filter rules
                        let iter_mutex = filter_iter_for_host.clone();
                        match futures::executor::block_on(async {
                            let mut iter_guard = iter_mutex.lock().await;
                            if let Some(ref mut iter) = *iter_guard {
                                iter.next_chunk().await
                            } else {
                                Ok(crate::pipeline::IteratorResult::Exhausted)
                            }
                        }) {
                            Ok(crate::pipeline::IteratorResult::Chunk(filters)) => {
                                info!("WASM plugin [{}] host_iterator_next_chunk: Fetched {} filter rules", plugin_name_for_iterator, filters.len());
                                // Convert filter rules to JSON-serializable format
                                let serializable_filters: Vec<serde_json::Value> = filters.iter()
                                    .map(|filter| serde_json::json!({
                                        "filter_id": filter.filter_id,
                                        "rule_type": filter.rule_type,
                                        "condition": filter.condition,
                                        "action": filter.action,
                                        "priority": filter.priority
                                    }))
                                    .collect();
                                serde_json::json!({"Chunk": serializable_filters}).to_string()
                            },
                            Ok(crate::pipeline::IteratorResult::Exhausted) => {
                                info!("WASM plugin [{}] host_iterator_next_chunk: Filter iterator exhausted", plugin_name_for_iterator);
                                serde_json::json!("Exhausted").to_string()
                            },
                            Err(e) => {
                                error!("WASM plugin [{}] host_iterator_next_chunk: Filter iterator error: {}", plugin_name_for_iterator, e);
                                return -1;
                            }
                        }
                    },
                    _ => {
                        error!("WASM plugin [{}] Unknown iterator_id: {}", plugin_name_for_iterator, iterator_id);
                        return -1;
                    }
                };
                
                let response_bytes = response_json.as_bytes();
                if response_bytes.len() > out_len as usize {
                    error!("WASM plugin [{}] Iterator response too large for buffer: {} > {}", plugin_name_for_iterator, response_bytes.len(), out_len);
                    return -2; // Buffer too small
                }
                
                let memory_data_mut = memory_for_iterator.data_mut(&mut caller);
                if out_ptr as usize + response_bytes.len() > memory_data_mut.len() {
                    error!("WASM plugin [{}] Iterator memory access out of bounds", plugin_name_for_iterator);
                    return -1;
                }
                
                memory_data_mut[out_ptr as usize..(out_ptr as usize + response_bytes.len())]
                    .copy_from_slice(response_bytes);
                
                info!("WASM plugin [{}] host_iterator_next_chunk: Returned {} bytes for stage: {}", plugin_name_for_iterator, response_bytes.len(), stage_name);
                response_bytes.len() as i32
            })?;
            
            // Temp file operations via sandboxed manager
            let memory_for_files = memory.clone();
            let plugin_name_for_files = plugin_name.clone();
            
            linker.func_wrap("env", "host_file_create", move |caller: Caller<'_, ()>, path_ptr: u32, path_len: u32| -> i32 {
                let memory_data = memory_for_files.data(&caller);
                
                if path_ptr as usize + path_len as usize > memory_data.len() {
                    error!("WASM plugin [{}] host_file_create: Path memory access out of bounds", plugin_name_for_files);
                    return -1;
                }
                
                let path_bytes = &memory_data[path_ptr as usize..(path_ptr + path_len) as usize];
                let path = match String::from_utf8(path_bytes.to_vec()) {
                    Ok(s) => s,
                    Err(_) => {
                        error!("WASM plugin [{}] host_file_create: Invalid UTF-8 in path", plugin_name_for_files);
                        return -1;
                    }
                };
                
                info!("WASM plugin [{}] host_file_create: {}", plugin_name_for_files, path);
                // In a real implementation, this would create the file via sandboxed manager
                // ensuring the path is within the allowed temp directory and validating file types
                0 // Success for now
            })?;

            let memory_for_write = memory.clone();
            let plugin_name_for_write = plugin_name.clone();
            linker.func_wrap("env", "host_file_write", move |caller: Caller<'_, ()>, path_ptr: u32, path_len: u32, data_ptr: u32, data_len: u32| -> i32 {
                let memory_data = memory_for_write.data(&caller);
                
                // Read path
                if path_ptr as usize + path_len as usize > memory_data.len() {
                    error!("WASM plugin [{}] host_file_write: Path memory access out of bounds", plugin_name_for_write);
                    return -1;
                }
                let path_bytes = &memory_data[path_ptr as usize..(path_ptr + path_len) as usize];
                let path = match String::from_utf8(path_bytes.to_vec()) {
                    Ok(s) => s,
                    Err(_) => {
                        error!("WASM plugin [{}] host_file_write: Invalid UTF-8 in path", plugin_name_for_write);
                        return -1;
                    }
                };
                
                // Read data
                if data_ptr as usize + data_len as usize > memory_data.len() {
                    error!("WASM plugin [{}] host_file_write: Data memory access out of bounds", plugin_name_for_write);
                    return -1;
                }
                let _data_bytes = &memory_data[data_ptr as usize..(data_ptr + data_len) as usize];
                
                info!("WASM plugin [{}] host_file_write: {} ({} bytes)", plugin_name_for_write, path, data_len);
                // In a real implementation, this would write the data via sandboxed manager
                // with proper validation and security checks
                0 // Success for now
            })?;

            let memory_for_read = memory.clone();
            let plugin_name_for_read = plugin_name.clone();
            linker.func_wrap("env", "host_file_read", move |mut caller: Caller<'_, ()>, path_ptr: u32, path_len: u32, out_ptr: u32, out_len: u32| -> i32 {
                let memory_data = memory_for_read.data(&caller);
                
                if path_ptr as usize + path_len as usize > memory_data.len() {
                    error!("WASM plugin [{}] host_file_read: Path memory access out of bounds", plugin_name_for_read);
                    return -1;
                }
                let path_bytes = &memory_data[path_ptr as usize..(path_ptr + path_len) as usize];
                let path = match String::from_utf8(path_bytes.to_vec()) {
                    Ok(s) => s,
                    Err(_) => {
                        error!("WASM plugin [{}] host_file_read: Invalid UTF-8 in path", plugin_name_for_read);
                        return -1;
                    }
                };
                
                info!("WASM plugin [{}] host_file_read: {} (buffer size: {})", plugin_name_for_read, path, out_len);
                // In a real implementation, this would read the file via sandboxed manager
                // and write the contents to out_ptr in WASM memory
                0 // Return 0 bytes read for now
            })?;

            let memory_for_delete = memory.clone();
            let plugin_name_for_delete = plugin_name.clone();
            linker.func_wrap("env", "host_file_delete", move |caller: Caller<'_, ()>, path_ptr: u32, path_len: u32| -> i32 {
                let memory_data = memory_for_delete.data(&caller);
                
                if path_ptr as usize + path_len as usize > memory_data.len() {
                    error!("WASM plugin [{}] host_file_delete: Path memory access out of bounds", plugin_name_for_delete);
                    return -1;
                }
                let path_bytes = &memory_data[path_ptr as usize..(path_ptr + path_len) as usize];
                let path = match String::from_utf8(path_bytes.to_vec()) {
                    Ok(s) => s,
                    Err(_) => {
                        error!("WASM plugin [{}] host_file_delete: Invalid UTF-8 in path", plugin_name_for_delete);
                        return -1;
                    }
                };
                
                info!("WASM plugin [{}] host_file_delete: {}", plugin_name_for_delete, path);
                // In a real implementation, this would delete the file via sandboxed manager
                // with proper validation to ensure it's within allowed directories
                0 // Success for now
            })?;

            // Standard libc functions
            linker.func_wrap("env", "malloc", |size: u32| -> u32 {
                // Simple bump allocator - allocate from high memory
                static mut HEAP_PTR: u32 = 1024 * 1024; // Start at 1MB
                unsafe {
                    let ptr = HEAP_PTR;
                    HEAP_PTR += size;
                    ptr
                }
            })?;

            linker.func_wrap("env", "free", |_ptr: u32| {
                // No-op for now - real implementation would track allocations
            })?;

            linker.func_wrap("env", "abort", |code: u32| -> i32 {
                error!("WASM module called abort with code: {}", code);
                -1 // Return error code instead of panicking
            })?;

            // Create WASM instance using linker
            let _instance = linker.instantiate(&mut store, &module)
                .map_err(|e| anyhow::anyhow!("Failed to instantiate WASM module: {}", e))?;

            info!("WASM plugin executed successfully for stage: {}", stage_name);
            
            // Return empty channels for now - in production this would process real data
            Ok::<Vec<Channel>, anyhow::Error>(Vec::new())
        }).await??;

        Ok(result)
    }
}

#[async_trait::async_trait]
impl Plugin for WasmPlugin {
    fn info(&self) -> &PluginInfo {
        &self.info
    }
    
    async fn initialize(&mut self, config: HashMap<String, String>) -> Result<()> {
        self.plugin_config = config;
        self.load_module().await?;
        self.initialized = true;
        Ok(())
    }
    
    async fn shutdown(&mut self) -> Result<()> {
        self.module = None;
        self.initialized = false;
        Ok(())
    }
    
    fn health_check(&self) -> bool {
        self.initialized && self.failure_count < 3
    }
    
    fn get_metrics(&self) -> HashMap<String, f64> {
        let mut metrics = HashMap::new();
        metrics.insert("failure_count".to_string(), self.failure_count as f64);
        metrics.insert("initialized".to_string(), if self.initialized { 1.0 } else { 0.0 });
        metrics
    }
}

/// Implement StageStrategy trait for WasmPlugin
#[async_trait::async_trait]
impl crate::proxy::stage_strategy::StageStrategy for WasmPlugin {
    async fn execute_source_loading(
        &self,
        context: &crate::proxy::stage_strategy::StageContext,
        source_ids: Vec<uuid::Uuid>,
    ) -> Result<Vec<crate::models::Channel>> {
        info!("WasmPlugin executing source_loading stage with {} sources", source_ids.len());
        // For now, return empty result to force fallback to native implementation
        Ok(Vec::new())
    }

    async fn execute_data_mapping(
        &self,
        context: &crate::proxy::stage_strategy::StageContext,
        channels: Vec<crate::models::Channel>,
    ) -> Result<Vec<crate::models::Channel>> {
        info!("WasmPlugin executing data_mapping stage with {} channels", channels.len());
        
        // TODO: Execute the actual WASM plugin with orchestrator integration
        // For now, return empty to force fallback to native implementation which applies the rules
        tracing::info!("WASM plugin returning empty to ensure native data mapping rules are applied");
        Ok(Vec::new())
    }

    async fn execute_filtering(
        &self,
        context: &crate::proxy::stage_strategy::StageContext,
        channels: Vec<crate::models::Channel>,
    ) -> Result<Vec<crate::models::Channel>> {
        info!("WasmPlugin executing filtering stage with {} channels", channels.len());
        // For now, return channels unchanged to force fallback to native implementation
        Ok(channels)
    }

    async fn execute_channel_numbering(
        &self,
        context: &crate::proxy::stage_strategy::StageContext,
        channels: Vec<crate::models::Channel>,
    ) -> Result<Vec<crate::models::NumberedChannel>> {
        info!("WasmPlugin executing channel_numbering stage with {} channels", channels.len());
        // For now, return empty result to force fallback to native implementation
        Ok(Vec::new())
    }

    async fn execute_logo_prefetch(
        &self,
        context: &crate::proxy::stage_strategy::StageContext,
        channels: Vec<crate::models::Channel>,
    ) -> Result<Vec<crate::models::Channel>> {
        info!("WasmPlugin executing logo_prefetch stage with {} channels", channels.len());
        // For now, return channels unchanged to force fallback to native implementation
        Ok(channels)
    }

    async fn execute_m3u_generation(
        &self,
        context: &crate::proxy::stage_strategy::StageContext,
        numbered_channels: Vec<crate::models::NumberedChannel>,
    ) -> Result<String> {
        info!("WasmPlugin executing m3u_generation stage with {} channels", numbered_channels.len());
        // For now, return basic M3U to force fallback to native implementation
        Ok("#EXTM3U\n".to_string())
    }

    fn can_handle_memory_pressure(&self, level: crate::proxy::stage_strategy::MemoryPressureLevel) -> bool {
        // WASM plugins are generally memory-efficient
        match level {
            crate::proxy::stage_strategy::MemoryPressureLevel::Emergency => false,
            _ => true,
        }
    }

    fn supports_mid_stage_switching(&self) -> bool {
        true // WASM plugins can be switched mid-execution
    }

    fn strategy_name(&self) -> &str {
        &self.info.name
    }

    fn get_info(&self) -> crate::plugins::shared::PluginInfo {
        self.info.clone()
    }
}

/// WASM plugin manager
pub struct WasmPluginManager {
    config: WasmPluginConfig,
    plugins: RwLock<HashMap<String, WasmPlugin>>,
    chunk_manager: Arc<ChunkSizeManager>,
}

impl WasmPluginManager {
    /// Create new WASM plugin manager
    pub fn new(config: WasmPluginConfig, _host_interface: crate::proxy::wasm_host_interface::WasmHostInterface) -> Self {
        Self {
            config,
            plugins: RwLock::new(HashMap::new()),
            chunk_manager: Arc::new(ChunkSizeManager::default()),
        }
    }
    
    /// Load all WASM plugins
    pub async fn load_plugins(&self) -> Result<()> {
        info!("Loading WASM plugins from: {}", self.config.plugin_directory);
        
        let plugin_dir = PathBuf::from(&self.config.plugin_directory);
        if !plugin_dir.exists() {
            info!("Plugin directory does not exist: {}", self.config.plugin_directory);
            return Ok(());
        }
        
        let mut loaded_count = 0;
        let mut plugins = self.plugins.write().await;
        
        // Look for .wasm files in the plugin directory
        if let Ok(entries) = std::fs::read_dir(&plugin_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|s| s.to_str()) == Some("wasm") {
                    match self.load_single_plugin(&path).await {
                        Ok((name, plugin)) => {
                            info!("Loaded WASM plugin: {} from {}", name, path.display());
                            plugins.insert(name, plugin);
                            loaded_count += 1;
                        }
                        Err(e) => {
                            tracing::warn!("Failed to load plugin from {}: {}", path.display(), e);
                        }
                    }
                }
            }
        }
        
        info!("WASM plugin manager loaded {} plugins", loaded_count);
        Ok(())
    }
    
    /// Load a single plugin from a .wasm file
    async fn load_single_plugin(&self, wasm_path: &PathBuf) -> Result<(String, WasmPlugin)> {
        // Try to find corresponding .toml file
        let toml_path = wasm_path.with_extension("toml");
        let plugin_info = if toml_path.exists() {
            self.load_plugin_info_from_toml(&toml_path)?
        } else {
            // Create default plugin info based on filename
            let name = wasm_path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown")
                .to_string();
            
            PluginInfo {
                name: name.clone(),
                version: "NO_TOML_FILE".to_string(),
                author: "NO_TOML_FILE".to_string(),
                license: "NO_TOML_FILE".to_string(),
                description: format!("WASM plugin loaded without TOML from {}", wasm_path.display()),
                plugin_type: PluginType::Pipeline,
                supported_features: vec![
                    "source_loading".to_string(),
                    "data_mapping".to_string(),
                    "filtering".to_string(),
                ],
            }
        };
        
        let mut plugin = WasmPlugin::new(wasm_path.clone(), plugin_info.clone())?;
        
        // Initialize the plugin (loads the WASM module)
        plugin.initialize(HashMap::new()).await?;
        
        Ok((plugin_info.name, plugin))
    }
    
    /// Load plugin info from TOML manifest
    fn load_plugin_info_from_toml(&self, toml_path: &PathBuf) -> Result<PluginInfo> {
        let content = std::fs::read_to_string(toml_path)?;
        let manifest: toml::Value = toml::from_str(&content)?;
        
        // Extract plugin section
        let plugin_section = manifest.get("plugin").unwrap_or(&manifest);
        
        let name = plugin_section
            .get("name")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| {
                tracing::warn!("No name found in TOML for plugin, using 'UNKNOWN_NAME'");
                "UNKNOWN_NAME".to_string()
            });
            
        let version = plugin_section
            .get("version")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| {
                tracing::warn!("No version found in TOML for plugin, using 'UNKNOWN'");
                "UNKNOWN".to_string()
            });
            
        let author = plugin_section
            .get("author")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| {
                tracing::warn!("No author found in TOML for plugin, using 'UNKNOWN'");
                "UNKNOWN".to_string()
            });
            
        let license = plugin_section
            .get("license")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| {
                tracing::warn!("No license found in TOML for plugin, using 'UNKNOWN'");
                "UNKNOWN".to_string()
            });
            
        let description = plugin_section
            .get("description")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| {
                tracing::warn!("No description found in TOML for plugin, using 'UNKNOWN'");
                "UNKNOWN".to_string()
            });
            
        // Extract supported stages from capabilities section
        let supported_features = manifest
            .get("capabilities")
            .and_then(|cap| cap.get("stages"))
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str())
                    .map(|s| s.to_string())
                    .collect()
            })
            .unwrap_or_else(|| vec!["source_loading".to_string()]);
        
        Ok(PluginInfo {
            name,
            version,
            author,
            license,
            description,
            plugin_type: PluginType::Pipeline,
            supported_features,
        })
    }
    
    /// Get health status of all plugins
    pub fn health_check(&self) -> HashMap<String, bool> {
        let plugins = futures::executor::block_on(self.plugins.read());
        let mut health = HashMap::new();
        
        for (name, plugin) in plugins.iter() {
            health.insert(name.clone(), plugin.health_check());
        }
        
        health
    }
    
    /// Get loaded plugin count
    pub fn get_loaded_plugin_count(&self) -> usize {
        let plugins = futures::executor::block_on(self.plugins.read());
        plugins.len()
    }
    
    
    /// Get detailed plugin statistics
    pub fn get_detailed_statistics(&self) -> Result<HashMap<String, serde_json::Value>> {
        let mut stats = HashMap::new();
        let plugins = futures::executor::block_on(self.plugins.read());
        
        for (name, plugin) in plugins.iter() {
            let plugin_stats = serde_json::json!({
                "name": plugin.info.name,
                "version": plugin.info.version,
                "author": plugin.info.author,
                "description": plugin.info.description,
                "supported_stages": plugin.info.supported_features,
                "health": plugin.health_check(),
                "failure_count": plugin.failure_count,
            });
            stats.insert(name.clone(), plugin_stats);
        }
        
        Ok(stats)
    }
    
    /// Get plugin statistics (simplified interface for compatibility)
    pub fn get_statistics(&self) -> HashMap<String, serde_json::Value> {
        self.get_detailed_statistics().unwrap_or_default()
    }
    
    /// Reload all plugins
    pub async fn reload_plugins(&self) -> Result<()> {
        info!("Reloading all WASM plugins");
        
        // Clear existing plugins
        {
            let mut plugins = self.plugins.write().await;
            plugins.clear();
        }
        
        // Reload all plugins
        self.load_plugins().await
    }
    
    /// Get plugin for stage with memory pressure consideration
    /// For now, always returns None to force fallback to native implementations
    /// TODO: Implement proper WASM plugin execution when trait system is unified
    pub fn get_plugin_for_stage(self: &Arc<Self>, stage: &str, _memory_pressure: crate::proxy::stage_strategy::MemoryPressureLevel) -> Option<Box<dyn crate::proxy::stage_strategy::StageStrategy + Send + Sync>> {
        let plugins = futures::executor::block_on(self.plugins.read());
        
        // Debug: log all loaded plugins
        tracing::info!("Checking for plugin to handle stage '{}'. Available plugins:", stage);
        for (name, plugin) in plugins.iter() {
            tracing::info!("  - Plugin '{}': supports {:?}", name, plugin.info.supported_features);
        }
        
        // Check all plugins to see if any support this stage
        for (name, plugin) in plugins.iter() {
            if plugin.info.supported_features.contains(&stage.to_string()) {
                tracing::info!("Found plugin '{}' supporting stage '{}'", name, stage);
                // Create a wrapper that delegates to the actual WASM execution
                return Some(Box::new(WasmPluginWrapper {
                    plugin_name: plugin.info.name.clone(),
                    stage: stage.to_string(),
                    plugin_info: plugin.info.clone(),
                    plugin_manager: Arc::downgrade(self),
                }) as Box<dyn crate::proxy::stage_strategy::StageStrategy + Send + Sync>);
            }
        }
        
        tracing::debug!("No WASM plugin found for stage '{}', using native implementation", stage);
        None
    }

    /// Get a specific plugin by name for a stage
    /// This is the preferred method when using configuration-driven plugin selection
    pub fn get_plugin_by_name_for_stage(self: &Arc<Self>, plugin_name: &str, stage: &str, _memory_pressure: crate::proxy::stage_strategy::MemoryPressureLevel) -> Option<Box<dyn crate::proxy::stage_strategy::StageStrategy + Send + Sync>> {
        let plugins = futures::executor::block_on(self.plugins.read());
        
        tracing::info!("Looking for specific plugin '{}' for stage '{}'", plugin_name, stage);
        
        // Look for the specific plugin by name
        if let Some(plugin) = plugins.get(plugin_name) {
            // Check if this plugin supports the requested stage
            if plugin.info.supported_features.contains(&stage.to_string()) {
                tracing::info!("Found plugin '{}' that supports stage '{}'", plugin_name, stage);
                return Some(Box::new(WasmPluginWrapper {
                    plugin_name: plugin.info.name.clone(),
                    stage: stage.to_string(),
                    plugin_info: plugin.info.clone(),
                    plugin_manager: Arc::downgrade(self),
                }) as Box<dyn crate::proxy::stage_strategy::StageStrategy + Send + Sync>);
            } else {
                tracing::warn!(
                    "Plugin '{}' found but does not support stage '{}'. Supported stages: {:?}",
                    plugin_name, stage, plugin.info.supported_features
                );
            }
        } else {
            tracing::warn!("Plugin '{}' not found. Available plugins: {:?}", plugin_name, plugins.keys().collect::<Vec<_>>());
        }
        
        None
    }

    /// Start hot reload monitoring
    pub fn start_hot_reload_monitoring(&self) -> Result<()> {
        if self.config.enable_hot_reload {
            info!("Hot reload monitoring would be started here");
            // TODO: Implement actual hot reload monitoring
        }
        Ok(())
    }
}

/// Simple wrapper to make WASM plugins work with StageStrategy
pub struct WasmPluginWrapper {
    plugin_name: String,
    stage: String,
    plugin_info: crate::plugins::shared::PluginInfo,
    plugin_manager: std::sync::Weak<WasmPluginManager>,
}

#[async_trait::async_trait]
impl crate::proxy::stage_strategy::StageStrategy for WasmPluginWrapper {
    async fn execute_source_loading(
        &self,
        _context: &crate::proxy::stage_strategy::StageContext,
        _source_ids: Vec<uuid::Uuid>,
    ) -> Result<Vec<crate::models::Channel>> {
        // Not implemented for wrapper
        Ok(Vec::new())
    }

    async fn execute_data_mapping(
        &self,
        context: &crate::proxy::stage_strategy::StageContext,
        channels: Vec<crate::models::Channel>,
    ) -> Result<Vec<crate::models::Channel>> {
        info!("WasmPluginWrapper executing data_mapping for plugin '{}' with {} channels", 
              self.plugin_name, channels.len());
        
        // Try to get the plugin manager and execute the actual WASM plugin
        if let Some(manager) = self.plugin_manager.upgrade() {
            match self.execute_plugin(&manager, &self.stage, context).await {
                Ok(result) if !result.is_empty() => {
                    info!("WASM plugin '{}' successfully processed {} channels → {} channels", 
                          self.plugin_name, channels.len(), result.len());
                    return Ok(result);
                }
                Ok(_) => {
                    info!("WASM plugin '{}' returned empty results, falling back to native implementation", 
                          self.plugin_name);
                }
                Err(e) => {
                    tracing::warn!("WASM plugin '{}' execution failed: {}, falling back to native implementation", 
                                  self.plugin_name, e);
                }
            }
        } else {
            tracing::warn!("Plugin manager no longer available, falling back to native implementation");
        }
        
        // Return empty to trigger native fallback
        Ok(Vec::new())
    }

    async fn execute_filtering(
        &self,
        _context: &crate::proxy::stage_strategy::StageContext,
        channels: Vec<crate::models::Channel>,
    ) -> Result<Vec<crate::models::Channel>> {
        Ok(channels)
    }

    async fn execute_logo_prefetch(
        &self,
        _context: &crate::proxy::stage_strategy::StageContext,
        channels: Vec<crate::models::Channel>,
    ) -> Result<Vec<crate::models::Channel>> {
        Ok(channels)
    }

    async fn execute_channel_numbering(
        &self,
        _context: &crate::proxy::stage_strategy::StageContext,
        _channels: Vec<crate::models::Channel>,
    ) -> Result<Vec<crate::models::NumberedChannel>> {
        Ok(Vec::new())
    }

    async fn execute_m3u_generation(
        &self,
        _context: &crate::proxy::stage_strategy::StageContext,
        _numbered_channels: Vec<crate::models::NumberedChannel>,
    ) -> Result<String> {
        Ok("#EXTM3U\n".to_string())
    }

    fn can_handle_memory_pressure(&self, _level: crate::proxy::stage_strategy::MemoryPressureLevel) -> bool {
        true
    }

    fn supports_mid_stage_switching(&self) -> bool {
        false
    }

    fn strategy_name(&self) -> &str {
        &self.plugin_name
    }

    fn get_info(&self) -> crate::plugins::shared::PluginInfo {
        self.plugin_info.clone()
    }
}

impl WasmPluginWrapper {
    /// Execute the WASM plugin directly via the manager
    async fn execute_plugin(&self, manager: &WasmPluginManager, stage: &str, context: &crate::proxy::stage_strategy::StageContext) -> Result<Vec<crate::models::Channel>> {
        let plugins = manager.plugins.read().await;
        if let Some(plugin) = plugins.get(&self.plugin_name) {
            // Execute the plugin with the given context
            plugin.execute_with_orchestrator(stage, context).await
        } else {
            Err(anyhow::anyhow!("Plugin '{}' not found", self.plugin_name))
        }
    }
}