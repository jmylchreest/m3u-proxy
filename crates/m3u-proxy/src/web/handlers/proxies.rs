//! Proxy HTTP handlers
//!
//! This module contains HTTP handlers for proxy operations.

use axum::{
    Json,
    extract::{Path, State},
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    models::{StreamProxy, StreamProxyMode},
    web::{
        AppState,
        extractors::{ListParams, RequestContext},
        responses::ok,
        utils::{extract_uuid_param, log_request},
    },
};

/// Request DTO for creating a stream proxy
#[derive(Debug, Clone, Deserialize)]
pub struct CreateStreamProxyRequest {
    pub name: String,
    pub description: Option<String>,
    pub proxy_mode: String, // Will be converted to StreamProxyMode
    pub upstream_timeout: Option<i32>,
    pub buffer_size: Option<i32>,
    pub max_concurrent_streams: Option<i32>,
    pub starting_channel_number: i32,
    pub stream_sources: Vec<ProxySourceRequest>,
    pub epg_sources: Vec<ProxyEpgSourceRequest>,
    pub filters: Vec<ProxyFilterRequest>,
    pub is_active: bool,
    #[serde(default)]
    pub auto_regenerate: bool, // TODO: Implement auto-regeneration functionality
}

/// Stream source assignment for proxy
#[derive(Debug, Clone, Deserialize)]
pub struct ProxySourceRequest {
    pub source_id: Uuid,
    pub priority_order: i32,
}

/// EPG source assignment for proxy
#[derive(Debug, Clone, Deserialize)]
pub struct ProxyEpgSourceRequest {
    pub epg_source_id: Uuid,
    pub priority_order: i32,
}

/// Filter assignment for proxy
#[derive(Debug, Clone, Deserialize)]
pub struct ProxyFilterRequest {
    pub filter_id: Uuid,
    pub priority_order: i32,
    pub is_active: bool,
}

/// Request DTO for updating a stream proxy
#[derive(Debug, Clone, Deserialize)]
pub struct UpdateStreamProxyRequest {
    pub name: String,
    pub description: Option<String>,
    pub proxy_mode: String,
    pub upstream_timeout: Option<i32>,
    pub buffer_size: Option<i32>,
    pub max_concurrent_streams: Option<i32>,
    pub starting_channel_number: i32,
    pub stream_sources: Vec<ProxySourceRequest>,
    pub epg_sources: Vec<ProxyEpgSourceRequest>,
    pub filters: Vec<ProxyFilterRequest>,
    pub is_active: bool,
    #[serde(default)]
    pub auto_regenerate: bool, // TODO: Implement auto-regeneration functionality
}

/// Response DTO for stream proxy
#[derive(Debug, Clone, Serialize)]
pub struct StreamProxyResponse {
    pub id: Uuid,
    pub ulid: String,
    pub name: String,
    pub description: Option<String>,
    pub proxy_mode: String,
    pub upstream_timeout: Option<i32>,
    pub buffer_size: Option<i32>,
    pub max_concurrent_streams: Option<i32>,
    pub starting_channel_number: i32,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub last_generated_at: Option<chrono::DateTime<chrono::Utc>>,
    pub is_active: bool,
    pub auto_regenerate: bool,
    pub stream_sources: Vec<ProxySourceResponse>,
    pub epg_sources: Vec<ProxyEpgSourceResponse>,
    pub filters: Vec<ProxyFilterResponse>,
}

/// Stream source in proxy response
#[derive(Debug, Clone, Serialize)]
pub struct ProxySourceResponse {
    pub source_id: Uuid,
    pub source_name: String,
    pub priority_order: i32,
}

/// EPG source in proxy response
#[derive(Debug, Clone, Serialize)]
pub struct ProxyEpgSourceResponse {
    pub epg_source_id: Uuid,
    pub epg_source_name: String,
    pub priority_order: i32,
}

/// Filter in proxy response
#[derive(Debug, Clone, Serialize)]
pub struct ProxyFilterResponse {
    pub filter_id: Uuid,
    pub filter_name: String,
    pub priority_order: i32,
    pub is_active: bool,
}

