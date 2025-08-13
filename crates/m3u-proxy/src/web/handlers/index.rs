//! Index page handler
//!
//! Serves the embedded dashboard HTML as the index page.

use axum::{
    http::StatusCode,
    response::{Html, IntoResponse},
    extract::State,
};

use crate::{
    assets::StaticAssets,
    web::{extractors::RequestContext, AppState},
};

/// Serve the index page from embedded static assets
pub async fn index(
    State(_state): State<AppState>,
    _context: RequestContext,
) -> impl IntoResponse {
    match StaticAssets::get_asset("static/index.html") {
        Some(file) => {
            let content = String::from_utf8_lossy(&file.data);
            Html(content.into_owned()).into_response()
        }
        None => {
            // Fallback if embedded asset is not found
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Html("<h1>500 Internal Server Error</h1><p>Dashboard not found</p>".to_string()),
            )
                .into_response()
        }
    }
}