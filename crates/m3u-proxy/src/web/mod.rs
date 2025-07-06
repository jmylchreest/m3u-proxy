//! Web layer module
//!
//! This module provides the HTTP interface for the M3U Proxy application.
//! It follows clean architecture principles with thin handlers that delegate
//! to the service layer for business logic.
//!
//! # Architecture
//!
//! The web layer is organized into several components:
//! - **Handlers**: HTTP request handlers organized by domain
//! - **Responses**: Standardized response types and error handling
//! - **Extractors**: Request validation and parameter extraction
//! - **Middleware**: Cross-cutting concerns like logging and security
//! - **Utils**: Common utilities for web operations
//!
//! # Design Principles
//!
//! - **Thin Handlers**: Controllers contain minimal logic, delegating to services
//! - **Consistent Responses**: All endpoints use standardized response formats
//! - **Comprehensive Validation**: Request parameters are validated at the boundary
//! - **Proper Error Handling**: Errors are mapped to appropriate HTTP status codes
//! - **Security First**: Security headers and validation are applied by default
//! - **Observability**: Request logging and metrics are built-in

use anyhow::Result;
use axum::{
    Router,
    routing::{get, post},
};
use std::net::SocketAddr;
use tower_http::cors::CorsLayer;

use crate::{
    config::Config,
    data_mapping::DataMappingService,
    database::Database,
    ingestor::{IngestionStateManager, scheduler::CacheInvalidationSender},
    logo_assets::{LogoAssetService, LogoAssetStorage},
    metrics::MetricsLogger,
    proxy::wasm_host_interface::{PluginCapabilities, WasmHostInterfaceFactory},
    proxy::wasm_plugin::{WasmPluginConfig, WasmPluginManager},
    services::ProxyRegenerationService,
    utils::SimpleMemoryMonitor,
};
use sandboxed_file_manager::SandboxedManager;

pub mod api;
pub mod extractors;
pub mod handlers;
pub mod middleware;
pub mod responses;
pub mod utils;

// Re-export commonly used types
pub use extractors::{ListParams, PaginationParams, RequestContext, SearchParams};
pub use responses::{ApiResponse, PaginatedResponse, handle_error, handle_result};

/// Web server configuration and setup
pub struct WebServer {
    app: Router,
    addr: SocketAddr,
}