impl CreateStreamProxyRequest {
    /// Convert to service layer request
    pub fn into_service_request(self) -> Result<crate::models::StreamProxyCreateRequest, String> {
        let proxy_mode = match self.proxy_mode.to_lowercase().as_str() {
            "redirect" => StreamProxyMode::Redirect,
            "proxy" => StreamProxyMode::Proxy,
            _ => return Err(format!("Invalid proxy mode: {}", self.proxy_mode)),
        };

        Ok(crate::models::StreamProxyCreateRequest {
            name: self.name,
            description: self.description,
            proxy_mode,
            upstream_timeout: self.upstream_timeout,
            buffer_size: self.buffer_size,
            max_concurrent_streams: self.max_concurrent_streams,
            starting_channel_number: self.starting_channel_number,
            stream_sources: self
                .stream_sources
                .into_iter()
                .map(|s| crate::models::ProxySourceCreateRequest {
                    source_id: s.source_id,
                    priority_order: s.priority_order,
                })
                .collect(),
            epg_sources: self
                .epg_sources
                .into_iter()
                .map(|e| crate::models::ProxyEpgSourceCreateRequest {
                    epg_source_id: e.epg_source_id,
                    priority_order: e.priority_order,
                })
                .collect(),
            filters: self
                .filters
                .into_iter()
                .map(|f| crate::models::ProxyFilterCreateRequest {
                    filter_id: f.filter_id,
                    priority_order: f.priority_order,
                    is_active: f.is_active,
                })
                .collect(),
            is_active: self.is_active,
            auto_regenerate: self.auto_regenerate,
        })
    }
}

impl From<StreamProxy> for StreamProxyResponse {
    fn from(proxy: StreamProxy) -> Self {
        Self {
            id: proxy.id,
            ulid: proxy.ulid,
            name: proxy.name,
            description: proxy.description,
            proxy_mode: match proxy.proxy_mode {
                StreamProxyMode::Redirect => "redirect".to_string(),
                StreamProxyMode::Proxy => "proxy".to_string(),
            },
            upstream_timeout: proxy.upstream_timeout,
            buffer_size: proxy.buffer_size,
            max_concurrent_streams: proxy.max_concurrent_streams,
            starting_channel_number: proxy.starting_channel_number,
            created_at: proxy.created_at,
            updated_at: proxy.updated_at,
            last_generated_at: proxy.last_generated_at,
            is_active: proxy.is_active,
            auto_regenerate: proxy.auto_regenerate,
            stream_sources: vec![], // Will be populated by service layer
            epg_sources: vec![],    // Will be populated by service layer
            filters: vec![],        // Will be populated by service layer
        }
    }
}

// Preview DTOs
/// Request DTO for previewing a proxy configuration
#[derive(Debug, Clone, Deserialize)]
pub struct PreviewProxyRequest {
    pub name: String,
    pub description: Option<String>,
    pub proxy_mode: String,
    pub upstream_timeout: Option<i32>,
    pub buffer_size: Option<i32>,
    pub max_concurrent_streams: Option<i32>,
    pub starting_channel_number: i32,
    pub stream_sources: Vec<ProxySourceRequest>,
    pub epg_sources: Vec<ProxyEpgSourceRequest>,
    pub filters: Vec<ProxyFilterRequest>,
}

/// Response DTO for proxy preview
#[derive(Debug, Clone, Serialize)]
pub struct PreviewProxyResponse {
    pub channels: Vec<PreviewChannel>,
    pub stats: PreviewStats,
    pub m3u_content: Option<String>,
    pub total_channels: usize,
    pub filtered_channels: usize,
}

/// Channel information in preview
#[derive(Debug, Clone, Serialize)]
pub struct PreviewChannel {
    pub channel_name: String,
    pub group_title: Option<String>,
    pub tvg_id: Option<String>,
    pub tvg_logo: Option<String>,
    pub stream_url: String,
    pub source_name: String,
    pub channel_number: i32,
}

/// Statistics for preview
#[derive(Debug, Clone, Serialize)]
pub struct PreviewStats {
    pub total_sources: usize,
    pub total_channels_before_filters: usize,
    pub total_channels_after_filters: usize,
    pub channels_by_group: std::collections::HashMap<String, usize>,
    pub channels_by_source: std::collections::HashMap<String, usize>,
    pub applied_filters: Vec<String>,
    pub excluded_channels: usize,
    pub included_channels: usize,

    // Pipeline metrics
    pub pipeline_stages: Option<usize>,
    pub filter_execution_time: Option<String>,
    pub processing_rate: Option<String>,
    pub pipeline_stages_detail: Option<Vec<PipelineStageDetail>>,

    // Memory metrics
    pub current_memory: Option<u64>,
    pub peak_memory: Option<u64>,
    pub memory_efficiency: Option<String>,
    pub gc_collections: Option<usize>,
    pub memory_by_stage: Option<std::collections::HashMap<String, u64>>,

    // Processing metrics
    pub total_processing_time: Option<String>,
    pub avg_channel_time: Option<String>,
    pub throughput: Option<String>,
    pub errors: Option<usize>,
    pub processing_timeline: Option<Vec<ProcessingEvent>>,
}

