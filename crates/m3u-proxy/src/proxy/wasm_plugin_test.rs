//! Test utilities and integration tests for WASM plugins
//!
//! This module provides test implementations and utilities for validating
//! the WASM plugin system functionality.

#[cfg(test)]
mod tests {
    use crate::proxy::stage_strategy::{MemoryPressureLevel, StageStrategy};
    use crate::proxy::wasm_host_interface::{
        PluginCapabilities, WasmHostInterface, WasmHostInterfaceFactory,
    };
    use crate::plugins::pipeline::wasm::{WasmPlugin, WasmPluginConfig, WasmPluginManager};
    use crate::utils::SimpleMemoryMonitor;
    use sandboxed_file_manager::SandboxedManager;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use tempfile::TempDir;
    use tokio;

    /// Create a test host interface
    async fn create_test_host_interface() -> WasmHostInterface {
        let temp_dir = TempDir::new().unwrap();
        let temp_manager = SandboxedManager::builder()
            .base_directory(temp_dir.path().to_path_buf())
            .build()
            .await
            .unwrap();
        let memory_monitor = SimpleMemoryMonitor::new(Some(512));

        WasmHostInterface::new(
            temp_manager,
            Some(memory_monitor),
            false, // Network disabled for tests
        )
    }

    /// Create a test plugin configuration
    fn create_test_plugin_config() -> WasmPluginConfig {
        WasmPluginConfig {
            enabled: true,
            plugin_directory: "./test_plugins".to_string(),
            max_memory_per_plugin: 64,
            timeout_seconds: 10,
            enable_hot_reload: false,
            max_plugin_failures: 3,
            fallback_timeout_ms: 1000,
        }
    }

    #[tokio::test]
    async fn test_wasm_plugin_creation() {
        let host_interface = create_test_host_interface().await;
        let plugin_config = HashMap::new();
        let module_path = PathBuf::from("test_plugin.wasm");

        let plugin = WasmPlugin::new(module_path, host_interface, plugin_config).await;

        assert!(plugin.is_ok());
        let plugin = plugin.unwrap();
        assert_eq!(plugin.get_info().name, "test_plugin");
        assert!(!plugin.is_initialized());
    }

    #[tokio::test]
    async fn test_wasm_plugin_initialization_missing_file() {
        let host_interface = create_test_host_interface().await;
        let plugin_config = HashMap::new();
        let module_path = PathBuf::from("non_existent_plugin.wasm");

        let mut plugin = WasmPlugin::new(module_path, host_interface, plugin_config)
            .await
            .unwrap();
        let result = plugin.initialize().await;

        assert!(result.is_err());
        assert!(!plugin.is_initialized());
    }

    #[tokio::test]
    async fn test_wasm_plugin_memory_pressure_handling() {
        let host_interface = create_test_host_interface().await;
        let plugin_config = HashMap::new();
        let module_path = PathBuf::from("test_plugin.wasm");

        let plugin = WasmPlugin::new(module_path, host_interface, plugin_config)
            .await
            .unwrap();

        // Test different memory pressure levels
        assert!(plugin.can_handle_memory_pressure(MemoryPressureLevel::Optimal));
        assert!(plugin.can_handle_memory_pressure(MemoryPressureLevel::Moderate));
        assert!(plugin.can_handle_memory_pressure(MemoryPressureLevel::High));
        assert!(plugin.can_handle_memory_pressure(MemoryPressureLevel::Critical));
        assert!(plugin.can_handle_memory_pressure(MemoryPressureLevel::Emergency));
    }

    #[tokio::test]
    async fn test_wasm_plugin_stage_support() {
        let host_interface = create_test_host_interface().await;
        let plugin_config = HashMap::new();
        let module_path = PathBuf::from("test_plugin.wasm");

        let plugin = WasmPlugin::new(module_path, host_interface, plugin_config)
            .await
            .unwrap();

        assert!(plugin.supports_stage("source_loading"));
        assert!(!plugin.supports_stage("unknown_stage"));
    }

    #[tokio::test]
    async fn test_wasm_plugin_manager_creation() {
        let host_interface = create_test_host_interface().await;
        let config = create_test_plugin_config();

        let manager = WasmPluginManager::new(config, host_interface);

        let stats = manager.get_statistics().await;
        assert_eq!(
            stats.get("total_plugins").unwrap(),
            &serde_json::Value::from(0)
        );
        assert_eq!(
            stats.get("enabled").unwrap(),
            &serde_json::Value::from(true)
        );
    }

