use anyhow::Result;
use axum::{
    routing::{get, post},
    Router,
};
use std::net::SocketAddr;
use tower_http::{cors::CorsLayer, services::ServeDir, trace::TraceLayer};

use crate::{config::Config, database::Database, ingestor::IngestionStateManager};

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
            .route("/api/sources/:id/progress", get(api::get_source_progress))
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
            .route(
                "/api/filters",
                get(api::list_filters).post(api::create_filter),
            )
            .route(
                "/api/filters/:id",
                get(api::get_filter)
                    .put(api::update_filter)
                    .delete(api::delete_filter),
            )
            // Web interface
            .route("/", get(handlers::index))
            .route("/sources", get(handlers::sources_page))
            .route("/proxies", get(handlers::proxies_page))
            .route("/filters", get(handlers::filters_page))
            // Static files
            .nest_service("/static", ServeDir::new("static"))
            // Middleware
            .layer(CorsLayer::permissive())
            .layer(TraceLayer::new_for_http())
            // Shared state
            .with_state(AppState {
                database,
                config: config.clone(),
                state_manager,
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
}