/// Pipeline stage detail
#[derive(Debug, Clone, Serialize)]
pub struct PipelineStageDetail {
    pub name: String,
    pub duration: u64,
    pub channels_processed: usize,
    pub memory_used: Option<u64>,
}

/// Processing event for timeline
#[derive(Debug, Clone, Serialize)]
pub struct ProcessingEvent {
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub description: String,
    pub stage: Option<String>,
    pub channels_count: Option<usize>,
}

/// List all proxies
pub async fn list_proxies(
    State(state): State<AppState>,
    context: RequestContext,
    list_params: ListParams,
) -> impl IntoResponse {
    log_request(
        &axum::http::Method::GET,
        &"/api/v1/proxies".parse().unwrap(),
        &context,
    );

    // Create service instances from state
    let proxy_repo = crate::repositories::StreamProxyRepository::new(state.database.pool().clone());
    let channel_repo = crate::repositories::ChannelRepository::new(state.database.pool().clone());
    let filter_repo = crate::repositories::FilterRepository::new(state.database.pool().clone());
    let stream_source_repo =
        crate::repositories::StreamSourceRepository::new(state.database.pool().clone());
    let filter_engine = crate::proxy::filter_engine::FilterEngine::new();

    let service = crate::services::StreamProxyService::new(
        proxy_repo,
        channel_repo,
        filter_repo,
        stream_source_repo,
        filter_engine,
        state.database.clone(),
        state.preview_file_manager.clone(),
        state.data_mapping_service.clone(),
        state.logo_asset_service.clone(),
        state.config.storage.clone(),
    );

    // Get proxies with pagination
    let limit = Some(list_params.pagination.limit as usize);
    let offset = Some(((list_params.pagination.page - 1) * list_params.pagination.limit) as usize);

    match service.list(limit, offset).await {
        Ok(proxies) => {
            let total_count = proxies.len() as u64;
            let response = crate::web::responses::PaginatedResponse::new(
                proxies,
                total_count, // In a real implementation, this would be the total count
                list_params.pagination.page,
                list_params.pagination.limit,
            );
            ok(response).into_response()
        }
        Err(err) => crate::web::responses::handle_error(err).into_response(),
    }
}

/// Get a specific proxy by ID
pub async fn get_proxy(
    State(state): State<AppState>,
    Path(id): Path<String>,
    context: RequestContext,
) -> impl IntoResponse {
    log_request(
        &axum::http::Method::GET,
        &format!("/api/v1/proxies/{}", id).parse().unwrap(),
        &context,
    );

    let uuid = match extract_uuid_param(&id) {
        Ok(uuid) => uuid,
        Err(error) => return crate::web::responses::bad_request(&error).into_response(),
    };

    // Create service instances
    let proxy_repo = crate::repositories::StreamProxyRepository::new(state.database.pool().clone());
    let channel_repo = crate::repositories::ChannelRepository::new(state.database.pool().clone());
    let filter_repo = crate::repositories::FilterRepository::new(state.database.pool().clone());
    let stream_source_repo =
        crate::repositories::StreamSourceRepository::new(state.database.pool().clone());
    let filter_engine = crate::proxy::filter_engine::FilterEngine::new();

    let service = crate::services::StreamProxyService::new(
        proxy_repo,
        channel_repo,
        filter_repo,
        stream_source_repo,
        filter_engine,
        state.database.clone(),
        state.preview_file_manager.clone(),
        state.data_mapping_service.clone(),
        state.logo_asset_service.clone(),
        state.config.storage.clone(),
    );

    match service.get_by_id(uuid).await {
        Ok(Some(proxy)) => ok(proxy).into_response(),
        Ok(None) => crate::web::responses::not_found("stream_proxy", &id).into_response(),
        Err(err) => crate::web::responses::handle_error(err).into_response(),
    }
}

/// Create a new proxy
pub async fn create_proxy(
    State(state): State<AppState>,
    context: RequestContext,
    Json(request): Json<CreateStreamProxyRequest>,
) -> impl IntoResponse {
    log_request(
        &axum::http::Method::POST,
        &"/api/v1/proxies".parse().unwrap(),
        &context,
    );

    let service_request = match request.into_service_request() {
        Ok(req) => req,
        Err(error) => return crate::web::responses::bad_request(&error).into_response(),
    };

    // Create service instances
    let proxy_repo = crate::repositories::StreamProxyRepository::new(state.database.pool().clone());
    let channel_repo = crate::repositories::ChannelRepository::new(state.database.pool().clone());
    let filter_repo = crate::repositories::FilterRepository::new(state.database.pool().clone());
    let stream_source_repo =
        crate::repositories::StreamSourceRepository::new(state.database.pool().clone());
    let filter_engine = crate::proxy::filter_engine::FilterEngine::new();

    let service = crate::services::StreamProxyService::new(
        proxy_repo,
        channel_repo,
        filter_repo,
        stream_source_repo,
        filter_engine,
        state.database.clone(),
        state.preview_file_manager.clone(),
        state.data_mapping_service.clone(),
        state.logo_asset_service.clone(),
        state.config.storage.clone(),
    );

    match service.create(service_request).await {
        Ok(proxy) => ok(proxy).into_response(),
        Err(err) => crate::web::responses::handle_error(err).into_response(),
    }
}

