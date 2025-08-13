//! Static asset handlers
//!
//! Serves embedded static assets (CSS, JS, images, etc.)

use axum::{
    extract::Request,
    http::{StatusCode, HeaderMap},
    response::IntoResponse,
};

use crate::assets::StaticAssets;

/// Catch-all handler for embedded static assets
/// Maps any request path to static/{path} in the embedded assets
/// For SPA routing, tries to serve index.html for directory-like paths
pub async fn serve_embedded_asset(request: Request) -> impl IntoResponse {
    let path = request.uri().path();
    // Remove leading slash and map to static/ prefix
    let asset_path = if path.starts_with('/') {
        format!("static{}", path)
    } else {
        format!("static/{}", path)
    };
    
    match StaticAssets::get_asset(&asset_path) {
        Some(file) => {
            let content_type = StaticAssets::get_content_type(path);
            let mut headers = HeaderMap::new();
            headers.insert("content-type", content_type.parse().unwrap());
            headers.insert("cache-control", "public, max-age=31536000".parse().unwrap());
            
            (headers, file.data.to_vec()).into_response()
        }
        None => {
            // Try serving index.html for directory-like paths (SPA routing)
            let index_path = if path.ends_with('/') {
                format!("{}index.html", asset_path)
            } else {
                format!("{}/index.html", asset_path)
            };
            
            match StaticAssets::get_asset(&index_path) {
                Some(file) => {
                    let mut headers = HeaderMap::new();
                    headers.insert("content-type", "text/html; charset=utf-8".parse().unwrap());
                    headers.insert("cache-control", "no-cache".parse().unwrap()); // Don't cache SPA pages
                    
                    (headers, file.data.to_vec()).into_response()
                }
                None => (StatusCode::NOT_FOUND, format!("Asset not found: {}", path)).into_response(),
            }
        }
    }
}

/// Serve the favicon
pub async fn serve_favicon() -> impl IntoResponse {
    match StaticAssets::get_asset("static/favicon.ico") {
        Some(file) => {
            let mut headers = HeaderMap::new();
            headers.insert("content-type", "image/x-icon".parse().unwrap());
            headers.insert("cache-control", "public, max-age=86400".parse().unwrap());
            
            (headers, file.data.to_vec()).into_response()
        }
        None => (StatusCode::NOT_FOUND, "Favicon not found").into_response(),
    }
}