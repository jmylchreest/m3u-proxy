use anyhow::Result;
use clap::Parser;
use std::{sync::Arc, time::Duration};
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use serde_json::Value;

// Use the library instead of redeclaring modules
use m3u_proxy::{
    config::Config,
    data_mapping::DataMappingService,
    database::{Database, repositories::{StreamProxySeaOrmRepository, EpgSourceSeaOrmRepository, StreamSourceSeaOrmRepository, ChannelSeaOrmRepository}},
    ingestor::{
        IngestionStateManager,
        scheduler::{SchedulerService, create_cache_invalidation_channel},
    },
    logo_assets::{LogoAssetService, LogoAssetStorage},
    services::{ProxyRegenerationService, StreamSourceBusinessService, EpgSourceService, UrlLinkingService},
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
        println!("  Rust: {rustc_version}");
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
                    println!("  {name}: {version}");
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

    // Initialize feature flags from config in runtime store
    runtime_settings_store.initialize_feature_flags_from_config(&config).await;
    
    // Initialize request logging setting from config
    runtime_settings_store.update_request_logging(config.web.enable_request_logging).await;

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

    // Initialize circuit breaker manager (always enabled with defaults if not configured)
    let circuit_breaker_manager = {
        let mut cb_config = config.circuitbreaker.clone().unwrap_or_default();
        
        // Override profile defaults for logo fetching service to be very lenient
        if !cb_config.profiles.contains_key("logo_fetch") {
            let logo_profile = m3u_proxy::config::CircuitBreakerProfileConfig {
                implementation_type: "simple".to_string(),
                failure_threshold: 10,  // Very high threshold - lots of 404s are expected
                operation_timeout: "10s".to_string(), // Longer timeout for logo downloads
                reset_timeout: "1m".to_string(), // Reasonable reset time for logos
                success_threshold: 3, // Need more successes to close circuit
                acceptable_status_codes: vec!["2xx".to_string(), "3xx".to_string(), "404".to_string()], // 404s are acceptable for logos
            };
            cb_config.profiles.insert("logo_fetch".to_string(), logo_profile);
            info!("Added default logo_fetch circuit breaker profile (lenient configuration with 404 acceptable)");
        }
        
        let manager = std::sync::Arc::new(m3u_proxy::services::CircuitBreakerManager::new(cb_config.clone()));
        info!("Circuit breaker manager initialized with {} profiles (using {} global settings)", 
              cb_config.profiles.len(),
              if config.circuitbreaker.is_some() { "configured" } else { "default" });
        manager
    };

    // Create HTTP client factory for consistent circuit breaker integration
    let http_client_factory = m3u_proxy::utils::HttpClientFactory::new(
        Some(circuit_breaker_manager.clone()),
        Duration::from_secs(5), // Default connect timeout
    );
    info!("HTTP client factory initialized with circuit breaker support");

    // Initialize database with better error handling
    let database = match Database::new(&config.database, &config.ingestion).await {
        Ok(db) => db,
        Err(e) => {
            eprintln!("Failed to initialize database at '{}': {}", config.database.url, e);
            eprintln!("This could be due to:");
            eprintln!("  - Invalid database URL format");
            eprintln!("  - Missing parent directory for SQLite database file");
            eprintln!("  - Insufficient permissions to create/write database file");
            eprintln!("  - Database file exists but is corrupted or locked");
            std::process::exit(1);
        }
    };
    
    if let Err(e) = database.migrate().await {
        eprintln!("Failed to run database migrations: {}", e);
        eprintln!("This could indicate database corruption or incompatible schema versions.");
        std::process::exit(1);
    }
    
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
    let data_mapping_service = DataMappingService::new(database.connection().clone());
    tracing::debug!("Data mapping service initialized");

    // Initialize logo asset service and storage using config paths
    let logo_asset_storage = LogoAssetStorage::new(
        config.storage.uploaded_logo_path.clone(),
        config.storage.cached_logo_path.clone(),
    );
    let mut logo_asset_service = LogoAssetService::new(database.connection().clone(), logo_asset_storage.clone(), &http_client_factory).await;

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


    let proxy_regeneration_service = {
        let proxy_repository = StreamProxySeaOrmRepository::new(database.connection().clone());
        ProxyRegenerationService::new(
            database.clone(),
            proxy_repository,
            config.clone(),
            None,
            pipeline_file_manager.clone(),
            progress_service.clone(), // Pass ProgressService to create ProgressManagers
            state_manager.clone(), // Pass IngestionStateManager to check for active operations
            Arc::new(http_client_factory.clone()), // Pass HttpClientFactory for circuit breaker support
        )
    };
    info!("Proxy regeneration service initialized with in-memory state");

    // Create shared services for both web server and scheduler
    let epg_source_service = {
        let epg_source_repo = EpgSourceSeaOrmRepository::new(database.connection().clone());
        let stream_source_repo_for_url = StreamSourceSeaOrmRepository::new(database.connection().clone());
        let epg_source_repo_for_url = EpgSourceSeaOrmRepository::new(database.connection().clone());
        let url_linking_service = UrlLinkingService::new(stream_source_repo_for_url, epg_source_repo_for_url);
        
        std::sync::Arc::new(EpgSourceService::new(
            database.clone(),
            epg_source_repo,
            url_linking_service,
            cache_invalidation_tx.clone(),
            http_client_factory.clone(),
        ))
    };

    let stream_source_service = {
        let stream_source_repo = StreamSourceSeaOrmRepository::new(database.connection().clone());
        let channel_repo = ChannelSeaOrmRepository::new(database.connection().clone());
        let stream_source_repo_for_url = StreamSourceSeaOrmRepository::new(database.connection().clone());
        let epg_source_repo_for_url = EpgSourceSeaOrmRepository::new(database.connection().clone());
        let epg_source_repo_for_service = epg_source_repo_for_url.clone();
        let url_linking_service = UrlLinkingService::new(stream_source_repo_for_url, epg_source_repo_for_url);
        
        std::sync::Arc::new(StreamSourceBusinessService::with_http_client_factory(
            stream_source_repo,
            channel_repo,
            epg_source_repo_for_service,
            url_linking_service,
            cache_invalidation_tx.clone(),
            http_client_factory.clone(),
        ))
    };

    // Create scheduler service now that proxy regeneration service exists
    let scheduler = SchedulerService::with_http_client_factory(
        progress_service.clone(),
        database.clone(),
        stream_source_service.clone(),
        epg_source_service.clone(),
        config.ingestion.run_missed_immediately,
        Some(cache_invalidation_rx),
        Some(proxy_regeneration_service.clone()),
        http_client_factory.clone(),
    );
    info!("Scheduler service initialized");



    // Initialize relay manager with shared system (SeaORM)
    let relay_manager = std::sync::Arc::new(
        m3u_proxy::services::RelayManager::new(
            database.clone(),
            temp_file_manager.clone(),
            std::sync::Arc::new(m3u_proxy::metrics::MetricsLogger::new(database.connection())),
            config.clone(),
        )
        .await,
    );
    info!("Relay manager initialized with shared system monitoring");

    // Initialize relay configuration resolver with SeaORM
    let relay_repository = m3u_proxy::database::repositories::relay::RelaySeaOrmRepository::new(database.connection().clone());
    let relay_config_resolver = m3u_proxy::services::RelayConfigResolver::new(relay_repository);
    info!("Relay configuration resolver initialized");


    let mut web_server = WebServer::new(
        m3u_proxy::web::WebServerBuilder {
            config,
            database,
            state_manager: (*state_manager).clone(),
            cache_invalidation_tx,
            data_mapping_service,
            logo_asset_service,
            logo_asset_storage,
            proxy_regeneration_service: proxy_regeneration_service.clone(),
            temp_file_manager: temp_file_manager.clone(),
            pipeline_file_manager,
            logos_cached_file_manager,
            proxy_output_file_manager,
            relay_manager,
            relay_config_resolver,
            system: system_manager.get_system(),
            progress_service: progress_service.clone(),
            stream_source_service: stream_source_service.clone(),
            epg_source_service: epg_source_service.clone(),
            log_broadcaster,
            runtime_settings_store,
            circuit_breaker_manager: Some(circuit_breaker_manager.clone()),
        },
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

    // Create cancellation token for coordinated shutdown of all services
    let cancellation_token = tokio_util::sync::CancellationToken::new();
    
    // Clone services for shutdown handling before they get moved
    let shutdown_proxy_service = proxy_regeneration_service.clone();
    
    // Set up signal handlers for graceful shutdown with force-kill capability
    let shutdown_token = cancellation_token.clone();
    let shutdown_state_manager = state_manager.clone();
    tokio::spawn(async move {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;
        
        let signal_count = Arc::new(AtomicUsize::new(0));
        const FORCE_KILL_THRESHOLD: usize = 3;
        
        #[cfg(unix)]
        {
            use tokio::signal::unix::{signal, SignalKind};
            let mut sigterm = signal(SignalKind::terminate()).expect("failed to install SIGTERM handler");
            let mut sigint = signal(SignalKind::interrupt()).expect("failed to install SIGINT handler");
            
            // First signal - initiate graceful shutdown
            tokio::select! {
                _ = sigterm.recv() => {
                    tracing::info!("Received SIGTERM, initiating graceful shutdown of all services");
                }
                _ = sigint.recv() => {
                    tracing::info!("Received SIGINT (Ctrl+C), initiating graceful shutdown of all services");
                }
            }
            
            // Cancel all active ingestions first
            shutdown_state_manager.cancel_all_ingestions().await;
            
            // Shutdown proxy regeneration service to cancel pending delays
            shutdown_proxy_service.shutdown();
            
            // Cancel all background services
            shutdown_token.cancel();
            
            // Set up force-kill handler for additional signals
            let signal_count_clone = signal_count.clone();
            tokio::spawn(async move {
                loop {
                    tokio::select! {
                        _ = sigterm.recv() => {
                            let count = signal_count_clone.fetch_add(1, Ordering::SeqCst) + 1;
                            if count >= FORCE_KILL_THRESHOLD {
                                tracing::warn!("Received {} SIGTERM signals - force killing application", count + 1);
                                std::process::exit(1);
                            } else {
                                tracing::warn!("Received additional SIGTERM ({}/{}), send {} more to force kill", count + 1, FORCE_KILL_THRESHOLD + 1, FORCE_KILL_THRESHOLD - count);
                            }
                        }
                        _ = sigint.recv() => {
                            let count = signal_count_clone.fetch_add(1, Ordering::SeqCst) + 1;
                            if count >= FORCE_KILL_THRESHOLD {
                                tracing::warn!("Received {} SIGINT signals - force killing application", count + 1);
                                std::process::exit(1);
                            } else {
                                tracing::warn!("Received additional SIGINT (Ctrl+C) ({}/{}), send {} more to force kill", count + 1, FORCE_KILL_THRESHOLD + 1, FORCE_KILL_THRESHOLD - count);
                            }
                        }
                    }
                }
            });
        }
        
        #[cfg(not(unix))]
        {
            use tokio::signal;
            signal::ctrl_c().await.expect("failed to install Ctrl+C handler");
            tracing::info!("Received Ctrl+C, initiating graceful shutdown of all services");
            
            // Cancel all active ingestions first
            shutdown_state_manager.cancel_all_ingestions().await;
            
            // Shutdown proxy regeneration service to cancel pending delays
            shutdown_proxy_service.shutdown();
            
            // Cancel all background services
            shutdown_token.cancel();
            
            // Set up force-kill handler for additional Ctrl+C
            let signal_count_clone = signal_count.clone();
            tokio::spawn(async move {
                loop {
                    if signal::ctrl_c().await.is_ok() {
                        let count = signal_count_clone.fetch_add(1, Ordering::SeqCst) + 1;
                        if count >= FORCE_KILL_THRESHOLD {
                            tracing::warn!("Received {} Ctrl+C signals - force killing application", count + 1);
                            std::process::exit(1);
                        } else {
                            tracing::warn!("Received additional Ctrl+C ({}/{}), send {} more to force kill", count + 1, FORCE_KILL_THRESHOLD + 1, FORCE_KILL_THRESHOLD - count);
                        }
                    }
                }
            });
        }
    });

    // Create a channel to signal when the server is ready or fails to bind
    let (server_ready_tx, server_ready_rx) = tokio::sync::oneshot::channel();

    // Get the bind address before moving web_server
    let _bind_addr = format!("{}:{}", web_server.host(), web_server.port());

    // Start the web server in a separate task
    let web_server_token = cancellation_token.clone();
    let server_handle = tokio::spawn(async move {
        // This will signal immediately when bind succeeds/fails, then block until shutdown
        if let Err(e) = web_server.serve_with_cancellation(server_ready_tx, Some(web_server_token)).await {
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
    // Note: Proxy regeneration is handled by scheduler completion handlers, no background polling needed

    info!("Starting scheduler service");
    let scheduler_token = cancellation_token.clone();
    let scheduler_handle = tokio::spawn(async move {
        if let Err(e) = scheduler.start_with_cancellation(Some(scheduler_token)).await {
            tracing::error!("Scheduler service failed: {}", e);
        }
    });

    info!("All services started successfully");

    // Wait for either server completion or cancellation signal
    tokio::select! {
        result = server_handle => {
            if let Err(e) = result {
                tracing::error!("Web server task failed: {}", e);
            }
        }
        _ = cancellation_token.cancelled() => {
            tracing::info!("Cancellation signal received, waiting for services to shut down");
            
            // Give services time to shut down gracefully, with extra time for database operations
            // Database operations like EPG ingestion can take several minutes and must complete
            // to avoid partial state corruption
            let shutdown_timeout = tokio::time::timeout(
                std::time::Duration::from_secs(300), // 5 minutes for database consistency
                scheduler_handle
            );
            
            match shutdown_timeout.await {
                Ok(Ok(())) => tracing::info!("All background services shut down gracefully"),
                Ok(Err(e)) => tracing::warn!("Background service error during shutdown: {}", e),
                Err(_) => tracing::warn!("Background services did not shut down within timeout"),
            }
        }
    }

    Ok(())
}
