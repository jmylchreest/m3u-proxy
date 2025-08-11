//! Stream sources HTTP handlers
//!
//! This module contains HTTP handlers for stream source operations.
//! All handlers are thin wrappers around service layer calls,
//! focusing only on HTTP concerns like request/response mapping.

use axum::{
    Json,
    extract::{Path, State},
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    models::{StreamSource, StreamSourceType},
    sources::SourceHandlerFactory,
    repositories::traits::Repository,
};

use crate::web::{
    AppState,
    extractors::{ListParams, RequestContext, StreamSourceFilterParams},
    responses::ok,
    utils::{extract_uuid_param, log_request},
};

/// Default value for ignore_channel_numbers field (defaults to true for new sources)
fn default_ignore_channel_numbers() -> bool {
    true
}

/// Default value for update_linked field (defaults to true)
fn default_update_linked() -> bool {
    true
}

/// Default value for is_active field (defaults to true)
fn default_is_active() -> bool {
    true
}

/// Request DTO for creating a stream source
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct CreateStreamSourceRequest {
    pub name: String,
    pub source_type: String, // Will be converted to StreamSourceType
    pub url: String,
    pub max_concurrent_streams: i32,
    pub update_cron: String,
    pub username: Option<String>,
    pub password: Option<String>,
    pub field_map: Option<String>,
    /// For Xtream sources: ignore channel numbers from API and allow renumbering
    #[serde(default = "default_ignore_channel_numbers")]
    pub ignore_channel_numbers: bool,
}

impl CreateStreamSourceRequest {
    /// Convert to service layer request
    pub fn into_service_request(self) -> Result<crate::models::StreamSourceCreateRequest, String> {
        let source_type = match self.source_type.to_lowercase().as_str() {
            "m3u" => StreamSourceType::M3u,
            "xtream" => StreamSourceType::Xtream,
            _ => return Err(format!("Invalid source type: {}", self.source_type)),
        };

        Ok(crate::models::StreamSourceCreateRequest {
            name: self.name,
            source_type,
            url: self.url,
            max_concurrent_streams: self.max_concurrent_streams,
            update_cron: self.update_cron,
            username: self.username,
            password: self.password,
            field_map: self.field_map,
            ignore_channel_numbers: self.ignore_channel_numbers,
        })
    }
}

/// Request DTO for updating a stream source
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UpdateStreamSourceRequest {
    pub name: String,
    pub source_type: String,
    pub url: String,
    pub max_concurrent_streams: i32,
    pub update_cron: String,
    pub username: Option<String>,
    pub password: Option<String>,
    pub field_map: Option<String>,
    /// For Xtream sources: ignore channel numbers from API and allow renumbering
    #[serde(default = "default_ignore_channel_numbers")]
    pub ignore_channel_numbers: bool,
    /// Whether to update linked sources with the same URL (defaults to true)
    #[serde(default = "default_update_linked")]
    pub update_linked: bool,
    /// Whether the source is active (defaults to true)
    #[serde(default = "default_is_active")]
    pub is_active: bool,
}

impl UpdateStreamSourceRequest {
    /// Convert to service layer request
    pub fn into_service_request(self) -> Result<crate::models::StreamSourceUpdateRequest, String> {
        let source_type = match self.source_type.to_lowercase().as_str() {
            "m3u" => StreamSourceType::M3u,
            "xtream" => StreamSourceType::Xtream,
            _ => return Err(format!("Invalid source type: {}", self.source_type)),
        };

        Ok(crate::models::StreamSourceUpdateRequest {
            name: self.name,
            source_type,
            url: self.url,
            max_concurrent_streams: self.max_concurrent_streams,
            update_cron: self.update_cron,
            username: self.username,
            password: self.password,
            field_map: self.field_map,
            ignore_channel_numbers: self.ignore_channel_numbers,
            is_active: self.is_active,
            update_linked: self.update_linked,
        })
    }
}

