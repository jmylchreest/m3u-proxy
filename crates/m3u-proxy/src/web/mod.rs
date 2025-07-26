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
    routing::{get, post, put},
};
use std::net::SocketAddr;
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::Mutex;
use tower_http::cors::CorsLayer;
use uuid::Uuid;

use crate::{
    config::Config,
    data_mapping::DataMappingService,
    database::Database,
    ingestor::{IngestionStateManager, scheduler::{CacheInvalidationSender, SchedulerEvent}},
    logo_assets::{LogoAssetService, LogoAssetStorage},
    metrics::MetricsLogger,
    services::{ProxyRegenerationService, progress_service::ProgressService},
};
use tokio::sync::mpsc;
use sandboxed_file_manager::SandboxedManager;
use tokio::sync::broadcast;

pub mod api;
pub mod extractors;
pub mod handlers;
pub mod middleware;
pub mod openapi;
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
        temp_file_manager: SandboxedManager, // Use temp for both temp and preview operations
        pipeline_file_manager: SandboxedManager, // Pipeline-specific file manager
        logos_cached_file_manager: SandboxedManager,
        proxy_output_file_manager: SandboxedManager,
        relay_manager: std::sync::Arc<crate::services::relay_manager::RelayManager>,
        system: std::sync::Arc<tokio::sync::RwLock<sysinfo::System>>,
        progress_service: std::sync::Arc<ProgressService>,
        log_broadcaster: broadcast::Sender<crate::web::api::log_streaming::LogEvent>,
    ) -> Result<Self> {
        tracing::info!("WebServer using native pipeline");

        // Create logo cache scanner for cached logo discovery
        // Use the same path that the logo asset storage is configured with
        let logo_cache_scanner = {
            let base_path = logo_asset_storage.cached_logo_dir.clone();

            Some(crate::services::logo_cache_scanner::LogoCacheScanner::new(
                logos_cached_file_manager.clone(),
                base_path,
            ))
        };

        // Use shared progress service passed from main application

        // Initialize new service layer components
        let epg_source_service = std::sync::Arc::new(crate::services::EpgSourceService::new(
            database.clone(),
            cache_invalidation_tx.clone(),
        ));

        let stream_source_service =
            std::sync::Arc::new(crate::services::StreamSourceBusinessService::new(
                database.clone(),
                epg_source_service.clone(),
                cache_invalidation_tx.clone(),
            ));

        let source_linking_service =
            std::sync::Arc::new(crate::services::SourceLinkingService::new(database.clone()));

        // Create proxy service with pipeline file manager for pipeline operations
        let proxy_service = crate::proxy::ProxyService::new(
            config.storage.clone(),
            pipeline_file_manager.clone(),
            proxy_output_file_manager.clone(),
            system.clone(),
        );

        // Use the log broadcaster passed from main.rs (already wired to tracing subscriber)
        let log_broadcaster = Some(log_broadcaster);

        let app = Self::create_router(AppState {
            database: database.clone(),
            config: config.clone(),
            state_manager,
            cache_invalidation_tx,
            data_mapping_service,
            logo_asset_service,
            logo_asset_storage,
            proxy_regeneration_service,
            preview_file_manager: temp_file_manager.clone(), // Use temp for preview operations
            logo_file_manager: logos_cached_file_manager,
            proxy_output_file_manager,
            temp_file_manager,
            metrics_logger: MetricsLogger::new(database.pool()),
            scheduler_event_tx: None, // Will be set later when scheduler is initialized
            logo_cache_scanner,
            session_tracker: std::sync::Arc::new(
                crate::proxy::session_tracker::SessionTracker::default(),
            ),
            relay_manager,
            system,
            // New service layer components
            stream_source_service,
            epg_source_service,
            source_linking_service,
            proxy_service,
            progress_service,
            active_regeneration_requests: Arc::new(Mutex::new(HashSet::new())),
            log_broadcaster,
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
            // OpenAPI documentation
            .merge(Self::openapi_routes())
            // API v1 routes
            .nest("/api/v1", Self::api_v1_routes())
            // Proxy/Streaming endpoints (non-API content serving)
            .route("/proxy/{ulid}/m3u8", get(handlers::proxies::serve_proxy_m3u))
            .route(
                "/proxy/{ulid}/xmltv",
                get(handlers::proxies::serve_proxy_xmltv),
            )
            .route(
                "/stream/{proxy_ulid}/{channel_id}",
                get(handlers::proxies::proxy_stream),
            )
            // .route("/logos/{logo_id}", get(handlers::static_assets::serve_logo))
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
                "/static/{*path}",
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

    /// OpenAPI documentation routes
    fn openapi_routes() -> Router<AppState> {
        use utoipa_rapidoc::RapiDoc;
        
        Router::new()
            // RapiDoc interactive documentation (includes OpenAPI spec endpoint)
            .merge(
                RapiDoc::with_openapi("/api/openapi.json", openapi::get_comprehensive_openapi_spec())
                    .path("/docs")
            )
    }

    /// API v1 routes with standard Axum routing
    fn api_v1_routes() -> Router<AppState> {        
        Router::new()
            // Stream Sources routes (with utoipa annotations)
            .route(
                "/sources/stream",
                get(handlers::stream_sources::list_stream_sources)
                    .post(handlers::stream_sources::create_stream_source),
            )
            .route(
                "/sources/stream/{id}",
                get(handlers::stream_sources::get_stream_source)
                    .put(handlers::stream_sources::update_stream_source)
                    .delete(handlers::stream_sources::delete_stream_source),
            )
            .route(
                "/sources/stream/validate",
                post(handlers::stream_sources::validate_stream_source),
            )
            .route(
                "/sources/capabilities/{source_type}",
                get(handlers::stream_sources::get_stream_source_capabilities),
            )
            // EPG Sources routes (with utoipa annotations)
            .route(
                "/sources/epg",
                get(handlers::epg_sources::list_epg_sources)
                    .post(handlers::epg_sources::create_epg_source),
            )
            .route(
                "/sources/epg/{id}",
                get(handlers::epg_sources::get_epg_source)
                    .put(handlers::epg_sources::update_epg_source)
                    .delete(handlers::epg_sources::delete_epg_source),
            )
            .route(
                "/sources/epg/validate",
                post(handlers::epg_sources::validate_epg_source),
            )
            // Additional routes that don't have utoipa annotations yet
            .route(
                "/sources/stream/{id}/refresh",
                post(api::refresh_stream_source),
            )
            .route(
                "/sources/stream/{id}/channels",
                get(api::get_stream_source_channels),
            )
            .route(
                "/sources/epg/{id}/refresh",
                post(api::refresh_epg_source_unified),
            )
            .route(
                "/sources/epg/{id}/channels",
                get(api::get_epg_source_channels_unified),
            )
            // Unified sources
            .route("/sources", get(api::list_all_sources))
            .route("/sources/unified", get(api::list_all_sources))
            // Legacy progress endpoints (deprecated - commented out to avoid conflicts)
            // .route("/progress/sources", get(api::get_sources_progress))
            // .route("/progress/epg", get(api::get_epg_progress))
            // New unified progress endpoints
            .route("/progress", get(api::unified_progress::get_unified_progress))
            .route("/progress/events", get(api::unified_progress::progress_events_stream))
            .route("/progress/operations/{operation_id}", get(api::unified_progress::get_operation_progress))
            .route("/progress/streams", get(api::unified_progress::get_stream_progress))
            .route("/progress/epg", get(api::unified_progress::get_epg_progress))
            .route("/progress/proxies", get(api::unified_progress::get_proxy_progress))
            .route("/progress/resources/streams/{source_id}", get(api::unified_progress::get_stream_source_progress))
            .route("/progress/resources/epg/{source_id}", get(api::unified_progress::get_epg_source_progress))
            .route("/progress/resources/proxies/{proxy_id}", get(api::unified_progress::get_proxy_regeneration_progress))
            // Logo assets
            .route("/logos", get(api::list_logo_assets))
            .route("/logos/stats", get(api::get_logo_cache_stats))
            .route("/logos/search", get(api::search_logo_assets))
            .route(
                "/logos/{id}",
                get(api::get_logo_asset_image)
                    .put(api::update_logo_asset)
                    .delete(api::delete_logo_asset),
            )
            .route("/logos/{id}/info", get(api::get_logo_asset_with_formats))
            .route(
                "/logos/{id}/formats/{format}",
                get(api::get_logo_asset_format),
            )
            .route("/logos/upload", post(api::upload_logo_asset))
            .route(
                "/logos/generate-metadata",
                post(api::generate_cached_logo_metadata),
            )
            // Cached logo endpoint (uses sandboxed file manager, no database)
            .route("/logos/cached/{cache_id}", get(api::get_cached_logo_asset))
            // Filters
            .route("/filters", get(api::list_filters).post(api::create_filter))
            .route(
                "/filters/{id}",
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
                "/data-mapping/{id}",
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
            .route("/data-mapping/fields/stream", get(api::get_data_mapping_stream_fields))
            .route("/data-mapping/fields/epg", get(api::get_data_mapping_epg_fields))
            // Generalized pipeline validation endpoints
            .route("/pipeline/validate", post(api::validate_pipeline_expression))
            .route("/pipeline/fields/{stage}", get(api::get_pipeline_stage_fields))
            // EPG viewer
            .route("/epg/viewer", get(api::get_epg_viewer_data))
            // Proxies
            .route(
                "/proxies",
                get(handlers::proxies::list_proxies).post(handlers::proxies::create_proxy),
            )
            .route(
                "/proxies/{id}",
                get(handlers::proxies::get_proxy)
                    .put(handlers::proxies::update_proxy)
                    .delete(handlers::proxies::delete_proxy),
            )
            .route(
                "/proxies/preview",
                post(handlers::proxies::preview_proxy_config),
            )
            .route(
                "/proxies/{id}/preview",
                get(handlers::proxies::preview_existing_proxy),
            )
            .route("/proxies/{id}/regenerate", post(api::regenerate_proxy))
            .route("/proxies/regenerate-all", post(api::regenerate_all_proxies))
            .route("/proxies/regeneration/status", get(api::get_regeneration_queue_status))
            .route("/progress/regeneration", get(api::get_proxy_regeneration_progress))
            // Relay system endpoints
            .merge(api::relay::relay_routes())
            // Active relay monitoring
            .route("/active-relays", get(api::active_relays::get_active_relays))
            .route("/active-relays/{config_id}", get(api::active_relays::get_active_relay_by_id))
            .route("/active-relays/health", get(api::active_relays::get_relay_health))
            // Metrics and analytics
            .route("/metrics/dashboard", get(api::get_dashboard_metrics))
            .route("/metrics/realtime", get(api::get_realtime_metrics))
            .route("/metrics/usage", get(api::get_usage_metrics))
            .route("/metrics/channels/popular", get(api::get_popular_channels))
            // Log streaming endpoints
            .route("/logs/stream", get(api::log_streaming::stream_logs))
            .route("/logs/stats", get(api::log_streaming::get_log_stats))
            .route("/logs/test", post(api::log_streaming::send_test_log))
            // Runtime settings endpoints
            .route("/settings", get(api::settings::get_settings))
            .route("/settings", put(api::settings::update_settings))
            .route("/settings/info", get(api::settings::get_settings_info))
    }

    /// Start the web server
    pub async fn serve(self) -> Result<()> {
        let listener = tokio::net::TcpListener::bind(&self.addr).await?;
        axum::serve(listener, self.app).await?;
        Ok(())
    }

    /// Serve with a notification when the server is actually listening or fails to bind
    pub async fn serve_with_signal(
        self,
        ready_signal: tokio::sync::oneshot::Sender<Result<()>>,
    ) -> Result<()> {
        match tokio::net::TcpListener::bind(&self.addr).await {
            Ok(listener) => {
                // Signal that we're now actually listening on the port
                let _ = ready_signal.send(Ok(()));

                // Now serve until shutdown
                axum::serve(listener, self.app).await?;
                Ok(())
            }
            Err(bind_error) => {
                // Signal the bind failure immediately
                let bind_err_msg = format!("Failed to bind to {}: {}", self.addr, bind_error);
                let _ = ready_signal.send(Err(anyhow::anyhow!("{}", bind_err_msg)));
                Err(anyhow::anyhow!("{}", bind_err_msg))
            }
        }
    }

    /// Get the host address
    pub fn host(&self) -> String {
        self.addr.ip().to_string()
    }

    /// Get the port number
    pub fn port(&self) -> u16 {
        self.addr.port()
    }
    
    /// Wire up duplicate protection between API requests and background auto-regeneration
    /// This prevents race conditions where manual and automatic regenerations run simultaneously
    pub async fn wire_duplicate_protection(&mut self) {
        // The duplicate protection is now handled via the shared progress service
        // The has_active_regeneration() method checks both local state and progress service
        tracing::info!("Duplicate protection enabled via shared progress service tracking");
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
    pub temp_file_manager: SandboxedManager,
    pub metrics_logger: MetricsLogger,
    pub scheduler_event_tx: Option<mpsc::UnboundedSender<SchedulerEvent>>,
    pub logo_cache_scanner: Option<crate::services::logo_cache_scanner::LogoCacheScanner>,
    pub session_tracker: std::sync::Arc<crate::proxy::session_tracker::SessionTracker>,
    pub relay_manager: std::sync::Arc<crate::services::relay_manager::RelayManager>,
    pub system: std::sync::Arc<tokio::sync::RwLock<sysinfo::System>>,
    // New service layer components
    pub stream_source_service: std::sync::Arc<crate::services::StreamSourceBusinessService>,
    pub epg_source_service: std::sync::Arc<crate::services::EpgSourceService>,
    pub source_linking_service: std::sync::Arc<crate::services::SourceLinkingService>,
    pub proxy_service: crate::proxy::ProxyService,
    pub progress_service: std::sync::Arc<ProgressService>,
    /// CONCURRENCY FIX: Track active API regeneration requests to prevent duplicates
    pub active_regeneration_requests: Arc<Mutex<HashSet<Uuid>>>,
    /// Log broadcaster for SSE streaming
    pub log_broadcaster: Option<broadcast::Sender<crate::web::api::log_streaming::LogEvent>>,
}

impl AppState {}