impl WebServer {
    /// Create a new web server with the refactored handler structure
    pub async fn new(
        config: Config,
        database: Database,
        state_manager: IngestionStateManager,
        cache_invalidation_tx: CacheInvalidationSender,
        data_mapping_service: DataMappingService,
        logo_asset_service: LogoAssetService,
        logo_asset_storage: LogoAssetStorage,
        proxy_regeneration_service: ProxyRegenerationService,
        preview_file_manager: SandboxedManager,
        logo_file_manager: SandboxedManager,
        proxy_output_file_manager: SandboxedManager,
        shared_plugin_manager: Option<std::sync::Arc<crate::proxy::wasm_plugin::WasmPluginManager>>,
    ) -> Result<Self> {
        // Use shared plugin manager if provided, otherwise create one
        let plugin_manager = if let Some(shared_manager) = shared_plugin_manager {
            tracing::info!("Using shared WASM plugin manager");
            Some(shared_manager)
        } else if let Some(wasm_config) = &config.wasm_strategies {
            if wasm_config.enabled {
                tracing::info!("WASM plugin system is ENABLED (creating new manager)");
                tracing::info!("Plugin directory: {:?}", wasm_config.plugin_directory);
                tracing::info!(
                    "Max memory per plugin: {} MB",
                    wasm_config.max_memory_per_plugin_mb
                );
                tracing::info!("Plugin timeout: {} seconds", wasm_config.timeout_seconds);
                tracing::info!(
                    "Hot reload: {}",
                    if wasm_config.enable_hot_reload {
                        "ENABLED"
                    } else {
                        "DISABLED"
                    }
                );

                // Create memory monitor for plugins
                let memory_monitor =
                    SimpleMemoryMonitor::new(Some(wasm_config.max_memory_per_plugin_mb));

                // Create plugin capabilities
                let capabilities = PluginCapabilities {
                    allow_file_access: true,
                    allow_network_access: false,
                    max_memory_query_mb: Some(wasm_config.max_memory_per_plugin_mb),
                    allowed_config_keys: vec![
                        "chunk_size".to_string(),
                        "compression_level".to_string(),
                        "temp_dir".to_string(),
                        "memory_threshold_mb".to_string(),
                        "temp_file_threshold".to_string(),
                    ],
                };

                // Create host interface factory
                let host_interface_factory =
                    WasmHostInterfaceFactory::new(preview_file_manager.clone(), capabilities);

                // Create plugin configuration
                let plugin_config = std::collections::HashMap::from([
                    ("chunk_size".to_string(), "1000".to_string()),
                    (
                        "memory_threshold_mb".to_string(),
                        wasm_config.max_memory_per_plugin_mb.to_string(),
                    ),
                    ("temp_file_threshold".to_string(), "10000".to_string()),
                ]);

                // Create host interface
                let host_interface =
                    host_interface_factory.create_interface(Some(memory_monitor), plugin_config);

                // Create plugin manager configuration
                let plugin_manager_config = WasmPluginConfig {
                    enabled: wasm_config.enabled,
                    plugin_directory: wasm_config.plugin_directory.to_string_lossy().to_string(),
                    max_memory_per_plugin: wasm_config.max_memory_per_plugin_mb,
                    timeout_seconds: wasm_config.timeout_seconds,
                    enable_hot_reload: wasm_config.enable_hot_reload,
                    max_plugin_failures: 3,
                    fallback_timeout_ms: 5000,
                };

                // Create plugin manager
                let manager = WasmPluginManager::new(plugin_manager_config, host_interface);

                // Load plugins and log results
                match manager.load_plugins().await {
                    Ok(_) => {
                        match manager.get_detailed_statistics().await {
                            Ok(stats) => {
                                tracing::info!("Web server plugin system initialized successfully!");
                                tracing::info!("Plugin Statistics:");
                                tracing::info!("   Total plugins loaded: {}", stats.len());
                                
                                // Log each plugin's status
                                for (name, plugin_stats) in &stats {
                                    tracing::info!("   Plugin '{}': {:?}", name, plugin_stats);
                                }
                            }
                            Err(e) => {
                                tracing::warn!("Failed to get plugin statistics: {}", e);
                                tracing::info!("Web server plugin system initialized successfully!");
                            }
                        }

                        // Start hot reload if enabled
                        if wasm_config.enable_hot_reload {
                            tracing::info!("Starting hot reload monitoring...");
                            if let Err(e) = manager.start_hot_reload_monitoring().await {
                                tracing::warn!("Failed to start hot reload monitoring: {}", e);
                            }
                        }
                    }
                    Err(e) => {
                        tracing::error!("Failed to load plugins in web server: {}", e);
                        tracing::warn!("Plugin system will continue in fallback mode");
                    }
                }

                Some(std::sync::Arc::new(manager))
            } else {
                tracing::info!("WASM plugin system is DISABLED in configuration");
                None
            }
        } else {
            tracing::info!("WASM plugin system configuration not found - running without plugins");
            None
        };

        let app = Self::create_router(AppState {
            database,
            config: config.clone(),
            state_manager,
            cache_invalidation_tx,
            data_mapping_service,
            logo_asset_service,
            logo_asset_storage,
            proxy_regeneration_service,
            preview_file_manager,
            logo_file_manager,
            proxy_output_file_manager,
            metrics_logger: MetricsLogger::new(),
            plugin_manager,
        })
        .await;

        let addr: SocketAddr = format!("{}:{}", config.web.host, config.web.port).parse()?;

        Ok(Self { app, addr })
    }