/// Update an existing proxy
pub async fn update_proxy(
    State(state): State<AppState>,
    Path(id): Path<String>,
    context: RequestContext,
    Json(request): Json<UpdateStreamProxyRequest>,
) -> impl IntoResponse {
    log_request(
        &axum::http::Method::PUT,
        &format!("/api/v1/proxies/{}", id).parse().unwrap(),
        &context,
    );

    let uuid = match extract_uuid_param(&id) {
        Ok(uuid) => uuid,
        Err(error) => return crate::web::responses::bad_request(&error).into_response(),
    };

    let service_request = crate::models::StreamProxyUpdateRequest {
        name: request.name,
        description: request.description,
        proxy_mode: match request.proxy_mode.to_lowercase().as_str() {
            "redirect" => crate::models::StreamProxyMode::Redirect,
            "proxy" => crate::models::StreamProxyMode::Proxy,
            _ => {
                return crate::web::responses::bad_request(&format!(
                    "Invalid proxy mode: {}",
                    request.proxy_mode
                ))
                .into_response();
            }
        },
        upstream_timeout: request.upstream_timeout,
        buffer_size: request.buffer_size,
        max_concurrent_streams: request.max_concurrent_streams,
        starting_channel_number: request.starting_channel_number,
        stream_sources: request
            .stream_sources
            .into_iter()
            .map(|s| crate::models::ProxySourceCreateRequest {
                source_id: s.source_id,
                priority_order: s.priority_order,
            })
            .collect(),
        epg_sources: request
            .epg_sources
            .into_iter()
            .map(|e| crate::models::ProxyEpgSourceCreateRequest {
                epg_source_id: e.epg_source_id,
                priority_order: e.priority_order,
            })
            .collect(),
        filters: request
            .filters
            .into_iter()
            .map(|f| crate::models::ProxyFilterCreateRequest {
                filter_id: f.filter_id,
                priority_order: f.priority_order,
                is_active: f.is_active,
            })
            .collect(),
        is_active: request.is_active,
        auto_regenerate: request.auto_regenerate,
    };

    // Create service instances
    let proxy_repo = crate::repositories::StreamProxyRepository::new(state.database.pool().clone());
    let channel_repo = crate::repositories::ChannelRepository::new(state.database.pool().clone());
    let filter_repo = crate::repositories::FilterRepository::new(state.database.pool().clone());
    let stream_source_repo =
        crate::repositories::StreamSourceRepository::new(state.database.pool().clone());
    let filter_engine = crate::proxy::filter_engine::FilterEngine::new();

    let service = crate::services::StreamProxyService::new(
        proxy_repo,
        channel_repo,
        filter_repo,
        stream_source_repo,
        filter_engine,
        state.database.clone(),
        state.preview_file_manager.clone(),
        state.data_mapping_service.clone(),
        state.logo_asset_service.clone(),
        state.config.storage.clone(),
    );

    match service.update(uuid, service_request).await {
        Ok(proxy) => ok(proxy).into_response(),
        Err(err) => crate::web::responses::handle_error(err).into_response(),
    }
}

/// Delete a proxy
pub async fn delete_proxy(
    State(state): State<AppState>,
    Path(id): Path<String>,
    context: RequestContext,
) -> impl IntoResponse {
    log_request(
        &axum::http::Method::DELETE,
        &format!("/api/v1/proxies/{}", id).parse().unwrap(),
        &context,
    );

    let uuid = match extract_uuid_param(&id) {
        Ok(uuid) => uuid,
        Err(error) => return crate::web::responses::bad_request(&error).into_response(),
    };

    // Create service instances
    let proxy_repo = crate::repositories::StreamProxyRepository::new(state.database.pool().clone());
    let channel_repo = crate::repositories::ChannelRepository::new(state.database.pool().clone());
    let filter_repo = crate::repositories::FilterRepository::new(state.database.pool().clone());
    let stream_source_repo =
        crate::repositories::StreamSourceRepository::new(state.database.pool().clone());
    let filter_engine = crate::proxy::filter_engine::FilterEngine::new();

    let service = crate::services::StreamProxyService::new(
        proxy_repo,
        channel_repo,
        filter_repo,
        stream_source_repo,
        filter_engine,
        state.database.clone(),
        state.preview_file_manager.clone(),
        state.data_mapping_service.clone(),
        state.logo_asset_service.clone(),
        state.config.storage.clone(),
    );

    match service.delete(uuid).await {
        Ok(()) => {
            crate::web::responses::ok(serde_json::json!({"message": "Proxy deleted successfully"}))
                .into_response()
        }
        Err(err) => crate::web::responses::handle_error(err).into_response(),
    }
}

