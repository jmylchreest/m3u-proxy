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
        scheduler::{create_cache_invalidation_channel, SchedulerService},
        IngestionStateManager,
    },
    logo_assets::{LogoAssetService, LogoAssetStorage},
    web::WebServer,
};

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

    info!("Starting M3U Proxy Service v{}", env!("CARGO_PKG_VERSION"));

    // Load configuration from specified file
    std::env::set_var("CONFIG_FILE", &cli.config);
    let mut config = Config::load()?;
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

    // Start scheduler service
    let scheduler = SchedulerService::new(
        state_manager.clone(),
        database.clone(),
        config.ingestion.run_missed_immediately,
        Some(cache_invalidation_rx),
    );

    tokio::spawn(async move {
        if let Err(e) = scheduler.start().await {
            tracing::error!("Scheduler service failed: {}", e);
        }
    });
    info!("Logo asset service and storage initialized");

    let web_server = WebServer::new(
        config,
        database,
        state_manager,
        cache_invalidation_tx,
        data_mapping_service,
        logo_asset_service,
        logo_asset_storage,
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
