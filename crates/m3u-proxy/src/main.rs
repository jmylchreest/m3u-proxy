use anyhow::Result;
use clap::Parser;
use std::time::Duration;
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use serde_json::Value;

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
    services::{ProxyRegenerationService, StreamSourceBusinessService, EpgSourceService},
    utils::{
        SystemManager,
    },
    web::WebServer,
};
// use std::{collections::HashMap, sync::Arc};
use sandboxed_file_manager::SandboxedManager;

/// Get dependencies from SBOM
fn get_dependencies() -> Result<Value, Box<dyn std::error::Error>> {
    let sbom_str = include_str!(concat!(env!("OUT_DIR"), "/sbom.json"));
    let sbom: Value = serde_json::from_str(sbom_str)?;
    Ok(sbom)
}

/// Print detailed version information including dependency versions
fn print_version_info() {
    println!("{} v{}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
    println!("{}", env!("CARGO_PKG_DESCRIPTION"));
    println!();
    println!("Build Information:");
    println!("  Target: {}-{}", std::env::consts::ARCH, std::env::consts::OS);
    if let Ok(rustc_version) = std::env::var("RUSTC_VERSION") {
        println!("  Rust: {}", rustc_version);
    }
    println!();
    println!("Software Bill of Materials:");
    
    match get_dependencies() {
        Ok(sbom) => {
            let mut dependencies = Vec::new();
            
            // Parse SPDX JSON format
            if let Some(packages) = sbom["packages"].as_array() {
                for package in packages {
                    if let (Some(name), Some(version)) = (
                        package["name"].as_str(),
                        package["versionInfo"].as_str()
                    ) {
                        // Skip our own package and path dependencies
                        if name != env!("CARGO_PKG_NAME") && !version.contains("path+") {
                            dependencies.push((name.to_string(), version.to_string()));
                        }
                    }
                }
            }
            
            // Sort dependencies alphabetically for consistent output
            dependencies.sort_by(|a, b| a.0.cmp(&b.0));
            
            if dependencies.is_empty() {
                println!("  (No external components found in SBOM)");
            } else {
                for (name, version) in dependencies {
                    println!("  {}: {}", name, version);
                }
            }
        }
        Err(_) => {
            println!("  (Unable to read SBOM data)");
        }
    }
    
    println!();
    println!("Repository: {}", env!("CARGO_PKG_REPOSITORY"));
    println!("License: {}", env!("CARGO_PKG_LICENSE"));
    println!("Authors: {}", env!("CARGO_PKG_AUTHORS"));
}

#[derive(Parser)]
#[command(name = "m3u-proxy")]
#[command(version = env!("CARGO_PKG_VERSION"))]
#[command(about = "A modern M3U proxy service with filtering and source management")]
#[command(long_about = None)]
#[command(disable_version_flag = true)]
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
    #[arg(short = 'l', long, default_value = "info")]
    log_level: String,



    /// Print version information including dependency versions
    #[arg(short = 'v', long)]
    version: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Handle version flag
    if cli.version {
        print_version_info();
        return Ok(());
    }

    // Initialize logging with specified level and log capture for SSE streaming
    let log_filter = if cli.log_level == "trace" {
        format!("m3u_proxy={},tower_http=trace", cli.log_level)
    } else {
        format!("m3u_proxy={}", cli.log_level)
    };
    
    // Set up log capture layer for SSE streaming
    let (log_capture_layer, log_broadcaster) = m3u_proxy::utils::log_capture::setup_log_capture_with_subscriber();
    
    // Set up reloadable tracing filter for runtime log level changes
    let initial_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| log_filter.into());
    let (filter_layer, reload_handle) = tracing_subscriber::reload::Layer::new(initial_filter);
    
    tracing_subscriber::registry()
        .with(filter_layer)
        .with(tracing_subscriber::fmt::layer())
        .with(log_capture_layer) // Add log capture for SSE streaming
        .init();
    
    // Create runtime settings store with tracing reload capability
    let runtime_settings_store = m3u_proxy::runtime_settings::RuntimeSettingsStore::with_tracing_reload(reload_handle);


    info!("Starting M3U Proxy Service v{}", env!("CARGO_PKG_VERSION"));
    
    // Test log capture is working
    info!("Log capture initialized for SSE streaming");


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
    let state_manager = std::sync::Arc::new(IngestionStateManager::new());
    tracing::debug!("Ingestion state manager initialized");
    
    // Create universal progress service
    let progress_service = std::sync::Arc::new(
        m3u_proxy::services::progress_service::ProgressService::new(state_manager.clone())
    );
    tracing::debug!("Universal progress service initialized");

    // Initialize data mapping service
    let data_mapping_service = DataMappingService::new(database.pool());
    tracing::debug!("Data mapping service initialized");

    // Initialize logo asset service and storage using config paths
    let logo_asset_storage = LogoAssetStorage::new(
        config.storage.uploaded_logo_path.clone(),
        config.storage.cached_logo_path.clone(),
    );
    let mut logo_asset_service = LogoAssetService::new(database.pool(), logo_asset_storage.clone());

    // Create cache invalidation channel for scheduler
    let (cache_invalidation_tx, cache_invalidation_rx) = create_cache_invalidation_channel();

    tracing::debug!("Logo asset service and storage initialized");

    // Initialize sandboxed file managers directly from storage config
    use sandboxed_file_manager::{CleanupPolicy, TimeMatch};

    // Helper function to parse duration strings (e.g., "5m", "30d", "12h")
    let parse_duration = |duration_str: &str| -> Result<std::time::Duration, anyhow::Error> {
        humantime::parse_duration(duration_str)
            .map_err(|e| anyhow::anyhow!("Invalid duration '{}': {}", duration_str, e))
    };

    // Create temp file manager using storage config
    let temp_path = config.storage.temp_path.as_deref().unwrap_or("./data/temp");
    let temp_file_manager = SandboxedManager::builder()
        .base_directory(temp_path)
        .cleanup_policy(CleanupPolicy::new()
            .remove_after(parse_duration(&config.storage.temp_retention)?)
            .time_match(TimeMatch::LastAccess))
        .cleanup_interval(parse_duration(&config.storage.temp_cleanup_interval)?)
        .build()
        .await?;

    // Create proxy output file manager using storage config
    let proxy_output_file_manager = SandboxedManager::builder()
        .base_directory(&config.storage.m3u_path)
        .cleanup_policy(CleanupPolicy::new()
            .remove_after(parse_duration(&config.storage.m3u_retention)?)
            .time_match(TimeMatch::Modified))
        .cleanup_interval(parse_duration(&config.storage.m3u_cleanup_interval)?)
        .build()
        .await?;

    // Create cached logo file manager using storage config
    let logos_cached_file_manager = SandboxedManager::builder()
        .base_directory(&config.storage.cached_logo_path)
        .cleanup_policy(CleanupPolicy::new()
            .remove_after(parse_duration(&config.storage.cached_logo_retention)?)
            .time_match(TimeMatch::LastAccess))
        .cleanup_interval(parse_duration(&config.storage.cached_logo_cleanup_interval)?)
        .build()
        .await?;

    // Create pipeline file manager with dedicated directory for pipeline intermediate files
    let pipeline_file_manager = SandboxedManager::builder()
        .base_directory(&config.storage.pipeline_path)
        .cleanup_policy(CleanupPolicy::new()
            .remove_after(parse_duration(&config.storage.pipeline_retention)?)
            .time_match(TimeMatch::LastAccess))
        .cleanup_interval(parse_duration(&config.storage.pipeline_cleanup_interval)?)
        .build()
        .await?;

    // Update logo asset service with the cached logo file manager
    logo_asset_service = logo_asset_service.with_file_manager(logos_cached_file_manager.clone());

    info!("Sandboxed file managers initialized with configured retention policies:");
    info!(
        "  temp: {} retention, cleanup every {}, path: {:?}",
        config.storage.temp_retention,
        config.storage.temp_cleanup_interval,
        temp_path
    );
    info!(
        "  pipeline: {} retention, cleanup every {}, path: {:?}",
        config.storage.pipeline_retention,
        config.storage.pipeline_cleanup_interval,
        config.storage.pipeline_path
    );
    info!(
        "  m3u_proxy_output: {} retention, cleanup every {}, path: {:?}",
        config.storage.m3u_retention,
        config.storage.m3u_cleanup_interval,
        config.storage.m3u_path
    );
    info!(
        "  logos_cached: {} retention, cleanup every {}, path: {:?}",
        config.storage.cached_logo_retention,
        config.storage.cached_logo_cleanup_interval,
        config.storage.cached_logo_path
    );
    info!(
        "  logos_uploaded: never expires (direct file ops via LogoAssetStorage), path: {:?}",
        config.storage.uploaded_logo_path
    );

    // Create shared system manager for centralized system monitoring
    let system_manager = SystemManager::new(Duration::from_secs(10));
    info!("Shared system manager initialized with 10-second refresh interval");


    // Initialize proxy regeneration service with managed pipeline file manager
    let proxy_regeneration_service = ProxyRegenerationService::new(
        database.pool(),
        config.clone(),
        None,
        pipeline_file_manager.clone(),
        system_manager.get_system(),
        progress_service.clone(), // Pass ProgressService to create ProgressManagers
        state_manager.clone(), // Pass IngestionStateManager to check for active operations
    );
    info!("Proxy regeneration service initialized with in-memory state");

    // Create shared services for both web server and scheduler
    let epg_source_service = std::sync::Arc::new(EpgSourceService::new(
        database.clone(),
        cache_invalidation_tx.clone(),
    ));

    let stream_source_service = std::sync::Arc::new(StreamSourceBusinessService::new(
        database.clone(),
        epg_source_service.clone(),
        cache_invalidation_tx.clone(),
    ));

    // Create scheduler service now that proxy regeneration service exists
    let scheduler = SchedulerService::new(
        progress_service.clone(),
        database.clone(),
        stream_source_service.clone(),
        epg_source_service.clone(),
        config.ingestion.run_missed_immediately,
        Some(cache_invalidation_rx),
        Some(proxy_regeneration_service.clone()),
    );
    info!("Scheduler service initialized");


    // Clone proxy regeneration service for background processing
    let bg_proxy_regeneration_service = proxy_regeneration_service.clone();

    // Configuration for services
    let metrics_config = config.clone();

    // Initialize relay manager with shared system
    let relay_manager = std::sync::Arc::new(
        m3u_proxy::services::RelayManager::new(
            database.clone(),
            temp_file_manager.clone(),
            std::sync::Arc::new(m3u_proxy::metrics::MetricsLogger::new(database.pool())),
            config.clone(),
        )
        .await,
    );
    info!("Relay manager initialized with shared system monitoring");

    let mut web_server = WebServer::new(
        config,
        database,
        (*state_manager).clone(),
        cache_invalidation_tx,
        data_mapping_service,
        logo_asset_service,
        logo_asset_storage,
        proxy_regeneration_service,
        temp_file_manager.clone(), // Use temp for both temp and preview operations
        pipeline_file_manager,
        logos_cached_file_manager,
        proxy_output_file_manager,
        relay_manager,
        system_manager.get_system(),
        progress_service.clone(),
        stream_source_service.clone(),
        epg_source_service.clone(),
        log_broadcaster,
        runtime_settings_store,
    )
    .await?;
    
    // RACE CONDITION FIX: Wire up the API request tracker to prevent duplicates
    // between manual API requests and background auto-regeneration
    web_server.wire_duplicate_protection().await;

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
    bg_proxy_regeneration_service.start_processor();

    info!("Starting scheduler service");
    tokio::spawn(async move {
        if let Err(e) = scheduler.start().await {
            tracing::error!("Scheduler service failed: {}", e);
        }
    });

    // Metrics housekeeper service disabled - historical statistics tracking removed
    if metrics_config.metrics.is_some() {
        info!("Metrics housekeeper disabled (historical statistics removed)");
    } else {
        tracing::debug!("Metrics housekeeper disabled (no configuration found)");
    }

    info!("All services started successfully");

    // Wait for the server to complete (this will block until shutdown)
    server_handle.await?;

    Ok(())
}