/// Preview a proxy configuration without saving
#[axum::debug_handler]
pub async fn preview_proxy_config(
    State(state): State<AppState>,
    context: RequestContext,
    Json(request): Json<PreviewProxyRequest>,
) -> impl IntoResponse {
    log_request(
        &axum::http::Method::POST,
        &"/api/v1/proxies/preview".parse().unwrap(),
        &context,
    );

    tracing::info!("Preview request received for proxy: {}", request.name);
    tracing::debug!("Preview request: {:?}", request);

    // Create service instances
    let proxy_repo = crate::repositories::StreamProxyRepository::new(state.database.pool().clone());
    let channel_repo = crate::repositories::ChannelRepository::new(state.database.pool().clone());
    let filter_repo = crate::repositories::FilterRepository::new(state.database.pool().clone());
    let stream_source_repo =
        crate::repositories::StreamSourceRepository::new(state.database.pool().clone());
    let filter_engine = crate::proxy::filter_engine::FilterEngine::new();

    let service = crate::services::StreamProxyService::new(
        proxy_repo,
        channel_repo,
        filter_repo,
        stream_source_repo,
        filter_engine,
        state.database.clone(),
        state.preview_file_manager.clone(),
        state.data_mapping_service.clone(),
        state.logo_asset_service.clone(),
        state.config.storage.clone(),
    );

    match service.generate_preview(request).await {
        Ok(preview) => {
            tracing::info!("Preview generated successfully");
            ok(preview).into_response()
        }
        Err(err) => {
            tracing::error!("Preview generation failed: {:?}", err);
            crate::web::responses::handle_error(err).into_response()
        }
    }
}

/// Preview an existing proxy by ID
#[axum::debug_handler]
pub async fn preview_existing_proxy(
    State(state): State<AppState>,
    Path(id): Path<String>,
    context: RequestContext,
) -> impl IntoResponse {
    log_request(
        &axum::http::Method::GET,
        &format!("/api/v1/proxies/{}/preview", id).parse().unwrap(),
        &context,
    );

    let uuid = match extract_uuid_param(&id) {
        Ok(uuid) => uuid,
        Err(error) => return crate::web::responses::bad_request(&error).into_response(),
    };

    // Create service instances
    let proxy_repo = crate::repositories::StreamProxyRepository::new(state.database.pool().clone());
    let channel_repo = crate::repositories::ChannelRepository::new(state.database.pool().clone());
    let filter_repo = crate::repositories::FilterRepository::new(state.database.pool().clone());
    let stream_source_repo =
        crate::repositories::StreamSourceRepository::new(state.database.pool().clone());
    let filter_engine = crate::proxy::filter_engine::FilterEngine::new();

    let service = crate::services::StreamProxyService::new(
        proxy_repo,
        channel_repo,
        filter_repo,
        stream_source_repo,
        filter_engine,
        state.database.clone(),
        state.preview_file_manager.clone(),
        state.data_mapping_service.clone(),
        state.logo_asset_service.clone(),
        state.config.storage.clone(),
    );

    // Get the existing proxy first
    match service.get_by_id(uuid).await {
        Ok(Some(proxy_response)) => {
            // Convert proxy response to preview request
            let preview_request = PreviewProxyRequest {
                name: proxy_response.name,
                description: proxy_response.description,
                proxy_mode: proxy_response.proxy_mode,
                upstream_timeout: proxy_response.upstream_timeout,
                buffer_size: proxy_response.buffer_size,
                max_concurrent_streams: proxy_response.max_concurrent_streams,
                starting_channel_number: proxy_response.starting_channel_number,
                stream_sources: proxy_response
                    .stream_sources
                    .into_iter()
                    .map(|s| ProxySourceRequest {
                        source_id: s.source_id,
                        priority_order: s.priority_order,
                    })
                    .collect(),
                epg_sources: proxy_response
                    .epg_sources
                    .into_iter()
                    .map(|e| ProxyEpgSourceRequest {
                        epg_source_id: e.epg_source_id,
                        priority_order: e.priority_order,
                    })
                    .collect(),
                filters: proxy_response
                    .filters
                    .into_iter()
                    .map(|f| ProxyFilterRequest {
                        filter_id: f.filter_id,
                        priority_order: f.priority_order,
                        is_active: f.is_active,
                    })
                    .collect(),
            };

            // Generate preview
            match service.generate_preview(preview_request).await {
                Ok(preview) => ok(preview).into_response(),
                Err(err) => crate::web::responses::handle_error(err).into_response(),
            }
        }
        Ok(None) => crate::web::responses::not_found("stream_proxy", &id).into_response(),
        Err(err) => crate::web::responses::handle_error(err).into_response(),
    }
}

