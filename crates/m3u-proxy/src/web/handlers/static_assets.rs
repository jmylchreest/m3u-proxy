//! Static asset handlers
//!
//! Serves embedded static assets (CSS, JS, images, etc.)

use axum::{
    extract::Path,
    http::{StatusCode, HeaderMap},
    response::IntoResponse,
};

use crate::assets::StaticAssets;

/// Serve a static asset by path
pub async fn serve_static_asset(Path(path): Path<String>) -> impl IntoResponse {
    let asset_path = format!("static/{path}");
    
    match StaticAssets::get_asset(&asset_path) {
        Some(file) => {
            let content_type = StaticAssets::get_content_type(&path);
            let mut headers = HeaderMap::new();
            headers.insert("content-type", content_type.parse().unwrap());
            headers.insert("cache-control", "public, max-age=31536000".parse().unwrap());
            
            (headers, file.data.to_vec()).into_response()
        }
        None => (StatusCode::NOT_FOUND, "Asset not found").into_response(),
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