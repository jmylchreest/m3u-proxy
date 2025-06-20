use anyhow::Result;
use clap::Parser;
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

pub mod assets;
mod config;
mod database;
mod ingestor;
mod models;
mod proxy;
mod web;

use config::Config;
use database::Database;
use ingestor::{
    scheduler::{create_cache_invalidation_channel, SchedulerService},
    IngestionStateManager,
};
use web::WebServer;

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
    let log_filter = format!("m3u_proxy={},tower_http=trace", cli.log_level);
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

    let web_server = WebServer::new(config, database, state_manager, cache_invalidation_tx).await?;

    info!(
        "Starting web server on {}:{}",
        web_server.host(),
        web_server.port()
    );
    web_server.serve().await?;

    Ok(())
}
