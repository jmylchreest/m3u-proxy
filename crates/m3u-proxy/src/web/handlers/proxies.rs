//! Proxy HTTP handlers
//!
//! This module contains HTTP handlers for proxy operations.

use axum::{
    Json,
    extract::{Path, State},
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;
use tracing::{debug, info, warn, error};

use crate::{
    database::repositories::{StreamProxySeaOrmRepository, ChannelSeaOrmRepository, FilterSeaOrmRepository, StreamSourceSeaOrmRepository},
    models::{StreamProxy, StreamProxyMode},
    proxy::session_tracker::{ClientInfo, SessionStats},
    utils::{resolve_proxy_id, uuid_parser::parse_uuid_flexible},
    web::{
        AppState,
        extractors::{ListParams, RequestContext},
        responses::ok,
        utils::log_request,
    },
};

/// Helper to create SeaORM repositories with shared connection
/// Since SeaORM handles connection pooling internally, we don't need separate read/write pools
fn create_repositories(database: &crate::database::Database) -> (
    StreamProxySeaOrmRepository,
    ChannelSeaOrmRepository,
    FilterSeaOrmRepository,
    StreamSourceSeaOrmRepository,
) {
    let connection = database.connection();
    (
        StreamProxySeaOrmRepository::new(connection.clone()),
        ChannelSeaOrmRepository::new(connection.clone()),
        FilterSeaOrmRepository::new(connection.clone()),
        StreamSourceSeaOrmRepository::new(connection),
    )
}

/// Request DTO for creating a stream proxy
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct CreateStreamProxyRequest {
    pub name: String,
    pub description: Option<String>,
    #[schema(example = "proxy")]
    pub proxy_mode: String, // Will be converted to StreamProxyMode
    #[schema(example = 30)]
    pub upstream_timeout: Option<i32>,
    #[schema(example = 1024)]
    pub buffer_size: Option<i32>,
    #[schema(example = 100)]
    pub max_concurrent_streams: Option<i32>,
    #[schema(example = 1)]
    pub starting_channel_number: i32,
    pub stream_sources: Vec<ProxySourceRequest>,
    pub epg_sources: Vec<ProxyEpgSourceRequest>,
    pub filters: Vec<ProxyFilterRequest>,
    pub is_active: bool,
    #[serde(default)]
    pub auto_regenerate: bool,
    #[serde(default = "default_cache_channel_logos")]
    pub cache_channel_logos: bool,
    #[serde(default)]
    pub cache_program_logos: bool,
    pub relay_profile_id: Option<Uuid>,
}

fn default_cache_channel_logos() -> bool {
    true
}

/// Stream source assignment for proxy
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct ProxySourceRequest {
    pub source_id: Uuid,
    pub priority_order: i32,
}

/// EPG source assignment for proxy
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct ProxyEpgSourceRequest {
    pub epg_source_id: Uuid,
    pub priority_order: i32,
}

/// Filter assignment for proxy
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct ProxyFilterRequest {
    pub filter_id: Uuid,
    pub priority_order: i32,
    pub is_active: bool,
}

/// Request DTO for updating a stream proxy
#[derive(Debug, Clone, Deserialize, ToSchema)]
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
    pub auto_regenerate: bool,
    #[serde(default = "default_cache_channel_logos")]
    pub cache_channel_logos: bool,
    #[serde(default)]
    pub cache_program_logos: bool,
    #[serde(deserialize_with = "crate::utils::deserialize_optional_uuid")]
    pub relay_profile_id: Option<Uuid>,
}

/// Response DTO for stream proxy
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct StreamProxyResponse {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub proxy_mode: String,
    pub upstream_timeout: Option<i32>,
    pub buffer_size: Option<i32>,
    pub max_concurrent_streams: Option<i32>,
    pub starting_channel_number: i32,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub is_active: bool,
    pub auto_regenerate: bool,
    pub cache_channel_logos: bool,
    pub cache_program_logos: bool,
    pub relay_profile_id: Option<Uuid>,
    pub stream_sources: Vec<ProxySourceResponse>,
    pub epg_sources: Vec<ProxyEpgSourceResponse>,
    pub filters: Vec<ProxyFilterResponse>,
    pub m3u8_url: String,
    pub xmltv_url: String,
}

/// Stream source in proxy response
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ProxySourceResponse {
    pub source_id: Uuid,
    pub source_name: String,
    pub priority_order: i32,
}

/// EPG source in proxy response
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ProxyEpgSourceResponse {
    pub epg_source_id: Uuid,
    pub epg_source_name: String,
    pub priority_order: i32,
}

/// Filter in proxy response
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ProxyFilterResponse {
    pub filter_id: Uuid,
    pub filter_name: String,
    pub priority_order: i32,
    pub is_active: bool,
    pub is_inverse: bool,
    pub source_type: crate::models::FilterSourceType,
}

impl CreateStreamProxyRequest {
    /// Convert to service layer request
    pub fn into_service_request(self) -> Result<crate::models::StreamProxyCreateRequest, String> {
        let proxy_mode = match self.proxy_mode.to_lowercase().as_str() {
            "redirect" => StreamProxyMode::Redirect,
            "proxy" => StreamProxyMode::Proxy,
            "relay" => StreamProxyMode::Relay,
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
            cache_channel_logos: self.cache_channel_logos,
            cache_program_logos: self.cache_program_logos,
            relay_profile_id: self.relay_profile_id,
        })
    }
}

impl StreamProxyResponse {
    /// Create a response from a StreamProxy with the provided base URL
    pub fn from_proxy_with_base_url(proxy: StreamProxy, base_url: &str) -> Self {
        use crate::utils::uuid_parser::uuid_to_base64;
        
        let trimmed_base_url = base_url.trim_end_matches('/');
        let proxy_id_b64 = uuid_to_base64(&proxy.id);
        
        Self {
            id: proxy.id,
            name: proxy.name,
            description: proxy.description,
            proxy_mode: match proxy.proxy_mode {
                StreamProxyMode::Redirect => "redirect".to_string(),
                StreamProxyMode::Proxy => "proxy".to_string(),
                StreamProxyMode::Relay => "relay".to_string(),
            },
            upstream_timeout: proxy.upstream_timeout,
            buffer_size: proxy.buffer_size,
            max_concurrent_streams: proxy.max_concurrent_streams,
            starting_channel_number: proxy.starting_channel_number,
            created_at: proxy.created_at,
            updated_at: proxy.updated_at,
            is_active: proxy.is_active,
            auto_regenerate: proxy.auto_regenerate,
            cache_channel_logos: proxy.cache_channel_logos,
            cache_program_logos: proxy.cache_program_logos,
            relay_profile_id: proxy.relay_profile_id,
            stream_sources: vec![], // Will be populated by service layer
            epg_sources: vec![],    // Will be populated by service layer
            filters: vec![],        // Will be populated by service layer
            m3u8_url: format!("{trimmed_base_url}/proxy/{proxy_id_b64}/m3u8"),
            xmltv_url: format!("{trimmed_base_url}/proxy/{proxy_id_b64}/xmltv"),
        }
    }
}

impl From<StreamProxy> for StreamProxyResponse {
    fn from(proxy: StreamProxy) -> Self {
        // Fallback implementation without base URL - URLs will be empty
        Self {
            id: proxy.id,
            name: proxy.name,
            description: proxy.description,
            proxy_mode: match proxy.proxy_mode {
                StreamProxyMode::Redirect => "redirect".to_string(),
                StreamProxyMode::Proxy => "proxy".to_string(),
                StreamProxyMode::Relay => "relay".to_string(),
            },
            upstream_timeout: proxy.upstream_timeout,
            buffer_size: proxy.buffer_size,
            max_concurrent_streams: proxy.max_concurrent_streams,
            starting_channel_number: proxy.starting_channel_number,
            created_at: proxy.created_at,
            updated_at: proxy.updated_at,
            is_active: proxy.is_active,
            auto_regenerate: proxy.auto_regenerate,
            cache_channel_logos: proxy.cache_channel_logos,
            cache_program_logos: proxy.cache_program_logos,
            relay_profile_id: proxy.relay_profile_id,
            stream_sources: vec![], // Will be populated by service layer
            epg_sources: vec![],    // Will be populated by service layer
            filters: vec![],        // Will be populated by service layer
            m3u8_url: String::new(),
            xmltv_url: String::new(),
        }
    }
}