// Proxy Content Serving Handlers (Non-API endpoints)

/// Serve M3U8 content for a proxy (from static file)
pub async fn serve_proxy_m3u(
    axum::extract::Path(ulid): axum::extract::Path<String>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    use axum::http::{HeaderMap, StatusCode};
    use tokio::fs;
    use tracing::{error, info, warn};

    info!("Serving static M3U8 for proxy: {}", ulid);

    // 1. Look up proxy by ULID to validate it exists and is active
    let _proxy = match state.database.get_proxy_by_ulid(&ulid).await {
        Ok(proxy) => {
            if !proxy.is_active {
                warn!("Proxy {} is not active", ulid);
                let mut headers = HeaderMap::new();
                headers.insert(
                    "content-type",
                    "application/vnd.apple.mpegurl".parse().unwrap(),
                );
                return (
                    StatusCode::NOT_FOUND,
                    headers,
                    "#EXTM3U\n# Proxy not active\n".to_string(),
                );
            }
            proxy
        }
        Err(e) => {
            error!("Failed to find proxy {}: {}", ulid, e);
            let mut headers = HeaderMap::new();
            headers.insert(
                "content-type",
                "application/vnd.apple.mpegurl".parse().unwrap(),
            );
            return (
                StatusCode::NOT_FOUND,
                headers,
                "#EXTM3U\n# Proxy not found\n".to_string(),
            );
        }
    };

    // 2. Try to serve static M3U8 file from disk
    let m3u_file_path = state.config.storage.m3u_path.join(format!("{}.m3u8", ulid));

    match fs::read_to_string(&m3u_file_path).await {
        Ok(content) => {
            info!(
                "Served static M3U8 for proxy {} from {}",
                ulid,
                m3u_file_path.display()
            );

            let mut headers = HeaderMap::new();
            headers.insert(
                "content-type",
                "application/vnd.apple.mpegurl".parse().unwrap(),
            );
            headers.insert("cache-control", "max-age=3600".parse().unwrap()); // 1 hour cache for static files

            (StatusCode::OK, headers, content)
        }
        Err(e) => {
            error!(
                "Failed to read M3U8 file for proxy {} at {}: {}",
                ulid,
                m3u_file_path.display(),
                e
            );

            // If file doesn't exist, suggest regeneration
            let mut headers = HeaderMap::new();
            headers.insert(
                "content-type",
                "application/vnd.apple.mpegurl".parse().unwrap(),
            );
            let content = format!(
                "#EXTM3U\n# Proxy {} M3U8 not generated yet - trigger regeneration\n",
                ulid
            );
            (StatusCode::NOT_FOUND, headers, content)
        }
    }
}

