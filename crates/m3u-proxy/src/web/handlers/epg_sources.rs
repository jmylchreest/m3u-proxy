//! EPG sources HTTP handlers
//!
//! This module contains HTTP handlers for EPG source operations.
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

use crate::models::{EpgSource, EpgSourceType};

use crate::web::{
    AppState,
    extractors::{EpgSourceFilterParams, ListParams, RequestContext},
    responses::{ok, ApiResponse, PaginatedResponse},
    utils::{extract_uuid_param, log_request},
};

/// Default value for update_linked field (defaults to true)
fn default_update_linked() -> bool {
    true
}

/// Request DTO for creating an EPG source
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct CreateEpgSourceRequest {
    pub name: String,
    pub source_type: String, // Will be converted to EpgSourceType
    pub url: String,
    pub update_cron: String,
    pub username: Option<String>,
    pub password: Option<String>,
    pub original_timezone: Option<String>,
    pub time_offset: Option<String>,
}

impl CreateEpgSourceRequest {
    /// Convert to service layer request
    pub fn into_service_request(self) -> Result<crate::models::EpgSourceCreateRequest, String> {
        let source_type = match self.source_type.to_lowercase().as_str() {
            "xmltv" => EpgSourceType::Xmltv,
            "xtream" => EpgSourceType::Xtream,
            _ => return Err(format!("Invalid source type: {}", self.source_type)),
        };

        Ok(crate::models::EpgSourceCreateRequest {
            name: self.name,
            source_type,
            url: self.url,
            update_cron: self.update_cron,
            username: self.username,
            password: self.password,
            timezone: self.original_timezone,
            time_offset: self.time_offset,
        })
    }
}

/// Request DTO for updating an EPG source
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UpdateEpgSourceRequest {
    pub name: String,
    pub source_type: String,
    pub url: String,
    pub update_cron: String,
    pub username: Option<String>,
    pub password: Option<String>,
    pub original_timezone: Option<String>,
    pub time_offset: Option<String>,
    /// Whether to update linked sources with the same URL (defaults to true)
    #[serde(default = "default_update_linked")]
    pub update_linked: bool,
}

impl UpdateEpgSourceRequest {
    /// Convert to service layer request
    pub fn into_service_request(self) -> Result<crate::models::EpgSourceUpdateRequest, String> {
        let source_type = match self.source_type.to_lowercase().as_str() {
            "xmltv" => EpgSourceType::Xmltv,
            "xtream" => EpgSourceType::Xtream,
            _ => return Err(format!("Invalid source type: {}", self.source_type)),
        };

        Ok(crate::models::EpgSourceUpdateRequest {
            name: self.name,
            source_type,
            url: self.url,
            update_cron: self.update_cron,
            username: self.username,
            password: self.password,
            timezone: self.original_timezone,
            time_offset: self.time_offset,
            is_active: true, // Default to active for updates
            update_linked: self.update_linked,
        })
    }
}

/// Response DTO for EPG source
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct EpgSourceResponse {
    pub id: Uuid,
    pub name: String,
    pub source_type: String,
    pub url: String,
    pub update_cron: String,
    pub username: Option<String>,
    // Note: password is intentionally omitted for security
    pub original_timezone: Option<String>,
    pub time_offset: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub last_ingested_at: Option<chrono::DateTime<chrono::Utc>>,
    pub is_active: bool,
    pub program_count: u64,
    pub next_scheduled_update: Option<chrono::DateTime<chrono::Utc>>,
}

impl From<EpgSource> for EpgSourceResponse {
    fn from(source: EpgSource) -> Self {
        Self {
            id: source.id,
            name: source.name,
            source_type: match source.source_type {
                EpgSourceType::Xmltv => "xmltv".to_string(),
                EpgSourceType::Xtream => "xtream".to_string(),
            },
            url: source.url,
            update_cron: source.update_cron,
            username: source.username,
            original_timezone: source.original_timezone,
            time_offset: Some(source.time_offset),
            created_at: source.created_at,
            updated_at: source.updated_at,
            last_ingested_at: source.last_ingested_at,
            is_active: source.is_active,
            program_count: 0, // Default value, should be set when creating from stats
            next_scheduled_update: None, // Default value, should be set when creating from stats
        }
    }
}


