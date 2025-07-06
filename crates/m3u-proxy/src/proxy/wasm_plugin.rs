//! Temporary compatibility stub for WASM plugin
//! 
//! This module provides temporary stubs for the old WASM plugin interface
//! to maintain compilation while transitioning to the new plugin architecture.

use std::collections::HashMap;
use std::sync::Arc;

/// Temporary stub for WasmPlugin
#[derive(Debug)]
pub struct WasmPlugin;

impl WasmPlugin {
    pub fn get_info(&self) -> PluginInfo {
        PluginInfo {
            name: "stub".to_string(),
            version: "0.1.0".to_string(),
            author: "stub".to_string(),
            license: "MIT".to_string(),
            description: "Temporary stub - plugins discovered but not executed".to_string(),
            supported_stages: vec!["source_loading".to_string()],
        }
    }
    
    pub async fn execute_with_context(&self, _context: crate::proxy::stage_strategy::StageContext) -> anyhow::Result<Vec<crate::models::Channel>> {
        // Stub implementation - should not be called
        tracing::warn!("Stub WASM plugin execute_with_context called - this should fall back to native strategies");
        Ok(Vec::new())
    }
    
    pub async fn execute_source_loading(&self, _context: &crate::proxy::stage_strategy::StageContext, _source_ids: Vec<uuid::Uuid>) -> anyhow::Result<Vec<crate::models::Channel>> {
        tracing::warn!("Stub WASM plugin execute_source_loading called - this should fall back to native strategies");
        Ok(Vec::new())
    }
    
    pub async fn execute_data_mapping(&self, _context: &crate::proxy::stage_strategy::StageContext, _channels: Vec<crate::models::Channel>) -> anyhow::Result<Vec<crate::models::Channel>> {
        tracing::warn!("Stub WASM plugin execute_data_mapping called - this should fall back to native strategies");
        Ok(Vec::new())
    }
    
    pub async fn execute_filtering(&self, _context: &crate::proxy::stage_strategy::StageContext, _channels: Vec<crate::models::Channel>) -> anyhow::Result<Vec<crate::models::Channel>> {
        tracing::warn!("Stub WASM plugin execute_filtering called - this should fall back to native strategies");
        Ok(Vec::new())
    }
    
    pub async fn execute_channel_numbering(&self, _context: &crate::proxy::stage_strategy::StageContext, _channels: Vec<crate::models::Channel>) -> anyhow::Result<Vec<crate::models::NumberedChannel>> {
        tracing::warn!("Stub WASM plugin execute_channel_numbering called - this should fall back to native strategies");
        Ok(Vec::new())
    }
    
    pub async fn execute_m3u_generation(&self, _context: &crate::proxy::stage_strategy::StageContext, _channels: Vec<crate::models::NumberedChannel>) -> anyhow::Result<String> {
        tracing::warn!("Stub WASM plugin execute_m3u_generation called - this should fall back to native strategies");
        Ok(String::new())
    }
}

/// Temporary stub for WasmPluginConfig  
#[derive(Debug, Clone)]
pub struct WasmPluginConfig {
    pub enabled: bool,
    pub plugin_directory: String,
    pub max_memory_per_plugin: usize,
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

/// Temporary stub for WasmPluginManager
#[derive(Debug)]
pub struct WasmPluginManager;

impl WasmPluginManager {
    pub fn new(
        _config: WasmPluginConfig,
        _host_interface: super::wasm_host_interface::WasmHostInterface,
    ) -> Self {
        Self
    }
    
    pub async fn load_plugins(&self) -> anyhow::Result<()> {
        use std::path::PathBuf;
        
        let plugin_dir = PathBuf::from("./target/wasm-plugins");
        if !plugin_dir.exists() {
            tracing::info!("Plugin directory does not exist: {}", plugin_dir.display());
            return Ok(());
        }
        
        let mut loaded_count = 0;
        
        // Look for .wasm files in the plugin directory
        if let Ok(entries) = std::fs::read_dir(&plugin_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|s| s.to_str()) == Some("wasm") {
                    let name = path.file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("unknown")
                        .to_string();
                    
                    tracing::info!("Found WASM plugin: {} at {}", name, path.display());
                    loaded_count += 1;
                }
            }
        }
        
