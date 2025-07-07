use anyhow::Result;
use clap::Parser;
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

// Use the library instead of redeclaring modules
use m3u_proxy::{
    config::Config,
    data_mapping::DataMappingService,
    database::Database,
    ingestor::{
        IngestionStateManager,
        scheduler::{SchedulerService, create_cache_invalidation_channel},
    },
    logo_assets::{LogoAssetService, LogoAssetStorage},
    plugins::pipeline::wasm::{WasmPluginManager, WasmPluginConfig},
    proxy::wasm_host_interface::{WasmHostInterfaceFactory, PluginCapabilities},
    services::ProxyRegenerationService,
    utils::{
        memory_config::{MemoryMonitoringConfig, MemoryVerbosity, init_global_memory_config},
        memory_monitor::SimpleMemoryMonitor,
    },
    web::WebServer,
};
use std::{collections::HashMap, sync::Arc};
use sandboxed_file_manager::SandboxedManager;

#[derive(Parser)]
#[command(name = "m3u-proxy")]
#[command(version = "0.1.0")]
#[command(about = "A modern M3U proxy service with filtering and source management")]
#[command(long_about = None)]
struct Cli {
    /// Configuration file path
    #[arg(short, long, default_value = "config.toml")]
    config: String,

    /// Listening IP address
    #[arg(short = 'H', long, value_name = "IP")]
    host: Option<String>,

    /// Listening port
    #[arg(short, long, value_name = "PORT")]
    port: Option<u16>,

    /// Database URL (overrides config file)
    #[arg(short = 'd', long, value_name = "URL")]
    database_url: Option<String>,

    /// Log level
    #[arg(short = 'v', long, default_value = "info")]
    log_level: String,

    /// Memory monitoring verbosity (silent, minimal, normal, verbose, debug)
    #[arg(long, default_value = "minimal")]
    memory_verbosity: String,

    /// Memory limit in MB
    #[arg(long, value_name = "MB")]
    memory_limit: Option<usize>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialize logging with specified level
    let log_filter = if cli.log_level == "trace" {
        format!("m3u_proxy={},tower_http=trace", cli.log_level)
    } else {
        format!("m3u_proxy={}", cli.log_level)
    };
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| log_filter.into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Parse memory verbosity from CLI or environment
    let memory_verbosity = std::env::var("M3U_PROXY_MEMORY_VERBOSITY")
        .unwrap_or_else(|_| cli.memory_verbosity.clone());

    let memory_verbosity = MemoryVerbosity::from_str(&memory_verbosity).unwrap_or_else(|_| {
        eprintln!(
            "Warning: Invalid memory verbosity '{}', using 'minimal'",
            memory_verbosity
        );
        MemoryVerbosity::Minimal
    });

    let memory_limit = cli.memory_limit.or_else(|| {
        std::env::var("M3U_PROXY_MEMORY_LIMIT")
            .ok()
            .and_then(|s| s.parse().ok())
    });

    // Create memory monitoring configuration
    let mut memory_config = MemoryMonitoringConfig::default();
    memory_config.verbosity = memory_verbosity;
    memory_config.memory_limit_mb = memory_limit;

    info!("Starting M3U Proxy Service v{}", env!("CARGO_PKG_VERSION"));
    info!(
        "Memory monitoring: verbosity={}, limit={:?}MB",
        memory_config.verbosity.as_str(),
        memory_config.memory_limit_mb
    );

    // Initialize global memory configuration
    init_global_memory_config(memory_config.clone());

    // Load configuration from specified file
    let mut config = Config::load_from_file(&cli.config)?;
    info!("Configuration loaded from: {}", cli.config);

    // Override config with CLI arguments
    if let Some(host) = cli.host {
        config.web.host = host;
    }
    if let Some(port) = cli.port {
        config.web.port = port;
    }
    if let Some(database_url) = cli.database_url {
        config.database.url = database_url;
    }