/// Response DTO for stream source
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct StreamSourceResponse {
    pub id: Uuid,
    pub name: String,
    pub source_type: String,
    pub url: String,
    pub max_concurrent_streams: i32,
    pub update_cron: String,
    pub username: Option<String>,
    // Note: password is intentionally omitted for security
    pub field_map: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub last_ingested_at: Option<chrono::DateTime<chrono::Utc>>,
    pub is_active: bool,
    pub channel_count: u64,
    pub next_scheduled_update: Option<chrono::DateTime<chrono::Utc>>,
}

impl From<StreamSource> for StreamSourceResponse {
    fn from(source: StreamSource) -> Self {
        Self {
            id: source.id,
            name: source.name,
            source_type: match source.source_type {
                StreamSourceType::M3u => "m3u".to_string(),
                StreamSourceType::Xtream => "xtream".to_string(),
            },
            url: source.url,
            max_concurrent_streams: source.max_concurrent_streams,
            update_cron: source.update_cron,
            username: source.username,
            field_map: source.field_map,
            created_at: source.created_at,
            updated_at: source.updated_at,
            last_ingested_at: source.last_ingested_at,
            is_active: source.is_active,
            channel_count: 0, // Default value, should be set when creating from stats
            next_scheduled_update: None, // Default value, should be set when creating from stats
        }
    }
}

/// List all stream sources with filtering and pagination
#[utoipa::path(
    get,
    path = "/sources/stream",
    tag = "stream-sources",
    params(
        ("page" = Option<u32>, Query, description = "Page number (1-based)", example = 1),
        ("limit" = Option<u32>, Query, description = "Items per page (1-100)", example = 20),
        ("search" = Option<String>, Query, description = "Search term for name or URL"),
        ("source_type" = Option<String>, Query, description = "Filter by source type: m3u, xtream"),
    ),
    responses(
        (status = 200, description = "List of stream sources retrieved successfully"),
        (status = 400, description = "Invalid query parameters"),
        (status = 500, description = "Internal server error"),
    )
)]
pub async fn list_stream_sources(
    State(state): State<AppState>,
    context: RequestContext,
    list_params: ListParams,
    filter_params: StreamSourceFilterParams,
) -> impl IntoResponse {
    log_request(
        &axum::http::Method::GET,
        &"/api/v1/sources/stream".parse().unwrap(),
        &context,
    );

    match state.stream_source_service.list_with_stats().await {
        Ok(sources_with_stats) => {
            // Convert to response format
            let mut response_items = Vec::new();
            for source_with_stats in sources_with_stats {
                let mut response = StreamSourceResponse::from(source_with_stats.source);
                response.channel_count = source_with_stats.channel_count;
                response.next_scheduled_update = source_with_stats.next_scheduled_update;
                response_items.push(response);
            }

            // Apply filtering if needed
            if let Some(search) = list_params.search.search {
                let search_lower = search.to_lowercase();
                response_items.retain(|item| {
                    item.name.to_lowercase().contains(&search_lower)
                        || item.url.to_lowercase().contains(&search_lower)
                });
            }

            if let Some(source_type_str) = filter_params.source_type {
                response_items.retain(|item| {
                    item.source_type.to_lowercase() == source_type_str.to_lowercase()
                });
            }

            let total = response_items.len();

            // Apply pagination
            let page = list_params.pagination.page;
            let limit = list_params.pagination.limit as usize;
            let offset = ((page - 1) * limit as u32) as usize;

            let paginated_items = if offset < response_items.len() {
                response_items
                    .into_iter()
                    .skip(offset)
                    .take(limit)
                    .collect()
            } else {
                Vec::new()
            };

            let paginated_response = crate::web::responses::PaginatedResponse::new(
                paginated_items,
                total as u64,
                page,
                limit as u32,
            );
            ok(paginated_response).into_response()
        }
        Err(e) => {
            tracing::error!("Failed to list stream sources: {}", e);
            crate::web::responses::internal_error(&format!("Failed to list stream sources: {e}"))
                .into_response()
        }
    }
}