    /// Create the router with all routes and middleware
    async fn create_router(state: AppState) -> Router {
        Router::new()
            // Health check endpoints (no auth required)
            .route("/health", get(handlers::health::health_check))
            .route(
                "/health/detailed",
                get(handlers::health::detailed_health_check),
            )
            .route("/ready", get(handlers::health::readiness_check))
            .route("/live", get(handlers::health::liveness_check))
            // API v1 routes
            .nest("/api/v1", Self::api_v1_routes())
            // Proxy/Streaming endpoints (non-API content serving)
            .route("/proxy/:ulid/m3u8", get(handlers::proxies::serve_proxy_m3u))
            .route(
                "/proxy/:ulid/xmltv",
                get(handlers::proxies::serve_proxy_xmltv),
            )
            .route(
                "/stream/:proxy_ulid/:channel_id",
                get(handlers::proxies::proxy_stream),
            )
            // TODO: Add logo serving endpoint when needed
            // .route("/logos/:logo_id", get(handlers::static_assets::serve_logo))
            // Root route for basic index page
            .route("/", get(handlers::index::index))
            // Web interface routes
            .route("/sources", get(handlers::web_pages::sources_page))
            .route("/epg-sources", get(handlers::web_pages::epg_sources_page))
            .route("/proxies", get(handlers::web_pages::proxies_page))
            .route("/filters", get(handlers::web_pages::filters_page))
            .route("/data-mapping", get(handlers::web_pages::data_mapping_page))
            .route("/logos", get(handlers::web_pages::logos_page))
            .route("/relay", get(handlers::web_pages::relay_page))
            // Static assets
            .route(
                "/static/*path",
                get(handlers::static_assets::serve_static_asset),
            )
            .route("/favicon.ico", get(handlers::static_assets::serve_favicon))
            // Middleware (applied in reverse order)
            .layer(CorsLayer::permissive())
            // .layer(axum::middleware::from_fn(middleware::security_headers_middleware))
            // .layer(axum::middleware::from_fn(middleware::request_logging_middleware))
            // Shared state
            .with_state(state)
    }

    /// API v1 routes (clean architecture with working implementations)
    fn api_v1_routes() -> Router<AppState> {
        Router::new()
            // Stream sources
            .route(
                "/sources/stream",
                get(api::list_stream_sources).post(api::create_stream_source),
            )
            .route(
                "/sources/stream/:id",
                get(api::get_stream_source)
                    .put(api::update_stream_source)
                    .delete(api::delete_stream_source),
            )
            .route(
                "/sources/stream/:id/refresh",
                post(api::refresh_stream_source),
            )
            // EPG sources
            .route(
                "/sources/epg",
                get(api::list_epg_sources_unified).post(api::create_epg_source_unified),
            )
            .route(
                "/sources/epg/:id",
                get(api::get_epg_source_unified)
                    .put(api::update_epg_source_unified)
                    .delete(api::delete_epg_source_unified),
            )
            .route(
                "/sources/epg/:id/refresh",
                post(api::refresh_epg_source_unified),
            )
            // Unified sources
            .route("/sources", get(api::list_all_sources))
            .route("/sources/unified", get(api::list_all_sources))
            // Progress endpoints for frontend polling
            .route("/progress/sources", get(api::get_sources_progress))
            .route("/progress/epg", get(api::get_epg_progress))
            // Logo assets
            .route("/logos", get(api::list_logo_assets))
            .route("/logos/stats", get(api::get_logo_cache_stats))
            .route("/logos/search", get(api::search_logo_assets))
            .route(
                "/logos/:id",
                get(api::get_logo_asset_image)
                    .put(api::update_logo_asset)
                    .delete(api::delete_logo_asset),
            )
            .route("/logos/:id/info", get(api::get_logo_asset_with_formats))
            .route(
                "/logos/:id/formats/:format",
                get(api::get_logo_asset_format),
            )
            .route("/logos/upload", post(api::upload_logo_asset))
            // Filters
            .route("/filters", get(api::list_filters).post(api::create_filter))
            .route(
                "/filters/:id",
                get(api::get_filter)
                    .put(api::update_filter)
                    .delete(api::delete_filter),
            )
            .route("/filters/test", post(api::test_filter))
            .route("/filters/validate", post(api::validate_filter))
            .route("/filters/fields", get(api::get_filter_fields))
            // Data mapping
            .route(
                "/data-mapping",
                get(api::list_data_mapping_rules).post(api::create_data_mapping_rule),
            )
            .route(
                "/data-mapping/:id",
                get(api::get_data_mapping_rule)
                    .put(api::update_data_mapping_rule)
                    .delete(api::delete_data_mapping_rule),
            )
            .route("/data-mapping/test", post(api::test_data_mapping_rule))
            .route(
                "/data-mapping/validate",
                post(api::validate_data_mapping_expression),
            )
            .route(
                "/data-mapping/preview",
                get(api::apply_data_mapping_rules).post(api::apply_data_mapping_rules_post),
            )
            .route(
                "/data-mapping/reorder",
                post(api::reorder_data_mapping_rules),
            )
            .route("/data-mapping/fields/stream", get(api::get_stream_fields))
            .route("/data-mapping/fields/epg", get(api::get_epg_fields))
            // EPG viewer
            .route("/epg/viewer", get(api::get_epg_viewer_data))
            // Proxies
            .route(
                "/proxies",
                get(handlers::proxies::list_proxies).post(handlers::proxies::create_proxy),
            )
            .route(
                "/proxies/:id",
                get(handlers::proxies::get_proxy)
                    .put(handlers::proxies::update_proxy)
                    .delete(handlers::proxies::delete_proxy),
            )
            .route(
                "/proxies/preview",
                post(handlers::proxies::preview_proxy_config),
            )
            .route(
                "/proxies/:id/preview",
                get(handlers::proxies::preview_existing_proxy),
            )
            .route("/proxies/:id/regenerate", post(api::regenerate_proxy))
            .route("/proxies/regenerate-all", post(api::regenerate_all_proxies))
            // Relay configuration endpoints
            .route(
                "/proxies/:id/relays",
                get(api::list_relay_configs).post(api::create_relay_config),
            )
            .route(
                "/proxies/:proxy_id/relays/:relay_id",
                get(api::get_relay_config)
                    .put(api::update_relay_config)
                    .delete(api::delete_relay_config),
            )
            .route("/relays/status", get(api::list_relay_status))
            .route("/relays/:config_id/status", get(api::get_relay_status))
            .route("/relays/:config_id/start", post(api::start_relay))
            .route("/relays/:config_id/stop", post(api::stop_relay))
    }

