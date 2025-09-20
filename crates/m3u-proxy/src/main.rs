use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use serde::Serialize;
use serde_json::Value;
use std::{sync::Arc, time::Duration};
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use m3u_proxy::{
    config::Config,
    data_mapping::DataMappingService,
    database::Database,
    ingestor::{IngestionStateManager, scheduler::create_cache_invalidation_channel},
    job_scheduling::{JobExecutor, JobQueue, JobQueueRunner, JobScheduler},
    logo_assets::{LogoAssetService, LogoAssetStorage},
    services::{
        EpgSourceService, ProxyRegenerationService, StreamSourceBusinessService, UrlLinkingService,
        logo_cache::LogoCacheService, logo_cache_maintenance::LogoCacheMaintenanceService,
    },
    utils::SystemManager,
    web::WebServer,
};

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};

/// ------------------------------
/// Version / SBOM helpers
/// ------------------------------
fn get_dependencies() -> Result<Value, Box<dyn std::error::Error>> {
    let sbom_str = include_str!(concat!(env!("OUT_DIR"), "/sbom.json"));
    let sbom: Value = serde_json::from_str(sbom_str)?;
    Ok(sbom)
}

fn print_version_info() {
    println!("{} v{}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
    println!("{}", env!("CARGO_PKG_DESCRIPTION"));
    println!();
    println!("Build Information:");
    println!(
        "  Target: {}-{}",
        std::env::consts::ARCH,
        std::env::consts::OS
    );
    if let Ok(rustc_version) = std::env::var("RUSTC_VERSION") {
        println!("  Rust: {rustc_version}");
    }
    println!();
    println!("Software Bill of Materials:");
    match get_dependencies() {
        Ok(sbom) => {
            let mut dependencies = Vec::new();
            if let Some(packages) = sbom["packages"].as_array() {
                for package in packages {
                    if let (Some(name), Some(version)) =
                        (package["name"].as_str(), package["versionInfo"].as_str())
                    {
                        if name != env!("CARGO_PKG_NAME") && !version.contains("path+") {
                            dependencies.push((name.to_string(), version.to_string()));
                        }
                    }
                }
            }
            dependencies.sort_by(|a, b| a.0.cmp(&b.0));
            if dependencies.is_empty() {
                println!("  (No external components found in SBOM)");
            } else {
                for (name, version) in dependencies {
                    println!("  {name}: {version}");
                }
            }
        }
        Err(_) => println!("  (Unable to read SBOM data)"),
    }
    println!();
    println!("Repository: {}", env!("CARGO_PKG_REPOSITORY"));
    println!("License: {}", env!("CARGO_PKG_LICENSE"));
    println!("Authors: {}", env!("CARGO_PKG_AUTHORS"));
}

/// ------------------------------
/// CLI model (Serve is default)
/// ------------------------------
#[derive(Parser)]
#[command(name = "m3u-proxy")]
#[command(version = env!("CARGO_PKG_VERSION"))]
#[command(about = "A modern M3U proxy service with filtering and source management")]
#[command(disable_version_flag = true)]
struct Cli {
    /// Configuration file path
    #[arg(short, long, default_value = "config.toml")]
    config: String,

    /// Listening IP address override
    #[arg(short = 'H', long)]
    host: Option<String>,

    /// Listening port override
    #[arg(short, long)]
    port: Option<u16>,

    /// Database URL override
    #[arg(short = 'd', long)]
    database_url: Option<String>,

    /// Log level
    #[arg(short = 'l', long, default_value = "info")]
    log_level: String,

    /// Print extended version (with dependency list) and exit
    #[arg(short = 'v', long)]
    version: bool,

    /// Optional subcommand (if omitted, we run the server)
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Show database schema & migration status (optionally apply missing migrations first)
    SchemaStatus {
        #[arg(short, long, default_value = "config.toml")]
        config: String,
        #[arg(short = 'd', long)]
        database_url: Option<String>,
        /// Output JSON instead of plain text
        #[arg(long)]
        json: bool,
        /// Apply pending migrations before reporting (runs the same migrate + auto-repair logic as serve)
        #[arg(long)]
        apply_migrations: bool,
    },
}

/// ------------------------------
/// Schema status reporting
/// ------------------------------
#[derive(Debug, Serialize)]
struct SchemaStatus {
    database_type: String,
    database_url_redacted: String,
    applied_migrations: Vec<String>,
    available_migrations: Vec<String>,
    missing_migrations: Vec<String>,
    extra_migrations: Vec<String>,
    filters_legacy_unique_name: bool,
    filters_composite_unique_index: bool,
    remediation: Option<String>,
}

fn redact_db_url(url: &str) -> String {
    if url.starts_with("sqlite:") {
        return url.to_string();
    }
    if let Some(scheme_sep) = url.find("://") {
        let (scheme, rest) = url.split_at(scheme_sep + 3);
        if let Some(at) = rest.find('@') {
            return format!("{scheme}***:***@{}", &rest[at + 1..]);
        }
    }
    url.to_string()
}

async fn gather_schema_status(db: &Database, original_url: &str) -> Result<SchemaStatus> {
    use m3u_proxy::database::migrations::Migrator;
    use sea_orm_migration::MigratorTrait;

    let available: Vec<String> = Migrator::migrations()
        .into_iter()
        .map(|m| m.name().to_string())
        .collect();

    let mut applied: Vec<String> = Vec::new();
    if let Ok(rows) = db
        .connection
        .query_all(Statement::from_string(
            db.backend(),
            "SELECT version FROM seaql_migrations ORDER BY version".to_string(),
        ))
        .await
    {
        for r in rows {
            if let Ok(v) = r.try_get::<String>("", "version") {
                applied.push(v);
            }
        }
    }

    let missing: Vec<String> = available
        .iter()
        .filter(|m| !applied.contains(&(**m)))
        .map(|s| s.to_string())
        .collect();
    let extra: Vec<String> = applied
        .iter()
        .filter(|m| !available.contains(&(**m)))
        .map(|s| s.to_string())
        .collect();

    let mut legacy_single = false;
    let mut composite_index = false;

    if db.backend() == DatabaseBackend::Postgres {
        let legacy_sql = r#"
SELECT 1
FROM pg_constraint c
JOIN pg_class t ON t.oid = c.conrelid
JOIN pg_namespace n ON n.oid = t.relnamespace
WHERE t.relname='filters'
  AND c.contype='u'
  AND (
    SELECT array_agg(att.attname ORDER BY att.attnum)
    FROM unnest(c.conkey) WITH ORDINALITY k(attnum,ord)
    JOIN pg_attribute att ON att.attrelid=c.conrelid AND att.attnum=k.attnum
  ) = ARRAY['name']
LIMIT 1;"#;
        if let Ok(row) = db
            .connection
            .query_one(Statement::from_string(db.backend(), legacy_sql.to_string()))
            .await
        {
            if row.is_some() {
                legacy_single = true;
            }
        }

        let composite_sql = r#"
SELECT 1
FROM pg_indexes
WHERE tablename='filters'
  AND indexname='idx_filters_name_source_type_unique'
LIMIT 1;"#;
        if let Ok(row) = db
            .connection
            .query_one(Statement::from_string(
                db.backend(),
                composite_sql.to_string(),
            ))
            .await
        {
            if row.is_some() {
                composite_index = true;
            }
        }
    } else {
        // Assume composite path handled by migrations for non-Postgres
        composite_index = true;
    }

    let remediation = if legacy_single {
        Some(
            "Start the service (auto-repair runs) or manually drop legacy UNIQUE(name) then create composite UNIQUE(name, source_type)."
                .to_string(),
        )
    } else {
        None
    };

    Ok(SchemaStatus {
        database_type: db.database_type.as_str().to_string(),
        database_url_redacted: redact_db_url(original_url),
        applied_migrations: applied,
        available_migrations: available,
        missing_migrations: missing,
        extra_migrations: extra,
        filters_legacy_unique_name: legacy_single,
        filters_composite_unique_index: composite_index,
        remediation,
    })
}

fn print_schema_status_text(s: &SchemaStatus) {
    println!("Database Type: {}", s.database_type);
    println!("Applied migrations ({}):", s.applied_migrations.len());
    for m in &s.applied_migrations {
        println!("  - {m}");
    }
    println!("Available migrations ({}):", s.available_migrations.len());
    for m in &s.available_migrations {
        println!("    {m}");
    }
    if !s.missing_migrations.is_empty() {
        println!("Missing migrations:");
        for m in &s.missing_migrations {
            println!("  * {m}");
        }
    }
    if !s.extra_migrations.is_empty() {
        println!("Extra migrations (in DB but not in binary):");
        for m in &s.extra_migrations {
            println!("  ! {m}");
        }
    }

    if !s.missing_migrations.is_empty() {
        println!(
            "NOTE: {} migration(s) are missing. Run: m3u-proxy schema-status --apply-migrations",
            s.missing_migrations.len()
        );
    }
    if let Some(r) = &s.remediation {
        println!("Remediation: {r}");
    }
}

/// ------------------------------
/// Main entry
/// ------------------------------
#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    if cli.version {
        print_version_info();
        return Ok(());
    }

    match &cli.command {
        Some(Command::SchemaStatus {
            config,
            database_url,
            json,
            apply_migrations,
        }) => {
            tracing_subscriber::registry()
                .with(tracing_subscriber::fmt::layer().with_target(false))
                .init();

            let mut cfg = Config::load_from_file(config)?;
            if let Some(override_db) = database_url {
                cfg.database.url = override_db.clone();
            }
            let db = Database::new(&cfg.database, &cfg.ingestion).await?;

            // Optionally apply migrations (same path as serve, includes auto-repair)
            if *apply_migrations {
                if let Err(e) = db.migrate().await {
                    eprintln!("Failed to apply migrations: {e}");
                    std::process::exit(1);
                } else {
                    info!("Migrations applied successfully (schema-status --apply-migrations)");
                }
            }

            let status = gather_schema_status(&db, &cfg.database.url).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&status)?);
            } else {
                print_schema_status_text(&status);
            }
            return Ok(());
        }
        None => {
            // Default to Serve
        }
    }

    // ------------------ Serve Path ------------------
    // Logging (with capture for SSE)
    let log_filter = if cli.log_level == "trace" {
        format!("m3u_proxy={},tower_http=trace", cli.log_level)
    } else {
        format!("m3u_proxy={}", cli.log_level)
    };

    let (log_capture_layer, log_broadcaster) =
        m3u_proxy::utils::log_capture::setup_log_capture_with_subscriber();
    let initial_filter =
        tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| log_filter.into());
    let (filter_layer, reload_handle) = tracing_subscriber::reload::Layer::new(initial_filter);

    tracing_subscriber::registry()
        .with(filter_layer)
        .with(tracing_subscriber::fmt::layer())
        .with(log_capture_layer)
        .init();

    info!("Starting M3U Proxy Service v{}", env!("CARGO_PKG_VERSION"));
    info!("Log capture layer initialized");

    // Load config
    let mut config = Config::load_from_file(&cli.config)?;
    if std::path::Path::new(&cli.config).exists() {
        info!("Configuration file: {}", &cli.config);
    }

    // Apply CLI overrides
    if let Some(h) = cli.host {
        config.web.host = h;
    }
    if let Some(p) = cli.port {
        config.web.port = p;
    }
    if let Some(db_url) = cli.database_url {
        config.database.url = db_url;
    }

    info!("Using database: {}", redact_db_url(&config.database.url));

    // Runtime settings (feature flags + request logging)
    let runtime_settings_store =
        m3u_proxy::runtime_settings::RuntimeSettingsStore::with_tracing_reload(reload_handle);
    let runtime_settings_arc = Arc::new(runtime_settings_store);

    runtime_settings_arc
        .initialize_feature_flags_from_config(&config)
        .await;
    runtime_settings_arc
        .update_request_logging(config.web.enable_request_logging)
        .await;

    // Circuit breaker manager
    let mut cb_config = config.circuitbreaker.clone().unwrap_or_default();
    if !cb_config.profiles.contains_key("logo_fetch") {
        let logo_profile = m3u_proxy::config::CircuitBreakerProfileConfig {
            implementation_type: "simple".to_string(),
            failure_threshold: 10,
            operation_timeout: "10s".to_string(),
            reset_timeout: "1m".to_string(),
            success_threshold: 3,
            acceptable_status_codes: vec!["2xx".into(), "3xx".into(), "404".into()],
        };
        cb_config
            .profiles
            .insert("logo_fetch".to_string(), logo_profile);
        info!("Added default circuit breaker profile: logo_fetch");
    }
    let circuit_breaker_manager = Arc::new(m3u_proxy::services::CircuitBreakerManager::new(
        cb_config.clone(),
    ));
    info!(
        "Circuit breaker manager initialized ({} profiles)",
        cb_config.profiles.len()
    );

    // HTTP client factory
    let http_client_factory = m3u_proxy::utils::HttpClientFactory::new(
        Some(circuit_breaker_manager.clone()),
        Duration::from_secs(5),
    );

    // Database + migrations (auto-repair inside migrate)
    let database = match Database::new(&config.database, &config.ingestion).await {
        Ok(db) => db,
        Err(e) => {
            eprintln!(
                "Failed to connect to database '{}': {}",
                config.database.url, e
            );
            std::process::exit(1);
        }
    };
    if let Err(e) = database.migrate().await {
        eprintln!("Failed to run migrations: {e}");
        std::process::exit(1);
    }
    info!("Database connected and migrations applied");

    // Ingestion state + progress service
    let ingestion_state = Arc::new(IngestionStateManager::new());
    let progress_service = Arc::new(m3u_proxy::services::progress_service::ProgressService::new(
        ingestion_state.clone(),
    ));

    // Data mapping service
    let data_mapping_service = DataMappingService::new(database.connection().clone());

    // Logo asset services
    let logo_asset_storage = LogoAssetStorage::new(
        config.storage.uploaded_logo_path.clone(),
        config.storage.cached_logo_path.clone(),
    );
    let mut logo_asset_service = LogoAssetService::new(
        database.connection().clone(),
        logo_asset_storage.clone(),
        &http_client_factory,
    )
    .await;

    // Cache invalidation channel
    let (cache_invalidation_tx, _cache_invalidation_rx) = create_cache_invalidation_channel();

    // File managers (sandboxed)
    use sandboxed_file_manager::{CleanupPolicy, SandboxedManager, TimeMatch};

    let parse_duration = |s: &str| -> Result<Duration> {
        humantime::parse_duration(s).map_err(|e| anyhow::anyhow!("Invalid duration '{}': {}", s, e))
    };

    // Temp
    let temp_path = config
        .storage
        .temp_path
        .clone()
        .unwrap_or("./data/temp".into());
    let temp_file_manager = SandboxedManager::builder()
        .base_directory(&temp_path)
        .cleanup_policy(
            CleanupPolicy::new()
                .remove_after(parse_duration(&config.storage.temp_retention)?)
                .time_match(TimeMatch::LastAccess),
        )
        .cleanup_interval(parse_duration(&config.storage.temp_cleanup_interval)?)
        .build()
        .await?;

    // M3U output
    let m3u_file_manager = SandboxedManager::builder()
        .base_directory(&config.storage.m3u_path)
        .cleanup_policy(
            CleanupPolicy::new()
                .remove_after(parse_duration(&config.storage.m3u_retention)?)
                .time_match(TimeMatch::Modified),
        )
        .cleanup_interval(parse_duration(&config.storage.m3u_cleanup_interval)?)
        .build()
        .await?;

    // Cached logos (no auto cleanup - handled by maintenance)
    let logos_cached_file_manager = SandboxedManager::builder()
        .base_directory(&config.storage.cached_logo_path)
        .build()
        .await?;

    // Pipeline
    let pipeline_file_manager = SandboxedManager::builder()
        .base_directory(&config.storage.pipeline_path)
        .cleanup_policy(
            CleanupPolicy::new()
                .remove_after(parse_duration(&config.storage.pipeline_retention)?)
                .time_match(TimeMatch::LastAccess),
        )
        .cleanup_interval(parse_duration(&config.storage.pipeline_cleanup_interval)?)
        .build()
        .await?;

    // Attach cached logo file manager
    logo_asset_service = logo_asset_service.with_file_manager(logos_cached_file_manager.clone());

    info!("File managers initialized:");
    info!(
        "  temp: {} retention / {}, path {:?}",
        config.storage.temp_retention, config.storage.temp_cleanup_interval, temp_path
    );
    info!(
        "  pipeline: {} retention / {}, path {:?}",
        config.storage.pipeline_retention,
        config.storage.pipeline_cleanup_interval,
        config.storage.pipeline_path
    );
    info!(
        "  m3u: {} retention / {}, path {:?}",
        config.storage.m3u_retention, config.storage.m3u_cleanup_interval, config.storage.m3u_path
    );
    info!(
        "  logos_cached: manual retention, path {:?}",
        config.storage.cached_logo_path
    );

    // System manager (basic monitoring)
    let system_manager = SystemManager::new(Duration::from_secs(10));

    // Observability
    let observability = Arc::new(
        m3u_proxy::observability::AppObservability::new("m3u-proxy")
            .context("Failed to initialize observability")?,
    );

    // Proxy regeneration
    let proxy_repository = m3u_proxy::database::repositories::StreamProxySeaOrmRepository::new(
        database.connection().clone(),
    );
    let proxy_regeneration_service = ProxyRegenerationService::new(
        database.clone(),
        proxy_repository,
        config.clone(),
        None,
        pipeline_file_manager.clone(),
        progress_service.clone(),
        ingestion_state.clone(),
        Arc::new(http_client_factory.clone()),
    )
    .with_observability(observability.clone());

    // EPG source service
    let epg_source_service = {
        let epg_repo = m3u_proxy::database::repositories::EpgSourceSeaOrmRepository::new(
            database.connection().clone(),
        );
        let stream_repo_for_url =
            m3u_proxy::database::repositories::StreamSourceSeaOrmRepository::new(
                database.connection().clone(),
            );
        let epg_repo_for_url = m3u_proxy::database::repositories::EpgSourceSeaOrmRepository::new(
            database.connection().clone(),
        );
        let url_service = UrlLinkingService::new(stream_repo_for_url, epg_repo_for_url);
        Arc::new(EpgSourceService::new(
            database.clone(),
            epg_repo,
            url_service,
            cache_invalidation_tx.clone(),
            http_client_factory.clone(),
        ))
    };

    // Stream source service
    let stream_source_service = {
        let stream_repo = m3u_proxy::database::repositories::StreamSourceSeaOrmRepository::new(
            database.connection().clone(),
        );
        let channel_repo = m3u_proxy::database::repositories::ChannelSeaOrmRepository::new(
            database.connection().clone(),
        );
        let stream_repo_for_url =
            m3u_proxy::database::repositories::StreamSourceSeaOrmRepository::new(
                database.connection().clone(),
            );
        let epg_repo_for_url = m3u_proxy::database::repositories::EpgSourceSeaOrmRepository::new(
            database.connection().clone(),
        );
        let url_service = UrlLinkingService::new(stream_repo_for_url, epg_repo_for_url.clone());
        Arc::new(
            StreamSourceBusinessService::with_http_client_factory(
                stream_repo,
                channel_repo,
                epg_repo_for_url.clone(),
                url_service,
                cache_invalidation_tx.clone(),
                http_client_factory.clone(),
            )
            .with_observability(observability.clone()),
        )
    };

    // Logo cache services
    let logo_cache_service = Arc::new(LogoCacheService::new(logos_cached_file_manager.clone())?);
    let logo_cache_maintenance_service = Arc::new(
        LogoCacheMaintenanceService::new(logo_cache_service.clone())
            .with_job_queue(Arc::new(JobQueue::new())),
    );
    logo_cache_maintenance_service.initialize().await?;
    info!("Logo cache maintenance initialized");

    // Job scheduling system
    let job_queue = Arc::new(JobQueue::new());
    let job_scheduler = Arc::new(JobScheduler::new(job_queue.clone(), database.clone()));
    let job_executor = Arc::new(JobExecutor::new(
        stream_source_service.clone(),
        epg_source_service.clone(),
        Arc::new(proxy_regeneration_service.clone()),
        logo_cache_maintenance_service.clone(),
        ingestion_state.clone(),
        database.clone(),
        config.clone(),
        temp_file_manager.clone(),
        Arc::new(http_client_factory.clone()),
        progress_service.clone(),
    ));
    let job_queue_runner = Arc::new(JobQueueRunner::new(
        job_queue.clone(),
        job_executor.clone(),
        job_scheduler.clone(),
        &config.job_scheduling.clone().unwrap_or_default(),
    ));
    info!("Job scheduling system initialized");

    // Relay manager + config resolver
    let relay_manager = Arc::new(
        m3u_proxy::services::RelayManager::new(
            database.clone(),
            temp_file_manager.clone(),
            config.clone(),
        )
        .await
        .with_observability(observability.clone()),
    );
    let relay_repo = m3u_proxy::database::repositories::relay::RelaySeaOrmRepository::new(
        database.connection().clone(),
    );
    let relay_config_resolver = m3u_proxy::services::RelayConfigResolver::new(relay_repo);

    // Build web server via builder
    let mut web_server = WebServer::new(m3u_proxy::web::WebServerBuilder {
        config: config.clone(),
        database: database.clone(),
        state_manager: (*ingestion_state).clone(),
        cache_invalidation_tx: cache_invalidation_tx.clone(),
        data_mapping_service,
        logo_asset_service,
        logo_asset_storage,
        proxy_regeneration_service: proxy_regeneration_service.clone(),
        temp_file_manager: temp_file_manager.clone(),
        pipeline_file_manager,
        logos_cached_file_manager: logos_cached_file_manager.clone(),
        proxy_output_file_manager: m3u_file_manager.clone(),
        relay_manager,
        relay_config_resolver,
        system: system_manager.get_system(),
        progress_service: progress_service.clone(),
        stream_source_service: stream_source_service.clone(),
        epg_source_service: epg_source_service.clone(),
        log_broadcaster,
        runtime_settings_store: runtime_settings_arc.clone(),
        circuit_breaker_manager: Some(circuit_breaker_manager.clone()),
        observability: observability.clone(),
        job_scheduler: job_scheduler.clone(),
        job_queue: job_queue.clone(),
        job_queue_runner: Arc::new(JobQueueRunner::new(
            job_queue.clone(),
            job_executor.clone(),
            job_scheduler.clone(),
            &config.job_scheduling.clone().unwrap_or_default(),
        )),
        logo_cache_service: logo_cache_service.clone(),
        logo_cache_maintenance_service: logo_cache_maintenance_service.clone(),
    })
    .await?;

    web_server.wire_duplicate_protection().await;
    info!("Listening on {}:{}", web_server.host(), web_server.port());

    // Cancellation tokens
    let web_server_cancellation_token = tokio_util::sync::CancellationToken::new();
    let scheduler_cancellation_token = tokio_util::sync::CancellationToken::new();

    // Spawn graceful shutdown signal handler
    {
        let shutdown_token = scheduler_cancellation_token.clone();
        let ingestion_state = ingestion_state.clone();
        let proxy_service = proxy_regeneration_service.clone();
        tokio::spawn(async move {
            #[cfg(unix)]
            {
                use tokio::signal::unix::{SignalKind, signal};
                let mut sigterm = signal(SignalKind::terminate()).expect("install SIGTERM handler");
                let mut sigint = signal(SignalKind::interrupt()).expect("install SIGINT handler");
                tokio::select! {
                    _ = sigterm.recv() => {
                        tracing::info!("SIGTERM received - initiating graceful shutdown");
                    }
                    _ = sigint.recv() => {
                        tracing::info!("SIGINT received - initiating graceful shutdown");
                    }
                }
            }
            #[cfg(not(unix))]
            {
                use tokio::signal;
                let _ = signal::ctrl_c().await;
                tracing::info!("Ctrl+C received - initiating graceful shutdown");
            }

            let cancelled = ingestion_state.cancel_all_ingestions().await;
            tracing::info!("Cancelled {cancelled} active ingestions");

            proxy_service.shutdown();
            proxy_service.cancel_all_pending().await;

            shutdown_token.cancel();
        });
    }

    // Server start (one-shot bind)
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
    let web_token = web_server_cancellation_token.clone();
    let server_handle = tokio::spawn(async move {
        if let Err(e) = web_server
            .serve_with_cancellation(ready_tx, Some(web_token))
            .await
        {
            tracing::error!("Web server failed: {e}");
        }
    });

    match ready_rx.await {
        Ok(Ok(())) => info!("Web server ready"),
        Ok(Err(e)) => {
            tracing::error!("Failed to bind web server: {e}");
            return Err(e);
        }
        Err(_) => {
            return Err(anyhow::anyhow!("Web server failed during startup"));
        }
    }

    // Start job scheduler & runner after server binds
    let sched_token = scheduler_cancellation_token.clone();
    let scheduler_handle = tokio::spawn(async move {
        if let Err(e) = job_scheduler.run(sched_token).await {
            tracing::error!("Job scheduler error: {e}");
        }
    });
    let runner_token = scheduler_cancellation_token.clone();
    let queue_runner_handle = tokio::spawn(async move {
        if let Err(e) = job_queue_runner.run(runner_token).await {
            tracing::error!("Job queue runner error: {e}");
        }
    });

    tracing::info!("All background services started");

    // Await cancellation
    scheduler_cancellation_token.cancelled().await;
    tracing::info!("Shutdown requested, stopping background services...");

    let shutdown_timeout = tokio::time::timeout(Duration::from_secs(300), async move {
        let s = scheduler_handle.await;
        let r = queue_runner_handle.await;
        (s, r)
    });

    match shutdown_timeout.await {
        Ok((Ok(_), Ok(_))) => tracing::info!("Scheduler & runner stopped cleanly"),
        Ok((scheduler_res, runner_res)) => {
            if let Err(e) = scheduler_res {
                tracing::warn!("Scheduler join error: {e}");
            }
            if let Err(e) = runner_res {
                tracing::warn!("Runner join error: {e}");
            }
        }
        Err(_) => tracing::warn!("Timeout waiting for job services shutdown"),
    }

    // Stop web server
    web_server_cancellation_token.cancel();
    let shutdown_start = std::time::Instant::now();
    match tokio::time::timeout(Duration::from_secs(3), server_handle).await {
        Ok(_) => {
            tracing::info!("Web server stopped after {:?}", shutdown_start.elapsed());
        }
        Err(_) => {
            tracing::warn!(
                "Web server did not shutdown within timeout ({:?})",
                shutdown_start.elapsed()
            );
        }
    }

    tracing::info!("Shutdown complete");
    Ok(())
}