    info!("Using database: {}", config.database.url);

    let database = Database::new(&config.database, &config.ingestion).await?;
    database.migrate().await?;
    info!("Database connection established and migrations applied");

    // Create state manager for ingestion progress tracking
    let state_manager = IngestionStateManager::new();
    info!("Ingestion state manager initialized");

    // Initialize data mapping service
    let data_mapping_service = DataMappingService::new(database.pool());
    info!("Data mapping service initialized");

    // Initialize logo asset service and storage
    let logo_asset_storage = LogoAssetStorage::new(
        config.storage.uploaded_logo_path.clone(),
        config.storage.cached_logo_path.clone(),
    );
    let logo_asset_service = LogoAssetService::new(database.pool());

    // Create cache invalidation channel for scheduler
    let (cache_invalidation_tx, cache_invalidation_rx) = create_cache_invalidation_channel();

    // Initialize proxy regeneration service
    let proxy_regeneration_service = ProxyRegenerationService::new(database.pool(), None);
    info!("Proxy regeneration service initialized");

    // Start the background processor for auto-regeneration
    let regeneration_processor = proxy_regeneration_service.clone();
    let regeneration_database = database.clone();
    let regeneration_data_mapping = data_mapping_service.clone();
    let regeneration_logo_service = logo_asset_service.clone();
    let regeneration_config = config.clone();
    tokio::spawn(async move {
        regeneration_processor
            .start_processor(
                regeneration_database,
                regeneration_data_mapping,
                regeneration_logo_service,
                regeneration_config,
            )
            .await;
    });

    // Start scheduler service
    let scheduler = SchedulerService::new(
        state_manager.clone(),
        database.clone(),
        config.ingestion.run_missed_immediately,
        Some(cache_invalidation_rx),
        Some(proxy_regeneration_service.clone()),
    );

    tokio::spawn(async move {
        if let Err(e) = scheduler.start().await {
            tracing::error!("Scheduler service failed: {}", e);
        }
    });
    info!("Logo asset service and storage initialized");

    // Initialize sandboxed file managers using configuration
    let cache_dir = std::env::temp_dir().join("m3u-proxy");
    let file_manager_config = config
        .file_manager
        .clone()
        .unwrap_or_else(|| m3u_proxy::config::FileManagerConfig::with_defaults(cache_dir.clone()));

    // Create file managers for each category
    let create_manager = |category: &str| {
        let category = category.to_string(); // Take ownership
        let config = file_manager_config.clone();
        async move {
            if let (Some(policy), Some(interval)) = (
                config.cleanup_policy_for_category(&category),
                config.cleanup_interval_for_category(&category),
            ) {
                if let Some(path) = config.category_path(&category) {
                    SandboxedManager::builder()
                        .base_directory(path)
                        .cleanup_policy(policy)
                        .cleanup_interval(interval)
                        .build()
                        .await
                        .map_err(|e| anyhow::anyhow!("Failed to create file manager: {}", e))
                } else {
                    Err(anyhow::anyhow!(
                        "No path configured for category: {}",
                        category
                    ))
                }
            } else {
                Err(anyhow::anyhow!(
                    "No configuration found for category: {}",
                    category
                ))
            }
        }
    };

    let preview_file_manager = create_manager("preview").await?;
    let logo_file_manager = create_manager("logo").await?;
    let proxy_output_file_manager = create_manager("proxy_output").await?;

    info!("Sandboxed file managers initialized with configured retention policies:");
    for category in file_manager_config.category_names() {
        if let Some(config_cat) = file_manager_config.get_category(category) {
            info!(
                "  {}: {} retention, cleanup every {}",
                category,
                humantime::format_duration(config_cat.retention_duration),
                humantime::format_duration(
                    file_manager_config
                        .cleanup_interval_for_category(category)
                        .unwrap_or_default()
                )
            );
        }
    }