/// Get a specific stream source by ID
#[utoipa::path(
    get,
    path = "/sources/stream/{id}",
    tag = "stream-sources",
    params(
        ("id" = String, Path, description = "Stream source ID (UUID)", example = "550e8400-e29b-41d4-a716-446655440000"),
    ),
    responses(
        (status = 200, description = "Stream source details retrieved successfully"),
        (status = 400, description = "Invalid UUID format"),
        (status = 404, description = "Stream source not found"),
        (status = 500, description = "Internal server error"),
    )
)]
pub async fn get_stream_source(
    State(state): State<AppState>,
    Path(id): Path<String>,
    context: RequestContext,
) -> impl IntoResponse {
    log_request(
        &axum::http::Method::GET,
        &format!("/api/v1/sources/stream/{id}").parse().unwrap(),
        &context,
    );

    let uuid = match extract_uuid_param(&id) {
        Ok(uuid) => uuid,
        Err(error) => return crate::web::responses::bad_request(&error).into_response(),
    };

    match state.stream_source_service.get_with_details(uuid).await {
        Ok(source_with_details) => {
            let response = StreamSourceResponse::from(source_with_details.source);
            ok(response).into_response()
        }
        Err(e) => {
            tracing::error!("Failed to get stream source {}: {}", uuid, e);
            crate::web::responses::not_found("stream_source", &id).into_response()
        }
    }
}

/// Create a new stream source
#[utoipa::path(
    post,
    path = "/sources/stream",
    tag = "stream-sources",
    request_body = CreateStreamSourceRequest,
    responses(
        (status = 201, description = "Stream source created successfully"),
        (status = 400, description = "Invalid request data or validation failed"),
        (status = 409, description = "Stream source with this name already exists"),
        (status = 500, description = "Internal server error"),
    )
)]
pub async fn create_stream_source(
    State(state): State<AppState>,
    context: RequestContext,
    Json(request): Json<CreateStreamSourceRequest>,
) -> impl IntoResponse {
    log_request(
        &axum::http::Method::POST,
        &"/api/v1/sources/stream".parse().unwrap(),
        &context,
    );

    let service_request = match request.into_service_request() {
        Ok(req) => req,
        Err(error) => return crate::web::responses::bad_request(&error).into_response(),
    };

    // Use the service layer to create the stream source with auto-EPG linking
    match state
        .stream_source_service
        .create_with_auto_epg(service_request)
        .await
    {
        Ok(source) => {
            let response = StreamSourceResponse::from(source);
            crate::web::responses::created(response).into_response()
        }
        Err(e) => {
            tracing::error!("Failed to create stream source: {}", e);
            crate::web::responses::internal_error(&format!("Failed to create stream source: {e}"))
                .into_response()
        }
    }
}

/// Update an existing stream source
#[utoipa::path(
    put,
    path = "/sources/stream/{id}",
    tag = "stream-sources",
    params(
        ("id" = String, Path, description = "Stream source ID (UUID)"),
    ),
    request_body = UpdateStreamSourceRequest,
    responses(
        (status = 200, description = "Stream source updated successfully"),
        (status = 400, description = "Invalid request data or UUID format"),
        (status = 404, description = "Stream source not found"),
        (status = 500, description = "Internal server error"),
    )
)]
pub async fn update_stream_source(
    State(state): State<AppState>,
    Path(id): Path<String>,
    context: RequestContext,
    Json(request): Json<UpdateStreamSourceRequest>,
) -> impl IntoResponse {
    log_request(
        &axum::http::Method::PUT,
        &format!("/api/v1/sources/stream/{id}").parse().unwrap(),
        &context,
    );

    let uuid = match extract_uuid_param(&id) {
        Ok(uuid) => uuid,
        Err(error) => return crate::web::responses::bad_request(&error).into_response(),
    };

    let service_request = match request.into_service_request() {
        Ok(req) => req,
        Err(error) => return crate::web::responses::bad_request(&error).into_response(),
    };

    match state
        .stream_source_service
        .update_with_validation(uuid, service_request)
        .await
    {
        Ok(_source) => {
            // Get the updated source with details including channel count
            match state.stream_source_service.get_with_details(uuid).await {
                Ok(source_with_details) => {
                    let mut response = StreamSourceResponse::from(source_with_details.source);
                    response.channel_count = source_with_details.channel_count;
                    response.next_scheduled_update = source_with_details.next_scheduled_update;
                    ok(response).into_response()
                }
                Err(e) => {
                    tracing::error!("Failed to get updated stream source details {}: {}", uuid, e);
                    crate::web::responses::internal_error(&format!("Failed to get updated source details: {e}"))
                        .into_response()
                }
            }
        }
        Err(e) => {
            tracing::error!("Failed to update stream source {}: {}", uuid, e);
            crate::web::responses::internal_error(&format!("Failed to update stream source: {e}"))
                .into_response()
        }
    }
}

