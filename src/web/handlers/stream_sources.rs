//! Stream sources HTTP handlers
//!
//! This module contains HTTP handlers for stream source operations.
//! All handlers are thin wrappers around service layer calls,
//! focusing only on HTTP concerns like request/response mapping.

use axum::{
    extract::{Path, State},
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    models::{StreamSource, StreamSourceType},
    services::{
        stream_source::StreamSourceServiceQuery,
    },
    sources::SourceHandlerFactory,
};

use crate::web::{
    extractors::{ListParams, RequestContext, StreamSourceFilterParams},
    responses::ok,
    utils::{log_request, extract_uuid_param},
    AppState,
};

/// Request DTO for creating a stream source
#[derive(Debug, Clone, Deserialize)]
pub struct CreateStreamSourceRequest {
    pub name: String,
    pub source_type: String, // Will be converted to StreamSourceType
    pub url: String,
    pub max_concurrent_streams: i32,
    pub update_cron: String,
    pub username: Option<String>,
    pub password: Option<String>,
    pub field_map: Option<String>,
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
        })
    }
}

/// Request DTO for updating a stream source
#[derive(Debug, Clone, Deserialize)]
pub struct UpdateStreamSourceRequest {
    pub name: String,
    pub source_type: String,
    pub url: String,
    pub max_concurrent_streams: i32,
    pub update_cron: String,
    pub username: Option<String>,
    pub password: Option<String>,
    pub field_map: Option<String>,
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
            is_active: true, // Default to active for updates
        })
    }
}

/// Response DTO for stream source
#[derive(Debug, Clone, Serialize)]
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
        }
    }
}

/// List all stream sources
pub async fn list_stream_sources(
    State(_state): State<AppState>,
    context: RequestContext,
    list_params: ListParams,
    filter_params: StreamSourceFilterParams,
) -> impl IntoResponse {
    log_request(&axum::http::Method::GET, &"/api/sources/stream".parse().unwrap(), &context);

    // TODO: Get service instance from state
    // For now, this is a placeholder implementation
    
    // Build service query from request parameters
    let mut service_query = StreamSourceServiceQuery::new();
    
    if let Some(search) = list_params.search.search {
        service_query = service_query.search(search);
    }
    
    if let Some(source_type_str) = filter_params.source_type {
        if let Ok(source_type) = crate::web::utils::parse_source_type(&source_type_str) {
            service_query = service_query.source_type(source_type);
        }
    }
    
    if let Some(enabled) = filter_params.enabled {
        service_query = service_query.enabled(enabled);
    }
    
    if let Some(sort_by) = list_params.search.sort_by {
        service_query = service_query.sort_by(sort_by, list_params.search.sort_ascending);
    }
    
    service_query = service_query.paginate(list_params.pagination.page, list_params.pagination.limit);

    // TODO: Replace with actual service call
    // let result = stream_source_service.list(service_query).await;
    // let response = result.map(|service_response| {
    //     map_service_list_response(
    //         service_response,
    //         list_params.pagination.page,
    //         list_params.pagination.limit,
    //         StreamSourceResponse::from,
    //     )
    // });
    // handle_result(response)

    // Placeholder response
    let empty_response = crate::web::responses::PaginatedResponse::new(
        Vec::<StreamSourceResponse>::new(),
        0,
        list_params.pagination.page,
        list_params.pagination.limit,
    );
    ok(empty_response)
}

/// Get a specific stream source by ID
pub async fn get_stream_source(
    State(_state): State<AppState>,
    Path(id): Path<String>,
    context: RequestContext,
) -> impl IntoResponse {
    log_request(&axum::http::Method::GET, &format!("/api/sources/stream/{}", id).parse().unwrap(), &context);

    let _uuid = match extract_uuid_param(&id) {
        Ok(uuid) => uuid,
        Err(error) => return crate::web::responses::bad_request(&error).into_response(),
    };

    // TODO: Get service instance from state and make service call
    // let result = stream_source_service.get_by_id(uuid).await;
    // let response = result.and_then(|opt| {
    //     opt.map(StreamSourceResponse::from)
    //        .ok_or_else(|| AppError::not_found("stream_source", uuid.to_string()))
    // });
    // handle_result(response)

    // Placeholder response
    crate::web::responses::not_found("stream_source", &id).into_response()
}

/// Create a new stream source
pub async fn create_stream_source(
    State(_state): State<AppState>,
    context: RequestContext,
    Json(request): Json<CreateStreamSourceRequest>,
) -> impl IntoResponse {
    log_request(&axum::http::Method::POST, &"/api/sources/stream".parse().unwrap(), &context);

    let _service_request = match request.into_service_request() {
        Ok(req) => req,
        Err(error) => return crate::web::responses::bad_request(&error).into_response(),
    };

    // TODO: Get service instance from state and make service call
    // let result = stream_source_service.create(service_request).await;
    // let response = result.map(StreamSourceResponse::from);
    // handle_result(response)

    // Placeholder response
    crate::web::responses::bad_request("Service layer not yet integrated").into_response()
}

