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
use axum_tracing_opentelemetry::middleware::OtelAxumLayer;

use crate::{
    config::Config,
    data_mapping::DataMappingService,
    database::Database,
    ingestor::{IngestionStateManager, scheduler::{CacheInvalidationSender, SchedulerEvent}},
    logo_assets::{LogoAssetService, LogoAssetStorage},
    observability::AppObservability,
    runtime_settings::RuntimeSettingsStore,
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
pub use responses::{ApiResponse, PaginatedResponse, handle_error, handle_result, CacheControl, with_cache_headers, ok_with_cache};

/// Web server configuration and setup
pub struct WebServer {
    app: Router,
    addr: SocketAddr,
}

/// Builder for WebServer with many dependencies
#[derive(Clone)]
pub struct WebServerBuilder {
    pub config: Config,
    pub database: Database,
    pub state_manager: IngestionStateManager,
    pub cache_invalidation_tx: CacheInvalidationSender,
    pub data_mapping_service: DataMappingService,
    pub logo_asset_service: LogoAssetService,
    pub logo_asset_storage: LogoAssetStorage,
    pub proxy_regeneration_service: ProxyRegenerationService,
    pub temp_file_manager: SandboxedManager,
    pub pipeline_file_manager: SandboxedManager,
    pub logos_cached_file_manager: SandboxedManager,
    pub proxy_output_file_manager: SandboxedManager,
    pub relay_manager: std::sync::Arc<crate::services::relay_manager::RelayManager>,
    pub relay_config_resolver: crate::services::relay_config_resolver::RelayConfigResolver,
    pub system: std::sync::Arc<tokio::sync::RwLock<sysinfo::System>>,
    pub progress_service: std::sync::Arc<ProgressService>,
    pub stream_source_service: std::sync::Arc<crate::services::StreamSourceBusinessService>,
    pub epg_source_service: std::sync::Arc<crate::services::EpgSourceService>,
    pub log_broadcaster: broadcast::Sender<crate::web::api::log_streaming::LogEvent>,
    pub runtime_settings_store: Arc<RuntimeSettingsStore>,
    pub circuit_breaker_manager: Option<std::sync::Arc<crate::services::CircuitBreakerManager>>,
    pub observability: Arc<AppObservability>,
}

impl WebServerBuilder {
    pub async fn build(self) -> Result<WebServer> {
        WebServer::new_from_builder(self).await
    }
}

impl WebServer {
    /// Create a new web server with the refactored handler structure
    pub async fn new(builder: WebServerBuilder) -> Result<Self> {
        Self::new_from_builder(builder).await
    }
    
    /// Create a new web server from builder (internal implementation)
    async fn new_from_builder(builder: WebServerBuilder) -> Result<Self> {
        tracing::info!("WebServer using native pipeline");

        let logo_cache_scanner = {
            // Use proper file manager separation: cached vs uploaded
            Some(crate::services::logo_cache_scanner::LogoCacheScanner::new(
                builder.logos_cached_file_manager.clone(), // For logos cached from URLs
                builder.temp_file_manager.clone(),          // For manually uploaded logos
            ))
        };

        let source_linking_service = {
            use crate::database::repositories::{StreamSourceSeaOrmRepository, EpgSourceSeaOrmRepository};
            
            let stream_source_repo = StreamSourceSeaOrmRepository::new(builder.database.connection().clone());
            let epg_source_repo = EpgSourceSeaOrmRepository::new(builder.database.connection().clone());
            let url_linking_service = crate::services::UrlLinkingService::new(stream_source_repo, epg_source_repo);
            
            std::sync::Arc::new(crate::services::SourceLinkingService::new(
                StreamSourceSeaOrmRepository::new(builder.database.connection().clone()),
                EpgSourceSeaOrmRepository::new(builder.database.connection().clone()),
                url_linking_service,
            ))
        };

        let proxy_service = crate::proxy::ProxyService::new(
            builder.pipeline_file_manager.clone(),
            builder.proxy_output_file_manager.clone(),
        );

        let log_broadcaster = Some(builder.log_broadcaster.clone());

        let app = Self::create_router(AppState {
            database: builder.database.clone(),
            config: builder.config.clone(),
            state_manager: builder.state_manager,
            cache_invalidation_tx: builder.cache_invalidation_tx,
            data_mapping_service: builder.data_mapping_service,
            logo_asset_service: builder.logo_asset_service,
            logo_asset_storage: builder.logo_asset_storage,
            proxy_regeneration_service: builder.proxy_regeneration_service,
            preview_file_manager: builder.temp_file_manager.clone(),
            logo_file_manager: builder.logos_cached_file_manager,
            proxy_output_file_manager: builder.proxy_output_file_manager,
            temp_file_manager: builder.temp_file_manager,
            observability: builder.observability.clone(),
            scheduler_event_tx: None,
            logo_cache_scanner,
            session_tracker: std::sync::Arc::new(
                crate::proxy::session_tracker::SessionTracker::default(),
            ),
            relay_manager: builder.relay_manager,
            relay_config_resolver: builder.relay_config_resolver,
            system: builder.system,
            stream_source_service: builder.stream_source_service,
            epg_source_service: builder.epg_source_service,
            source_linking_service,
            proxy_service,
            progress_service: builder.progress_service,
            active_regeneration_requests: Arc::new(Mutex::new(HashSet::new())),
            log_broadcaster,
            start_time: chrono::Utc::now(),
            runtime_settings_store: builder.runtime_settings_store,
            circuit_breaker_manager: builder.circuit_breaker_manager,
        })
        .await;

        let addr: SocketAddr = format!("{}:{}", builder.config.web.host, builder.config.web.port).parse()?;

        Ok(Self { app, addr })
    }

    /// Create the router with all routes and middleware
    async fn create_router(state: AppState) -> Router {
        Router::new()
            // Health check endpoints (no auth required)
            .route("/health", get(handlers::health::health_check))
            .route("/ready", get(handlers::health::readiness_check))
            .route("/live", get(handlers::health::liveness_check))
            // Prometheus metrics endpoint
            .route("/metrics", get(handlers::metrics::prometheus_metrics))
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
            .route(
                "/channel/{channel_id}/stream",
                get(handlers::channels::proxy_channel_stream),
            )
            // .route("/logos/{logo_id}", get(handlers::static_assets::serve_logo))
            // Root route for basic index page
            .route("/", get(handlers::index::index))
            // Web interface routes are now handled by Next.js via fallback
            // Static assets
            .route("/favicon.ico", get(handlers::static_assets::serve_favicon))
            // Catch-all route for static assets - this should be LAST to avoid conflicts
            .fallback(handlers::static_assets::serve_embedded_asset)
            // Middleware (applied in reverse order)
            .layer(CorsLayer::permissive())
            // OpenTelemetry tracing middleware (should be outer layer to capture all requests)
            .layer(OtelAxumLayer::default())
            // Security headers middleware
            .layer(axum::middleware::from_fn(middleware::security_headers_middleware))
            // Conditional request logging middleware (respects runtime settings)
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                middleware::conditional_request_logging_middleware
            ))
            // Shared state
            .with_state(state)
    }

    /// OpenAPI documentation routes
    fn openapi_routes() -> Router<AppState> {
        use utoipa_swagger_ui::SwaggerUi;
        
        Router::new()
            // Swagger UI integration - automatically serves both /docs and /api/openapi.json
            .merge(SwaggerUi::new("/docs")
                .url("/api/openapi.json", openapi::get_comprehensive_openapi_spec()))
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
            // Stream source refresh route
            .route(
                "/sources/stream/{id}/refresh",
                post(handlers::stream_sources::refresh_stream_source),
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
            // Progress events SSE endpoint
            .route("/progress/events", get(api::progress_events::progress_events_stream))
            // Progress operations REST endpoint  
            .route("/progress/operations", get(api::get_operation_progress))
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
            .route("/logos/{id}/image", put(api::replace_logo_asset_image))
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
            // Expression validation (generalized endpoints)
            .route("/expressions/validate", post(api::validate_expression))
            .route("/expressions/validate/stream", post(api::validate_stream_expression))
            .route("/expressions/validate/epg", post(api::validate_epg_expression))
            .route("/expressions/validate/data-mapping", post(api::validate_data_mapping_expression))
            // Filters
            .route("/filters", get(api::list_filters).post(api::create_filter))
            .route(
                "/filters/{id}",
                get(api::get_filter)
                    .put(api::update_filter)
                    .delete(api::delete_filter),
            )
            .route("/filters/test", post(api::test_filter))
            .route("/filters/fields/stream", get(api::get_stream_filter_fields))
            .route("/filters/fields/epg", get(api::get_epg_filter_fields))
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
            .route("/data-mapping/helpers", get(api::get_data_mapping_helpers))
            .route("/data-mapping/helpers/logo/search", get(api::search_logo_assets_for_helper))
            .route("/data-mapping/helpers/date/complete", post(api::get_date_completion_options))
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
            .route("/proxies/regeneration/status", get(api::get_regeneration_queue_status))
            // Relay system endpoints
            .merge(api::relay::relay_routes())
            // Metrics and analytics
            .route("/metrics/dashboard", get(api::get_dashboard_metrics))
            // Log streaming endpoints
            .route("/logs/stream", get(api::log_streaming::stream_logs))
            .route("/logs/stats", get(api::log_streaming::get_log_stats))
            .route("/logs/test", post(api::log_streaming::send_test_log))
            // Runtime settings endpoints
            .route("/settings", get(api::settings::get_settings))
            .route("/settings", put(api::settings::update_settings))
            .route("/settings/info", get(api::settings::get_settings_info))
            // Feature flags endpoints
            .route("/features", get(handlers::features::get_features)
                .put(handlers::features::update_features))
            // Channel browser endpoints
            .route("/channels", get(handlers::channels::list_channels))
            .route("/channels/proxy/{proxy_id}", get(handlers::channels::get_proxy_channels))
            .route("/channels/{channel_id}/stream", get(handlers::channels::get_channel_stream))
            .route("/channels/{channel_id}/probe", post(handlers::channels::probe_channel_codecs))
            // EPG viewer endpoints
            .route("/epg/programs", get(handlers::epg::list_epg_programs))
            .route("/epg/programs/{source_id}", get(handlers::epg::get_source_epg_programs))
            .route("/epg/sources", get(handlers::epg::list_epg_sources))
            .route("/epg/guide", get(handlers::epg::get_epg_guide))
            // Circuit breaker management endpoints
            .route("/circuit-breakers", get(handlers::circuit_breaker::get_circuit_breaker_stats))
            .route("/circuit-breakers/config", 
                get(handlers::circuit_breaker::get_circuit_breaker_config)
                .put(handlers::circuit_breaker::update_circuit_breaker_config))
            .route("/circuit-breakers/services", get(handlers::circuit_breaker::list_active_services))
            .route("/circuit-breakers/services/{service_name}", put(handlers::circuit_breaker::update_service_profile))
            .route("/circuit-breakers/services/{service_name}/force", post(handlers::circuit_breaker::force_circuit_state))
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
        self.serve_with_cancellation(ready_signal, None).await
    }

    /// Serve with cancellation support and ready notification
    pub async fn serve_with_cancellation(
        self,
        ready_signal: tokio::sync::oneshot::Sender<Result<()>>,
        cancellation_token: Option<tokio_util::sync::CancellationToken>,
    ) -> Result<()> {
        match tokio::net::TcpListener::bind(&self.addr).await {
            Ok(listener) => {
                // Signal that we're now actually listening on the port
                let _ = ready_signal.send(Ok(()));

                // Create graceful shutdown signal
                let shutdown_signal = async move {
                    if let Some(token) = &cancellation_token {
                        token.cancelled().await;
                        tracing::info!("Web server received cancellation signal, shutting down gracefully");
                    } else {
                        // Fallback to signal handling if no cancellation token provided
                        #[cfg(unix)]
                        {
                            use tokio::signal::unix::{signal, SignalKind};
                            let mut sigterm = signal(SignalKind::terminate()).expect("failed to install SIGTERM handler");
                            let mut sigint = signal(SignalKind::interrupt()).expect("failed to install SIGINT handler");
                            
                            tokio::select! {
                                _ = sigterm.recv() => {
                                    tracing::info!("Received SIGTERM, shutting down gracefully");
                                }
                                _ = sigint.recv() => {
                                    tracing::info!("Received SIGINT (Ctrl+C), shutting down gracefully");
                                }
                            }
                        }
                        
                        #[cfg(not(unix))]
                        {
                            use tokio::signal;
                            signal::ctrl_c().await.expect("failed to install Ctrl+C handler");
                            tracing::info!("Received Ctrl+C, shutting down gracefully");
                        }
                    }
                };

                // Now serve until shutdown signal
                axum::serve(listener, self.app)
                    .with_graceful_shutdown(shutdown_signal)
                    .await?;
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
    pub observability: Arc<AppObservability>,
    pub scheduler_event_tx: Option<mpsc::UnboundedSender<SchedulerEvent>>,
    pub logo_cache_scanner: Option<crate::services::logo_cache_scanner::LogoCacheScanner>,
    pub session_tracker: std::sync::Arc<crate::proxy::session_tracker::SessionTracker>,
    pub relay_manager: std::sync::Arc<crate::services::relay_manager::RelayManager>,
    pub relay_config_resolver: crate::services::relay_config_resolver::RelayConfigResolver,
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
    /// Application start time for uptime calculation
    pub start_time: chrono::DateTime<chrono::Utc>,
    /// Runtime settings store for dynamic configuration changes
    pub runtime_settings_store: Arc<RuntimeSettingsStore>,
    /// Circuit breaker manager for resilience patterns
    pub circuit_breaker_manager: Option<std::sync::Arc<crate::services::CircuitBreakerManager>>,
}

impl AppState {}