/// List all EPG sources with utoipa automatic discovery
#[utoipa::path(
    get,
    path = "/sources/epg",
    tag = "epg-sources",
    summary = "List EPG sources",
    description = "Retrieve a paginated list of EPG sources with optional filtering",
    params(
        ("page" = Option<u32>, Query, description = "Page number (1-based)"),
        ("limit" = Option<u32>, Query, description = "Number of items per page"),
        ("search" = Option<String>, Query, description = "Search term"),
        ("source_type" = Option<String>, Query, description = "Filter by source type"),
        ("enabled" = Option<bool>, Query, description = "Filter by enabled status"),
        ("healthy" = Option<bool>, Query, description = "Filter by health status"),
    ),
    responses(
        (status = 200, description = "List of EPG sources", body = PaginatedResponse<EpgSourceResponse>),
        (status = 400, description = "Bad request"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn list_epg_sources(
    State(state): State<AppState>,
    context: RequestContext,
    list_params: ListParams,
    filter_params: EpgSourceFilterParams,
) -> impl IntoResponse {
    log_request(
        &axum::http::Method::GET,
        &"/api/v1/sources/epg".parse().unwrap(),
        &context,
    );

    match state.epg_source_service.list_with_stats().await {
        Ok(sources_with_stats) => {
            // Convert to response format
            let mut response_items = Vec::new();
            for source_with_stats in sources_with_stats {
                let mut response = EpgSourceResponse::from(source_with_stats.source);
                response.program_count = source_with_stats.program_count as u64;
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
            tracing::error!("Failed to list EPG sources: {}", e);
            crate::web::responses::internal_error(&format!("Failed to list EPG sources: {e}"))
                .into_response()
        }
    }
}

/// Get a specific EPG source by ID
#[utoipa::path(
    get,
    path = "/sources/epg/{id}",
    tag = "epg-sources",
    summary = "Get EPG source",
    description = "Retrieve a specific EPG source by ID",
    params(
        ("id" = String, Path, description = "EPG source ID (UUID)", example = "550e8400-e29b-41d4-a716-446655440000"),
    ),
    responses(
        (status = 200, description = "EPG source details", body = ApiResponse<EpgSourceResponse>),
        (status = 400, description = "Invalid UUID format"),
        (status = 404, description = "EPG source not found"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_epg_source(
    State(state): State<AppState>,
    Path(id): Path<String>,
    context: RequestContext,
) -> impl IntoResponse {
    log_request(
        &axum::http::Method::GET,
        &format!("/api/v1/sources/epg/{id}").parse().unwrap(),
        &context,
    );

    let uuid = match extract_uuid_param(&id) {
        Ok(uuid) => uuid,
        Err(error) => return crate::web::responses::bad_request(&error).into_response(),
    };

    match state.epg_source_service.get_with_details(uuid).await {
        Ok(source_with_details) => {
            let response = EpgSourceResponse::from(source_with_details.source);
            ok(response).into_response()
        }
        Err(e) => {
            tracing::error!("Failed to get EPG source {}: {}", uuid, e);
            crate::web::responses::not_found("epg_source", &id).into_response()
        }
    }
}

/// Create a new EPG source with utoipa automatic discovery
#[utoipa::path(
    post,
    path = "/sources/epg",
    tag = "epg-sources",
    summary = "Create EPG source",
    description = "Create a new EPG source configuration",
    request_body = CreateEpgSourceRequest,
    responses(
        (status = 201, description = "EPG source created successfully", body = ApiResponse<EpgSourceResponse>),
        (status = 400, description = "Invalid request"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn create_epg_source(
    State(state): State<AppState>,
    context: RequestContext,
    Json(request): Json<CreateEpgSourceRequest>,
) -> impl IntoResponse {
    log_request(
        &axum::http::Method::POST,
        &"/api/v1/sources/epg".parse().unwrap(),
        &context,
    );

    let service_request = match request.into_service_request() {
        Ok(req) => req,
        Err(error) => return crate::web::responses::bad_request(&error).into_response(),
    };

    // Use the service layer to create the EPG source with auto-stream linking
    match state
        .epg_source_service
        .create_with_auto_stream(service_request)
        .await
    {
        Ok(source) => {
            let response = EpgSourceResponse::from(source);
            crate::web::responses::created(response).into_response()
        }
        Err(e) => {
            tracing::error!("Failed to create EPG source: {}", e);
            crate::web::responses::internal_error(&format!("Failed to create EPG source: {e}"))
                .into_response()
        }
    }
}

/// Update an existing EPG source
#[utoipa::path(
    put,
    path = "/sources/epg/{id}",
    tag = "epg-sources",
    summary = "Update EPG source",
    description = "Update an existing EPG source configuration",
    params(
        ("id" = String, Path, description = "EPG source ID (UUID)"),
    ),
    request_body = UpdateEpgSourceRequest,
    responses(
        (status = 200, description = "EPG source updated successfully", body = ApiResponse<EpgSourceResponse>),
        (status = 400, description = "Invalid request data or UUID format"),
        (status = 404, description = "EPG source not found"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn update_epg_source(
    State(state): State<AppState>,
    Path(id): Path<String>,
    context: RequestContext,
    Json(request): Json<UpdateEpgSourceRequest>,
) -> impl IntoResponse {
    log_request(
        &axum::http::Method::PUT,
        &format!("/api/v1/sources/epg/{id}").parse().unwrap(),
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
        .epg_source_service
        .update_with_validation(uuid, service_request)
        .await
    {
        Ok(source) => {
            let response = EpgSourceResponse::from(source);
            ok(response).into_response()
        }
        Err(e) => {
            tracing::error!("Failed to update EPG source {}: {}", uuid, e);
            crate::web::responses::internal_error(&format!("Failed to update EPG source: {e}"))
                .into_response()
        }
    }
}

/// Delete an EPG source
#[utoipa::path(
    delete,
    path = "/sources/epg/{id}",
    tag = "epg-sources",
    summary = "Delete EPG source",
    description = "Delete an EPG source and clean up related data",
    params(
        ("id" = String, Path, description = "EPG source ID (UUID)", example = "550e8400-e29b-41d4-a716-446655440000"),
    ),
    responses(
        (status = 200, description = "EPG source deleted successfully"),
        (status = 400, description = "Invalid UUID format"),
        (status = 404, description = "EPG source not found"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn delete_epg_source(
    State(state): State<AppState>,
    Path(id): Path<String>,
    context: RequestContext,
) -> impl IntoResponse {
    log_request(
        &axum::http::Method::DELETE,
        &format!("/api/v1/sources/epg/{id}").parse().unwrap(),
        &context,
    );

    let uuid = match extract_uuid_param(&id) {
        Ok(uuid) => uuid,
        Err(error) => return crate::web::responses::bad_request(&error).into_response(),
    };

    match state.epg_source_service.delete_with_cleanup(uuid).await {
        Ok(()) => crate::web::responses::ok(
            serde_json::json!({"message": "EPG source deleted successfully"}),
        )
        .into_response(),
        Err(e) => {
            tracing::error!("Failed to delete EPG source {}: {}", uuid, e);
            crate::web::responses::internal_error(&format!("Failed to delete EPG source: {e}"))
                .into_response()
        }
    }
}

/// Validate an EPG source configuration
#[utoipa::path(
    post,
    path = "/sources/epg/validate",
    tag = "epg-sources",
    summary = "Validate EPG source",
    description = "Test an EPG source configuration for validity",
    request_body = CreateEpgSourceRequest,
    responses(
        (status = 200, description = "EPG source validation result"),
        (status = 400, description = "Invalid request data"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn validate_epg_source(
    State(state): State<AppState>,
    context: RequestContext,
    Json(request): Json<CreateEpgSourceRequest>,
) -> impl IntoResponse {
    log_request(
        &axum::http::Method::POST,
        &"/api/v1/sources/epg/validate".parse().unwrap(),
        &context,
    );

    let service_request = match request.into_service_request() {
        Ok(req) => req,
        Err(error) => return crate::web::responses::bad_request(&error).into_response(),
    };

    // Use the service layer to test the connection
    match state
        .epg_source_service
        .test_connection(&service_request)
        .await
    {
        Ok(test_result) => ok(serde_json::json!({
            "valid": test_result.success,
            "message": test_result.message,
            "has_epg": test_result.has_epg,
            "has_streams": test_result.has_streams
        }))
        .into_response(),
        Err(e) => {
            tracing::error!("Failed to validate EPG source: {}", e);
            ok(serde_json::json!({
                "valid": false,
                "message": format!("Validation failed: {}", e),
                "has_epg": false,
                "has_streams": false
            }))
            .into_response()
        }
    }
}