        tracing::info!("WASM plugin manager (stub) discovered {} plugins", loaded_count);
        Ok(())
    }
    
    pub async fn get_loaded_plugin_count(&self) -> usize {
        use std::path::PathBuf;
        
        let plugin_dir = PathBuf::from("./target/wasm-plugins");
        if !plugin_dir.exists() {
            return 0;
        }
        
        std::fs::read_dir(&plugin_dir)
            .map(|entries| {
                entries.flatten()
                    .filter(|entry| {
                        entry.path().extension().and_then(|s| s.to_str()) == Some("wasm")
                    })
                    .count()
            })
            .unwrap_or(0)
    }
    
    pub async fn health_check(&self) -> HashMap<String, bool> {
        use std::path::PathBuf;
        let mut health = HashMap::new();
        
        let plugin_dir = PathBuf::from("./target/wasm-plugins");
        if !plugin_dir.exists() {
            return health;
        }
        
        if let Ok(entries) = std::fs::read_dir(&plugin_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|s| s.to_str()) == Some("wasm") {
                    let name = path.file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("unknown")
                        .to_string();
                    
                    // For now, consider a plugin healthy if the .wasm file exists
                    health.insert(name, true);
                }
            }
        }
        
        health
    }
    
    pub async fn get_detailed_statistics(&self) -> anyhow::Result<HashMap<String, serde_json::Value>> {
        use std::path::PathBuf;
        let mut stats = HashMap::new();
        
        let plugin_dir = PathBuf::from("./target/wasm-plugins");
        if !plugin_dir.exists() {
            return Ok(stats);
        }
        
        if let Ok(entries) = std::fs::read_dir(&plugin_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|s| s.to_str()) == Some("wasm") {
                    let name = path.file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("unknown")
                        .to_string();
                    
                    let plugin_stats = serde_json::json!({
                        "name": name,
                        "version": "unknown",
                        "path": path.display().to_string(),
                        "loaded": false,
                        "status": "stub - not actually loaded"
                    });
                    stats.insert(name, plugin_stats);
                }
            }
        }
        
        Ok(stats)
    }
    
    pub async fn start_hot_reload_monitoring(&self) -> anyhow::Result<()> {
        Ok(())
    }
    
    pub async fn get_plugin_for_stage(&self, stage: &str, _memory_pressure: crate::proxy::stage_strategy::MemoryPressureLevel) -> Option<WasmPlugin> {
        use std::path::PathBuf;
        
        let plugin_dir = PathBuf::from("./target/wasm-plugins");
        if !plugin_dir.exists() {
            return None;
        }
        
        // Check if we have plugins that support this stage
        if let Ok(entries) = std::fs::read_dir(&plugin_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|s| s.to_str()) == Some("wasm") {
                    // Check TOML file for supported stages
                    let toml_path = path.with_extension("toml");
                    if toml_path.exists() {
                        if let Ok(content) = std::fs::read_to_string(&toml_path) {
                            if let Ok(manifest) = toml::from_str::<toml::Value>(&content) {
                                let supported_stages = manifest
                                    .get("capabilities")
                                    .and_then(|caps| caps.get("stages"))
                                    .and_then(|v| v.as_array())
                                    .map(|arr| {
                                        arr.iter()
                                            .filter_map(|v| v.as_str())
                                            .collect::<Vec<_>>()
                                    });
                                
                                if let Some(stages) = supported_stages {
                                    if stages.contains(&stage) {
                                        let name = path.file_stem()
                                            .and_then(|s| s.to_str())
                                            .unwrap_or("unknown")
                                            .to_string();
                                        
                                        tracing::info!("Selected WASM plugin '{}' for stage '{}' (capabilities verified)", name, stage);
                                        
                                        // Return stub plugin - actual execution will fall back to native strategies
                                        return Some(WasmPlugin);
                                    }
                                }
                            }
                        }
                    } else {
                        // If no TOML, assume the plugin supports common stages
                        let common_stages = ["source_loading", "data_mapping", "filtering"];
                        if common_stages.contains(&stage) {
                            let name = path.file_stem()
                                .and_then(|s| s.to_str())
                                .unwrap_or("unknown")
                                .to_string();
                            
                            tracing::info!("Selected WASM plugin '{}' for stage '{}' (fallback - no manifest)", name, stage);
                            return Some(WasmPlugin);
                        }
                    }
                }
            }
        }
        
        tracing::debug!("No plugin found for stage '{}'", stage);
        None
    }
    
    pub async fn get_statistics(&self) -> HashMap<String, serde_json::Value> {
        HashMap::new()
    }
    
    pub async fn reload_plugins(&self) -> anyhow::Result<()> {
        Ok(())
    }
}

/// Temporary stub for PluginInfo
#[derive(Debug, Clone)]
pub struct PluginInfo {
    pub name: String,
    pub version: String,
    pub author: String,
    pub license: String,
    pub description: String,
    pub supported_stages: Vec<String>,
}

/// Extract plugin info from TOML manifest
pub fn extract_plugin_info(path: &std::path::Path) -> anyhow::Result<Option<PluginInfo>> {
    let toml_path = path.with_extension("toml");
    if !toml_path.exists() {
        return Ok(None);
    }
    
    let content = std::fs::read_to_string(&toml_path)?;
    let manifest: toml::Value = toml::from_str(&content)?;
    
    // Extract plugin metadata
    let plugin_section = manifest.get("plugin");
    let capabilities_section = manifest.get("capabilities");
    
    let name = plugin_section
        .and_then(|p| p.get("name"))
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();
    
    let version = plugin_section
        .and_then(|p| p.get("version"))
        .and_then(|v| v.as_str())
        .unwrap_or("0.0.1")
        .to_string();
    
    let author = plugin_section
        .and_then(|p| p.get("author"))
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();
    
    let license = plugin_section
        .and_then(|p| p.get("license"))
        .and_then(|v| v.as_str())
        .unwrap_or("MIT")
        .to_string();
    
    let description = plugin_section
        .and_then(|p| p.get("description"))
        .and_then(|v| v.as_str())
        .unwrap_or("WASM plugin")
        .to_string();
    
    let supported_stages = capabilities_section
        .and_then(|caps| caps.get("stages"))
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str())
                .map(|s| s.to_string())
                .collect()
        })
        .unwrap_or_else(|| vec!["source_loading".to_string()]);
    
    Ok(Some(PluginInfo {
        name,
        version,
        author,
        license,
        description,
        supported_stages,
    }))
}