// Preview DTOs
/// Request DTO for previewing a proxy configuration
#[derive(Debug, Clone, Deserialize, ToSchema)]
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
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct PreviewProxyResponse {
    pub channels: Vec<PreviewChannel>,
    pub stats: PreviewStats,
    pub m3u_content: Option<String>,
    pub total_channels: usize,
    pub filtered_channels: usize,
}

/// Channel information in preview
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct PreviewChannel {
    pub channel_name: String,
    pub group_title: Option<String>,
    pub tvg_id: Option<String>,
    pub tvg_logo: Option<String>,
    pub stream_url: String,
    pub source_name: String,
    pub channel_number: i32,
    pub tvg_chno: Option<String>,
    pub tvg_shift: Option<String>,
    pub tvg_language: Option<String>,
    pub tvg_country: Option<String>,
    pub group_logo: Option<String>,
    pub radio: Option<String>,
    pub extinf_line: String,
}

/// Statistics for preview
#[derive(Debug, Clone, Serialize, ToSchema)]
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
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct PipelineStageDetail {
    pub name: String,
    pub duration: u64,
    pub channels_processed: usize,
    pub memory_used: Option<u64>,
}

/// Processing event for timeline
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ProcessingEvent {
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub description: String,
    pub stage: Option<String>,
    pub channels_count: Option<usize>,
}

/// List all proxies
#[utoipa::path(
    get,
    path = "/proxies",
    tag = "proxies",
    summary = "List stream proxies",
    description = "Retrieve a list of stream proxy configurations",
    params(
        ("page" = Option<u32>, Query, description = "Page number (1-based)"),
        ("limit" = Option<u32>, Query, description = "Number of items per page"),
        ("search" = Option<String>, Query, description = "Search term"),
    ),
    responses(
        (status = 200, description = "List of stream proxies"),
        (status = 500, description = "Internal server error")
    )
)]
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

    // Create service instances from state using read repositories
    let (proxy_repo, channel_repo, filter_repo, stream_source_repo) = 
        create_repositories(&state.database);

    let service = crate::services::StreamProxyService::new(
        crate::services::StreamProxyServiceBuilder {
            proxy_repo,
            channel_repo,
            filter_repo,
            stream_source_repo,
            // TODO: Remove - superseded by pipeline-based filtering
            database: state.database.clone(),
            preview_file_manager: state.preview_file_manager.clone(),
            data_mapping_service: state.data_mapping_service.clone(),
            logo_service: state.logo_asset_service.clone(),
            storage_config: state.config.storage.clone(),
            app_config: state.config.clone(),
            temp_file_manager: state.temp_file_manager.clone(),
            proxy_output_file_manager: state.proxy_output_file_manager.clone(),
            system: state.system.clone(),
        },
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
#[utoipa::path(
    get,
    path = "/proxies/{id}",
    tag = "proxies",
    summary = "Get stream proxy",
    description = "Retrieve a specific stream proxy configuration by ID",
    params(
        ("id" = String, Path, description = "Proxy ID (UUID or friendly name)"),
    ),
    responses(
        (status = 200, description = "Stream proxy details"),
        (status = 404, description = "Stream proxy not found"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_proxy(
    State(state): State<AppState>,
    Path(id): Path<String>,
    context: RequestContext,
) -> impl IntoResponse {
    log_request(
        &axum::http::Method::GET,
        &format!("/api/v1/proxies/{id}").parse().unwrap(),
        &context,
    );

    let uuid = match resolve_proxy_id(&id) {
        Ok(uuid) => uuid,
        Err(error) => {
            return crate::web::responses::bad_request(&error.to_string()).into_response();
        }
    };

    // Create service instances
    let (proxy_repo, channel_repo, filter_repo, stream_source_repo) = 
        create_repositories(&state.database);

    let service = crate::services::StreamProxyService::new(
        crate::services::StreamProxyServiceBuilder {
            proxy_repo,
            channel_repo,
            filter_repo,
            stream_source_repo,
            // TODO: Remove - superseded by pipeline-based filtering
            database: state.database.clone(),
            preview_file_manager: state.preview_file_manager.clone(),
            data_mapping_service: state.data_mapping_service.clone(),
            logo_service: state.logo_asset_service.clone(),
            storage_config: state.config.storage.clone(),
            app_config: state.config.clone(),
            temp_file_manager: state.temp_file_manager.clone(),
            proxy_output_file_manager: state.proxy_output_file_manager.clone(),
            system: state.system.clone(),
        },
    );

    match service.get_by_id(uuid).await {
        Ok(Some(proxy)) => ok(proxy).into_response(),
        Ok(None) => crate::web::responses::not_found("stream_proxy", &id).into_response(),
        Err(err) => crate::web::responses::handle_error(err).into_response(),
    }
}

/// Create a new proxy
#[utoipa::path(
    post,
    path = "/proxies",
    tag = "proxies",
    summary = "Create stream proxy",
    description = "Create a new stream proxy configuration",
    request_body = CreateStreamProxyRequest,
    responses(
        (status = 201, description = "Stream proxy created successfully"),
        (status = 400, description = "Invalid request"),
        (status = 500, description = "Internal server error")
    )
)]
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

    // Create service instances using write repositories for mutations
    let (proxy_repo, channel_repo, filter_repo, stream_source_repo) = 
        create_repositories(&state.database);

    let service = crate::services::StreamProxyService::new(
        crate::services::StreamProxyServiceBuilder {
            proxy_repo,
            channel_repo,
            filter_repo,
            stream_source_repo,
            // TODO: Remove - superseded by pipeline-based filtering
            database: state.database.clone(),
            preview_file_manager: state.preview_file_manager.clone(),
            data_mapping_service: state.data_mapping_service.clone(),
            logo_service: state.logo_asset_service.clone(),
            storage_config: state.config.storage.clone(),
            app_config: state.config.clone(),
            temp_file_manager: state.temp_file_manager.clone(),
            proxy_output_file_manager: state.proxy_output_file_manager.clone(),
            system: state.system.clone(),
        },
    );

    match service.create(service_request).await {
        Ok(proxy) => ok(proxy).into_response(),
        Err(err) => crate::web::responses::handle_error(err).into_response(),
    }
}