/// Delete a stream source
#[utoipa::path(
    delete,
    path = "/sources/stream/{id}",
    tag = "stream-sources",
    params(
        ("id" = String, Path, description = "Stream source ID (UUID)", example = "550e8400-e29b-41d4-a716-446655440000"),
    ),
    responses(
        (status = 200, description = "Stream source deleted successfully"),
        (status = 400, description = "Invalid UUID format"),
        (status = 404, description = "Stream source not found"),
        (status = 500, description = "Internal server error"),
    )
)]
pub async fn delete_stream_source(
    State(state): State<AppState>,
    Path(id): Path<String>,
    context: RequestContext,
) -> impl IntoResponse {
    log_request(
        &axum::http::Method::DELETE,
        &format!("/api/v1/sources/stream/{id}").parse().unwrap(),
        &context,
    );

    let uuid = match extract_uuid_param(&id) {
        Ok(uuid) => uuid,
        Err(error) => return crate::web::responses::bad_request(&error).into_response(),
    };

    match state.stream_source_service.delete_with_cleanup(uuid).await {
        Ok(()) => crate::web::responses::ok(
            serde_json::json!({"message": "Stream source deleted successfully"}),
        )
        .into_response(),
        Err(e) => {
            tracing::error!("Failed to delete stream source {}: {}", uuid, e);
            crate::web::responses::internal_error(&format!("Failed to delete stream source: {e}"))
                .into_response()
        }
    }
}

// TODO: Add refresh functionality when ingestion service is available

/// Validate a stream source configuration
#[utoipa::path(
    post,
    path = "/sources/stream/validate",
    tag = "stream-sources",
    request_body = CreateStreamSourceRequest,
    responses(
        (status = 200, description = "Stream source validation result"),
        (status = 400, description = "Invalid request data"),
        (status = 500, description = "Internal server error"),
    )
)]
pub async fn validate_stream_source(
    State(state): State<AppState>,
    context: RequestContext,
    Json(request): Json<CreateStreamSourceRequest>,
) -> impl IntoResponse {
    log_request(
        &axum::http::Method::POST,
        &"/api/v1/sources/stream/validate".parse().unwrap(),
        &context,
    );

    let service_request = match request.into_service_request() {
        Ok(req) => req,
        Err(error) => return crate::web::responses::bad_request(&error).into_response(),
    };

    // Use the service layer to test the connection
    match state
        .stream_source_service
        .test_connection(&service_request)
        .await
    {
        Ok(test_result) => ok(serde_json::json!({
            "valid": test_result.success,
            "message": test_result.message,
            "has_streams": test_result.has_streams,
            "has_epg": test_result.has_epg
        }))
        .into_response(),
        Err(e) => {
            tracing::error!("Failed to validate stream source: {}", e);
            ok(serde_json::json!({
                "valid": false,
                "message": format!("Validation failed: {}", e),
                "has_streams": false,
                "has_epg": false
            }))
            .into_response()
        }
    }
}

/// Get stream source capabilities
#[utoipa::path(
    get,
    path = "/sources/capabilities/{source_type}",
    tag = "capabilities",
    params(
        ("source_type" = String, Path, description = "Stream source type (m3u, xtream)", example = "m3u"),
    ),
    responses(
        (status = 200, description = "Stream source capabilities retrieved successfully"),
        (status = 400, description = "Invalid source type"),
        (status = 500, description = "Internal server error"),
    )
)]
pub async fn get_stream_source_capabilities(
    State(_state): State<AppState>,
    Path(source_type): Path<String>,
    context: RequestContext,
) -> impl IntoResponse {
    log_request(
        &axum::http::Method::GET,
        &format!("/api/v1/sources/capabilities/{source_type}")
            .parse()
            .unwrap(),
        &context,
    );

    let parsed_source_type = match crate::web::utils::parse_source_type(&source_type) {
        Ok(st) => st,
        Err(error) => return crate::web::responses::bad_request(&error).into_response(),
    };

    match SourceHandlerFactory::get_handler_capabilities(&parsed_source_type) {
        Ok(capabilities) => ok(capabilities).into_response(),
        Err(error) => crate::web::responses::handle_error(error).into_response(),
    }
}

