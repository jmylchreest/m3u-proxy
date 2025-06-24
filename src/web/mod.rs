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
            .route(
                "/api/sources",
                get(api::list_sources).post(api::create_source),
            )
            .route(
                "/api/sources/:id",
                get(api::get_source)
                    .put(api::update_source)
                    .delete(api::delete_source),
            )
            .route("/api/sources/:id/refresh", post(api::refresh_source))
            .route(
                "/api/sources/:id/cancel",
                post(api::cancel_source_ingestion),
            )
            .route("/api/sources/:id/progress", get(api::get_source_progress))
            .route(
                "/api/sources/:id/processing",
                get(api::get_source_processing_info),
            )
            .route("/api/sources/:id/channels", get(api::get_source_channels))
            .route("/api/progress", get(api::get_all_progress))
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
            .route(
                "/api/filters",
                get(api::list_filters).post(api::create_filter),
            )
            .route("/api/filters/fields", get(api::get_filter_fields))
            .route(
                "/api/filters/:id",
                get(api::get_filter)
                    .put(api::update_filter)
                    .delete(api::delete_filter),
            )
            .route("/api/filters/test", post(api::test_filter))
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
            .route("/api/data-mapping/test", post(api::test_data_mapping_rule))
            .route(
                "/api/data-mapping/preview/:source_id",
                get(api::preview_data_mapping),
            )
            .route(
                "/api/data-mapping/preview",
                get(api::preview_data_mapping_rules),
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
            // EPG Sources API
            .route(
                "/api/epg-sources",
                get(api::list_epg_sources).post(api::create_epg_source),
            )
            .route(
                "/api/epg-sources/:id",
                get(api::get_epg_source)
                    .put(api::update_epg_source)
                    .delete(api::delete_epg_source),
            )
            .route(
                "/api/epg-sources/:id/refresh",
                post(api::refresh_epg_source),
            )
            .route(
                "/api/epg-sources/:id/channels",
                get(api::get_epg_source_channels),
            )
            .route("/api/epg/viewer", get(api::get_epg_viewer_data))
            .route("/api/epg/dlq", get(api::get_epg_dlq_entries))
            .route("/api/epg/dlq/resolve", post(api::resolve_epg_dlq_conflicts))
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