    #[tokio::test]
    async fn test_wasm_plugin_manager_load_plugins_no_directory() {
        let host_interface = create_test_host_interface().await;
        let config = create_test_plugin_config();

        let manager = WasmPluginManager::new(config, host_interface);

        // Should succeed even if directory doesn't exist
        let result = manager.load_plugins().await;
        assert!(result.is_ok());

        let stats = manager.get_statistics().await;
        assert_eq!(
            stats.get("total_plugins").unwrap(),
            &serde_json::Value::from(0)
        );
    }

    #[tokio::test]
    async fn test_wasm_plugin_manager_disabled() {
        let host_interface = create_test_host_interface().await;
        let mut config = create_test_plugin_config();
        config.enabled = false;

        let manager = WasmPluginManager::new(config, host_interface);

        let result = manager.load_plugins().await;
        assert!(result.is_ok());

        let stats = manager.get_statistics().await;
        assert_eq!(
            stats.get("enabled").unwrap(),
            &serde_json::Value::from(false)
        );
    }

    #[tokio::test]
    async fn test_wasm_plugin_manager_health_check() {
        let host_interface = create_test_host_interface().await;
        let config = create_test_plugin_config();

        let manager = WasmPluginManager::new(config, host_interface);

        let health = manager.health_check().await;
        assert!(health.is_empty()); // No plugins loaded
    }

    #[tokio::test]
    async fn test_wasm_plugin_manager_get_plugin_for_stage() {
        let host_interface = create_test_host_interface().await;
        let config = create_test_plugin_config();

        let manager = WasmPluginManager::new(config, host_interface);

        let plugin = manager
            .get_plugin_for_stage("source_loading", MemoryPressureLevel::Optimal)
            .await;
        assert!(plugin.is_none()); // No plugins loaded
    }

    #[tokio::test]
    async fn test_wasm_plugin_stage_strategy_execution() {
        let host_interface = create_test_host_interface().await;
        let plugin_config = HashMap::new();
        let module_path = PathBuf::from("test_plugin.wasm");

        let plugin = WasmPlugin::new(module_path, host_interface, plugin_config)
            .await
            .unwrap();

        use crate::models::{GenerationOutput, ResolvedProxyConfig};
        use crate::proxy::stage_strategy::StageContext;
        use uuid::Uuid;

        // Create a minimal context for testing
        let proxy_config = ResolvedProxyConfig {
            proxy: crate::models::StreamProxy {
                id: Uuid::new_v4(),
                ulid: "test-ulid".to_string(),
                name: "Test Proxy".to_string(),
                description: None,
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
                last_generated_at: None,
                is_active: true,
                auto_regenerate: false,
                proxy_mode: crate::models::StreamProxyMode::Proxy,
                upstream_timeout: None,
                buffer_size: None,
                max_concurrent_streams: None,
                starting_channel_number: 1,
            },
            sources: vec![],
            filters: vec![],
            epg_sources: vec![],
        };

        let context = StageContext {
            proxy_config: proxy_config.clone(),
            output: GenerationOutput::InMemory,
            base_url: "http://localhost:8080".to_string(),
            engine_config: None,
            memory_pressure: MemoryPressureLevel::Optimal,
            available_memory_mb: Some(256),
            current_stage: "test_stage".to_string(),
            stats: crate::models::GenerationStats::new("test".to_string()),
            database: None,
            logo_service: None,
            iterator_registry: None,
        };

        // Test source loading execution
        let source_ids = vec![Uuid::new_v4()];
        let result = plugin.execute_source_loading(&context, source_ids).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 0); // Returns empty since plugin isn't initialized

        // Test data mapping execution
        let channels = vec![];
        let result = plugin.execute_data_mapping(&context, channels).await;
        assert!(result.is_ok());

        // Test filtering execution
        let channels = vec![];
        let result = plugin.execute_filtering(&context, channels).await;
        assert!(result.is_ok());

        // Test channel numbering execution
        let channels = vec![];
        let result = plugin.execute_channel_numbering(&context, channels).await;
        assert!(result.is_ok());