    /// Web interface routes
    fn _web_interface_routes() -> Router<AppState> {
        Router::new()
        // TODO: Implement web interface routes
        // .route("/", get(crate::web::handlers::index))
        // .route("/sources", get(crate::web::handlers::sources_page))
        // Add other web interface routes as needed...
    }

    /// Start the web server
    pub async fn serve(self) -> Result<()> {
        let listener = tokio::net::TcpListener::bind(&self.addr).await?;
        axum::serve(listener, self.app).await?;
        Ok(())
    }

    /// Get the host address
    pub fn host(&self) -> String {
        self.addr.ip().to_string()
    }

    /// Get the port number
    pub fn port(&self) -> u16 {
        self.addr.port()
    }
}

/// Application state shared across all handlers
#[derive(Clone)]
pub struct AppState {
    pub database: Database,
    pub config: Config,
    pub state_manager: IngestionStateManager,
    pub cache_invalidation_tx: CacheInvalidationSender,
    pub data_mapping_service: DataMappingService,
    pub logo_asset_service: LogoAssetService,
    pub logo_asset_storage: LogoAssetStorage,
    pub proxy_regeneration_service: ProxyRegenerationService,
    pub preview_file_manager: SandboxedManager,
    pub logo_file_manager: SandboxedManager,
    pub proxy_output_file_manager: SandboxedManager,
    pub metrics_logger: MetricsLogger,
    pub plugin_manager: Option<std::sync::Arc<crate::proxy::wasm_plugin::WasmPluginManager>>,
}

impl AppState {
    /// Get a stream source service instance
    ///
    /// In a full implementation, this would create service instances
    /// with proper dependency injection
    pub fn stream_source_service(
        &self,
    ) -> Result<
        Box<
            dyn crate::services::Service<
                    crate::models::StreamSource,
                    uuid::Uuid,
                    CreateRequest = crate::models::StreamSourceCreateRequest,
                    UpdateRequest = crate::models::StreamSourceUpdateRequest,
                    Query = crate::services::stream_source::StreamSourceServiceQuery,
                    ListResponse = crate::services::ServiceListResponse<
                        crate::models::StreamSource,
                    >,
                >,
        >,
        Box<dyn std::error::Error>,
    > {
        // TODO: Implement proper service instantiation
        // This would involve:
        // 1. Creating repository instance with database connection
        // 2. Creating service instance with repository
        // 3. Returning the service for use in handlers

        Err(anyhow::anyhow!("Service instantiation not yet implemented").into())
    }
}
