use anyhow::Result;
use axum::{
    routing::{delete, get, post},
    Router,
};
use std::net::SocketAddr;
use tower_http::cors::CorsLayer;

use crate::{
    config::Config,
    data_mapping::DataMappingService,
    database::Database,
    ingestor::{scheduler::CacheInvalidationSender, IngestionStateManager},
    logo_assets::{LogoAssetService, LogoAssetStorage},
};

pub mod api;
pub mod handlers;

pub struct WebServer {
    app: Router,
    addr: SocketAddr,
}

impl WebServer {
    pub async fn new(
        config: Config,
        database: Database,
        state_manager: IngestionStateManager,
        cache_invalidation_tx: CacheInvalidationSender,
        data_mapping_service: DataMappingService,
        logo_asset_service: LogoAssetService,
        logo_asset_storage: LogoAssetStorage,
    ) -> Result<Self> {
        let app = Router::new()
            // Proxy endpoints
            .route("/proxy/:ulid.m3u8", get(handlers::serve_proxy_m3u))
            .route("/logos/:logo_id", get(handlers::serve_logo))
            // API endpoints
            // Unified Sources API
            .route("/api/sources", get(api::list_all_sources))
            .route("/api/sources/unified", get(api::list_all_sources))
            .route(
                "/api/sources/stream",
                get(api::list_stream_sources).post(api::create_stream_source),
            )
            .route(
                "/api/sources/epg",
                get(api::list_epg_sources_unified).post(api::create_epg_source_unified),
            )
            .route(
                "/api/sources/stream/:id",
                get(api::get_stream_source)
                    .put(api::update_stream_source)
                    .delete(api::delete_stream_source),
            )
            .route(
                "/api/sources/stream/:id/refresh",
                post(api::refresh_stream_source),
            )
            .route(
                "/api/sources/stream/:id/cancel",
                post(api::cancel_stream_source_ingestion),
            )
            .route(
                "/api/sources/stream/:id/progress",
                get(api::get_stream_source_progress),
            )
            .route(
                "/api/sources/stream/:id/processing",
                get(api::get_stream_source_processing_info),
            )
            .route(
                "/api/sources/stream/:id/channels",
                get(api::get_stream_source_channels),
            )
            .route(
                "/api/sources/epg/:id",
                get(api::get_epg_source_unified)
                    .put(api::update_epg_source_unified)
                    .delete(api::delete_epg_source_unified),
            )
            .route(
                "/api/sources/epg/:id/refresh",
                post(api::refresh_epg_source_unified),
            )
            .route(
                "/api/sources/epg/:id/channels",
                get(api::get_epg_source_channels_unified),
            )
            .route(
                "/api/sources/epg/:id/progress",
                get(api::get_epg_source_progress),
            )
            .route("/api/progress", get(api::get_all_progress))
            .route("/api/progress/sources", get(api::get_all_source_progress))
            .route("/api/progress/operations", get(api::get_operation_progress))
            .route(
                "/api/proxies",
                get(api::list_proxies).post(api::create_proxy),
            )
            .route(
                "/api/proxies/:id",
                get(api::get_proxy)
                    .put(api::update_proxy)
                    .delete(api::delete_proxy),
            )
            .route("/api/proxies/:id/regenerate", post(api::regenerate_proxy))
            // Source-specific filter endpoints
            .route(
                "/api/sources/stream/:id/filters",
                get(api::list_stream_source_filters).post(api::create_stream_source_filter),
            )
            .route(
                "/api/sources/epg/:id/filters",
                get(api::list_epg_source_filters).post(api::create_epg_source_filter),
            )
            // Cross-source filter operations
            .route("/api/filters/stream", get(api::list_stream_filters))
            .route("/api/filters/epg", get(api::list_epg_filters))
            .route(
                "/api/filters/:id",
                get(api::get_filter)
                    .put(api::update_filter)
                    .delete(api::delete_filter),
            )
            .route(
                "/api/filters/stream/fields",
                get(api::get_stream_filter_fields),
            )
            .route("/api/filters/epg/fields", get(api::get_epg_filter_fields))
            .route("/api/filters/test", post(api::test_filter))
            // Legacy filter endpoints (for backward compatibility)
            .route(
                "/api/filters",
                get(api::list_filters).post(api::create_filter),
            )
            .route("/api/filters/fields", get(api::get_filter_fields))
            // Data Mapping API
            .route(
                "/api/data-mapping",
                get(api::list_data_mapping_rules).post(api::create_data_mapping_rule),
            )
            .route(
                "/api/data-mapping/:id",
                get(api::get_data_mapping_rule)
                    .put(api::update_data_mapping_rule)
                    .delete(api::delete_data_mapping_rule),
            )
            .route(
                "/api/data-mapping/reorder",
                post(api::reorder_data_mapping_rules),
            )
            .route(
                "/api/data-mapping/validate",
                post(api::validate_data_mapping_expression),
            )
            .route(
                "/api/data-mapping/fields/stream",
                get(api::get_data_mapping_stream_fields),
            )
            .route(
                "/api/data-mapping/fields/epg",
                get(api::get_data_mapping_epg_fields),
            )
            .route("/api/data-mapping/test", post(api::test_data_mapping_rule))
            .route(
                "/api/data-mapping/preview",
                get(api::apply_data_mapping_rules).post(api::apply_data_mapping_rules_post),
            )
            .route(
                "/api/sources/stream/:id/data-mapping/preview",
                get(api::apply_stream_source_data_mapping),
            )
            .route(
                "/api/sources/epg/:id/data-mapping/preview",
                get(api::apply_epg_source_data_mapping),
            )
            // Logo Assets API
            .route("/api/logos", get(api::list_logo_assets))
            .route("/api/logos/upload", post(api::upload_logo_asset))
            .route(
                "/api/logos/:id",
                get(api::get_logo_asset)
                    .put(api::update_logo_asset)
                    .delete(api::delete_logo_asset),
            )
            .route(
                "/api/logos/:id/formats",
                get(api::get_logo_asset_with_formats),
            )
            .route("/api/logos/search", get(api::search_logo_assets))
            .route("/api/logos/stats", get(api::get_logo_cache_stats))
            .route("/api/epg/viewer", get(api::get_epg_viewer_data))
            // Channel Mapping API
            .route(
                "/api/channel-mappings",
                get(api::list_channel_mappings).post(api::create_channel_mapping),
            )
            .route(
                "/api/channel-mappings/:id",
                delete(api::delete_channel_mapping),
            )
            .route(
                "/api/channel-mappings/auto-map",
                post(api::auto_map_channels),
            )
            // Linked Xtream Sources API
            .route(
                "/api/linked-xtream-sources",
                get(api::list_linked_xtream_sources).post(api::create_linked_xtream_source),
            )
            .route(
                "/api/linked-xtream-sources/:link_id",
                get(api::get_linked_xtream_source)
                    .put(api::update_linked_xtream_source)
                    .delete(api::delete_linked_xtream_source),
            )
            // Health check endpoint
            .route("/health", get(api::health_check))
            // Favicon
            .route("/favicon.ico", get(handlers::serve_favicon))
            // Web interface
            .route("/", get(handlers::index))
            .route("/sources", get(handlers::sources_page))
            .route("/proxies", get(handlers::proxies_page))
            .route("/filters", get(handlers::filters_page))
            .route("/data-mapping", get(handlers::data_mapping_page))
            .route("/logos", get(handlers::logo_assets_page))
            .route("/relay", get(handlers::relay_page))
            .route("/epg-sources", get(handlers::epg_sources_page))
            // Static files (embedded)
            .route("/static/*path", get(handlers::serve_static_asset))
            // Middleware
            .layer(CorsLayer::permissive())
            // Shared state
            .with_state(AppState {
                database,
                config: config.clone(),
                state_manager,
                cache_invalidation_tx,
                data_mapping_service,
                logo_asset_service,
                logo_asset_storage,
            });

        let addr: SocketAddr = format!("{}:{}", config.web.host, config.web.port).parse()?;

        Ok(Self { app, addr })
    }

    pub async fn serve(self) -> Result<()> {
        let listener = tokio::net::TcpListener::bind(&self.addr).await?;
        axum::serve(listener, self.app).await?;
        Ok(())
    }

    pub fn host(&self) -> String {
        self.addr.ip().to_string()
    }

    pub fn port(&self) -> u16 {
        self.addr.port()
    }
}

#[derive(Clone)]
pub struct AppState {
    pub database: Database,
    #[allow(dead_code)]
    pub config: Config,
    pub state_manager: IngestionStateManager,
    pub cache_invalidation_tx: CacheInvalidationSender,
    pub data_mapping_service: DataMappingService,
    pub logo_asset_service: LogoAssetService,
    pub logo_asset_storage: LogoAssetStorage,
}