/// Serve XMLTV content for a proxy (from static file)
pub async fn serve_proxy_xmltv(
    axum::extract::Path(ulid): axum::extract::Path<String>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    use axum::http::{HeaderMap, StatusCode};
    use tokio::fs;
    use tracing::{error, info, warn};

    info!("Serving static XMLTV for proxy: {}", ulid);

    // 1. Look up proxy by ULID to validate it exists and is active
    let proxy = match state.database.get_proxy_by_ulid(&ulid).await {
        Ok(proxy) => {
            if !proxy.is_active {
                warn!("Proxy {} is not active", ulid);
                let mut headers = HeaderMap::new();
                headers.insert("content-type", "application/xml".parse().unwrap());
                return (StatusCode::NOT_FOUND, headers, "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<tv><!-- Proxy not active --></tv>".to_string());
            }
            proxy
        }
        Err(e) => {
            error!("Failed to find proxy {}: {}", ulid, e);
            let mut headers = HeaderMap::new();
            headers.insert("content-type", "application/xml".parse().unwrap());
            return (
                StatusCode::NOT_FOUND,
                headers,
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<tv><!-- Proxy not found --></tv>"
                    .to_string(),
            );
        }
    };

    // 2. Try to serve static XMLTV file from disk
    let xmltv_file_path = state
        .config
        .storage
        .m3u_path
        .join(format!("{}.xmltv", ulid));

    match fs::read_to_string(&xmltv_file_path).await {
        Ok(content) => {
            info!(
                "Served static XMLTV for proxy {} from {}",
                ulid,
                xmltv_file_path.display()
            );

            let mut headers = HeaderMap::new();
            headers.insert("content-type", "application/xml".parse().unwrap());
            headers.insert("cache-control", "max-age=3600".parse().unwrap()); // 1 hour cache for static files

            (StatusCode::OK, headers, content)
        }
        Err(e) => {
            error!(
                "Failed to read XMLTV file for proxy {} at {}: {}",
                ulid,
                xmltv_file_path.display(),
                e
            );

            // If file doesn't exist, return placeholder XMLTV
            let mut headers = HeaderMap::new();
            headers.insert("content-type", "application/xml".parse().unwrap());
            let content = format!(
                r#"<?xml version="1.0" encoding="UTF-8"?>
<tv generator-info-name="M3U Proxy" generator-info-url="https://github.com/jmylchreest/m3u-proxy">
  <!-- Proxy {} XMLTV not generated yet - trigger regeneration -->
</tv>"#,
                proxy.name
            );

            (StatusCode::NOT_FOUND, headers, content)
        }
    }
}

/// Proxy stream requests to original sources or relays
pub async fn proxy_stream(
    axum::extract::Path((proxy_ulid, channel_id)): axum::extract::Path<(String, uuid::Uuid)>,
    headers: axum::http::HeaderMap,
    State(state): State<AppState>,
) -> impl IntoResponse {
    use axum::http::StatusCode;
    use tracing::{error, info, warn};

    let client_ip = headers
        .get("x-forwarded-for")
        .or_else(|| headers.get("x-real-ip"))
        .and_then(|h| h.to_str().ok())
        .unwrap_or("unknown");

    let user_agent = headers
        .get("user-agent")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("unknown");

    info!(
        "Stream proxy request: proxy={}, channel={}, client_ip={}, user_agent={}",
        proxy_ulid, channel_id, client_ip, user_agent
    );

    // 1. Look up proxy to get its configuration
    let proxy = match state.database.get_proxy_by_ulid(&proxy_ulid).await {
        Ok(proxy) => {
            if !proxy.is_active {
                warn!("Proxy {} is not active", proxy_ulid);
                return (StatusCode::NOT_FOUND, "Proxy not active".to_string()).into_response();
            }
            proxy
        }
        Err(e) => {
            error!("Failed to find proxy {}: {}", proxy_ulid, e);
            return (StatusCode::NOT_FOUND, "Proxy not found".to_string()).into_response();
        }
    };

    // 2. Look up channel within proxy context
    let channel = match state
        .database
        .get_channel_for_proxy(&proxy_ulid, channel_id)
        .await
    {
        Ok(Some(channel)) => channel,
        Ok(None) => {
            warn!(
                "Channel {} not found in proxy {} or proxy not active",
                channel_id, proxy_ulid
            );
            return (
                StatusCode::NOT_FOUND,
                "Channel not found in this proxy".to_string(),
            )
                .into_response();
        }
        Err(e) => {
            error!(
                "Failed to lookup channel {} in proxy {}: {}",
                channel_id, proxy_ulid, e
            );
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Database error".to_string(),
            )
                .into_response();
        }
    };

    // 3. Check if relay is active for this channel (placeholder for now)
    // TODO: Implement relay lookup when relay manager is ready

    // 4. Log access metrics
    let session = state
        .metrics_logger
        .log_stream_start(
            proxy_ulid.clone(),
            channel_id,
            client_ip.to_string(),
            Some(user_agent.to_string()),
            headers
                .get("referer")
                .and_then(|h| h.to_str().ok())
                .map(|s| s.to_string()),
        )
        .await;

    match proxy.proxy_mode {
        StreamProxyMode::Redirect => {
            info!(
                "Redirecting stream request for channel '{}' to original URL: {}",
                channel.channel_name, channel.stream_url
            );

            // For redirects, immediately finish the session since we're not tracking the stream
            tokio::spawn(async move {
                session.finish(&state.metrics_logger, 0).await;
            });

            use axum::response::Redirect;
            Redirect::temporary(&channel.stream_url).into_response()
        }
        StreamProxyMode::Proxy => {
            info!(
                "Proxying stream request for channel '{}' from URL: {}",
                channel.channel_name, channel.stream_url
            );

            // Implement HTTP proxying
            proxy_http_stream(state, proxy, &channel.stream_url, headers, session).await
        }
    }
}