/// Update an existing stream source
pub async fn update_stream_source(
    State(_state): State<AppState>,
    Path(id): Path<String>,
    context: RequestContext,
    Json(request): Json<UpdateStreamSourceRequest>,
) -> impl IntoResponse {
    log_request(&axum::http::Method::PUT, &format!("/api/sources/stream/{}", id).parse().unwrap(), &context);

    let _uuid = match extract_uuid_param(&id) {
        Ok(uuid) => uuid,
        Err(error) => return crate::web::responses::bad_request(&error).into_response(),
    };

    let _service_request = match request.into_service_request() {
        Ok(req) => req,
        Err(error) => return crate::web::responses::bad_request(&error).into_response(),
    };

    // TODO: Get service instance from state and make service call
    // let result = stream_source_service.update(uuid, service_request).await;
    // let response = result.map(StreamSourceResponse::from);
    // handle_result(response)

    // Placeholder response
    crate::web::responses::bad_request("Service layer not yet integrated").into_response()
}

/// Delete a stream source
pub async fn delete_stream_source(
    State(_state): State<AppState>,
    Path(id): Path<String>,
    context: RequestContext,
) -> impl IntoResponse {
    log_request(&axum::http::Method::DELETE, &format!("/api/sources/stream/{}", id).parse().unwrap(), &context);

    let _uuid = match extract_uuid_param(&id) {
        Ok(uuid) => uuid,
        Err(error) => return crate::web::responses::bad_request(&error).into_response(),
    };

    // TODO: Get service instance from state and make service call
    // let result = stream_source_service.delete(uuid).await;
    // handle_result(result.map(|_| ()))

    // Placeholder response
    crate::web::responses::bad_request("Service layer not yet integrated").into_response()
}

/// Refresh a stream source (trigger ingestion)
pub async fn refresh_stream_source(
    State(_state): State<AppState>,
    Path(id): Path<String>,
    context: RequestContext,
) -> impl IntoResponse {
    log_request(&axum::http::Method::POST, &format!("/api/sources/stream/{}/refresh", id).parse().unwrap(), &context);

    let _uuid = match extract_uuid_param(&id) {
        Ok(uuid) => uuid,
        Err(error) => return crate::web::responses::bad_request(&error).into_response(),
    };

    // TODO: Implement source refresh using source handlers
    // 1. Get stream source from service
    // 2. Create appropriate source handler using factory
    // 3. Trigger ingestion with progress reporting
    // 4. Return operation status

    // Placeholder response
    crate::web::responses::bad_request("Refresh not yet implemented").into_response()
}

/// Validate a stream source configuration
pub async fn validate_stream_source(
    State(_state): State<AppState>,
    context: RequestContext,
    Json(request): Json<CreateStreamSourceRequest>,
) -> impl IntoResponse {
    log_request(&axum::http::Method::POST, &"/api/sources/stream/validate".parse().unwrap(), &context);

    let _service_request = match request.into_service_request() {
        Ok(req) => req,
        Err(error) => return crate::web::responses::bad_request(&error).into_response(),
    };

    // TODO: Implement validation using source handlers
    // 1. Create appropriate source handler based on source type
    // 2. Use handler to validate source configuration
    // 3. Return validation results with details

    // For demonstration, create a source handler and validate
    match SourceHandlerFactory::create_handler(&_service_request.source_type) {
        Ok(handler) => {
            // Convert service request to StreamSource for validation
            let temp_source = crate::models::StreamSource {
                id: Uuid::new_v4(),
                name: _service_request.name,
                source_type: _service_request.source_type,
                url: _service_request.url,
                max_concurrent_streams: _service_request.max_concurrent_streams,
                update_cron: _service_request.update_cron,
                username: _service_request.username,
                password: _service_request.password,
                field_map: _service_request.field_map,
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
                last_ingested_at: None,
                is_active: true,
            };

            // TODO: Make actual validation call
            // match handler.validate_source(&temp_source).await {
            //     Ok(validation_result) => ok(validation_result),
            //     Err(error) => handle_result(Err(error)),
            // }

            // Placeholder response
            ok(serde_json::json!({
                "valid": true,
                "message": "Source validation not yet fully implemented"
            })).into_response()
        }
        Err(error) => crate::web::responses::handle_error(error).into_response(),
    }
}

/// Get stream source capabilities
pub async fn get_stream_source_capabilities(
    State(_state): State<AppState>,
    Path(source_type): Path<String>,
    context: RequestContext,
) -> impl IntoResponse {
    log_request(&axum::http::Method::GET, &format!("/api/sources/stream/capabilities/{}", source_type).parse().unwrap(), &context);

    let parsed_source_type = match crate::web::utils::parse_source_type(&source_type) {
        Ok(st) => st,
        Err(error) => return crate::web::responses::bad_request(&error).into_response(),
    };

    match SourceHandlerFactory::get_handler_capabilities(&parsed_source_type) {
        Ok(capabilities) => ok(capabilities).into_response(),
        Err(error) => crate::web::responses::handle_error(error).into_response(),
    }
}