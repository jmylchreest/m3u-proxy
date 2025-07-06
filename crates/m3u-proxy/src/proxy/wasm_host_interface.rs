//! Temporary compatibility stub for WASM host interface
//! 
//! This module provides temporary stubs for the old WASM host interface
//! to maintain compilation while transitioning to the new plugin architecture.


/// Temporary stub for WasmHostInterface
#[derive(Debug, Clone)]
pub struct WasmHostInterface;

impl WasmHostInterface {
    pub fn new(
        _temp_file_manager: sandboxed_file_manager::SandboxedManager,
        _memory_monitor: Option<crate::utils::SimpleMemoryMonitor>, 
        _network_enabled: bool,
    ) -> Self {
        Self
    }
    
    pub async fn write_temp_file(&self, _file_id: &str, _data: &[u8]) -> anyhow::Result<()> {
        Ok(())
    }
    
    pub async fn read_temp_file(&self, _file_id: &str) -> anyhow::Result<Vec<u8>> {
        Ok(Vec::new())
    }
    
    pub async fn delete_temp_file(&self, _file_id: &str) -> anyhow::Result<()> {
        Ok(())
    }
    
    pub fn get_memory_usage(&self) -> usize {
        0
    }
    
    pub fn get_memory_pressure(&self) -> PluginMemoryPressure {
        PluginMemoryPressure::Optimal
    }
    
    pub fn log(&self, _level: PluginLogLevel, _message: &str) {
        // Stub log function
    }
}

/// Temporary stub for PluginCapabilities
#[derive(Debug, Clone)]
pub struct PluginCapabilities {
    pub allow_file_access: bool,
    pub allow_network_access: bool,
    pub max_memory_query_mb: Option<usize>,
    pub allowed_config_keys: Vec<String>,
}

/// Temporary stub for WasmHostInterfaceFactory
#[derive(Debug, Clone)]
pub struct WasmHostInterfaceFactory;

impl WasmHostInterfaceFactory {
    pub fn new(
        _file_manager: sandboxed_file_manager::SandboxedManager,
        _capabilities: PluginCapabilities,
    ) -> Self {
        Self
    }
    
    pub fn create_interface(&self, _memory_monitor: Option<crate::utils::SimpleMemoryMonitor>, _plugin_config: std::collections::HashMap<String, String>) -> WasmHostInterface {
        WasmHostInterface
    }
}

/// Temporary stub for PluginLogLevel
#[derive(Debug, Clone)]
pub enum PluginLogLevel {
    Error,
    Warn,
    Info,
    Debug,
}

/// Temporary stub for PluginMemoryPressure
#[derive(Debug, Clone)]
pub enum PluginMemoryPressure {
    Optimal,
    Moderate,
    High,
    Critical,
}