        // Test M3U generation execution
        let numbered_channels = vec![];
        let result = plugin
            .execute_m3u_generation(&context, numbered_channels)
            .await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "#EXTM3U\n");
    }

    #[tokio::test]
    async fn test_wasm_plugin_strategy_traits() {
        let host_interface = create_test_host_interface().await;
        let plugin_config = HashMap::new();
        let module_path = PathBuf::from("test_plugin.wasm");

        let plugin = WasmPlugin::new(module_path, host_interface, plugin_config)
            .await
            .unwrap();

        // Test strategy traits
        assert!(plugin.can_handle_memory_pressure(MemoryPressureLevel::Optimal));
        assert!(plugin.supports_mid_stage_switching());
        assert_eq!(plugin.strategy_name(), "test_plugin");

    }

    #[tokio::test]
    async fn test_wasm_host_interface_factory() {
        let temp_dir = TempDir::new().unwrap();
        let temp_manager = SandboxedManager::builder()
            .base_directory(temp_dir.path().to_path_buf())
            .build()
            .await
            .unwrap();
        let capabilities = PluginCapabilities::default();

        let factory = WasmHostInterfaceFactory::new(temp_manager, capabilities);

        let memory_monitor = SimpleMemoryMonitor::new(Some(512));
        let plugin_config = HashMap::new();

        let interface = factory.create_interface(Some(memory_monitor), plugin_config);

        // Test interface creation
        let memory_usage = interface.get_memory_usage().await;
        assert!(memory_usage > 0);

        let memory_pressure = interface.get_memory_pressure().await;
        assert!(matches!(
            memory_pressure,
            crate::proxy::wasm_host_interface::PluginMemoryPressure::Optimal
        ));
    }

    #[tokio::test]
    async fn test_wasm_host_interface_temp_file_operations() {
        let host_interface = create_test_host_interface().await;

        // Test temp file creation and writing
        let file_id = "test_file";
        let test_data = b"Hello, WASM plugin!";

        let result = host_interface.create_temp_file(file_id).await;
        assert!(result.is_ok());

        let result = host_interface.write_temp_file(file_id, test_data).await;
        assert!(result.is_ok());

        let result = host_interface.read_temp_file(file_id).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), test_data);

        let result = host_interface.delete_temp_file(file_id).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_wasm_host_interface_logging() {
        let host_interface = create_test_host_interface().await;

        // Test different log levels
        use crate::proxy::wasm_host_interface::PluginLogLevel;

        host_interface.log(PluginLogLevel::Debug, "Debug message");
        host_interface.log(PluginLogLevel::Info, "Info message");
        host_interface.log(PluginLogLevel::Warn, "Warning message");
        host_interface.log(PluginLogLevel::Error, "Error message");

        // Test progress reporting
        host_interface.report_progress("test_stage", 50, 100);
        host_interface.report_progress("test_stage", 100, 100);
    }

    #[tokio::test]
    async fn test_wasm_host_interface_configuration() {
        let host_interface = create_test_host_interface().await;

        let mut config = HashMap::new();
        config.insert("chunk_size".to_string(), "1000".to_string());
        config.insert("compression_level".to_string(), "6".to_string());

        host_interface.set_config(config).await;

        let value = host_interface.get_config("chunk_size").await;
        assert_eq!(value, Some("1000".to_string()));

        let value = host_interface.get_config("compression_level").await;
        assert_eq!(value, Some("6".to_string()));

        let value = host_interface.get_config("nonexistent").await;
        assert_eq!(value, None);
    }

    #[tokio::test]
    async fn test_wasm_host_interface_network_disabled() {
        let host_interface = create_test_host_interface().await;

        let result = host_interface
            .http_request("GET", "https://example.com", b"")
            .await;
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Network access disabled")
        );
    }

    #[tokio::test]
    async fn test_wasm_plugin_manager_clone() {
        let host_interface = create_test_host_interface().await;
        let config = create_test_plugin_config();

        let manager1 = WasmPluginManager::new(config, host_interface);
        let manager2 = manager1.clone();

        // Both managers should have the same configuration
        let stats1 = manager1.get_statistics().await;
        let stats2 = manager2.get_statistics().await;

        assert_eq!(stats1.get("enabled"), stats2.get("enabled"));
        assert_eq!(stats1.get("total_plugins"), stats2.get("total_plugins"));
    }

    #[tokio::test]
    async fn test_wasm_plugin_health_status() {
        let host_interface = create_test_host_interface().await;
        let plugin_config = HashMap::new();
        let module_path = PathBuf::from("test_plugin.wasm");

        let plugin = WasmPlugin::new(module_path, host_interface, plugin_config)
            .await
            .unwrap();

        // Initially healthy
        assert!(plugin.is_healthy());
        assert_eq!(plugin.get_failure_count(), 0);
    }
}
