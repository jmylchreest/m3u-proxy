use axum::{
    body::Body,
    extract::{Path, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
};

use super::AppState;
use crate::assets::StaticAssets;

pub async fn serve_proxy_m3u(
    Path(_ulid): Path<String>,
    State(_state): State<AppState>,
) -> Result<Response<String>, StatusCode> {
    // TODO: Implement M3U serving logic
    Ok(Response::builder()
        .header("content-type", "application/vnd.apple.mpegurl")
        .body(format!("#EXTM3U\n# Proxy ULID: {}\n", _ulid))
        .unwrap())
}

pub async fn serve_logo(
    Path(_logo_id): Path<String>,
    State(_state): State<AppState>,
) -> StatusCode {
    // TODO: Implement logo serving logic
    StatusCode::NOT_FOUND
}

pub async fn index() -> impl IntoResponse {
    serve_embedded_asset("static/html/index.html").await
}

pub async fn sources_page() -> impl IntoResponse {
    serve_embedded_asset("static/html/sources.html").await
}

pub async fn proxies_page() -> impl IntoResponse {
    serve_embedded_asset("static/html/proxies.html").await
}

pub async fn filters_page() -> impl IntoResponse {
    serve_embedded_asset("static/html/filters.html").await
}

pub async fn serve_static_asset(Path(path): Path<String>) -> impl IntoResponse {
    let asset_path = format!("static/{}", path);
    serve_embedded_asset(&asset_path).await
}

async fn serve_embedded_asset(path: &str) -> impl IntoResponse {
    match StaticAssets::get_asset(path) {
        Some(asset) => {
            let content_type = StaticAssets::get_content_type(path);
            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, content_type)
                .header(header::CACHE_CONTROL, "public, max-age=31536000")
                .body(Body::from(asset.data.to_vec()))
                .unwrap()
        }
        None => Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::from("Asset not found"))
            .unwrap(),
    }
}