/// Refresh stream source
#[utoipa::path(
    post,
    path = "/sources/stream/{id}/refresh",
    tag = "sources",
    summary = "Refresh stream source",
    description = "Manually trigger a refresh of a stream source to reload channels",
    params(
        ("id" = String, Path, description = "Stream source ID (UUID)"),
    ),
    responses(
        (status = 200, description = "Refresh initiated successfully"),
        (status = 404, description = "Stream source not found"),
        (status = 409, description = "Operation already in progress"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn refresh_stream_source(
    State(state): State<AppState>,
    Path(id): Path<String>,
    context: RequestContext,
) -> impl IntoResponse {
    log_request(
        &axum::http::Method::POST,
        &format!("/api/v1/sources/stream/{id}/refresh").parse().unwrap(),
        &context,
    );

    let uuid = match extract_uuid_param(&id) {
        Ok(uuid) => uuid,
        Err(error) => return crate::web::responses::bad_request(&error).into_response(),
    };

    let stream_source_repo = crate::repositories::StreamSourceRepository::new(state.database.pool().clone());
    match stream_source_repo.find_by_id(uuid).await {
        Ok(Some(source)) => {
            // Create progress manager for manual refresh operation
            let progress_manager = match state.progress_service.create_staged_progress_manager(
                source.id, // Use source ID as owner
                "stream_source".to_string(),
                crate::services::progress_service::OperationType::StreamIngestion,
                format!("Manual Refresh: {}", source.name),
            ).await {
                Ok(manager) => {
                    // Add ingestion stage
                    let manager_with_stage = manager.add_stage("stream_ingestion", "Stream Ingestion").await;
                    Some((manager_with_stage, manager.get_stage_updater("stream_ingestion").await))
                },
                Err(e) => {
                    tracing::warn!("Failed to create progress manager for stream source manual refresh {}: {} - continuing without progress", source.name, e);
                    None
                }
            };
            
            let progress_updater = progress_manager.as_ref().and_then(|(_, updater)| updater.as_ref());
            
            // Call stream source service refresh with progress tracking
            match state.stream_source_service.refresh_with_progress_updater(&source, progress_updater).await {
                Ok(_channel_count) => {
                    // Complete progress operation if it was created
                    if let Some((manager, _)) = progress_manager {
                        manager.complete().await;
                    }
                    
                    // Emit scheduler event for manual refresh trigger
                    state.database.emit_scheduler_event(crate::ingestor::scheduler::SchedulerEvent::ManualRefreshTriggered(uuid));
                    
                    ok(serde_json::json!({
                        "message": "Stream source refresh started",
                        "source_id": uuid,
                        "source_name": source.name
                    })).into_response()
                }
                Err(e) => {
                    // Fail progress operation if it was created
                    if let Some((manager, _)) = progress_manager {
                        manager.fail(&format!("Stream source refresh failed: {e}")).await;
                    }
                    
                    tracing::error!("Failed to refresh stream source {}: {}", source.id, e);
                    
                    // Check if it's an operation in progress error
                    if let Some(crate::errors::AppError::OperationInProgress { .. }) = e.downcast_ref::<crate::errors::AppError>() {
                        return crate::web::responses::conflict("Operation already in progress").into_response();
                    }
                    
                    crate::web::responses::handle_error(crate::errors::AppError::Internal { 
                        message: "Stream source refresh failed".to_string() 
                    }).into_response()
                }
            }
        }
        Ok(None) => {
            crate::web::responses::not_found("Stream source", &uuid.to_string()).into_response()
        },
        Err(e) => {
            tracing::error!("Failed to get stream source {}: {}", uuid, e);
            crate::web::responses::handle_error(crate::errors::AppError::Internal { 
                message: "Failed to retrieve stream source".to_string() 
            }).into_response()
        }
    }
}