/// HTTP proxy implementation for streaming content
async fn proxy_http_stream(
    state: AppState,
    proxy: StreamProxy,
    upstream_url: &str,
    incoming_headers: axum::http::HeaderMap,
    session: crate::metrics::StreamAccessSession,
) -> axum::response::Response {
    use axum::body::Body;
    use axum::http::{HeaderMap, StatusCode, header};
    use axum::response::Response;
    use futures::StreamExt;
    use tracing::{debug, error, warn};

    let timeout_secs = proxy.upstream_timeout.unwrap_or(30) as u64;
    let _buffer_size = proxy.buffer_size.unwrap_or(8192) as usize;

    // Create HTTP client with timeout
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(timeout_secs))
        .build()
    {
        Ok(client) => client,
        Err(e) => {
            error!("Failed to create HTTP client: {}", e);
            let state_clone = state.clone();
            tokio::spawn(async move {
                session.finish(&state_clone.metrics_logger, 0).await;
            });
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to create HTTP client".to_string(),
            )
                .into_response();
        }
    };

    // Prepare headers to forward to upstream
    let mut upstream_headers = reqwest::header::HeaderMap::new();

    // Forward specific headers that are safe to proxy
    let safe_headers = [
        "accept",
        "accept-encoding",
        "accept-language",
        "range",
        "if-modified-since",
        "if-none-match",
        "cache-control",
    ];

    for header_name in safe_headers {
        if let Some(value) = incoming_headers.get(header_name) {
            if let Ok(header_value) = reqwest::header::HeaderValue::from_bytes(value.as_bytes()) {
                upstream_headers.insert(
                    reqwest::header::HeaderName::from_static(header_name),
                    header_value,
                );
            }
        }
    }

    debug!("Making upstream request to: {}", upstream_url);

    // Make upstream request
    let upstream_response = match client
        .get(upstream_url)
        .headers(upstream_headers)
        .send()
        .await
    {
        Ok(response) => response,
        Err(e) => {
            error!("Failed to connect to upstream {}: {}", upstream_url, e);
            let state_clone = state.clone();
            tokio::spawn(async move {
                session.finish(&state_clone.metrics_logger, 0).await;
            });
            return (
                StatusCode::BAD_GATEWAY,
                "Failed to connect to upstream".to_string(),
            )
                .into_response();
        }
    };

    // Get upstream status and headers
    let upstream_status = upstream_response.status();
    let upstream_headers = upstream_response.headers().clone();

    debug!("Upstream response status: {}", upstream_status);

    // Prepare response headers
    let mut response_headers = HeaderMap::new();

    // Forward specific upstream headers
    let forward_headers = [
        "content-type",
        "content-length",
        "content-range",
        "accept-ranges",
        "cache-control",
        "last-modified",
        "etag",
        "expires",
    ];

    for header_name in forward_headers {
        if let Some(value) = upstream_headers.get(header_name) {
            if let Ok(parsed_name) = header::HeaderName::from_bytes(header_name.as_bytes()) {
                if let Ok(parsed_value) = header::HeaderValue::from_bytes(value.as_bytes()) {
                    response_headers.insert(parsed_name, parsed_value);
                }
            }
        }
    }

    // Convert reqwest response to axum response
    let status =
        StatusCode::from_u16(upstream_status.as_u16()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);

    // Create streaming body
    let stream = upstream_response.bytes_stream();
    let mut bytes_served = 0u64;

    // Create a stream that tracks bytes for metrics
    let _state_clone = state.clone();
    let _session_clone = session.clone();
    let tracked_stream = stream.map(move |chunk_result| match chunk_result {
        Ok(chunk) => {
            bytes_served += chunk.len() as u64;
            debug!("Served {} bytes, total: {}", chunk.len(), bytes_served);
            Ok(chunk)
        }
        Err(e) => {
            warn!("Error streaming chunk: {}", e);
            Err(e)
        }
    });

    // Convert to axum Body
    let body = Body::from_stream(tracked_stream);

    // Spawn task to finish metrics when stream completes
    let final_state = state.clone();
    let final_session = session;
    tokio::spawn(async move {
        // Note: In a real implementation, we'd need to track when the stream actually completes
        // For now, we'll just log the start of the stream
        final_session
            .finish(&final_state.metrics_logger, bytes_served)
            .await;
    });

    // Build response
    let mut response = Response::builder().status(status);

    // Add headers
    for (name, value) in response_headers {
        if let Some(name) = name {
            response = response.header(name, value);
        }
    }

    match response.body(body) {
        Ok(response) => response.into_response(),
        Err(e) => {
            error!("Failed to build response: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to build response".to_string(),
            )
                .into_response()
        }
    }
}
