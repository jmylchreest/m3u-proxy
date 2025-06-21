use axum::{
    body::Body,
    extract::{Path, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
};

use super::AppState;
use crate::assets::StaticAssets;

pub async fn serve_proxy_m3u(
    Path(ulid): Path<String>,
    State(state): State<AppState>,
) -> Result<Response<String>, StatusCode> {
    // Construct path to M3U file using configured storage path
    let filename = format!("{}.m3u8", ulid);
    let file_path = state.config.storage.m3u_path.join(filename);

    // Try to read the M3U file
    match std::fs::read_to_string(&file_path) {
        Ok(content) => Ok(Response::builder()
            .header("content-type", "application/vnd.apple.mpegurl")
            .header("cache-control", "no-cache")
            .body(content)
            .unwrap()),
        Err(_) => {
            // Return 404 if file doesn't exist
            Err(StatusCode::NOT_FOUND)
        }
    }
}

pub async fn serve_logo(
    Path(logo_id): Path<String>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    // Get logo asset from database
    match uuid::Uuid::parse_str(&logo_id) {
        Ok(uuid) => {
            match state.logo_asset_service.get_asset(uuid).await {
                Ok(logo_asset) => {
                    // Use storage service to get file
                    match state
                        .logo_asset_storage
                        .get_file(&logo_asset.file_path)
                        .await
                    {
                        Ok(file_data) => Response::builder()
                            .header("content-type", &logo_asset.mime_type)
                            .header("cache-control", "public, max-age=3600")
                            .body(Body::from(file_data))
                            .unwrap(),
                        Err(_) => Response::builder()
                            .status(StatusCode::NOT_FOUND)
                            .body(Body::from("Logo file not found"))
                            .unwrap(),
                    }
                }
                Err(_) => Response::builder()
                    .status(StatusCode::NOT_FOUND)
                    .body(Body::from("Logo not found"))
                    .unwrap(),
            }
        }
        Err(_) => Response::builder()
            .status(StatusCode::BAD_REQUEST)
            .body(Body::from("Invalid logo ID"))
            .unwrap(),
    }
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

pub async fn data_mapping_page() -> impl IntoResponse {
    serve_embedded_asset("static/html/data-mapping.html").await
}

pub async fn logo_assets_page() -> impl IntoResponse {
    serve_embedded_asset("static/html/logos.html").await
}

pub async fn relay_page() -> impl IntoResponse {
    serve_embedded_asset("static/html/relay.html").await
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