    // Initialize shared WASM plugin system if enabled
    let shared_plugin_manager = if let Some(wasm_config) = &config.wasm_plugins {
        if wasm_config.enabled {
            info!("Initializing shared WASM plugin system");
            
            // Use default values for optional config fields
            let default_dir = std::path::PathBuf::from("./target/wasm-plugins");
            let plugin_directory = wasm_config.plugin_directory.as_ref().unwrap_or(&default_dir);
            let timeout_seconds = wasm_config.timeout_seconds.unwrap_or(30);
            let enable_hot_reload = wasm_config.enable_hot_reload.unwrap_or(false);
            
            info!("Plugin directory: {:?}", plugin_directory);
            info!("Plugin timeout: {} seconds", timeout_seconds);
            info!("Hot reload: {}", if enable_hot_reload { "ENABLED" } else { "DISABLED" });

            // Create memory monitor for plugins (memory pressure is handled by plugins now)
            let memory_monitor = SimpleMemoryMonitor::new(Some(512)); // Default 512MB limit

            // Create plugin capabilities
            let capabilities = PluginCapabilities {
                allow_file_access: true,
                allow_network_access: false,
                max_memory_query_mb: Some(512), // Default memory limit
                allowed_config_keys: vec![
                    "chunk_size".to_string(),
                    "compression_level".to_string(),
                    "temp_dir".to_string(),
                    "memory_threshold_mb".to_string(),
                    "temp_file_threshold".to_string(),
                ],
            };

            // Create host interface factory
            let host_interface_factory = WasmHostInterfaceFactory::new(preview_file_manager.clone(), capabilities);

            // Create plugin configuration (memory pressure handled by plugin itself)
            let plugin_config = HashMap::from([
                ("chunk_size".to_string(), "1000".to_string()),
                ("memory_threshold_mb".to_string(), "512".to_string()), // Default threshold
                ("temp_file_threshold".to_string(), "10000".to_string()),
            ]);

            // Create host interface
            let host_interface = host_interface_factory.create_interface(Some(memory_monitor), plugin_config);

            // Create plugin manager configuration
            let plugin_manager_config = WasmPluginConfig {
                enabled: wasm_config.enabled,
                plugin_directory: plugin_directory.to_string_lossy().to_string(),
                max_memory_per_plugin: 512, // Default memory limit (plugins manage themselves)
                timeout_seconds,
                enable_hot_reload,
                max_plugin_failures: 3,
                fallback_timeout_ms: 5000,
            };

            // Create shared plugin manager
            let manager = Arc::new(WasmPluginManager::new(plugin_manager_config, host_interface));

            // Load plugins once at startup
            match manager.load_plugins().await {
                Ok(()) => {
                    match manager.get_detailed_statistics() {
                        Ok(stats) => {
                            info!("Shared plugin system initialized successfully!");
                            info!("Plugin Statistics:");
                            info!("   Total plugins loaded: {}", stats.len());
                            for (plugin_name, plugin_stats) in stats {
                                info!("   Plugin '{}': {:?}", plugin_name, plugin_stats);
                            }
                        }
                        Err(e) => {
                            tracing::warn!("Failed to get plugin statistics: {}", e);
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("Failed to load plugins in shared plugin manager: {}", e);
                }
            }

            Some(manager)
        } else {
            info!("WASM plugin system is DISABLED");
            None
        }
    } else {
        info!("WASM plugin system not configured");
        None
    };

    let web_server = WebServer::new(
        config,
        database,
        state_manager,
        cache_invalidation_tx,
        data_mapping_service,
        logo_asset_service,
        logo_asset_storage,
        proxy_regeneration_service,
        preview_file_manager,
        logo_file_manager,
        proxy_output_file_manager,
        shared_plugin_manager,
    )
    .await?;

    info!(
        "Starting web server on {}:{}",
        web_server.host(),
        web_server.port()
    );
    web_server.serve().await?;

    Ok(())
}
