//! Host interface for WASM plugins
//!
//! This module defines the functions that WASM plugins can call back to the host
//! for memory monitoring, file I/O, networking, logging, etc.

use anyhow::Result;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

/// Log levels that can be passed from WASM plugins
#[derive(Debug, Clone, Copy)]
pub enum PluginLogLevel {
    Error = 1,
    Warn = 2,
    Info = 3,
    Debug = 4,
}

/// Memory pressure levels returned to plugins
#[derive(Debug, Clone, Copy)]
pub enum PluginMemoryPressure {
    Optimal = 1,
    Moderate = 2,
    High = 3,
    Critical = 4,
    Emergency = 5,
}

/// Host interface that WASM plugins can call
/// 
/// These functions are exposed to WASM plugins via the WASM runtime
/// and provide access to host capabilities in a controlled manner.
#[derive(Debug)]
pub struct WasmHostInterface {
    temp_file_manager: sandboxed_file_manager::SandboxedManager,
    memory_monitor: Arc<RwLock<Option<crate::utils::SimpleMemoryMonitor>>>,
    config: Arc<RwLock<HashMap<String, String>>>,
    network_enabled: bool,
}

impl WasmHostInterface {
    pub fn new(
        temp_file_manager: sandboxed_file_manager::SandboxedManager,
        memory_monitor: Option<crate::utils::SimpleMemoryMonitor>,
        network_enabled: bool,
    ) -> Self {
        Self {
            temp_file_manager,
            memory_monitor: Arc::new(RwLock::new(memory_monitor)),
            config: Arc::new(RwLock::new(HashMap::new())),
            network_enabled,
        }
    }

    /// Set configuration values that plugins can query
    pub async fn set_config(&self, config: HashMap<String, String>) {
        let mut cfg = self.config.write().await;
        *cfg = config;
    }

    /// Get current memory usage in bytes
    /// WASM export: `fn host_get_memory_usage() -> u64`
    pub async fn get_memory_usage(&self) -> u64 {
        // In a real implementation, this would query system memory
        // For now, return a mock value based on the memory monitor
        if let Some(ref monitor) = *self.memory_monitor.read().await {
            monitor.get_statistics().peak_mb as u64 * 1024 * 1024
        } else {
            // Fallback: estimate based on current process
            self.estimate_process_memory()
        }
    }

    /// Get memory limit in bytes (if set)
    /// WASM export: `fn host_get_memory_limit() -> u64` (0 = no limit)
    pub async fn get_memory_limit(&self) -> u64 {
        if let Some(ref monitor) = *self.memory_monitor.read().await {
            monitor.memory_limit_mb.unwrap_or(0) as u64 * 1024 * 1024
        } else {
            0 // No limit
        }
    }

    /// Get current memory pressure level
    /// WASM export: `fn host_get_memory_pressure() -> u32`
    pub async fn get_memory_pressure(&self) -> PluginMemoryPressure {
        if let Some(ref monitor) = *self.memory_monitor.read().await {
            match monitor.check_memory_limit() {
                Ok(crate::utils::MemoryLimitStatus::Ok) => PluginMemoryPressure::Optimal,
                Ok(crate::utils::MemoryLimitStatus::Warning) => PluginMemoryPressure::High,
                Ok(crate::utils::MemoryLimitStatus::Exceeded) => PluginMemoryPressure::Critical,
                Err(_) => PluginMemoryPressure::Emergency,
            }
        } else {
            // No monitoring available, assume optimal
            PluginMemoryPressure::Optimal
        }
    }

    /// Create a temporary file for plugin use
    /// WASM export: `fn host_create_temp_file(id_ptr: *const u8, id_len: usize) -> i32` (0 = success)
    pub async fn create_temp_file(&self, id: &str) -> Result<()> {
        debug!("Plugin creating temp file: {}", id);
        // The SandboxedManager handles file creation automatically on write
        Ok(())
    }

    /// Write data to a temporary file
    /// WASM export: `fn host_write_temp_file(id_ptr: *const u8, id_len: usize, data_ptr: *const u8, data_len: usize) -> i32`
    pub async fn write_temp_file(&self, id: &str, data: &[u8]) -> Result<()> {
        debug!("Plugin writing {} bytes to temp file: {}", data.len(), id);
        self.temp_file_manager
            .write(id, data)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to write temp file '{}': {}", id, e))
    }

    /// Read data from a temporary file
    /// WASM export: `fn host_read_temp_file(id_ptr: *const u8, id_len: usize, out_ptr: *mut *mut u8, out_len: *mut usize) -> i32`
    pub async fn read_temp_file(&self, id: &str) -> Result<Vec<u8>> {
        debug!("Plugin reading temp file: {}", id);
        self.temp_file_manager
            .read(id)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to read temp file '{}': {}", id, e))
    }

    /// Delete a temporary file
    /// WASM export: `fn host_delete_temp_file(id_ptr: *const u8, id_len: usize) -> i32`
    pub async fn delete_temp_file(&self, id: &str) -> Result<()> {
        debug!("Plugin deleting temp file: {}", id);
        self.temp_file_manager
            .remove_file(id)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to delete temp file '{}': {}", id, e))
    }

