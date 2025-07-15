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
    services::ProxyRegenerationService,
    utils::{
        memory_config::{MemoryMonitoringConfig, MemoryVerbosity, init_global_memory_config},
        memory_monitor::SimpleMemoryMonitor,
    },
    web::WebServer,
};
// use std::{collections::HashMap, sync::Arc};
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

    // Initialize logo asset service and storage using config paths
    let logo_asset_storage = LogoAssetStorage::new(
        config.storage.uploaded_logo_path.clone(),
        config.storage.cached_logo_path.clone(),
    );
    let mut logo_asset_service = LogoAssetService::new(database.pool(), logo_asset_storage.clone());

    // Create cache invalidation channel for scheduler
    let (cache_invalidation_tx, cache_invalidation_rx) = create_cache_invalidation_channel();

    info!("Logo asset service and storage initialized");

    // Initialize sandboxed file managers using configuration
    let cache_dir = std::env::temp_dir().join("m3u-proxy");
    let file_manager_config = config
        .file_manager
        .clone()
        .unwrap_or_else(|| m3u_proxy::config::FileManagerConfig::with_defaults(cache_dir.clone()));

    // Validate no duplicate base directories before creating any managers
    let mut all_paths = std::collections::HashMap::new();
    
    // Check file manager config categories
    for category in file_manager_config.category_names() {
        if let Some(path) = file_manager_config.category_path(category) {
            let canonical_path = path.canonicalize().unwrap_or(path.clone());
            if let Some(existing_category) = all_paths.insert(canonical_path.clone(), category.clone()) {
                return Err(anyhow::anyhow!(
                    "Duplicate base directory detected! Categories '{}' and '{}' both use path: {:?}. This would cause cleanup conflicts.",
                    existing_category, category, canonical_path
                ));
            }
        }
    }
    
    // Check logo paths
    let cached_logo_canonical = config.storage.cached_logo_path.canonicalize()
        .unwrap_or(config.storage.cached_logo_path.clone());
    if let Some(existing_category) = all_paths.get(&cached_logo_canonical) {
        return Err(anyhow::anyhow!(
            "Duplicate base directory detected! Category '{}' and logos_cached both use path: {:?}. This would cause cleanup conflicts.",
            existing_category, cached_logo_canonical
        ));
    }

    // Create file managers for each category (now safe from duplicates)
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


    // Create specialized logo file manager for cached logos (duplicate check already done above)
    // Note: uploaded logos use LogoAssetStorage direct file operations (could be enhanced with sandboxing)
    let logos_cached_file_manager = m3u_proxy::config::FileManagerConfig::create_logo_manager(
        &config.storage.cached_logo_path,
        false, // Regular retention with cleanup
    ).await?;

    // Update logo asset service with the cached logo file manager
    logo_asset_service = logo_asset_service.with_file_manager(logos_cached_file_manager.clone());

    let proxy_output_file_manager = create_manager("proxy_output").await?;
    let temp_file_manager = create_manager("temp").await?;

    info!("Sandboxed file managers initialized with configured retention policies:");
    info!(
        "  logos_cached: 3 months retention, cleanup every 12 hours, path: {:?}",
        Some(&config.storage.cached_logo_path)
    );
    info!(
        "  logos_uploaded: never expires (direct file ops via LogoAssetStorage), path: {:?}",
        Some(&config.storage.uploaded_logo_path)
    );
    for category in file_manager_config.category_names() {
        if let Some(config_cat) = file_manager_config.get_category(category) {
            let path = file_manager_config.category_path(category);
            info!(
                "  {}: {} retention, cleanup every {}, path: {:?}",
                category,
                humantime::format_duration(config_cat.retention_duration),
                humantime::format_duration(
                    file_manager_config
                        .cleanup_interval_for_category(category)
                        .unwrap_or_default()
                ),
                path
            );
        }
    }

    // Create shared memory monitor for global memory management
    let _shared_memory_monitor = SimpleMemoryMonitor::new(memory_limit);
    info!(
        "Shared memory monitor initialized with limit: {:?}MB",
        memory_limit
    );

    // Initialize proxy regeneration service with managed temp file manager
    let proxy_regeneration_service =
        ProxyRegenerationService::new(database.pool(), None, temp_file_manager.clone());
    info!("Proxy regeneration service initialized");

    // Create scheduler service now that proxy regeneration service exists
    let scheduler = SchedulerService::new(
        state_manager.clone(),
        database.clone(),
        config.ingestion.run_missed_immediately,
        Some(cache_invalidation_rx),
        Some(proxy_regeneration_service.clone()),
    );
    info!("Scheduler service initialized");

    // Native pipeline only - no plugin system needed

    // Clone values needed for background services before moving into web server
    let bg_proxy_regeneration_service = proxy_regeneration_service.clone();
    let bg_database = database.clone();
    let bg_data_mapping_service = data_mapping_service.clone();
    let bg_logo_asset_service = logo_asset_service.clone();
    let bg_config = config.clone();
    
    // Clone again for metrics housekeeper
    let metrics_database = database.clone();
    let metrics_config = config.clone();

    let web_server = WebServer::new(
        config,
        database,
        state_manager,
        cache_invalidation_tx,
        data_mapping_service,
        logo_asset_service,
        logo_asset_storage,
        proxy_regeneration_service,
        temp_file_manager.clone(), // Use temp for both temp and preview operations
        logos_cached_file_manager,
        proxy_output_file_manager,
    )
    .await?;

    info!(
        "Starting web server on {}:{}",
        web_server.host(),
        web_server.port()
    );

    // Create a channel to signal when the server is ready or fails to bind
    let (server_ready_tx, server_ready_rx) = tokio::sync::oneshot::channel();

    // Get the bind address before moving web_server
    let _bind_addr = format!("{}:{}", web_server.host(), web_server.port());

    // Start the web server in a separate task
    let server_handle = tokio::spawn(async move {
        // This will signal immediately when bind succeeds/fails, then block until shutdown
        if let Err(e) = web_server.serve_with_signal(server_ready_tx).await {
            tracing::error!("Web server failed: {}", e);
        }
    });

    // Wait for the server bind result (success or failure)
    match server_ready_rx.await {
        Ok(Ok(())) => {
            info!("Web server is now listening, starting background services...");
        }
        Ok(Err(bind_error)) => {
            tracing::error!("Failed to bind web server: {}", bind_error);
            return Err(bind_error);
        }
        Err(_) => {
            tracing::error!("Web server task completed without signaling");
            return Err(anyhow::anyhow!("Web server failed to start"));
        }
    }

    // Now start the background services after the web server is listening
    info!("Starting background processor for auto-regeneration");
    tokio::spawn(async move {
        bg_proxy_regeneration_service
            .start_processor(
                bg_database,
                bg_data_mapping_service,
                bg_logo_asset_service,
                bg_config,
            )
            .await;
    });

    info!("Starting scheduler service");
    tokio::spawn(async move {
        if let Err(e) = scheduler.start().await {
            tracing::error!("Scheduler service failed: {}", e);
        }
    });

    // Start metrics housekeeper service if configured
    if let Some(metrics_config_section) = &metrics_config.metrics {
        info!("Starting metrics housekeeper service");
        let housekeeper_db = metrics_database.pool();
        let housekeeper_config = metrics_config_section.clone();
        tokio::spawn(async move {
            match m3u_proxy::services::MetricsHousekeeper::from_config(housekeeper_db, &housekeeper_config) {
                Ok(housekeeper) => {
                    housekeeper.start().await;
                }
                Err(e) => {
                    tracing::error!("Failed to start metrics housekeeper: {}", e);
                }
            }
        });
    } else {
        info!("Metrics housekeeper disabled (no configuration found)");
    }

    info!("All services started successfully");

    // Wait for the server to complete (this will block until shutdown)
    server_handle.await?;

    Ok(())
}
