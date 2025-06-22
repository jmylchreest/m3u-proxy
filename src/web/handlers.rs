use axum::{
    body::Body,
    extract::{Path, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
};
use tracing::{error, info, warn};

use super::AppState;
use crate::assets::StaticAssets;
use crate::models::StreamProxy;
use crate::proxy::ProxyService;

/// Serve proxy M3U files with on-demand generation
/// This applies the complete pipeline: original data -> data mapping -> filtering -> M3U generation
pub async fn serve_proxy_m3u(
    Path(ulid): Path<String>,
    State(state): State<AppState>,
) -> Result<Response<String>, StatusCode> {
    info!("Serving proxy M3U for ULID: {}", ulid);

    // Find the proxy by ULID
    let proxy = match state.database.get_proxy_by_ulid(&ulid).await {
        Ok(proxy) => proxy,
        Err(_) => {
            warn!("Proxy not found for ULID: {}", ulid);
            return Err(StatusCode::NOT_FOUND);
        }
    };

    if !proxy.is_active {
        warn!("Proxy '{}' is inactive", proxy.name);
        return Err(StatusCode::NOT_FOUND);
    }

    info!("Found proxy '{}' for ULID: {}", proxy.name, ulid);

    // Check if we have a cached M3U file first
    let filename = format!("{}.m3u8", ulid);
    let file_path = state.config.storage.m3u_path.join(&filename);

    // Try to read cached file (if it exists and is recent)
    if let Ok(content) = std::fs::read_to_string(&file_path) {
        if let Ok(metadata) = std::fs::metadata(&file_path) {
            if let Ok(modified) = metadata.modified() {
                // Check if file is less than 5 minutes old
                if let Ok(elapsed) = modified.elapsed() {
                    if elapsed.as_secs() < 300 {
                        info!("Serving cached M3U for proxy '{}'", proxy.name);
                        return Ok(Response::builder()
                            .header("content-type", "application/vnd.apple.mpegurl")
                            .header("cache-control", "no-cache")
                            .body(content)
                            .unwrap());
                    }
                }
            }
        }
    }

    // Generate M3U on-demand with full pipeline
    info!("Generating M3U on-demand for proxy '{}'", proxy.name);

    let proxy_service = ProxyService::new(state.config.storage.clone());

    match proxy_service
        .generate_proxy(
            &proxy,
            &state.database,
            &state.data_mapping_service,
            &state.logo_asset_service,
            &state.config.web.base_url,
        )
        .await
    {
        Ok(generation) => {
            info!(
                "Successfully generated M3U for proxy '{}': {} channels",
                proxy.name, generation.channel_count
            );

            // Save to cache for future requests
            if let Err(e) = proxy_service
                .save_m3u_file(proxy.id, &generation.m3u_content)
                .await
            {
                warn!("Failed to cache M3U file for proxy '{}': {}", proxy.name, e);
            }

            // Clean up old versions
            if let Err(e) = proxy_service.cleanup_old_versions(proxy.id).await {
                warn!(
                    "Failed to cleanup old versions for proxy '{}': {}",
                    proxy.name, e
                );
            }

            // Return the generated content
            Ok(Response::builder()
                .header("content-type", "application/vnd.apple.mpegurl")
                .header("cache-control", "no-cache")
                .body(generation.m3u_content)
                .unwrap())
        }
        Err(e) => {
            error!("Failed to generate M3U for proxy '{}': {}", proxy.name, e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
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

pub async fn epg_sources_page() -> impl IntoResponse {
    serve_embedded_asset("static/html/epg-sources.html").await
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
