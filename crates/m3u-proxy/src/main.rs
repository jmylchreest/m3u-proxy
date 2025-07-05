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
    utils::memory_config::{MemoryMonitoringConfig, MemoryVerbosity, init_global_memory_config},
    web::WebServer,
};
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