    /// Make an HTTP request (if network access is enabled)
    /// WASM export: `fn host_http_request(method_ptr: *const u8, method_len: usize, url_ptr: *const u8, url_len: usize, body_ptr: *const u8, body_len: usize, out_ptr: *mut *mut u8, out_len: *mut usize) -> i32`
    pub async fn http_request(&self, method: &str, url: &str, _body: &[u8]) -> Result<Vec<u8>> {
        if !self.network_enabled {
            return Err(anyhow::anyhow!("Network access disabled for plugins"));
        }

        debug!("Plugin making {} request to: {}", method, url);
        
        // In a real implementation, this would use reqwest or similar
        // For now, return a mock response
        warn!("HTTP requests not implemented in mock version");
        Ok(b"Mock HTTP response".to_vec())
    }

    /// Log a message from the plugin
    /// WASM export: `fn host_log(level: u32, msg_ptr: *const u8, msg_len: usize)`
    pub fn log(&self, level: PluginLogLevel, message: &str) {
        let prefixed_message = format!("[PLUGIN] {}", message);
        
        match level {
            PluginLogLevel::Error => error!("{}", prefixed_message),
            PluginLogLevel::Warn => warn!("{}", prefixed_message),
            PluginLogLevel::Info => info!("{}", prefixed_message),
            PluginLogLevel::Debug => debug!("{}", prefixed_message),
        }
    }

    /// Report progress from the plugin
    /// WASM export: `fn host_report_progress(stage_ptr: *const u8, stage_len: usize, processed: usize, total: usize)`
    pub fn report_progress(&self, stage: &str, processed: usize, total: usize) {
        let percentage = if total > 0 {
            (processed as f64 / total as f64) * 100.0
        } else {
            0.0
        };
        
        debug!("Plugin progress [{}]: {}/{} ({:.1}%)", stage, processed, total, percentage);
    }

    /// Get a configuration value
    /// WASM export: `fn host_get_config(key_ptr: *const u8, key_len: usize, out_ptr: *mut *mut u8, out_len: *mut usize) -> i32` (0 = found)
    pub async fn get_config(&self, key: &str) -> Option<String> {
        let config = self.config.read().await;
        config.get(key).cloned()
    }

    /// Estimate current process memory usage
    fn estimate_process_memory(&self) -> u64 {
        // In a real implementation, this would use system APIs
        // For now, return a mock value
        256 * 1024 * 1024 // 256MB
    }
}

/// Plugin capability configuration
#[derive(Debug, Clone)]
pub struct PluginCapabilities {
    /// Allow file system access via temp files
    pub allow_file_access: bool,
    /// Allow network requests
    pub allow_network_access: bool,
    /// Maximum memory the plugin can request info about
    pub max_memory_query_mb: Option<usize>,
    /// Allowed configuration keys the plugin can read
    pub allowed_config_keys: Vec<String>,
}

impl Default for PluginCapabilities {
    fn default() -> Self {
        Self {
            allow_file_access: true,
            allow_network_access: false,
            max_memory_query_mb: Some(1024), // 1GB max
            allowed_config_keys: vec![
                "chunk_size".to_string(),
                "compression_level".to_string(),
                "temp_dir".to_string(),
            ],
        }
    }
}

/// Factory for creating configured host interfaces
pub struct WasmHostInterfaceFactory {
    temp_file_manager: sandboxed_file_manager::SandboxedManager,
    capabilities: PluginCapabilities,
}

impl WasmHostInterfaceFactory {
    pub fn new(
        temp_file_manager: sandboxed_file_manager::SandboxedManager,
        capabilities: PluginCapabilities,
    ) -> Self {
        Self {
            temp_file_manager,
            capabilities,
        }
    }

    pub fn create_interface(
        &self,
        memory_monitor: Option<crate::utils::SimpleMemoryMonitor>,
        plugin_config: HashMap<String, String>,
    ) -> WasmHostInterface {
        let mut filtered_config = HashMap::new();
        
        // Filter config to only allowed keys
        for (key, value) in plugin_config {
            if self.capabilities.allowed_config_keys.contains(&key) {
                filtered_config.insert(key, value);
            }
        }

        let interface = WasmHostInterface::new(
            self.temp_file_manager.clone(),
            memory_monitor,
            self.capabilities.allow_network_access,
        );

        // Set the filtered configuration
        tokio::spawn({
            let interface = interface.clone();
            let config = filtered_config.clone();
            async move {
                interface.set_config(config).await;
            }
        });

        interface
    }
}

// Implement Clone for WasmHostInterface to support sharing between plugins
impl Clone for WasmHostInterface {
    fn clone(&self) -> Self {
        Self {
            temp_file_manager: self.temp_file_manager.clone(),
            memory_monitor: self.memory_monitor.clone(),
            config: self.config.clone(),
            network_enabled: self.network_enabled,
        }
    }
}