/// Update an existing proxy
#[utoipa::path(
    put,
    path = "/proxies/{id}",
    tag = "proxies",
    summary = "Update stream proxy",
    description = "Update an existing stream proxy configuration",
    params(
        ("id" = String, Path, description = "Proxy ID (UUID or friendly name)"),
    ),
    request_body = UpdateStreamProxyRequest,
    responses(
        (status = 200, description = "Stream proxy updated successfully"),
        (status = 400, description = "Invalid request"),
        (status = 404, description = "Stream proxy not found"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn update_proxy(
    State(state): State<AppState>,
    Path(id): Path<String>,
    context: RequestContext,
    Json(request): Json<UpdateStreamProxyRequest>,
) -> impl IntoResponse {
    log_request(
        &axum::http::Method::PUT,
        &format!("/api/v1/proxies/{id}").parse().unwrap(),
        &context,
    );

    let uuid = match resolve_proxy_id(&id) {
        Ok(uuid) => uuid,
        Err(error) => {
            return crate::web::responses::bad_request(&error.to_string()).into_response();
        }
    };

    let service_request = crate::models::StreamProxyUpdateRequest {
        name: request.name,
        description: request.description,
        proxy_mode: match request.proxy_mode.to_lowercase().as_str() {
            "redirect" => crate::models::StreamProxyMode::Redirect,
            "proxy" => crate::models::StreamProxyMode::Proxy,
            "relay" => crate::models::StreamProxyMode::Relay,
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
        cache_channel_logos: request.cache_channel_logos,
        cache_program_logos: request.cache_program_logos,
        relay_profile_id: request.relay_profile_id,
    };

    // Create service instances using write repositories for mutations
    let (proxy_repo, channel_repo, filter_repo, stream_source_repo) = 
        create_repositories(&state.database);

    let service = crate::services::StreamProxyService::new(
        crate::services::StreamProxyServiceBuilder {
            proxy_repo,
            channel_repo,
            filter_repo,
            stream_source_repo,
            // TODO: Remove - superseded by pipeline-based filtering
            database: state.database.clone(),
            preview_file_manager: state.preview_file_manager.clone(),
            data_mapping_service: state.data_mapping_service.clone(),
            logo_service: state.logo_asset_service.clone(),
            storage_config: state.config.storage.clone(),
            app_config: state.config.clone(),
            temp_file_manager: state.temp_file_manager.clone(),
            proxy_output_file_manager: state.proxy_output_file_manager.clone(),
            system: state.system.clone(),
        },
    );

    match service.update(uuid, service_request).await {
        Ok(proxy) => ok(proxy).into_response(),
        Err(err) => crate::web::responses::handle_error(err).into_response(),
    }
}

/// Delete a proxy
#[utoipa::path(
    delete,
    path = "/proxies/{id}",
    tag = "proxies",
    summary = "Delete stream proxy",
    description = "Delete a stream proxy configuration",
    params(
        ("id" = String, Path, description = "Proxy ID (UUID or friendly name)"),
    ),
    responses(
        (status = 200, description = "Stream proxy deleted successfully"),
        (status = 404, description = "Stream proxy not found"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn delete_proxy(
    State(state): State<AppState>,
    Path(id): Path<String>,
    context: RequestContext,
) -> impl IntoResponse {
    log_request(
        &axum::http::Method::DELETE,
        &format!("/api/v1/proxies/{id}").parse().unwrap(),
        &context,
    );

    let uuid = match resolve_proxy_id(&id) {
        Ok(uuid) => uuid,
        Err(error) => {
            return crate::web::responses::bad_request(&error.to_string()).into_response();
        }
    };

    // Create service instances using write repositories for mutations
    let (proxy_repo, channel_repo, filter_repo, stream_source_repo) = 
        create_repositories(&state.database);

    let service = crate::services::StreamProxyService::new(
        crate::services::StreamProxyServiceBuilder {
            proxy_repo,
            channel_repo,
            filter_repo,
            stream_source_repo,
            // TODO: Remove - superseded by pipeline-based filtering
            database: state.database.clone(),
            preview_file_manager: state.preview_file_manager.clone(),
            data_mapping_service: state.data_mapping_service.clone(),
            logo_service: state.logo_asset_service.clone(),
            storage_config: state.config.storage.clone(),
            app_config: state.config.clone(),
            temp_file_manager: state.temp_file_manager.clone(),
            proxy_output_file_manager: state.proxy_output_file_manager.clone(),
            system: state.system.clone(),
        },
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
#[utoipa::path(
    post,
    path = "/proxies/preview",
    tag = "proxies",
    summary = "Preview proxy configuration",
    description = "Generate a preview of a proxy configuration without saving it to the database.

This endpoint allows you to:
- Test proxy configurations before creating them
- Preview the generated M3U playlist and channel count
- Validate source connections and filter rules
- See metadata transformations and logo assignments

The preview uses the same pipeline as actual proxy generation but stores results temporarily.",
    request_body = PreviewProxyRequest,
    responses(
        (status = 200, description = "Proxy preview generated successfully", body = PreviewProxyResponse),
        (status = 400, description = "Invalid proxy configuration"),
        (status = 422, description = "Validation errors in request"),
        (status = 500, description = "Internal server error during preview generation")
    )
)]
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
    let (proxy_repo, channel_repo, filter_repo, stream_source_repo) = 
        create_repositories(&state.database);

    let service = crate::services::StreamProxyService::new(
        crate::services::StreamProxyServiceBuilder {
            proxy_repo,
            channel_repo,
            filter_repo,
            stream_source_repo,
            // TODO: Remove - superseded by pipeline-based filtering
            database: state.database.clone(),
            preview_file_manager: state.preview_file_manager.clone(),
            data_mapping_service: state.data_mapping_service.clone(),
            logo_service: state.logo_asset_service.clone(),
            storage_config: state.config.storage.clone(),
            app_config: state.config.clone(),
            temp_file_manager: state.temp_file_manager.clone(),
            proxy_output_file_manager: state.proxy_output_file_manager.clone(),
            system: state.system.clone(),
        },
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
#[utoipa::path(
    get,
    path = "/proxies/{id}/preview",
    tag = "proxies",
    summary = "Preview existing proxy",
    description = "Generate a preview of an existing proxy using its current configuration.

This endpoint:
- Loads the existing proxy configuration from the database
- Regenerates the preview using current sources and filters
- Shows what the proxy would look like if regenerated now
- Useful for testing changes to sources/filters before regenerating

The ID can be provided in any supported format (UUID, base64-encoded UUID, etc.).",
    params(
        ("id" = String, Path, description = "Proxy identifier (UUID, base64, or other supported format)")
    ),
    responses(
        (status = 200, description = "Proxy preview generated successfully", body = PreviewProxyResponse),
        (status = 404, description = "Proxy not found"),
        (status = 400, description = "Invalid proxy ID format"),
        (status = 500, description = "Internal server error during preview generation")
    )
)]
#[axum::debug_handler]
pub async fn preview_existing_proxy(
    State(state): State<AppState>,
    Path(id): Path<String>,
    context: RequestContext,
) -> impl IntoResponse {
    log_request(
        &axum::http::Method::GET,
        &format!("/api/v1/proxies/{id}/preview").parse().unwrap(),
        &context,
    );

    let uuid = match resolve_proxy_id(&id) {
        Ok(uuid) => uuid,
        Err(error) => {
            return crate::web::responses::bad_request(&error.to_string()).into_response();
        }
    };

    // Create service instances
    let (proxy_repo, channel_repo, filter_repo, stream_source_repo) = 
        create_repositories(&state.database);

    let service = crate::services::StreamProxyService::new(
        crate::services::StreamProxyServiceBuilder {
            proxy_repo,
            channel_repo,
            filter_repo,
            stream_source_repo,
            // TODO: Remove - superseded by pipeline-based filtering
            database: state.database.clone(),
            preview_file_manager: state.preview_file_manager.clone(),
            data_mapping_service: state.data_mapping_service.clone(),
            logo_service: state.logo_asset_service.clone(),
            storage_config: state.config.storage.clone(),
            app_config: state.config.clone(),
            temp_file_manager: state.temp_file_manager.clone(),
            proxy_output_file_manager: state.proxy_output_file_manager.clone(),
            system: state.system.clone(),
        },
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
#[utoipa::path(
    get,
    path = "/proxies/{id}/playlist.m3u",
    tag = "proxies",
    summary = "Get proxy M3U playlist",
    description = "Retrieve the M3U playlist for a specific proxy",
    params(
        ("id" = String, Path, description = "Proxy ID (UUID or friendly name)"),
    ),
    responses(
        (status = 200, description = "M3U playlist content", content_type = "application/vnd.apple.mpegurl"),
        (status = 404, description = "Proxy not found or no playlist available"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn serve_proxy_m3u(
    axum::extract::Path(id): axum::extract::Path<String>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    use crate::utils::resolve_proxy_id;
    use axum::http::{HeaderMap, StatusCode};
    use tokio::fs;
    use tracing::{error, info, trace, warn};

    trace!("Serving static M3U8 for proxy: {}", id);

    // 1. Resolve proxy ID from any format and look up proxy
    let resolved_uuid = match resolve_proxy_id(&id) {
        Ok(uuid) => uuid,
        Err(e) => {
            error!("Invalid proxy ID format '{}': {}", id, e);
            let mut headers = HeaderMap::new();
            headers.insert("content-type", "application/x-mpegurl".parse().unwrap());
            return (
                StatusCode::BAD_REQUEST,
                headers,
                format!("Invalid proxy ID format: {e}"),
            );
        }
    };

    let proxy_repo = crate::database::repositories::StreamProxySeaOrmRepository::new(state.database.connection().clone());
    let _proxy = match proxy_repo
        .find_by_id(&resolved_uuid)
        .await
    {
        Ok(Some(proxy)) => {
            if !proxy.is_active {
                warn!("Proxy {} is not active", id);
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
        Ok(None) => {
            warn!("Proxy {} not found", id);
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
        Err(e) => {
            error!("Failed to find proxy {}: {}", id, e);
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

    // 2. Try to serve static M3U8 file from disk using resolved UUID
    let m3u_file_path = state
        .config
        .storage
        .m3u_path
        .join(format!("{resolved_uuid}.m3u8"));

    match fs::read_to_string(&m3u_file_path).await {
        Ok(content) => {
            info!(
                "Served static M3U8 for proxy {} from {}",
                id,
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
                id,
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
                "#EXTM3U\n# Proxy {id} M3U8 not generated yet - trigger regeneration\n"
            );
            (StatusCode::NOT_FOUND, headers, content)
        }
    }
}

/// Serve XMLTV Electronic Program Guide for a proxy
#[utoipa::path(
    get,
    path = "/proxy/{id}/xmltv",
    tag = "streaming",
    summary = "Get proxy XMLTV EPG",
    description = "Retrieve the XMLTV Electronic Program Guide file for a specific proxy.

This endpoint serves the EPG data in XMLTV format that corresponds to the channels
in the proxy's M3U playlist. The XMLTV file includes:
- Channel definitions with display names and logos
- Program schedules with titles, descriptions, and timing
- Filtered program data matching the proxy's channel selection

The ID can be provided in any supported format (UUID, base64-encoded UUID, etc.).",
    params(
        ("id" = String, Path, description = "Proxy identifier (UUID, base64, or other supported format)")
    ),
    responses(
        (status = 200, description = "XMLTV EPG content", content_type = "application/xml"),
        (status = 404, description = "Proxy not found or XMLTV not generated yet"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn serve_proxy_xmltv(
    axum::extract::Path(id): axum::extract::Path<String>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    use crate::utils::resolve_proxy_id;
    use axum::http::{HeaderMap, StatusCode};
    use tokio::fs;
    use tracing::{error, info, warn};

    info!("Serving static XMLTV for proxy: {}", id);

    // 1. Resolve proxy ID from any format and look up proxy
    let resolved_uuid = match resolve_proxy_id(&id) {
        Ok(uuid) => uuid,
        Err(e) => {
            error!("Invalid proxy ID format '{}': {}", id, e);
            let mut headers = HeaderMap::new();
            headers.insert("content-type", "application/xml".parse().unwrap());
            return (
                StatusCode::BAD_REQUEST,
                headers,
                format!("<!-- Invalid proxy ID format: {e} -->"),
            );
        }
    };

    let proxy_repo2 = crate::database::repositories::StreamProxySeaOrmRepository::new(state.database.connection().clone());
    let proxy = match proxy_repo2
        .find_by_id(&resolved_uuid)
        .await
    {
        Ok(Some(proxy)) => {
            if !proxy.is_active {
                warn!("Proxy {} is not active", id);
                let mut headers = HeaderMap::new();
                headers.insert("content-type", "application/xml".parse().unwrap());
                return (StatusCode::NOT_FOUND, headers, "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<tv><!-- Proxy not active --></tv>".to_string());
            }
            proxy
        }
        Ok(None) => {
            warn!("Proxy {} not found", id);
            let mut headers = HeaderMap::new();
            headers.insert("content-type", "application/xml".parse().unwrap());
            return (
                StatusCode::NOT_FOUND,
                headers,
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<tv><!-- Proxy not found --></tv>"
                    .to_string(),
            );
        }
        Err(e) => {
            error!("Failed to find proxy {}: {}", id, e);
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
        .join(format!("{resolved_uuid}.xmltv"));

    match fs::read_to_string(&xmltv_file_path).await {
        Ok(content) => {
            info!(
                "Served static XMLTV for proxy {} from {}",
                id,
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
                id,
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

/// Stream content through the proxy
#[utoipa::path(
    get,
    path = "/stream/{proxy_id}/{channel_id}",
    tag = "streaming",
    summary = "Stream IPTV channel",
    description = "Stream live IPTV content through the proxy with metrics tracking and relay support.

This is the main streaming endpoint that:
- Proxies live streams from original sources
- Captures viewing metrics and statistics  
- Supports relay/transcoding when configured
- Handles client connection management
- Provides health monitoring for upstream sources

The proxy_id and channel_id are base64-encoded UUIDs from the M3U playlist.",
    params(
        ("proxy_id" = String, Path, description = "Base64-encoded proxy UUID (from M3U playlist)"),
        ("channel_id" = String, Path, description = "Base64-encoded channel UUID (from M3U playlist)")
    ),
    responses(
        (status = 200, description = "Streaming content (video/audio stream)", content_type = "video/mp2t"),
        (status = 404, description = "Proxy or channel not found"),
        (status = 502, description = "Upstream source unavailable"),
        (status = 503, description = "Service temporarily unavailable")
    ),
    security(
        // No authentication required for streaming
    )
)]
pub async fn proxy_stream(
    axum::extract::Path((proxy_id, channel_id_str)): axum::extract::Path<(String, String)>,
    headers: axum::http::HeaderMap,
    State(state): State<AppState>,
) -> impl IntoResponse {
    use axum::http::StatusCode;
    use tracing::{debug, error, info, warn};

    let client_ip = headers
        .get("x-forwarded-for")
        .or_else(|| headers.get("x-real-ip"))
        .and_then(|h| h.to_str().ok())
        .unwrap_or("unknown")
        .to_string();

    let user_agent = headers
        .get("user-agent")
        .and_then(|h| h.to_str().ok())
        .map(|s| s.to_string());

    let referer = headers
        .get("referer")
        .and_then(|h| h.to_str().ok())
        .map(|s| s.to_string());

    // 1. Resolve proxy ID from any supported format
    let resolved_proxy_uuid = match resolve_proxy_id(&proxy_id) {
        Ok(uuid) => uuid,
        Err(e) => {
            error!("Invalid proxy ID format '{}': {}", proxy_id, e);
            return (
                StatusCode::BAD_REQUEST,
                format!("Invalid proxy ID format: {e}"),
            )
                .into_response();
        }
    };

    // 2. Resolve channel ID from any supported format (UUID or base64)
    let channel_id = match parse_uuid_flexible(&channel_id_str) {
        Ok(uuid) => uuid,
        Err(e) => {
            error!("Invalid channel ID format '{}': {}", channel_id_str, e);
            return (
                StatusCode::BAD_REQUEST,
                format!("Invalid channel ID format: {e}"),
            )
                .into_response();
        }
    };

    debug!(
        "Stream proxy request: proxy={}, channel={}, client_ip={}, user_agent={}",
        proxy_id,
        channel_id,
        client_ip,
        user_agent.as_deref().unwrap_or("unknown")
    );

    // 3. Look up proxy to get its configuration
    let proxy_repo3 = crate::database::repositories::StreamProxySeaOrmRepository::new(state.database.connection().clone());
    let proxy = match proxy_repo3
        .find_by_id(&resolved_proxy_uuid)
        .await
    {
        Ok(Some(proxy)) => {
            if !proxy.is_active {
                warn!("Proxy {} is not active", proxy_id);
                return (StatusCode::NOT_FOUND, "Proxy not active".to_string()).into_response();
            }
            proxy
        }
        Ok(None) => {
            warn!("Proxy {} not found", proxy_id);
            return (StatusCode::NOT_FOUND, "Proxy not found".to_string()).into_response();
        }
        Err(e) => {
            error!("Failed to find proxy {}: {}", proxy_id, e);
            return (StatusCode::NOT_FOUND, "Proxy not found".to_string()).into_response();
        }
    };

    // 2. Look up channel within proxy context using repository
    let stream_proxy_repo = StreamProxySeaOrmRepository::new(state.database.connection().clone());
    let channel = match stream_proxy_repo
        .get_channel_for_proxy(resolved_proxy_uuid, channel_id)
        .await
    {
        Ok(Some(channel)) => channel,
        Ok(None) => {
            // Enhanced diagnostic information for channel access issues
            let diagnostic_info = diagnose_channel_access_issue_seaorm(
                &state.database,
                &resolved_proxy_uuid,
                &channel_id.to_string(),
            ).await;
            
            warn!(
                "Channel {} not accessible in proxy {}: {}",
                channel_id, resolved_proxy_uuid, diagnostic_info
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
                channel_id, resolved_proxy_uuid, e
            );
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Database error".to_string(),
            )
                .into_response();
        }
    };

    // Note: Relay configuration is now handled per-proxy basis in the match statement below

    // 4. Log access metrics and create active session
    let session = state
        .metrics_logger
        .log_stream_start(
            proxy.name.clone(),
            channel_id,
            client_ip.clone(),
            user_agent.clone(),
            headers
                .get("referer")
                .and_then(|h| h.to_str().ok())
                .map(|s| s.to_string()),
        )
        .await;

    // Note: Relay logic is now handled in the match statement below based on proxy_mode

    match proxy.proxy_mode {
        StreamProxyMode::Redirect => {
            info!(
                "Redirecting stream request for channel '{}' to original URL: {}",
                channel.channel_name, channel.stream_url
            );

            // Create active session for redirect (will be cleaned up quickly by housekeeper)
            let session_id = match state
                .metrics_logger
                .create_active_session(
                    proxy.name.clone(),
                    channel_id,
                    client_ip.clone(),
                    user_agent.clone(),
                    referer.clone(),
                    "redirect",
                )
                .await
            {
                Ok(id) => id,
                Err(e) => {
                    error!("Failed to create active session: {}", e);
                    // Continue with redirect even if metrics fail
                    String::new()
                }
            };

            // Create session tracker entry
            let client_info = ClientInfo {
                ip: client_ip.clone(),
                user_agent: user_agent.clone(),
                referer: referer.clone(),
            };

            let session_stats = SessionStats::new(
                session_id.clone(),
                client_info,
                proxy.name.clone(),
                proxy.name.clone(),
                channel_id.to_string(),
                channel.channel_name.clone(),
                channel.stream_url.clone(),
            );

            state.session_tracker.start_session(session_stats).await;

            // For redirects, immediately finish the session since we're not tracking the stream
            let state_clone = state.clone();
            let session_id_clone = session_id.clone();
            tokio::spawn(async move {
                session.finish(&state_clone.metrics_logger, 0).await;
                if !session_id_clone.is_empty() {
                    if let Err(e) = state_clone
                        .metrics_logger
                        .complete_active_session(&session_id_clone)
                        .await
                    {
                        error!("Failed to complete active session: {}", e);
                    }
                }
                // End session tracking for redirect
                state_clone
                    .session_tracker
                    .end_session(&session_id_clone)
                    .await;
            });

            use axum::response::Redirect;
            Redirect::temporary(&channel.stream_url).into_response()
        }
        StreamProxyMode::Proxy => {
            info!(
                "Proxying stream request for channel '{}' from URL: {}",
                channel.channel_name, channel.stream_url
            );

            // Create active session for proxy tracking
            let session_id = match state
                .metrics_logger
                .create_active_session(
                    proxy.name.clone(),
                    channel_id,
                    client_ip.clone(),
                    user_agent.clone(),
                    referer.clone(),
                    "proxy",
                )
                .await
            {
                Ok(id) => id,
                Err(e) => {
                    error!("Failed to create active session: {}", e);
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "Failed to create session".to_string(),
                    )
                        .into_response();
                }
            };

            // Create session tracker entry for proxy mode
            let client_info = ClientInfo {
                ip: client_ip.clone(),
                user_agent: user_agent.clone(),
                referer: referer.clone(),
            };

            let session_stats = SessionStats::new(
                session_id.clone(),
                client_info,
                proxy.name.clone(),
                proxy.name.clone(),
                channel_id.to_string(),
                channel.channel_name.clone(),
                channel.stream_url.clone(),
            );

            state.session_tracker.start_session(session_stats.clone()).await;

            // Implement HTTP proxying (SeaORM-compatible implementation)
            proxy_http_stream_seaorm(
                channel.stream_url.clone(),
                headers,
                state.session_tracker.clone(),
                session_stats,
            ).await
        }
        StreamProxyMode::Relay => {
            info!(
                "Relay mode stream request for channel '{}' from URL: {}",
                channel.channel_name, channel.stream_url
            );

            // Check if proxy has a relay profile configured
            let _relay_profile_id = match proxy.relay_profile_id {
                Some(id) => id,
                None => {
                    error!(
                        "Relay mode requested but no relay profile configured for proxy {}",
                        proxy.id
                    );
                    return (
                        StatusCode::BAD_REQUEST,
                        "No relay profile configured for this proxy",
                    )
                        .into_response();
                }
            };

            // Create session for relay mode
            let session_id = match state
                .metrics_logger
                .create_active_session(
                    proxy.name.clone(),
                    channel_id,
                    client_ip.clone(),
                    user_agent.clone(),
                    referer.clone(),
                    "relay",
                )
                .await
            {
                Ok(id) => id,
                Err(e) => {
                    error!("Failed to create active session: {}", e);
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "Failed to create session".to_string(),
                    )
                        .into_response();
                }
            };

            // Create session tracker entry for relay mode
            let client_info = ClientInfo {
                ip: client_ip.clone(),
                user_agent: user_agent.clone(),
                referer: referer.clone(),
            };

            let session_stats = SessionStats::new(
                session_id.clone(),
                client_info,
                proxy.name.clone(),
                proxy.name.clone(),
                channel_id.to_string(),
                channel.channel_name.clone(),
                channel.stream_url.clone(),
            );

            state.session_tracker.start_session(session_stats.clone()).await;

            // Resolve relay configuration
            let relay_config = match state.relay_config_resolver.resolve_relay_config(
                proxy.id,
                channel_id,
                _relay_profile_id,
            ).await {
                Ok(config) => config,
                Err(e) => {
                    error!("Failed to resolve relay configuration: {}", e);
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "Failed to resolve relay configuration",
                    )
                        .into_response();
                }
            };

            // Ensure relay is running
            if let Err(e) = state.relay_manager.ensure_relay_running(&relay_config, &channel.stream_url).await {
                error!("Failed to start relay: {}", e);
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Failed to start relay process",
                )
                    .into_response();
            }

            // Create client info for relay
            let client_info = ClientInfo {
                ip: client_ip.clone(),
                user_agent: user_agent.clone(),
                referer: referer.clone(),
            };

            // Serve relay content
            match state.relay_manager.serve_relay_content(
                relay_config.config.id,
                "",
                &client_info,
            ).await {
                Ok(crate::models::relay::RelayContent::Stream(stream)) => {
                    use futures::StreamExt;
                    use axum::body::Body;
                    use axum::http::{StatusCode, header};
                    
                    // Track bytes served for session tracking
                    let state_clone = state.clone();
                    let session_id_clone = session_id.clone();
                    let tracked_stream = stream.map(move |chunk_result| {
                        match &chunk_result {
                            Ok(chunk) => {
                                let bytes_len = chunk.len() as u64;
                                // Update session tracker with bytes served (fire and forget)
                                let state_update = state_clone.clone();
                                let session_update = session_id_clone.clone();
                                tokio::spawn(async move {
                                    state_update
                                        .session_tracker
                                        .update_session_bytes(&session_update, bytes_len)
                                        .await;
                                });
                            }
                            Err(e) => {
                                error!("Stream error: {}", e);
                            }
                        }
                        chunk_result
                    });
                    
                    axum::response::Response::builder()
                        .status(StatusCode::OK)
                        .header(header::CONTENT_TYPE, "video/mp2t")
                        .header(header::CACHE_CONTROL, "no-cache, no-store")
                        .body(Body::from_stream(tracked_stream))
                        .unwrap()
                        .into_response()
                }
                Ok(crate::models::relay::RelayContent::Segment(data)) => {
                    use axum::body::Body;
                    use axum::http::{StatusCode, header};
                    
                    info!("Serving relay segment: {} bytes", data.len());
                    
                    // Update session tracker with bytes served
                    let bytes_len = data.len() as u64;
                    tokio::spawn(async move {
                        state
                            .session_tracker
                            .update_session_bytes(&session_id, bytes_len)
                            .await;
                    });
                    
                    axum::response::Response::builder()
                        .status(StatusCode::OK)
                        .header(header::CONTENT_TYPE, "video/mp2t")
                        .header(header::CACHE_CONTROL, "no-cache, no-store")
                        .body(Body::from(data))
                        .unwrap()
                        .into_response()
                }
                Ok(crate::models::relay::RelayContent::Playlist(playlist_content)) => {
                    use axum::body::Body;
                    use axum::http::{StatusCode, header};
                    
                    info!("Serving relay playlist: {} bytes", playlist_content.len());
                    
                    axum::response::Response::builder()
                        .status(StatusCode::OK)
                        .header(header::CONTENT_TYPE, "application/vnd.apple.mpegurl")
                        .header(header::CACHE_CONTROL, "no-cache, no-store")
                        .body(Body::from(playlist_content))
                        .unwrap()
                        .into_response()
                }
                Err(e) => {
                    error!("Failed to serve relay content: {}", e);
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "Failed to serve relay content",
                    )
                        .into_response()
                }
            }
        }
    }
}

/// HTTP stream proxying implementation compatible with SeaORM migration
async fn proxy_http_stream_seaorm(
    stream_url: String,
    _headers: axum::http::HeaderMap,
    session_tracker: std::sync::Arc<crate::proxy::session_tracker::SessionTracker>,
    session_stats: crate::proxy::session_tracker::SessionStats,
) -> axum::response::Response<axum::body::Body> {
    use axum::response::Response;
    use axum::body::Body;
    use axum::http::{header, StatusCode};
    
    info!("Proxying HTTP stream from: {}", stream_url);
    
    // Create HTTP client for proxying
    let client = reqwest::Client::builder()
        .user_agent("m3u-proxy/SeaORM")
        .timeout(std::time::Duration::from_secs(30))
        .build();
    
    let client = match client {
        Ok(client) => client,
        Err(e) => {
            error!("Failed to create HTTP client: {}", e);
            session_tracker.end_session(&session_stats.session_id).await;
            return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to create HTTP client").into_response();
        }
    };
    
    // Make request to source stream
    let response = match client.get(&stream_url).send().await {
        Ok(response) => response,
        Err(e) => {
            error!("Failed to connect to stream URL {}: {}", stream_url, e);
            session_tracker.end_session(&session_stats.session_id).await;
            return (StatusCode::BAD_GATEWAY, format!("Failed to connect to stream: {}", e)).into_response();
        }
    };
    
    // Check if source responded successfully
    if !response.status().is_success() {
        error!("Stream source returned error: {}", response.status());
        session_tracker.end_session(&session_stats.session_id).await;
        return (StatusCode::BAD_GATEWAY, format!("Stream source error: {}", response.status())).into_response();
    }
    
    // Extract content type from source
    let content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|ct| ct.to_str().ok())
        .unwrap_or("video/mp2t") // Default to MPEG-TS for streams
        .to_string(); // Convert to owned String
    
    info!("Streaming content type: {}", content_type);
    
    // Convert response to streaming body
    let stream_body = response.bytes_stream();
    let body = Body::from_stream(stream_body);
    
    // Create response with appropriate headers
    match Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CACHE_CONTROL, "no-cache, no-store")
        .header("Access-Control-Allow-Origin", "*")
        .header("Access-Control-Allow-Methods", "GET, HEAD, OPTIONS")
        .header("Access-Control-Allow-Headers", "Range, Content-Type")
        .body(body)
    {
        Ok(response) => response.into_response(),
        Err(e) => {
            error!("Failed to create response: {}", e);
            tokio::spawn(async move {
                session_tracker.end_session(&session_stats.session_id).await;
            });
            (StatusCode::INTERNAL_SERVER_ERROR, "Internal server error").into_response()
        }
    }
}

/// Diagnose why a channel cannot be accessed in a proxy using SeaORM
async fn diagnose_channel_access_issue_seaorm(
    database: &crate::database::Database,
    proxy_uuid: &uuid::Uuid,
    channel_id: &str,
) -> String {
    use crate::database::repositories::{StreamProxySeaOrmRepository, ChannelSeaOrmRepository, StreamSourceSeaOrmRepository};
    
    // Create repositories
    let proxy_repo = StreamProxySeaOrmRepository::new(database.connection().clone());
    let channel_repo = ChannelSeaOrmRepository::new(database.connection().clone());
    let _source_repo = StreamSourceSeaOrmRepository::new(database.connection().clone());
    
    let mut diagnostics = Vec::new();
    
    // Check if proxy exists
    match proxy_repo.find_by_id(proxy_uuid).await {
        Ok(Some(_)) => {
            // Proxy exists, check its sources
            match proxy_repo.get_proxy_sources(*proxy_uuid).await {
                Ok(sources) => {
                    if sources.is_empty() {
                        diagnostics.push("Proxy has no configured sources".to_string());
                    } else {
                        diagnostics.push(format!("Proxy has {} configured source(s)", sources.len()));
                        
                        // Check if channel exists in any source (simplified for now)
                        diagnostics.push(format!("Checking {} proxy sources for channel", sources.len()));
                    }
                }
                Err(e) => {
                    diagnostics.push(format!("Failed to get proxy sources: {}", e));
                }
            }
        }
        Ok(None) => {
            diagnostics.push("Proxy not found in database".to_string());
        }
        Err(e) => {
            diagnostics.push(format!("Database error checking proxy: {}", e));
        }
    }
    
    // Check if channel exists anywhere (simplified search)
    if let Ok(channels) = channel_repo.find_all().await {
        let matching_channels: Vec<_> = channels.iter()
            .filter(|ch| ch.id.to_string() == channel_id)
            .collect();
        
        if matching_channels.is_empty() {
            diagnostics.push("Channel does not exist in any source".to_string());
        } else {
            diagnostics.push(format!("Channel exists in {} source(s)", matching_channels.len()));
        }
    } else {
        diagnostics.push("Error searching for channel globally".to_string());
    }
    
    diagnostics.join("; ")
}

// TODO: Re-enable unreachable relay functionality code when SeaORM migration is complete
/*
            let session_id = match state
                .metrics_logger
                .create_active_session(
                    proxy.name.clone(),
                    channel_id,
                    client_ip.clone(),
                    user_agent.clone(),
                    referer.clone(),
                    "relay",
                )
                .await
            {
                Ok(id) => id,
                Err(e) => {
                    error!("Failed to create active session: {}", e);
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "Failed to create session".to_string(),
                    )
                        .into_response();
                }
            };

            // Create session tracker entry for relay mode
            let client_info = crate::proxy::session_tracker::ClientInfo {
                ip: client_ip.clone(),
                user_agent: user_agent.clone(),
                referer: referer.clone(),
            };

            let session_stats = crate::proxy::session_tracker::SessionStats::new(
                session_id.clone(),
                client_info,
                proxy.name.clone(),
                proxy.name.clone(),
                channel_id.to_string(),
                channel.channel_name.clone(),
                channel.stream_url.clone(),
            );

            state.session_tracker.start_session(session_stats).await;

            // Ensure FFmpeg relay process is running
            match state
                .relay_manager
                .ensure_relay_running(&resolved_config, &channel.stream_url)
                .await
            {
                Ok(_) => {
                }
                Err(e) => {
                    error!("Failed to start FFmpeg relay process: {}", e);
                    // Clean up session
                    session.finish(&state.metrics_logger, 0).await;
                    if let Err(e) = state
                        .metrics_logger
                        .complete_active_session(&session_id)
                        .await
                    {
                        error!("Failed to complete active session: {}", e);
                    }
                    state.session_tracker.end_session(&session_id).await;
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "Failed to start relay process",
                    )
                        .into_response();
                }
            }

            // Serve content using relay manager directly
            
            let client_info = crate::proxy::session_tracker::ClientInfo {
                ip: client_ip.clone(),
                user_agent: user_agent.clone(),
                referer: referer.clone(),
            };

            // TODO: Re-enable when relay_manager is available
                resolved_config.config.id,
                "stream.ts",
                &client_info,
            ).await {
                Ok(crate::models::relay::RelayContent::Stream(stream)) => {
                    use futures::StreamExt;
                    use axum::body::Body;
                    use axum::http::{StatusCode, header};
                    
                    // Track bytes served for session tracking
                    let state_clone = state.clone();
                    let session_id_clone = session_id.clone();
                    let tracked_stream = stream.map(move |chunk_result| {
                        match &chunk_result {
                            Ok(chunk) => {
                                let bytes_len = chunk.len() as u64;
                                // Update session tracker with bytes served (fire and forget)
                                let state_update = state_clone.clone();
                                let session_update = session_id_clone.clone();
                                tokio::spawn(async move {
                                    state_update
                                        .session_tracker
                                        .update_session_bytes(&session_update, bytes_len)
                                        .await;
                                });
                            }
                            Err(e) => {
                                error!("Error in relay stream: {}", e);
                                // Record error in session tracker
                                let state_error = state_clone.clone();
                                let session_error = session_id_clone.clone();
                                let error_msg = e.to_string();
                                tokio::spawn(async move {
                                    state_error
                                        .session_tracker
                                        .record_session_error(&session_error, error_msg)
                                        .await;
                                });
                            }
                        }
                        chunk_result
                    });

                    axum::response::Response::builder()
                        .status(StatusCode::OK)
                        .header(header::CONTENT_TYPE, "video/mp2t")
                        .header(header::CACHE_CONTROL, "no-cache, no-store")
                        .body(Body::from_stream(tracked_stream))
                        .unwrap()
                        .into_response()
                }
                Ok(crate::models::relay::RelayContent::Segment(data)) => {
                    use axum::body::Body;
                    use axum::http::{StatusCode, header};
                    
                    info!("Serving relay segment: {} bytes", data.len());

                    axum::response::Response::builder()
                        .status(StatusCode::OK)
                        .header(header::CONTENT_TYPE, "video/mp2t")
                        .header(header::CACHE_CONTROL, "no-cache, no-store")
                        .body(Body::from(data))
                        .unwrap()
                        .into_response()
                }
                Ok(_content) => {
                    error!("Unexpected content type from relay (expected stream or segment)");
                    (StatusCode::INTERNAL_SERVER_ERROR, "Unexpected content type").into_response()
                }
                Err(e) => {
                    error!("Failed to serve relay content: {}", e);
                    (StatusCode::INTERNAL_SERVER_ERROR, "Failed to serve relay content").into_response()
                }
            }
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
    session_id: String,
) -> axum::response::Response {
    use axum::body::Body;
    use axum::http::{HeaderMap, StatusCode, header};
    use axum::response::Response;
    use futures::StreamExt;
    use tracing::{debug, error, warn};

    let connection_timeout_secs = 10u64; // Connection timeout
    let _read_timeout_secs = proxy.upstream_timeout.unwrap_or(120) as u64; // Read timeout between chunks
    let _buffer_size = proxy.buffer_size.unwrap_or(8192) as usize;

    // Create HTTP client with proper timeouts for streaming
    let client = match reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(connection_timeout_secs))
        // Set a very large timeout for streaming (24 hours)
        .timeout(std::time::Duration::from_secs(24 * 60 * 60))
        .tcp_keepalive(std::time::Duration::from_secs(30))
        .tcp_nodelay(true)
        .build()
    {
        Ok(client) => client,
        Err(e) => {
            error!("Failed to create HTTP client: {}", e);
            let state_clone = state.clone();
            let session_id_clone = session_id.clone();
            tokio::spawn(async move {
                session.finish(&state_clone.metrics_logger, 0).await;
                if let Err(e) = state_clone
                    .metrics_logger
                    .complete_active_session(&session_id_clone)
                    .await
                {
                    error!("Failed to complete active session: {}", e);
                }
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

    debug!(
        "Making upstream request to: {} (connect_timeout={}s, stream_timeout=disabled)",
        UrlUtils::obfuscate_credentials(upstream_url),
        connection_timeout_secs
    );

    // Make upstream request
    let upstream_response = match client
        .get(upstream_url)
        .headers(upstream_headers)
        .send()
        .await
    {
        Ok(response) => response,
        Err(e) => {
            error!(
                "Failed to connect to upstream {}: {}",
                UrlUtils::obfuscate_credentials(upstream_url),
                e
            );
            let state_clone = state.clone();
            let session_id_clone = session_id.clone();
            tokio::spawn(async move {
                session.finish(&state_clone.metrics_logger, 0).await;
                if let Err(e) = state_clone
                    .metrics_logger
                    .complete_active_session(&session_id_clone)
                    .await
                {
                    error!("Failed to complete active session: {}", e);
                }
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

    // Create streaming body with proper completion tracking
    let stream = upstream_response.bytes_stream();

    // Create a stream that tracks bytes for metrics and handles completion
    let state_clone = state.clone();
    let session_id_clone = session_id.clone();
    let session_clone = session.clone();

    // Use a shared counter for total bytes served
    let total_bytes_served = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
    let stream_ended = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));

    debug!(
        "Starting stream for session: {} (upstream: {})",
        session_id_clone,
        UrlUtils::obfuscate_credentials(upstream_url)
    );

    let tracked_stream = stream.map({
        let total_bytes_served = total_bytes_served.clone();
        let stream_ended = stream_ended.clone();
        move |chunk_result| {
            match chunk_result {
                Ok(chunk) => {
                    let bytes_len = chunk.len() as u64;
                    let total = total_bytes_served
                        .fetch_add(bytes_len, std::sync::atomic::Ordering::Relaxed)
                        + bytes_len;
                    tracing::trace!("Served {} bytes, total: {}", bytes_len, total);

                    // Update active session with new bytes (fire and forget)
                    let state_update = state_clone.clone();
                    let session_update = session_id_clone.clone();
                    tokio::spawn(async move {
                        if let Err(e) = state_update
                            .metrics_logger
                            .update_active_session(&session_update, bytes_len)
                            .await
                        {
                            debug!("Failed to update active session metrics: {}", e);
                        }
                        // Update session tracker with bytes served
                        state_update
                            .session_tracker
                            .update_session_bytes(&session_update, bytes_len)
                            .await;
                    });

                    Ok(chunk)
                }
                Err(e) => {
                    warn!("Error streaming chunk: {}", e);

                    // Record error in session tracker
                    let state_error = state_clone.clone();
                    let session_error = session_id_clone.clone();
                    let error_msg = e.to_string();
                    tokio::spawn(async move {
                        state_error
                            .session_tracker
                            .record_session_error(&session_error, error_msg)
                            .await;
                    });

                    // Mark stream as ended on error (only once)
                    if !stream_ended.swap(true, std::sync::atomic::Ordering::Relaxed) {
                        let final_state = state_clone.clone();
                        let final_session = session_clone.clone();
                        let final_session_id = session_id_clone.clone();
                        let final_bytes =
                            total_bytes_served.load(std::sync::atomic::Ordering::Relaxed);
                        tokio::spawn(async move {
                            debug!("Stream ended with error for session: {}", final_session_id);

                            // Finish metrics tracking
                            final_session
                                .finish(&final_state.metrics_logger, final_bytes)
                                .await;

                            // Complete the active session
                            if let Err(e) = final_state
                                .metrics_logger
                                .complete_active_session(&final_session_id)
                                .await
                            {
                                error!("Failed to complete active session: {}", e);
                            }

                            // End session tracking
                            final_state
                                .session_tracker
                                .end_session(&final_session_id)
                                .await;
                        });
                    }

                    Err(e)
                }
            }
        }
    });

    // Wrap the stream to detect completion (both success and error)
    let completion_state = state.clone();
    let completion_session = session.clone();
    let completion_session_id = session_id.clone();
    let completion_bytes = total_bytes_served.clone();
    let completion_ended = stream_ended.clone();

    // Create a cancellation token to coordinate cleanup
    let cancellation_token = tokio_util::sync::CancellationToken::new();
    let token_for_heartbeat = cancellation_token.clone();

    // Start a heartbeat task to detect truly stale connections
    let heartbeat_state = completion_state.clone();
    let heartbeat_session_id = completion_session_id.clone();
    let heartbeat_session = completion_session.clone();
    let heartbeat_bytes = completion_bytes.clone();
    let heartbeat_ended = completion_ended.clone();
    
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
        let mut last_bytes = 0u64;
        let mut stale_count = 0u8;
        
        loop {
            tokio::select! {
                _ = token_for_heartbeat.cancelled() => {
                    debug!("Heartbeat cancelled for session: {}", heartbeat_session_id);
                    break;
                }
                _ = interval.tick() => {
                    let current_bytes = heartbeat_bytes.load(std::sync::atomic::Ordering::Relaxed);
                    
                    // Check if bytes are still increasing (stream is active)
                    if current_bytes == last_bytes {
                        stale_count += 1;
                        debug!(
                            "Session {} stale check {}/3: {} bytes (no progress)",
                            heartbeat_session_id, stale_count, current_bytes
                        );
                        
                        if stale_count >= 3 {
                            // No progress for 90 seconds - client likely disconnected
                            if !heartbeat_ended.swap(true, std::sync::atomic::Ordering::Relaxed) {
                                warn!(
                                    "Detected stale connection for session {} - no progress for {}s, cleaning up",
                                    heartbeat_session_id, stale_count * 30
                                );
                                
                                // Handle cleanup asynchronously
                                let cleanup_state = heartbeat_state.clone();
                                let cleanup_session = heartbeat_session.clone();
                                let cleanup_session_id = heartbeat_session_id.clone();
                                tokio::spawn(async move {
                                    // Finish metrics tracking
                                    cleanup_session
                                        .finish(&cleanup_state.metrics_logger, current_bytes)
                                        .await;

                                    // Complete the active session
                                    if let Err(e) = cleanup_state
                                        .metrics_logger
                                        .complete_active_session(&cleanup_session_id)
                                        .await
                                    {
                                        error!("Failed to complete stale session: {}", e);
                                    }

                                    // End session tracking
                                    cleanup_state
                                        .session_tracker
                                        .end_session(&cleanup_session_id)
                                        .await;
                                });
                            }
                            break;
                        }
                    } else {
                        stale_count = 0; // Reset counter if bytes are increasing
                        debug!(
                            "Session {} active: {} bytes (+{} since last check)",
                            heartbeat_session_id, current_bytes, current_bytes - last_bytes
                        );
                    }
                    last_bytes = current_bytes;
                }
            }
        }
    });

    let stream_with_completion =
        futures::stream::unfold((tracked_stream, false), move |(mut stream, completed)| {
            let state_ref = completion_state.clone();
            let session_ref = completion_session.clone();
            let session_id_ref = completion_session_id.clone();
            let bytes_ref = completion_bytes.clone();
            let ended_ref = completion_ended.clone();
            let cancel_token = cancellation_token.clone();

            async move {
                use futures::StreamExt;

                match stream.next().await {
                    Some(result) => Some((result, (stream, completed))),
                    None => {
                        // Stream completed (either successfully or client disconnected)
                        cancel_token.cancel(); // Cancel heartbeat task
                        
                        if !completed && !ended_ref.swap(true, std::sync::atomic::Ordering::Relaxed)
                        {
                            let final_bytes = bytes_ref.load(std::sync::atomic::Ordering::Relaxed);
                            debug!(
                                "Stream completed normally for session: {} ({} bytes)",
                                session_id_ref, final_bytes
                            );

                            // Handle completion asynchronously
                            let final_state = state_ref.clone();
                            let final_session = session_ref.clone();
                            let final_session_id = session_id_ref.clone();
                            tokio::spawn(async move {
                                // Finish metrics tracking
                                final_session
                                    .finish(&final_state.metrics_logger, final_bytes)
                                    .await;

                                // Complete the active session
                                if let Err(e) = final_state
                                    .metrics_logger
                                    .complete_active_session(&final_session_id)
                                    .await
                                {
                                    error!("Failed to complete active session: {}", e);
                                }

                                // End session tracking
                                final_state
                                    .session_tracker
                                    .end_session(&final_session_id)
                                    .await;
                            });
                        }
                        None
                    }
                }
            }
        });

    // Convert to axum Body
    let body = Body::from_stream(stream_with_completion);

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




/// Diagnose why a channel is not accessible in a proxy
async fn diagnose_channel_proxy_issue(
    database: &crate::database::Database,
    proxy_id: Uuid,
    channel_id: Uuid,
) -> String {
    // Check if channel exists at all and get additional info - rationalized to SeaORM
    use crate::entities::{prelude::*, channels, stream_sources};
    use sea_orm::{EntityTrait, QueryFilter, ColumnTrait, RelationTrait, QuerySelect, JoinType};
    
    let channel_info = Channels::find()
        .filter(channels::Column::Id.eq(channel_id))
        .join_rev(JoinType::LeftJoin, stream_sources::Relation::Channels.def())
        .select_only()
        .column(channels::Column::ChannelName)
        .column(stream_sources::Column::Name)
        .into_tuple::<(String, Option<String>)>()
        .one(&*database.connection())
        .await;

    let (channel_name, source_name) = match channel_info {
        Ok(Some((channel_name, source_name))) => {
            // Channel exists - continue with other checks
            (channel_name, source_name)
        }
        Ok(None) => {
            // Channel doesn't exist - this suggests stale M3U
            let proxy_info = StreamProxies::find()
                .filter(stream_proxies::Column::Id.eq(proxy_id))
                .select_only()
                .column(stream_proxies::Column::LastGeneratedAt)
                .into_tuple::<(Option<String>,)>()
                .one(&*database.connection())
                .await;
            
            let timing_info = match proxy_info {
                Ok(Some((Some(generated_at),))) => format!(" (proxy last generated: {generated_at})"),
                Ok(Some((None,))) => " (proxy never generated)".to_string(),
                _ => " (proxy status unknown)".to_string(),
            };
            
            return format!("channel does not exist - M3U may be stale{timing_info}");
        }
        Err(_) => {
            return "database error checking channel".to_string();
        }
    };

    // Check if proxy exists and is active - rationalized to SeaORM
    use crate::entities::{prelude::StreamProxies, stream_proxies};
    
    let proxy_status = StreamProxies::find()
        .filter(stream_proxies::Column::Id.eq(proxy_id))
        .one(&*database.connection())
        .await;

    match proxy_status {
        Ok(Some(proxy)) => {
            if !proxy.is_active {
                return "proxy is not active".to_string();
            }
        }
        Ok(None) => {
            return "proxy does not exist".to_string();
        }
        Err(_) => {
            return "database error checking proxy".to_string();
        }
    }

    // Check if channel's source is linked to the proxy - rationalized to SeaORM
    use crate::entities::{prelude::{Channels, ProxySources}, proxy_sources};
    use sea_orm::PaginatorTrait;
    
    // First get the channel to find its source_id
    let channel = Channels::find()
        .filter(channels::Column::Id.eq(channel_id))
        .one(&*database.connection())
        .await;
        
    let source_linked = match channel {
        Ok(Some(channel)) => {
            // Check if this source is linked to the proxy
            ProxySources::find()
                .filter(proxy_sources::Column::SourceId.eq(channel.source_id))
                .filter(proxy_sources::Column::ProxyId.eq(proxy_id))
                .count(&*database.connection())
                .await
                .unwrap_or(0) > 0
        }
        _ => false,
    };

    if !source_linked {
        let source_info = source_name.unwrap_or_else(|| "unknown source".to_string());
        return format!(
            "channel '{channel_name}' from source '{source_info}' is not linked to this proxy"
        );
    }

    format!(
        "unknown reason for channel '{channel_name}' (this should not happen)"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::StreamProxyMode;
    use uuid::Uuid;

    #[test]
    fn test_stream_proxy_response_url_generation() {
        use crate::utils::uuid_parser::uuid_to_base64;
        
        let proxy_id = Uuid::new_v4();
        let base_url = "https://example.com:8080";
        
        let proxy = StreamProxy {
            id: proxy_id,
            name: "Test Proxy".to_string(),
            description: Some("Test Description".to_string()),
            proxy_mode: StreamProxyMode::Proxy,
            upstream_timeout: Some(30),
            buffer_size: Some(1024),
            max_concurrent_streams: Some(100),
            starting_channel_number: 1,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            last_generated_at: None,
            is_active: true,
            auto_regenerate: false,
            cache_channel_logos: true,
            cache_program_logos: false,
            relay_profile_id: None,
        };

        let response = StreamProxyResponse::from_proxy_with_base_url(proxy, base_url);
        let expected_proxy_id_b64 = uuid_to_base64(&proxy_id);

        assert_eq!(response.m3u8_url, format!("https://example.com:8080/proxy/{expected_proxy_id_b64}/m3u8"));
        assert_eq!(response.xmltv_url, format!("https://example.com:8080/proxy/{expected_proxy_id_b64}/xmltv"));
        assert_eq!(response.name, "Test Proxy");
        assert_eq!(response.id, proxy_id);
    }

    #[test]
    fn test_stream_proxy_response_url_generation_with_trailing_slash() {
        use crate::utils::uuid_parser::uuid_to_base64;
        
        let proxy_id = Uuid::new_v4();
        let base_url = "https://example.com:8080/"; // Note the trailing slash
        
        let proxy = StreamProxy {
            id: proxy_id,
            name: "Test Proxy".to_string(),
            description: Some("Test Description".to_string()),
            proxy_mode: StreamProxyMode::Proxy,
            upstream_timeout: Some(30),
            buffer_size: Some(1024),
            max_concurrent_streams: Some(100),
            starting_channel_number: 1,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            last_generated_at: None,
            is_active: true,
            auto_regenerate: false,
            cache_channel_logos: true,
            cache_program_logos: false,
            relay_profile_id: None,
        };

        let response = StreamProxyResponse::from_proxy_with_base_url(proxy, base_url);
        let expected_proxy_id_b64 = uuid_to_base64(&proxy_id);

        // Should properly handle trailing slash
        assert_eq!(response.m3u8_url, format!("https://example.com:8080/proxy/{expected_proxy_id_b64}/m3u8"));
        assert_eq!(response.xmltv_url, format!("https://example.com:8080/proxy/{expected_proxy_id_b64}/xmltv"));
    }
}




*/
