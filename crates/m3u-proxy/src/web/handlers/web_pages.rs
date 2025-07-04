//! Web page handlers
//!
//! Serves the embedded HTML pages for the web interface.

use axum::{
    http::StatusCode,
    response::{Html, IntoResponse},
    extract::State,
};

use crate::{
    assets::StaticAssets,
    web::{extractors::RequestContext, AppState},
};

/// Serve the sources page
pub async fn sources_page(
    State(_state): State<AppState>,
    _context: RequestContext,
) -> impl IntoResponse {
    serve_html_page("static/html/sources.html").await
}

/// Serve the EPG sources page
pub async fn epg_sources_page(
    State(_state): State<AppState>,
    _context: RequestContext,
) -> impl IntoResponse {
    serve_html_page("static/html/epg-sources.html").await
}

/// Serve the proxies page
pub async fn proxies_page(
    State(_state): State<AppState>,
    _context: RequestContext,
) -> impl IntoResponse {
    serve_html_page("static/html/proxies.html").await
}

/// Serve the filters page
pub async fn filters_page(
    State(_state): State<AppState>,
    _context: RequestContext,
) -> impl IntoResponse {
    serve_html_page("static/html/filters.html").await
}

/// Serve the data mapping page
pub async fn data_mapping_page(
    State(_state): State<AppState>,
    _context: RequestContext,
) -> impl IntoResponse {
    serve_html_page("static/html/data-mapping.html").await
}

/// Serve the logos page
pub async fn logos_page(
    State(_state): State<AppState>,
    _context: RequestContext,
) -> impl IntoResponse {
    serve_html_page("static/html/logos.html").await
}

/// Serve the relay page
pub async fn relay_page(
    State(_state): State<AppState>,
    _context: RequestContext,
) -> impl IntoResponse {
    serve_html_page("static/html/relay.html").await
}

/// Helper function to serve an HTML page from embedded assets
async fn serve_html_page(asset_path: &str) -> impl IntoResponse {
    match StaticAssets::get_asset(asset_path) {
        Some(file) => {
            let content = String::from_utf8_lossy(&file.data);
            Html(content.into_owned()).into_response()
        }
        None => {
            (
                StatusCode::NOT_FOUND,
                Html(format!("<h1>404 Not Found</h1><p>Page not found: {}</p>", asset_path)),
            ).into_response()
        }
    }
}