use axum::{
    extract::{Multipart, Path, Query, State},
    http::StatusCode,
    response::Json,
};
use chrono::{DateTime, Utc};
use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::json;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use super::AppState;

pub mod relay;
pub mod active_relays;

use crate::data_mapping::DataMappingService;
use crate::models::*;

#[derive(Debug, Deserialize)]
pub struct ChannelQueryParams {
    pub page: Option<u32>,
    pub limit: Option<u32>,
    pub filter: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct DataMappingPreviewRequest {
    pub source_type: String,
    pub limit: Option<u32>,
}

// Helper function to convert MappedChannel to test format
fn mapped_channel_to_test_format(
    mc: &crate::models::data_mapping::MappedChannel,
) -> (
    HashMap<String, Option<String>>,
    HashMap<String, Option<String>>,
) {
    let mut original_values = HashMap::new();
    original_values.insert(
        "channel_name".to_string(),
        Some(mc.original.channel_name.clone()),
    );
    original_values.insert("tvg_id".to_string(), mc.original.tvg_id.clone());
    original_values.insert("tvg_name".to_string(), mc.original.tvg_name.clone());
    original_values.insert("tvg_logo".to_string(), mc.original.tvg_logo.clone());
    original_values.insert("tvg_shift".to_string(), mc.original.tvg_shift.clone());
    original_values.insert("group_title".to_string(), mc.original.group_title.clone());

    let mut mapped_values = HashMap::new();
    mapped_values.insert(
        "channel_name".to_string(),
        Some(mc.mapped_channel_name.clone()),
    );
    mapped_values.insert("tvg_id".to_string(), mc.mapped_tvg_id.clone());
    mapped_values.insert("tvg_name".to_string(), mc.mapped_tvg_name.clone());
    mapped_values.insert("tvg_logo".to_string(), mc.mapped_tvg_logo.clone());
    mapped_values.insert("tvg_shift".to_string(), mc.mapped_tvg_shift.clone());
    mapped_values.insert("group_title".to_string(), mc.mapped_group_title.clone());

    (original_values, mapped_values)
}

// Helper function to get the resolved value for a field from a MappedChannel
#[allow(dead_code)]
fn get_resolved_value(
    mc: &crate::models::data_mapping::MappedChannel,
    field: &str,
) -> Option<String> {
    match field {
        "channel_name" => Some(mc.mapped_channel_name.clone()),
        "tvg_id" => mc.mapped_tvg_id.clone(),
        "tvg_name" => mc.mapped_tvg_name.clone(),
        "tvg_logo" => mc.mapped_tvg_logo.clone(),
        "tvg_shift" => mc.mapped_tvg_shift.clone(),
        "group_title" => mc.mapped_group_title.clone(),
        _ => None,
    }
}

// Helper function to transform MappedChannel to frontend-compatible format
fn mapped_channel_to_frontend_format(
    mc: &crate::models::data_mapping::MappedChannel,
) -> serde_json::Value {
    json!({
        "id": mc.original.id,
        "source_id": mc.original.source_id,
        "channel_name": mc.mapped_channel_name,
        "tvg_id": mc.mapped_tvg_id.as_ref().or(mc.original.tvg_id.as_ref()),
        "tvg_name": mc.mapped_tvg_name.as_ref().or(mc.original.tvg_name.as_ref()),
        "tvg_logo": mc.mapped_tvg_logo.as_ref().or(mc.original.tvg_logo.as_ref()),
        "tvg_shift": mc.mapped_tvg_shift.as_ref().or(mc.original.tvg_shift.as_ref()),
        "group_title": mc.mapped_group_title.as_ref().or(mc.original.group_title.as_ref()),
        "stream_url": mc.original.stream_url,
        "is_removed": mc.is_removed,
        "applied_rules": mc.applied_rules,
        "created_at": mc.original.created_at,
        "updated_at": mc.original.updated_at,
        // Original values with original_ prefix
        "original_channel_name": mc.original.channel_name,
        "original_tvg_id": mc.original.tvg_id,
        "original_tvg_name": mc.original.tvg_name,
        "original_tvg_logo": mc.original.tvg_logo,
        "original_tvg_shift": mc.original.tvg_shift,
        "original_group_title": mc.original.group_title,
        // Mapped values for reference
        "mapped_channel_name": mc.mapped_channel_name,
        "mapped_tvg_id": mc.mapped_tvg_id,
        "mapped_tvg_name": mc.mapped_tvg_name,
        "mapped_tvg_logo": mc.mapped_tvg_logo,
        "mapped_tvg_shift": mc.mapped_tvg_shift,
        "mapped_group_title": mc.mapped_group_title,
        "capture_group_values": mc.capture_group_values
    })
}

// Stream Sources API

// Progress API

pub async fn get_all_progress(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let all_progress = state.state_manager.get_all_progress().await;

    // Get processing info for all sources with progress
    let mut enhanced_progress = std::collections::HashMap::new();

    for (source_id, progress) in all_progress.iter() {
        let processing_info = state.state_manager.get_processing_info(*source_id).await;
        enhanced_progress.insert(
            source_id,
            serde_json::json!({
                "progress": progress,
                "processing_info": processing_info
            }),
        );
    }

    let result = serde_json::json!({
        "success": true,
        "message": "All progress retrieved",
        "progress": enhanced_progress,
        "total_sources": all_progress.len()
    });

    Ok(Json(result))
}

pub async fn get_all_source_progress(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let all_progress = state.state_manager.get_all_progress().await;

    // Get processing info for all sources with progress
    let mut enhanced_progress = std::collections::HashMap::new();

    for (source_id, progress) in all_progress.iter() {
        let processing_info = state.state_manager.get_processing_info(*source_id).await;
        enhanced_progress.insert(
            source_id,
            serde_json::json!({
                "progress": progress,
                "processing_info": processing_info
            }),
        );
    }

    let result = serde_json::json!({
        "success": true,
        "message": "Source progress retrieved",
        "progress": enhanced_progress,
        "total_sources": all_progress.len()
    });

    Ok(Json(result))
}

pub async fn get_operation_progress(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let all_progress = state.state_manager.get_all_progress().await;

    // Filter to show only active operations (not completed or failed)
    let active_operations: std::collections::HashMap<Uuid, IngestionProgress> = all_progress
        .into_iter()
        .filter(|(_, progress)| {
            // Include active operations
            matches!(
                progress.state,
                crate::models::IngestionState::Connecting
                    | crate::models::IngestionState::Downloading
                    | crate::models::IngestionState::Parsing
            )
        })
        .collect();

    let result = serde_json::json!({
        "success": true,
        "message": "Active operation progress retrieved",
        "progress": active_operations,
        "total_operations": active_operations.len()
    });

    Ok(Json(result))
}

pub async fn get_epg_source_progress(
    Path(id): Path<Uuid>,
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let progress = state.state_manager.get_progress(id).await;
    let processing_info = state.state_manager.get_processing_info(id).await;

    let result = serde_json::json!({
        "success": true,
        "message": "EPG source progress retrieved",
        "source_id": id,
        "progress": progress,
        "processing_info": processing_info
    });

    Ok(Json(result))
}

// Stream Proxies API - implementations are in handlers/proxies.rs

pub async fn regenerate_proxy(
    Path(proxy_id): Path<Uuid>,
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    info!("Regenerating proxy {}", proxy_id);
    
    // Get the proxy
    let proxy = match state.database.get_stream_proxy(proxy_id).await {
        Ok(Some(proxy)) => proxy,
        Ok(None) => {
            error!("Proxy {} not found", proxy_id);
            return Err(StatusCode::NOT_FOUND);
        }
        Err(e) => {
            error!("Failed to get proxy {}: {}", proxy_id, e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    // Create repositories for dependency injection
    use crate::repositories::{
        FilterRepository, StreamProxyRepository, StreamSourceRepository,
    };
    let proxy_repo = StreamProxyRepository::new(state.database.pool());
    let stream_source_repo = StreamSourceRepository::new(state.database.pool());
    let filter_repo = FilterRepository::new(state.database.pool());

    let config_resolver = crate::proxy::config_resolver::ProxyConfigResolver::new(
        proxy_repo,
        stream_source_repo,
        filter_repo,
        state.database.clone(),
    );

    // Resolve proxy configuration upfront (single database query)
    match config_resolver.resolve_config(proxy_id).await {
        Ok(resolved_config) => {
            // Validate configuration
            if let Err(e) = config_resolver.validate_config(&resolved_config) {
                error!("Invalid proxy configuration for {}: {}", proxy_id, e);
                return Err(StatusCode::BAD_REQUEST);
            }

            // Create production output destination  
            let output = crate::models::GenerationOutput::InMemory;

            // Generate using dependency injection
            match state.proxy_service
                .generate_proxy_with_config(
                    resolved_config,
                    output,
                    &state.database,
                    &state.data_mapping_service,
                    &state.logo_asset_service,
                    &state.config.web.base_url,
                    state.config.data_mapping_engine.clone(),
                    &state.config,
                )
                .await
            {
                Ok(generation) => {
                    // Save the M3U file using the proxy service
                    match state.proxy_service
                        .save_m3u_file_with_manager(
                            &proxy.id.to_string(),
                            &generation.m3u_content,
                            None,
                        )
                        .await
                    {
                        Ok(_) => {
                            // Update the last_generated_at timestamp for the proxy
                            if let Err(e) = state.database.update_proxy_last_generated(proxy_id).await {
                                error!("Failed to update last_generated_at for proxy {}: {}", proxy_id, e);
                            }
                            
                            info!(
                                "Successfully regenerated proxy '{}' with {} channels using ProxyService",
                                proxy.name, generation.channel_count
                            );
                            Ok(Json(serde_json::json!({
                                "success": true,
                                "message": format!("Proxy '{}' regenerated successfully", proxy.name),
                                "channel_count": generation.channel_count,
                                "regenerated_at": generation.created_at
                            })))
                        }
                        Err(e) => {
                            error!(
                                "Failed to save regenerated M3U for proxy {}: {}",
                                proxy_id, e
                            );
                            Err(StatusCode::INTERNAL_SERVER_ERROR)
                        }
                    }
                }
                Err(e) => {
                    error!(
                        "Failed to regenerate proxy {} using ProxyService: {}",
                        proxy_id, e
                    );
                    Err(StatusCode::INTERNAL_SERVER_ERROR)
                }
            }
        }
        Err(e) => {
            error!(
                "Failed to resolve proxy configuration for {}: {}",
                proxy_id, e
            );
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

// Filters API
/// List all filters
#[utoipa::path(
    get,
    path = "/filters",
    tag = "filters",
    summary = "List filters",
    description = "Retrieve all filters with usage statistics and expression trees",
    responses(
        (status = 200, description = "List of filters with statistics"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn list_filters(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    match state.database.get_filters_with_usage().await {
        Ok(filters) => {
            // Generate expression trees for each filter
            let enhanced_filters: Vec<serde_json::Value> = filters
                .into_iter()
                .map(|filter_with_usage| {
                    let expression_tree =
                        if !filter_with_usage.filter.condition_tree.trim().is_empty() {
                            // Parse the condition_tree JSON to generate expression tree
                            if let Ok(condition_tree) =
                                serde_json::from_str::<crate::models::ConditionTree>(
                                    &filter_with_usage.filter.condition_tree,
                                )
                            {
                                Some(generate_expression_tree_json(&condition_tree))
                            } else {
                                None
                            }
                        } else {
                            None
                        };

                    serde_json::json!({
                        "filter": filter_with_usage.filter,
                        "usage_count": filter_with_usage.usage_count,
                        "expression_tree": expression_tree,
                    })
                })
                .collect();

            Ok(Json(serde_json::Value::Array(enhanced_filters)))
        }
        Err(e) => {
            error!("Failed to list filters: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

pub async fn get_filter_fields(
    State(state): State<AppState>,
) -> Result<Json<Vec<FilterFieldInfo>>, StatusCode> {
    match state.database.get_available_filter_fields().await {
        Ok(fields) => Ok(Json(fields)),
        Err(e) => {
            error!("Failed to get filter fields: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

/// Create a new filter
#[utoipa::path(
    post,
    path = "/filters",
    tag = "filters",
    summary = "Create filter",
    description = "Create a new channel filter with conditions",
    request_body = FilterCreateRequest,
    responses(
        (status = 200, description = "Filter created successfully"),
        (status = 400, description = "Invalid filter data"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn create_filter(
    State(state): State<AppState>,
    Json(payload): Json<FilterCreateRequest>,
) -> Result<Json<Filter>, StatusCode> {
    match state.database.create_filter(&payload).await {
        Ok(filter) => Ok(Json(filter)),
        Err(e) => {
            error!("Failed to create filter: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

#[utoipa::path(
    get,
    path = "/filters/{id}",
    tag = "filters",
    summary = "Get filter",
    description = "Retrieve a specific filter by ID",
    params(
        ("id" = String, Path, description = "Filter ID (UUID)"),
    ),
    responses(
        (status = 200, description = "Filter details"),
        (status = 404, description = "Filter not found"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_filter(
    Path(id): Path<Uuid>,
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    match state.database.get_filter(id).await {
        Ok(Some(filter)) => {
            let expression_tree = if !filter.condition_tree.trim().is_empty() {
                // Parse the condition_tree JSON to generate expression tree
                if let Ok(condition_tree) = serde_json::from_str::<crate::models::ConditionTree>(&filter.condition_tree) {
                    Some(generate_expression_tree_json(&condition_tree))
                } else {
                    None
                }
            } else {
                None
            };

            let response = serde_json::json!({
                "filter": filter,
                "expression_tree": expression_tree,
            });
            Ok(Json(response))
        },
        Ok(None) => Err(StatusCode::NOT_FOUND),
        Err(e) => {
            error!("Failed to get filter {}: {}", id, e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

#[utoipa::path(
    put,
    path = "/filters/{id}",
    tag = "filters",
    summary = "Update filter",
    description = "Update an existing filter",
    params(
        ("id" = String, Path, description = "Filter ID (UUID)"),
    ),
    responses(
        (status = 200, description = "Filter updated successfully"),
        (status = 404, description = "Filter not found"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn update_filter(
    Path(id): Path<Uuid>,
    State(state): State<AppState>,
    Json(payload): Json<FilterUpdateRequest>,
) -> Result<Json<Filter>, StatusCode> {
    match state.database.update_filter(id, &payload).await {
        Ok(Some(filter)) => Ok(Json(filter)),
        Ok(None) => Err(StatusCode::NOT_FOUND),
        Err(e) => {
            error!("Failed to update filter {}: {}", id, e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

#[utoipa::path(
    delete,
    path = "/filters/{id}",
    tag = "filters",
    summary = "Delete filter",
    description = "Delete a filter",
    params(
        ("id" = String, Path, description = "Filter ID (UUID)"),
    ),
    responses(
        (status = 204, description = "Filter deleted successfully"),
        (status = 404, description = "Filter not found"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn delete_filter(
    Path(id): Path<Uuid>,
    State(state): State<AppState>,
) -> Result<StatusCode, StatusCode> {
    match state.database.delete_filter(id).await {
        Ok(true) => Ok(StatusCode::NO_CONTENT),
        Ok(false) => Err(StatusCode::NOT_FOUND),
        Err(e) => {
            error!("Failed to delete filter {}: {}", id, e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

pub async fn test_filter(
    State(state): State<AppState>,
    Json(payload): Json<FilterTestRequest>,
) -> Result<Json<FilterTestResult>, StatusCode> {
    match state
        .database
        .test_filter_pattern(payload.source_id, &payload)
        .await
    {
        Ok(result) => Ok(Json(result)),
        Err(e) => {
            error!("Failed to test filter: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

pub async fn validate_filter(
    Json(payload): Json<FilterTestRequest>,
) -> Result<Json<FilterTestResult>, StatusCode> {
    // Parse the filter expression to validate syntax and generate expression tree
    let parser = crate::filter_parser::FilterParser::new();
    let condition_tree = match parser.parse(&payload.filter_expression) {
        Ok(tree) => tree,
        Err(e) => {
            return Ok(Json(FilterTestResult {
                is_valid: false,
                error: Some(format!("Syntax error: {}", e)),
                matching_channels: vec![],
                total_channels: 0,
                matched_count: 0,
                expression_tree: None,
            }));
        }
    };

    // Generate expression tree for frontend
    let expression_tree = generate_expression_tree_json(&condition_tree);

    Ok(Json(FilterTestResult {
        is_valid: true,
        error: None,
        matching_channels: vec![], // No actual testing for validation
        total_channels: 0,
        matched_count: 0,
        expression_tree: Some(expression_tree),
    }))
}

// Source-specific filter endpoints
pub async fn list_stream_source_filters(
    Path(source_id): Path<Uuid>,
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    // Verify stream source exists
    match state.database.get_stream_source(source_id).await {
        Ok(Some(_)) => {}
        Ok(None) => return Err(StatusCode::NOT_FOUND),
        Err(e) => {
            error!("Failed to verify stream source {}: {}", source_id, e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    }

    // Get filters for this source type
    match state.database.get_filters_with_usage().await {
        Ok(filters) => {
            let stream_filters: Vec<_> = filters
                .into_iter()
                .filter(|f| matches!(f.filter.source_type, FilterSourceType::Stream))
                .collect();

            let result = serde_json::json!({
                "success": true,
                "message": "Stream source filters retrieved",
                "source_id": source_id,
                "source_type": "stream",
                "filters": stream_filters,
                "total_filters": stream_filters.len()
            });

            Ok(Json(result))
        }
        Err(e) => {
            error!("Failed to list stream source filters: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

pub async fn create_stream_source_filter(
    Path(source_id): Path<Uuid>,
    State(state): State<AppState>,
    Json(mut payload): Json<FilterCreateRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    // Verify stream source exists
    match state.database.get_stream_source(source_id).await {
        Ok(Some(_)) => {}
        Ok(None) => return Err(StatusCode::NOT_FOUND),
        Err(e) => {
            error!("Failed to verify stream source {}: {}", source_id, e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    }

    // Ensure source type is set to Stream
    payload.source_type = FilterSourceType::Stream;

    match state.database.create_filter(&payload).await {
        Ok(filter) => {
            let result = serde_json::json!({
                "success": true,
                "message": "Stream source filter created",
                "source_id": source_id,
                "source_type": "stream",
                "filter": filter
            });

            Ok(Json(result))
        }
        Err(e) => {
            error!("Failed to create stream source filter: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

pub async fn list_epg_source_filters(
    Path(source_id): Path<Uuid>,
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    // Verify EPG source exists
    match state.database.get_epg_source(source_id).await {
        Ok(Some(_)) => {}
        Ok(None) => return Err(StatusCode::NOT_FOUND),
        Err(e) => {
            error!("Failed to verify EPG source {}: {}", source_id, e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    }

    // Get filters for this source type
    match state.database.get_filters_with_usage().await {
        Ok(filters) => {
            let epg_filters: Vec<_> = filters
                .into_iter()
                .filter(|f| matches!(f.filter.source_type, FilterSourceType::Epg))
                .collect();

            let result = serde_json::json!({
                "success": true,
                "message": "EPG source filters retrieved",
                "source_id": source_id,
                "source_type": "epg",
                "filters": epg_filters,
                "total_filters": epg_filters.len()
            });

            Ok(Json(result))
        }
        Err(e) => {
            error!("Failed to list EPG source filters: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

pub async fn create_epg_source_filter(
    Path(source_id): Path<Uuid>,
    State(state): State<AppState>,
    Json(mut payload): Json<FilterCreateRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    // Verify EPG source exists
    match state.database.get_epg_source(source_id).await {
        Ok(Some(_)) => {}
        Ok(None) => return Err(StatusCode::NOT_FOUND),
        Err(e) => {
            error!("Failed to verify EPG source {}: {}", source_id, e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    }

    // Ensure source type is set to EPG
    payload.source_type = FilterSourceType::Epg;

    match state.database.create_filter(&payload).await {
        Ok(filter) => {
            let result = serde_json::json!({
                "success": true,
                "message": "EPG source filter created",
                "source_id": source_id,
                "source_type": "epg",
                "filter": filter
            });

            Ok(Json(result))
        }
        Err(e) => {
            error!("Failed to create EPG source filter: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

// Cross-source filter operations
pub async fn list_stream_filters(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    match state.database.get_filters_with_usage().await {
        Ok(filters) => {
            let stream_filters: Vec<_> = filters
                .into_iter()
                .filter(|f| matches!(f.filter.source_type, FilterSourceType::Stream))
                .collect();

            let result = serde_json::json!({
                "success": true,
                "message": "All stream filters retrieved",
                "source_type": "stream",
                "filters": stream_filters,
                "total_filters": stream_filters.len()
            });

            Ok(Json(result))
        }
        Err(e) => {
            error!("Failed to list stream filters: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

pub async fn list_epg_filters(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    match state.database.get_filters_with_usage().await {
        Ok(filters) => {
            let epg_filters: Vec<_> = filters
                .into_iter()
                .filter(|f| matches!(f.filter.source_type, FilterSourceType::Epg))
                .collect();

            let result = serde_json::json!({
                "success": true,
                "message": "All EPG filters retrieved",
                "source_type": "epg",
                "filters": epg_filters,
                "total_filters": epg_filters.len()
            });

            Ok(Json(result))
        }
        Err(e) => {
            error!("Failed to list EPG filters: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

pub async fn get_stream_filter_fields(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    match state.database.get_available_filter_fields().await {
        Ok(fields) => {
            let stream_fields: Vec<_> = fields
                .into_iter()
                .filter(|f| matches!(f.source_type, FilterSourceType::Stream))
                .collect();

            let result = serde_json::json!({
                "success": true,
                "message": "Stream filter fields retrieved",
                "source_type": "stream",
                "fields": stream_fields,
                "total_fields": stream_fields.len()
            });

            Ok(Json(result))
        }
        Err(e) => {
            error!("Failed to get stream filter fields: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

pub async fn get_epg_filter_fields(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    match state.database.get_available_filter_fields().await {
        Ok(fields) => {
            let epg_fields: Vec<_> = fields
                .into_iter()
                .filter(|f| matches!(f.source_type, FilterSourceType::Epg))
                .collect();

            let result = serde_json::json!({
                "success": true,
                "message": "EPG filter fields retrieved",
                "source_type": "epg",
                "fields": epg_fields,
                "total_fields": epg_fields.len()
            });

            Ok(Json(result))
        }
        Err(e) => {
            error!("Failed to get EPG filter fields: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

// Data Mapping API
/// List all data mapping rules
#[utoipa::path(
    get,
    path = "/data-mapping",
    tag = "data-mapping",
    summary = "List data mapping rules",
    description = "Retrieve all data mapping rules with enhanced metadata",
    responses(
        (status = 200, description = "List of data mapping rules"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn list_data_mapping_rules(
    State(state): State<AppState>,
) -> Result<Json<Vec<serde_json::Value>>, StatusCode> {
    match state.data_mapping_service.get_all_rules().await {
        Ok(rules) => {
            let enhanced_rules = rules
                .into_iter()
                .map(|rule| {
                    // Parse expression to get counts
                    let (condition_count, action_count) = if let Some(expression) = &rule.expression
                    {
                        let available_fields = vec![
                            "tvg_id".to_string(),
                            "tvg_name".to_string(),
                            "tvg_logo".to_string(),
                            "tvg_shift".to_string(),
                            "group_title".to_string(),
                            "channel_name".to_string(),
                        ];
                        let parser =
                            crate::filter_parser::FilterParser::new().with_fields(available_fields);
                        if let Ok(parsed) = parser.parse_extended(expression) {
                            match parsed {
                                crate::models::ExtendedExpression::ConditionOnly(
                                    condition_tree,
                                ) => (count_conditions_in_tree(&condition_tree), 0),
                                crate::models::ExtendedExpression::ConditionWithActions {
                                    condition,
                                    actions,
                                } => (count_conditions_in_tree(&condition), actions.len()),
                                crate::models::ExtendedExpression::ConditionalActionGroups(
                                    groups,
                                ) => {
                                    let condition_count: usize = groups
                                        .iter()
                                        .map(|g| count_conditions_in_tree(&g.conditions))
                                        .sum();
                                    let action_count: usize =
                                        groups.iter().map(|g| g.actions.len()).sum();
                                    (condition_count, action_count)
                                }
                            }
                        } else {
                            (0, 0)
                        }
                    } else {
                        (0, 0)
                    };

                    // Generate JSON expression tree for frontend display
                    let expression_tree = if let Some(expression) = &rule.expression {
                        let available_fields = vec![
                            "tvg_id".to_string(),
                            "tvg_name".to_string(),
                            "tvg_logo".to_string(),
                            "tvg_shift".to_string(),
                            "group_title".to_string(),
                            "channel_name".to_string(),
                        ];
                        let parser =
                            crate::filter_parser::FilterParser::new().with_fields(available_fields);
                        if let Ok(parsed) = parser.parse_extended(expression) {
                            match parsed {
                                crate::models::ExtendedExpression::ConditionOnly(
                                    condition_tree,
                                ) => Some(generate_expression_tree_json(&condition_tree)),
                                crate::models::ExtendedExpression::ConditionWithActions {
                                    condition,
                                    ..
                                } => Some(generate_expression_tree_json(&condition)),
                                crate::models::ExtendedExpression::ConditionalActionGroups(
                                    groups,
                                ) => {
                                    if let Some(first_group) = groups.first() {
                                        Some(generate_expression_tree_json(&first_group.conditions))
                                    } else {
                                        None
                                    }
                                }
                            }
                        } else {
                            None
                        }
                    } else {
                        None
                    };

                    serde_json::json!({
                        "id": rule.id,
                        "name": rule.name,
                        "description": rule.description,
                        "source_type": rule.source_type,
                        "scope": rule.scope,
                        "expression": rule.expression,
                        "sort_order": rule.sort_order,
                        "is_active": rule.is_active,
                        "created_at": rule.created_at,
                        "updated_at": rule.updated_at,
                        "condition_count": condition_count,
                        "action_count": action_count,
                        "expression_tree": expression_tree,
                    })
                })
                .collect();

            Ok(Json(enhanced_rules))
        }
        Err(e) => {
            error!("Failed to list data mapping rules: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

#[utoipa::path(
    post,
    path = "/data-mapping",
    tag = "data-mapping",
    summary = "Create data mapping rule",
    description = "Create a new data mapping rule for transforming channel metadata",
    responses(
        (status = 200, description = "Data mapping rule created successfully"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn create_data_mapping_rule(
    State(state): State<AppState>,
    Json(payload): Json<crate::models::data_mapping::DataMappingRuleCreateRequest>,
) -> Result<Json<crate::models::data_mapping::DataMappingRule>, StatusCode> {
    match state.data_mapping_service.create_rule(payload).await {
        Ok(rule) => Ok(Json(rule)),
        Err(e) => {
            error!("Failed to create data mapping rule: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

#[utoipa::path(
    get,
    path = "/data-mapping/{id}",
    tag = "data-mapping",
    summary = "Get data mapping rule",
    description = "Retrieve a specific data mapping rule by ID",
    params(
        ("id" = String, Path, description = "Data mapping rule ID (UUID)"),
    ),
    responses(
        (status = 200, description = "Data mapping rule details"),
        (status = 404, description = "Data mapping rule not found"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_data_mapping_rule(
    Path(id): Path<Uuid>,
    State(state): State<AppState>,
) -> Result<Json<crate::models::data_mapping::DataMappingRule>, StatusCode> {
    match state.data_mapping_service.get_rule_with_details(id).await {
        Ok(rule) => Ok(Json(rule)),
        Err(e) => {
            error!("Failed to get data mapping rule {}: {}", id, e);
            Err(StatusCode::NOT_FOUND)
        }
    }
}

#[utoipa::path(
    put,
    path = "/data-mapping/{id}",
    tag = "data-mapping",
    summary = "Update data mapping rule",
    description = "Update an existing data mapping rule",
    params(
        ("id" = String, Path, description = "Data mapping rule ID (UUID)"),
    ),
    responses(
        (status = 200, description = "Data mapping rule updated successfully"),
        (status = 404, description = "Data mapping rule not found"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn update_data_mapping_rule(
    Path(id): Path<Uuid>,
    State(state): State<AppState>,
    Json(payload): Json<crate::models::data_mapping::DataMappingRuleUpdateRequest>,
) -> Result<Json<crate::models::data_mapping::DataMappingRule>, StatusCode> {
    match state.data_mapping_service.update_rule(id, payload).await {
        Ok(rule) => Ok(Json(rule)),
        Err(e) => {
            error!("Failed to update data mapping rule {}: {}", id, e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

#[utoipa::path(
    delete,
    path = "/data-mapping/{id}",
    tag = "data-mapping",
    summary = "Delete data mapping rule",
    description = "Delete a data mapping rule",
    params(
        ("id" = String, Path, description = "Data mapping rule ID (UUID)"),
    ),
    responses(
        (status = 204, description = "Data mapping rule deleted successfully"),
        (status = 404, description = "Data mapping rule not found"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn delete_data_mapping_rule(
    Path(id): Path<Uuid>,
    State(state): State<AppState>,
) -> Result<StatusCode, StatusCode> {
    match state.data_mapping_service.delete_rule(id).await {
        Ok(_) => Ok(StatusCode::NO_CONTENT),
        Err(e) => {
            error!("Failed to delete data mapping rule {}: {}", id, e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

#[utoipa::path(
    put,
    path = "/data-mapping/reorder",
    tag = "data-mapping",
    summary = "Reorder data mapping rules",
    description = "Update the order/priority of data mapping rules",
    responses(
        (status = 204, description = "Data mapping rules reordered successfully"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn reorder_data_mapping_rules(
    State(state): State<AppState>,
    Json(payload): Json<Vec<(Uuid, i32)>>,
) -> Result<StatusCode, StatusCode> {
    match state.data_mapping_service.reorder_rules(payload).await {
        Ok(_) => Ok(StatusCode::NO_CONTENT),
        Err(e) => {
            error!("Failed to reorder data mapping rules: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

#[utoipa::path(
    post,
    path = "/data-mapping/validate-expression",
    tag = "data-mapping",
    summary = "Validate data mapping expression",
    description = "Validate a data mapping expression for syntax and available fields",
    responses(
        (status = 200, description = "Expression validation result"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn validate_data_mapping_expression(
    Json(payload): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    use crate::filter_parser::FilterParser;
    use crate::models::data_mapping::{EpgMappingFields, StreamMappingFields};

    let expression = payload
        .get("expression")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim();

    let source_type = payload
        .get("source_type")
        .and_then(|v| v.as_str())
        .unwrap_or("stream");

    if expression.is_empty() {
        return Ok(Json(serde_json::json!({
            "isValid": false,
            "error": "Expression cannot be empty"
        })));
    }

    // Get available fields for this source type
    let available_fields = match source_type {
        "stream" => StreamMappingFields::available_fields()
            .into_iter()
            .map(|s| s.to_string())
            .collect(),
        "epg" => EpgMappingFields::available_fields()
            .into_iter()
            .map(|s| s.to_string())
            .collect(),
        _ => {
            return Ok(Json(serde_json::json!({
                "isValid": false,
                "error": "Invalid source type"
            })));
        }
    };

    let parser = FilterParser::new().with_fields(available_fields);

    // Parse and validate the expression
    match parser.parse_extended(expression) {
        Ok(parsed) => match parser.validate_extended(&parsed) {
            Ok(_) => Ok(Json(serde_json::json!({
                "isValid": true
            }))),
            Err(e) => Ok(Json(serde_json::json!({
                "isValid": false,
                "error": format!("Validation error: {}", e)
            }))),
        },
        Err(e) => Ok(Json(serde_json::json!({
            "isValid": false,
            "error": format!("Syntax error: {}", e)
        }))),
    }
}

#[utoipa::path(
    get,
    path = "/data-mapping/fields/stream",
    tag = "data-mapping",
    summary = "Get stream mapping fields",
    description = "Get available fields for stream data mapping expressions",
    responses(
        (status = 200, description = "List of available stream mapping fields"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_data_mapping_stream_fields() -> Result<Json<serde_json::Value>, StatusCode> {
    use crate::models::data_mapping::StreamMappingFields;

    let fields = StreamMappingFields::available_fields()
        .into_iter()
        .map(|field| {
            serde_json::json!({
                "name": field,
                "description": get_field_description(&field),
                "type": "string"
            })
        })
        .collect::<Vec<_>>();

    Ok(Json(serde_json::json!({
        "success": true,
        "fields": fields
    })))
}

#[utoipa::path(
    get,
    path = "/data-mapping/fields/epg",
    tag = "data-mapping",
    summary = "Get EPG mapping fields",
    description = "Get available fields for EPG data mapping expressions",
    responses(
        (status = 200, description = "List of available EPG mapping fields"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_data_mapping_epg_fields() -> Result<Json<serde_json::Value>, StatusCode> {
    use crate::models::data_mapping::EpgMappingFields;

    let fields = EpgMappingFields::available_fields()
        .into_iter()
        .map(|field| {
            serde_json::json!({
                "name": field,
                "description": get_field_description(&field),
                "type": "string"
            })
        })
        .collect::<Vec<_>>();

    Ok(Json(serde_json::json!({
        "success": true,
        "fields": fields
    })))
}

fn get_field_description(field: &str) -> &'static str {
    match field {
        "channel_name" => "The name/title of the channel",
        "tvg_id" => "Electronic program guide identifier",
        "tvg_name" => "Name for EPG matching",
        "tvg_logo" => "URL to channel logo image",
        "tvg_shift" => "Time shift in hours for EPG data",
        "group_title" => "Category/group name for the channel",
        "stream_url" => "Direct URL to the media stream",
        "channel_id" => "Unique channel identifier",
        "channel_logo" => "Channel logo URL",
        "channel_group" => "Channel category/group",
        "language" => "Channel language",
        _ => "Channel data field",
    }
}

#[utoipa::path(
    post,
    path = "/data-mapping/test",
    tag = "data-mapping",
    summary = "Test data mapping rule",
    description = "Test a data mapping rule against sample channel data",
    responses(
        (status = 200, description = "Data mapping test result"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn test_data_mapping_rule(
    State(state): State<AppState>,
    Json(payload): Json<crate::models::data_mapping::DataMappingTestRequest>,
) -> Result<Json<crate::models::data_mapping::DataMappingTestResult>, StatusCode> {
    use crate::data_mapping::DataMappingEngine;

    // Get channels from the source
    let channels = match state
        .database
        .get_channels_for_source(payload.source_id)
        .await
    {
        Ok(channels) => channels,
        Err(e) => {
            error!(
                "Failed to get channels for source {}: {}",
                payload.source_id, e
            );
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    let total_channels_count = channels.len();

    // Get logo assets (simplified for now - could be enhanced later)
    let logo_assets = HashMap::new();

    // Use the engine to test the mapping rule directly
    let mut engine = DataMappingEngine::new();
    let start_time = std::time::Instant::now();
    match engine.test_mapping_rule(
        channels,
        logo_assets,
        &state.config.web.base_url,
        &payload.expression,
    ) {
        Ok(mapped_channels) => {
            let _execution_time = start_time.elapsed().as_micros();

            let test_channels: Vec<crate::models::data_mapping::DataMappingTestChannel> =
                mapped_channels
                    .into_iter()
                    .filter(|mc| !mc.applied_rules.is_empty()) // Only show channels that had rules applied
                    .map(|mc| {
                        let (original_values, mapped_values) = mapped_channel_to_test_format(&mc);

                        crate::models::data_mapping::DataMappingTestChannel {
                            channel_name: mc.original.channel_name,
                            group_title: mc.original.group_title,
                            original_values: serde_json::to_value(original_values)
                                .unwrap_or_default(),
                            mapped_values: serde_json::to_value(mapped_values).unwrap_or_default(),
                            applied_actions: mc.applied_rules,
                        }
                    })
                    .collect();

            let result = crate::models::data_mapping::DataMappingTestResult {
                is_valid: true,
                error: None, // No error for successful tests
                matching_channels: test_channels.clone(),
                total_channels: total_channels_count as i32,
                matched_count: test_channels.len() as i32,
            };

            Ok(Json(result))
        }
        Err(e) => {
            let result = crate::models::data_mapping::DataMappingTestResult {
                is_valid: false,
                error: Some(e.to_string()),
                matching_channels: vec![],
                total_channels: total_channels_count as i32,
                matched_count: 0,
            };
            Ok(Json(result))
        }
    }
}

pub async fn apply_stream_source_data_mapping(
    State(state): State<AppState>,
    Path(source_id): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    info!("Applying data mapping for stream source {}", source_id);

    let source_uuid = match uuid::Uuid::parse_str(&source_id) {
        Ok(uuid) => uuid,
        Err(_) => {
            error!("Invalid source ID format: {}", source_id);
            return Err(StatusCode::BAD_REQUEST);
        }
    };

    // Get source
    let source = match state.database.get_stream_source(source_uuid).await {
        Ok(Some(source)) => source,
        Ok(None) => {
            error!("Stream source {} not found", source_uuid);
            return Err(StatusCode::NOT_FOUND);
        }
        Err(e) => {
            error!("Failed to get stream source {}: {}", source_uuid, e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    // Get original channels
    let channels = match state.database.get_source_channels(source_uuid).await {
        Ok(channels) => channels,
        Err(e) => {
            error!("Failed to get channels for source {}: {}", source_uuid, e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    if channels.is_empty() {
        return Ok(Json(serde_json::json!({
            "success": true,
            "message": "No channels found for preview",
            "original_count": 0,
            "mapped_count": 0,
            "preview_channels": []
        })));
    }

    // Apply data mapping and get channels with metadata
    let (mapped_channels, rule_performance) = match state
        .data_mapping_service
        .apply_mapping_with_metadata(
            channels.clone(),
            source_uuid,
            &state.logo_asset_service,
            &state.config.web.base_url,
            state.config.data_mapping_engine.clone(),
        )
        .await
    {
        Ok(result) => result,
        Err(e) => {
            error!("Data mapping failed for source '{}': {}", source.name, e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    // Debug logging for performance data
    info!(
        "Performance data collected for {} rules",
        rule_performance.len()
    );
    info!("Available performance rule IDs:");
    for (rule_id, (total_time, avg_time, processed)) in &rule_performance {
        info!(
            "  Performance Rule ID '{}': total={}μs, avg={}μs, channels={}",
            rule_id, total_time, avg_time, processed
        );
    }

    // Filter to show only modified channels for preview
    let modified_channels = DataMappingService::filter_modified_channels(mapped_channels.clone());

    let transformed_channels: Vec<serde_json::Value> = modified_channels
        .iter()
        .map(|mc| mapped_channel_to_frontend_format(mc))
        .collect();

    // Get rule metadata for frontend
    let rules = match state.data_mapping_service.get_all_rules().await {
        Ok(rules) => rules
            .into_iter()
            .filter(|r| {
                r.is_active
                    && r.source_type
                        == crate::models::data_mapping::DataMappingSourceType::Stream
            })
            .map(|rule| {
                let affected_count = modified_channels
                    .iter()
                    .filter(|mc| mc.applied_rules.contains(&rule.name))
                    .count();

                // Get performance stats for this rule
                let (total_execution_time, avg_execution_time) = rule_performance
                    .get(&rule.id)
                    .map(|(total_micros, avg_micros, _)| (*total_micros, *avg_micros))
                    .unwrap_or((0, 0));

                // Debug logging for performance data
                info!(
                    "Looking up rule '{}' (ID: {}) in performance data...",
                    rule.name, rule.id
                );
                info!(
                    "Rule '{}' (ID: {}): performance stats lookup - found: {}, total_time: {}μs, avg_time: {}μs",
                    rule.name,
                    rule.id,
                    rule_performance.contains_key(&rule.id),
                    total_execution_time,
                    avg_execution_time
                );
                if !rule_performance.contains_key(&rule.id) {
                    info!("Available IDs in performance data: {:?}", rule_performance.keys().collect::<Vec<_>>());
                }

                // Parse expression to get counts
                let (condition_count, action_count) = if let Some(expression) = &rule.expression {
                    let available_fields = vec![
                        "tvg_id".to_string(),
                        "tvg_name".to_string(),
                        "tvg_logo".to_string(),
                        "tvg_shift".to_string(),
                        "group_title".to_string(),
                        "channel_name".to_string(),
                    ];
                    let parser = crate::filter_parser::FilterParser::new().with_fields(available_fields);
                    if let Ok(parsed) = parser.parse_extended(expression) {
                        match parsed {
                            crate::models::ExtendedExpression::ConditionOnly(condition_tree) => {
                                (count_conditions_in_tree(&condition_tree), 0)
                            }
                            crate::models::ExtendedExpression::ConditionWithActions { condition, actions } => {
                                (count_conditions_in_tree(&condition), actions.len())
                            }
                            crate::models::ExtendedExpression::ConditionalActionGroups(groups) => {
                                let condition_count: usize = groups
                                    .iter()
                                    .map(|g| count_conditions_in_tree(&g.conditions))
                                    .sum();
                                let action_count: usize = groups.iter().map(|g| g.actions.len()).sum();
                                (condition_count, action_count)
                            }
                        }
                    } else {
                        (0, 0)
                    }
                } else {
                    (0, 0)
                };

                // Generate human-readable condition tree for frontend display
                let expression_tree = if let Some(expression) = &rule.expression {
                    let available_fields = vec![
                        "tvg_id".to_string(),
                        "tvg_name".to_string(),
                        "tvg_logo".to_string(),
                        "tvg_shift".to_string(),
                        "group_title".to_string(),
                        "channel_name".to_string(),
                    ];
                    let parser = crate::filter_parser::FilterParser::new().with_fields(available_fields);
                    if let Ok(parsed) = parser.parse_extended(expression) {
                        match parsed {
                            crate::models::ExtendedExpression::ConditionOnly(condition_tree) => {
                                Some(generate_expression_tree_json(&condition_tree))
                            }
                            crate::models::ExtendedExpression::ConditionWithActions { condition, .. } => {
                                Some(generate_expression_tree_json(&condition))
                            }
                            crate::models::ExtendedExpression::ConditionalActionGroups(groups) => {
                                if let Some(first_group) = groups.first() {
                                    Some(generate_expression_tree_json(&first_group.conditions))
                                } else {
                                    None
                                }
                            }
                        }
                    } else {
                        None
                    }
                } else {
                    None
                };

                serde_json::json!({
                    "rule_id": rule.id,
                    "rule_name": rule.name,
                    "rule_description": rule.description,
                    "affected_channels_count": affected_count,
                    "expression": rule.expression,
                    "condition_count": condition_count,
                    "action_count": action_count,
                    "expression_tree": expression_tree,
                    "avg_execution_time": avg_execution_time,
                    "total_execution_time": total_execution_time,
                    "sort_order": rule.sort_order,
                })
            })
            .collect::<Vec<_>>(),
        Err(_) => vec![],
    };

    let result = serde_json::json!({
        "success": true,
        "message": "Data mapping applied successfully",
        "source_name": source.name,
        "source_type": "stream",
        "original_count": channels.len(),
        "mapped_count": modified_channels.len(),
        "total_rules": rules.len(),
        "rules": rules,
        "final_channels": transformed_channels
    });

    info!(
        "Data mapping applied for stream source '{}': {} original -> {} modified channels shown in preview",
        source.name,
        channels.len(),
        modified_channels.len()
    );

    Ok(Json(result))
}

pub async fn apply_epg_source_data_mapping(
    State(state): State<AppState>,
    Path(source_id): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    info!("Applying data mapping for EPG source {}", source_id);

    let source_uuid = match uuid::Uuid::parse_str(&source_id) {
        Ok(uuid) => uuid,
        Err(_) => {
            error!("Invalid source ID format: {}", source_id);
            return Err(StatusCode::BAD_REQUEST);
        }
    };

    // Get EPG source
    let source = match state.database.get_epg_source(source_uuid).await {
        Ok(Some(source)) => source,
        Ok(None) => {
            error!("EPG source {} not found", source_uuid);
            return Err(StatusCode::NOT_FOUND);
        }
        Err(e) => {
            error!("Failed to get EPG source {}: {}", source_uuid, e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    // Get EPG channels (this will be different from stream channels)
    let channels = match state.database.get_epg_source_channels(source_uuid).await {
        Ok(channels) => channels,
        Err(e) => {
            error!(
                "Failed to get EPG channels for source {}: {}",
                source_uuid, e
            );
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    if channels.is_empty() {
        return Ok(Json(serde_json::json!({
            "success": true,
            "message": "No EPG channels found for preview",
            "original_count": 0,
            "mapped_count": 0,
            "preview_channels": []
        })));
    }

    // For now, return placeholder response since EPG data mapping may need different logic
    let result = serde_json::json!({
        "success": true,
        "message": "EPG data mapping preview completed",
        "source_name": source.name,
        "source_type": "epg",
        "original_count": channels.len(),
        "mapped_count": channels.len(),
        "preview_channels": channels
    });

    info!(
        "EPG data mapping preview completed for source '{}': {} channels",
        source.name,
        channels.len()
    );

    Ok(Json(result))
}

// Global data mapping application endpoints for "Preview All Rules" functionality
pub async fn apply_data_mapping_rules_post(
    State(state): State<AppState>,
    Json(payload): Json<DataMappingPreviewRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    apply_data_mapping_rules_impl(state, &payload.source_type, payload.limit).await
}

pub async fn apply_data_mapping_rules(
    Query(params): Query<std::collections::HashMap<String, String>>,
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let source_type = params
        .get("source_type")
        .unwrap_or(&"stream".to_string())
        .clone();

    let limit = params.get("limit").and_then(|s| s.parse::<u32>().ok());

    apply_data_mapping_rules_impl(state, &source_type, limit).await
}

async fn apply_data_mapping_rules_impl(
    state: AppState,
    source_type: &str,
    limit: Option<u32>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    match source_type {
        "stream" => {
            // Get all active stream sources
            let sources = match state.database.list_stream_sources().await {
                Ok(sources) => sources
                    .into_iter()
                    .filter(|s| s.is_active)
                    .collect::<Vec<_>>(),
                Err(e) => {
                    error!("Failed to get stream sources for preview: {}", e);
                    return Err(StatusCode::INTERNAL_SERVER_ERROR);
                }
            };

            if sources.is_empty() {
                return Ok(Json(serde_json::json!({
                    "success": true,
                    "message": "No active stream sources found",
                    "total_sources": 0,
                    "total_channels": 0,
                    "final_channels": []
                })));
            }

            let mut all_preview_channels = Vec::new();
            let mut total_channels = 0;
            let mut combined_performance_data = std::collections::HashMap::new();

            // Process all sources
            for source in sources.iter() {
                let channels = match state.database.get_source_channels(source.id).await {
                    Ok(channels) => channels,
                    Err(_) => continue,
                };

                total_channels += channels.len();

                if !channels.is_empty() {
                    let (mapped_channels, rule_performance) = match state
                        .data_mapping_service
                        .apply_mapping_with_metadata(
                            channels.clone(),
                            source.id,
                            &state.logo_asset_service,
                            &state.config.web.base_url,
                            state.config.data_mapping_engine.clone(),
                        )
                        .await
                    {
                        Ok(result) => result,
                        Err(_) => continue,
                    };

                    // Merge performance data from all sources
                    for (rule_id, (total_time, avg_time, processed_count)) in rule_performance {
                        let entry = combined_performance_data
                            .entry(rule_id)
                            .or_insert((0u128, 0u128, 0usize));
                        entry.0 += total_time; // Sum total execution times
                        entry.1 = if entry.2 + processed_count > 0 {
                            (entry.1 * entry.2 as u128 + avg_time * processed_count as u128)
                                / (entry.2 + processed_count) as u128
                        } else {
                            0
                        }; // Recalculate weighted average
                        entry.2 += processed_count; // Sum processed counts
                    }

                    // Filter to show only modified channels
                    let modified_channels =
                        DataMappingService::filter_modified_channels(mapped_channels);
                    all_preview_channels.extend(modified_channels);
                }
            }

            // Apply limit if specified
            let limited_channels = if let Some(limit) = limit {
                all_preview_channels
                    .into_iter()
                    .take(limit as usize)
                    .collect()
            } else {
                all_preview_channels
            };

            let transformed_channels: Vec<serde_json::Value> = limited_channels
                .iter()
                .map(|mc| mapped_channel_to_frontend_format(mc))
                .collect();

            // Get rule metadata for frontend
            let rules = match state.data_mapping_service.get_all_rules().await {
                Ok(rules) => rules
                    .into_iter()
                    .filter(|r| {
                        r.is_active
                            && r.source_type
                                == crate::models::data_mapping::DataMappingSourceType::Stream
                    })
                    .map(|rule| {
                        let affected_count = limited_channels
                            .iter()
                            .filter(|mc| mc.applied_rules.contains(&rule.name))
                            .count();

                        // Get actual performance data for this rule
                        let (total_execution_time, avg_execution_time, _processed_count) =
                            combined_performance_data
                                .get(&rule.id)
                                .map(|(total, avg, count)| (*total, *avg, *count))
                                .unwrap_or((0, 0, 0));

                        // Parse expression to get counts
                        let (condition_count, action_count) =
                            if let Some(expression) = &rule.expression {
                                let available_fields = vec![
                                    "tvg_id".to_string(),
                                    "tvg_name".to_string(),
                                    "tvg_logo".to_string(),
                                    "tvg_shift".to_string(),
                                    "group_title".to_string(),
                                    "channel_name".to_string(),
                                ];
                                let parser = crate::filter_parser::FilterParser::new()
                                    .with_fields(available_fields);
                                if let Ok(parsed) = parser.parse_extended(expression) {
                                    match parsed {
                                    crate::models::ExtendedExpression::ConditionOnly(
                                        condition_tree,
                                    ) => (count_conditions_in_tree(&condition_tree), 0),
                                    crate::models::ExtendedExpression::ConditionWithActions {
                                        condition,
                                        actions,
                                    } => (count_conditions_in_tree(&condition), actions.len()),
                                    crate::models::ExtendedExpression::ConditionalActionGroups(
                                        groups,
                                    ) => {
                                        let condition_count: usize = groups
                                            .iter()
                                            .map(|g| count_conditions_in_tree(&g.conditions))
                                            .sum();
                                        let action_count: usize =
                                            groups.iter().map(|g| g.actions.len()).sum();
                                        (condition_count, action_count)
                                    }
                                }
                                } else {
                                    (0, 0)
                                }
                            } else {
                                (0, 0)
                            };

                        // Generate JSON expression tree for frontend display
                        let expression_tree = if let Some(expression) = &rule.expression {
                            let available_fields = vec![
                                "tvg_id".to_string(),
                                "tvg_name".to_string(),
                                "tvg_logo".to_string(),
                                "tvg_shift".to_string(),
                                "group_title".to_string(),
                                "channel_name".to_string(),
                            ];
                            let parser = crate::filter_parser::FilterParser::new()
                                .with_fields(available_fields);
                            if let Ok(parsed) = parser.parse_extended(expression) {
                                match parsed {
                                    crate::models::ExtendedExpression::ConditionOnly(
                                        condition_tree,
                                    ) => Some(generate_expression_tree_json(&condition_tree)),
                                    crate::models::ExtendedExpression::ConditionWithActions {
                                        condition,
                                        ..
                                    } => Some(generate_expression_tree_json(&condition)),
                                    crate::models::ExtendedExpression::ConditionalActionGroups(
                                        groups,
                                    ) => {
                                        if let Some(first_group) = groups.first() {
                                            Some(generate_expression_tree_json(
                                                &first_group.conditions,
                                            ))
                                        } else {
                                            None
                                        }
                                    }
                                }
                            } else {
                                None
                            }
                        } else {
                            None
                        };

                        serde_json::json!({
                                "rule_id": rule.id,
                                "rule_name": rule.name,
                                "rule_description": rule.description,
                                "affected_channels_count": affected_count,
                                "expression": rule.expression,
                                "condition_count": condition_count,
                                "action_count": action_count,
                                "expression_tree": expression_tree,
                                "avg_execution_time": avg_execution_time,
                        "total_execution_time": total_execution_time,
                                "sort_order": rule.sort_order,
                        })
                    })
                    .collect::<Vec<_>>(),
                Err(_) => vec![],
            };

            Ok(Json(serde_json::json!({
                "success": true,
                "message": "Stream rules applied successfully",
                "source_type": "stream",
                "total_sources": sources.len(),
                "total_channels": total_channels,
                "total_rules": rules.len(),
                "rules": rules,
                "final_channels": transformed_channels
            })))
        }
        "epg" => {
            // Get all active EPG sources
            let sources = match state.database.list_epg_sources().await {
                Ok(sources) => sources
                    .into_iter()
                    .filter(|s| s.is_active)
                    .collect::<Vec<_>>(),
                Err(e) => {
                    error!("Failed to get EPG sources for preview: {}", e);
                    return Err(StatusCode::INTERNAL_SERVER_ERROR);
                }
            };

            if sources.is_empty() {
                return Ok(Json(serde_json::json!({
                    "success": true,
                    "message": "No active EPG sources found",
                    "total_sources": 0,
                    "total_channels": 0,
                    "final_channels": []
                })));
            }

            // EPG mapping implementation would go here
            // For now, return placeholder
            Ok(Json(serde_json::json!({
                "success": true,
                "message": "EPG rules applied successfully (placeholder)",
                "source_type": "epg",
                "total_sources": sources.len(),
                "total_channels": 0,
                "final_channels": []
            })))
        }
        _ => {
            error!("Invalid source_type parameter: {}", source_type);
            Err(StatusCode::BAD_REQUEST)
        }
    }
}

// Logo Assets API
/// List logo assets with optional filtering
#[utoipa::path(
    get,
    path = "/logos",
    tag = "logos",
    summary = "List logo assets",
    description = "Retrieve a list of logo assets with optional filtering and cached logo inclusion",
    params(
        ("include_cached" = Option<bool>, Query, description = "Include cached logos in the response"),
        ("page" = Option<u32>, Query, description = "Page number for pagination"),
        ("limit" = Option<u32>, Query, description = "Number of items per page"),
    ),
    responses(
        (status = 200, description = "List of logo assets"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn list_logo_assets(
    Query(params): Query<crate::models::logo_asset::LogoAssetListRequest>,
    State(state): State<AppState>,
) -> Result<Json<crate::models::logo_asset::LogoAssetListResponse>, StatusCode> {
    let include_cached = params.include_cached.unwrap_or(true);

    if include_cached && state.logo_cache_scanner.is_some() {
        // Use enhanced listing with cached logos
        match list_logo_assets_with_cached(params, &state).await {
            Ok(response) => Ok(Json(response)),
            Err(e) => {
                error!("Failed to list logo assets with cached: {}", e);
                Err(StatusCode::INTERNAL_SERVER_ERROR)
            }
        }
    } else {
        // Use standard database-only listing
        match state.logo_asset_service.list_assets(params).await {
            Ok(response) => Ok(Json(response)),
            Err(e) => {
                error!("Failed to list logo assets: {}", e);
                Err(StatusCode::INTERNAL_SERVER_ERROR)
            }
        }
    }
}

/// Enhanced logo listing that includes cached logos from filesystem
async fn list_logo_assets_with_cached(
    params: crate::models::logo_asset::LogoAssetListRequest,
    state: &AppState,
) -> Result<crate::models::logo_asset::LogoAssetListResponse, anyhow::Error> {
    use crate::models::logo_asset::*;

    let page = params.page.unwrap_or(1);
    let limit = params.limit.unwrap_or(20);
    let search_query = params.search.as_deref();

    // Get database assets
    let db_response = state.logo_asset_service.list_assets(params.clone()).await?;
    let mut all_assets = db_response.assets;

    // Add cached logos if scanner is available
    if let Some(scanner) = &state.logo_cache_scanner {
        debug!(
            "Logo cache scanner available, searching for cached logos with query: {:?}",
            search_query
        );
        let cached_logos = scanner
            .search_cached_logos(search_query, None)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to search cached logos: {}", e))?;

        debug!("Found {} cached logos from scanner", cached_logos.len());

        // Convert cached logos to LogoAssetWithUrl format
        let mut converted_count = 0;
        for cached_logo in cached_logos {
            let asset_like = cached_logo.to_logo_asset_like(&state.config.web.base_url);

            // Convert JSON value to LogoAssetWithUrl
            match serde_json::from_value::<LogoAssetWithUrl>(asset_like.clone()) {
                Ok(logo_asset_with_url) => {
                    all_assets.push(logo_asset_with_url);
                    converted_count += 1;
                }
                Err(e) => {
                    debug!(
                        "Failed to convert cached logo to LogoAssetWithUrl: {}. JSON: {}",
                        e, asset_like
                    );
                }
            }
        }
        debug!(
            "Successfully converted {} cached logos to LogoAssetWithUrl",
            converted_count
        );
    } else {
        debug!("No logo cache scanner available");
    }

    // Apply search filter if provided (for database assets that might not have been filtered)
    if let Some(query) = search_query {
        let query_lower = query.to_lowercase();
        all_assets.retain(|asset| {
            asset.asset.name.to_lowercase().contains(&query_lower)
                || asset.asset.file_name.to_lowercase().contains(&query_lower)
                || asset
                    .asset
                    .description
                    .as_ref()
                    .map(|desc| desc.to_lowercase().contains(&query_lower))
                    .unwrap_or(false)
        });
    }

    // Sort by updated_at descending (most recent first)
    all_assets.sort_by(|a, b| b.asset.updated_at.cmp(&a.asset.updated_at));

    // Calculate pagination
    let total_count = all_assets.len() as i64;
    let total_pages = ((total_count as f64) / (limit as f64)).ceil() as u32;

    // Apply pagination
    let start_idx = ((page - 1) * limit) as usize;
    let end_idx = (start_idx + limit as usize).min(all_assets.len());
    let paginated_assets = if start_idx < all_assets.len() {
        all_assets[start_idx..end_idx].to_vec()
    } else {
        Vec::new()
    };

    Ok(LogoAssetListResponse {
        assets: paginated_assets,
        total_count,
        page,
        limit,
        total_pages,
    })
}

/// Upload a new logo asset
#[utoipa::path(
    post,
    path = "/logos/upload",
    tag = "logos",
    summary = "Upload logo asset",
    description = "Upload a new logo asset file with metadata",
    request_body(content = String, description = "Multipart form data with logo file and metadata"),
    responses(
        (status = 200, description = "Logo asset uploaded successfully"),
        (status = 400, description = "Invalid upload data"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn upload_logo_asset(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<Json<crate::models::logo_asset::LogoAssetUploadResponse>, StatusCode> {
    use crate::models::logo_asset::{LogoAssetCreateRequest, LogoAssetUploadResponse};

    let mut file_data: Option<(String, String, axum::body::Bytes)> = None;
    let mut logo_name: Option<String> = None;
    let mut logo_description: Option<String> = None;

    // Process all multipart fields
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|_| StatusCode::BAD_REQUEST)?
    {
        match field.name() {
            Some("file") => {
                let file_name = field
                    .file_name()
                    .ok_or(StatusCode::BAD_REQUEST)?
                    .to_string();

                let content_type = field
                    .content_type()
                    .unwrap_or("application/octet-stream")
                    .to_string();

                let data = field.bytes().await.map_err(|_| StatusCode::BAD_REQUEST)?;

                // Validate file type
                if !content_type.starts_with("image/") {
                    return Err(StatusCode::BAD_REQUEST);
                }

                file_data = Some((file_name, content_type, data));
            }
            Some("name") => {
                let data = field.bytes().await.map_err(|_| StatusCode::BAD_REQUEST)?;
                logo_name = Some(String::from_utf8_lossy(&data).to_string());
            }
            Some("description") => {
                let data = field.bytes().await.map_err(|_| StatusCode::BAD_REQUEST)?;
                let desc = String::from_utf8_lossy(&data).to_string();
                if !desc.trim().is_empty() {
                    logo_description = Some(desc);
                }
            }
            _ => {} // Ignore other fields
        }
    }

    // Ensure we have the required file data
    let (file_name, content_type, data) = file_data.ok_or(StatusCode::BAD_REQUEST)?;

    // Create the logo asset with proper name
    let create_request = LogoAssetCreateRequest {
        name: logo_name.unwrap_or(file_name.clone()),
        description: logo_description,
        asset_type: crate::models::logo_asset::LogoAssetType::Uploaded,
        source_url: None,
    };

    // For now, use a simplified upload approach until we implement the full conversion logic
    let asset_id = uuid::Uuid::new_v4();
    let file_extension = content_type.split('/').last().unwrap_or("img");

    match state
        .logo_asset_service
        .storage
        .save_uploaded_file(data.to_vec(), asset_id, file_extension)
        .await
    {
        Ok((file_name, file_path, file_size, mime_type, dimensions)) => {
            match state
                .logo_asset_service
                .create_asset_with_id(
                    asset_id,
                    create_request.name,
                    create_request.description,
                    file_name,
                    file_path,
                    file_size,
                    mime_type,
                    crate::models::logo_asset::LogoAssetType::Uploaded,
                    None, // source_url
                    dimensions.map(|(w, _)| w as i32),
                    dimensions.map(|(_, h)| h as i32),
                )
                .await
            {
                Ok(asset) => Ok(Json(LogoAssetUploadResponse {
                    id: asset.id,
                    name: asset.name,
                    file_name: asset.file_name,
                    file_size: asset.file_size,
                    url: format!("/api/v1/logos/{}", asset.id),
                })),
                Err(e) => {
                    error!("Failed to create logo asset: {}", e);
                    Err(StatusCode::INTERNAL_SERVER_ERROR)
                }
            }
        }
        Err(e) => {
            error!("Failed to save uploaded file: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

/// Get logo asset image bytes with preference for PNG format
/// Returns image bytes for /api/v1/logos/:id endpoint
#[utoipa::path(
    get,
    path = "/logos/{id}",
    tag = "logos",
    summary = "Get logo asset image",
    description = "Retrieve logo asset image data with preference for PNG format",
    params(
        ("id" = String, Path, description = "Logo asset ID (UUID)"),
    ),
    responses(
        (status = 200, description = "Logo asset image data"),
        (status = 404, description = "Logo asset not found"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_logo_asset_image(
    Path(id): Path<Uuid>,
    State(state): State<AppState>,
) -> Result<(axum::http::HeaderMap, Vec<u8>), StatusCode> {
    // Get the main asset first
    let main_asset = match state.logo_asset_service.get_asset(id).await {
        Ok(asset) => asset,
        Err(e) => {
            error!("Failed to get logo asset {}: {}", id, e);
            return Err(StatusCode::NOT_FOUND);
        }
    };

    // Use preference order: png > svg > webp > original
    let preference_order = ["png", "svg", "webp"];

    // Get all assets (main + linked)
    let mut all_assets = vec![main_asset.clone()];
    if let Ok(linked_assets) = state.logo_asset_service.get_linked_assets(id).await {
        for linked_asset in linked_assets {
            all_assets.push(linked_asset);
        }
    }

    // Try preferred formats in order
    for preferred_format in preference_order {
        for asset in &all_assets {
            if asset_matches_format(asset, preferred_format) {
                return serve_asset(&state, asset.clone()).await;
            }
        }
    }

    // Fall back to original asset
    serve_asset(&state, main_asset).await
}

/// Get logo asset in a specific format
/// Returns image bytes for /api/v1/logos/:id/formats/:format endpoint
pub async fn get_logo_asset_format(
    Path((id, format)): Path<(Uuid, String)>,
    State(state): State<AppState>,
) -> Result<(axum::http::HeaderMap, Vec<u8>), StatusCode> {
    // Get the main asset first
    let main_asset = match state.logo_asset_service.get_asset(id).await {
        Ok(asset) => asset,
        Err(e) => {
            error!("Failed to get logo asset {}: {}", id, e);
            return Err(StatusCode::NOT_FOUND);
        }
    };

    // First check if the main asset matches the requested format
    if asset_matches_format(&main_asset, &format) {
        return serve_asset(&state, main_asset).await;
    }

    // Look for linked assets with the requested format
    if let Ok(linked_assets) = state.logo_asset_service.get_linked_assets(id).await {
        for linked_asset in linked_assets {
            if asset_matches_format(&linked_asset, &format) {
                return serve_asset(&state, linked_asset).await;
            }
        }
    }

    // Requested format not found
    Err(StatusCode::NOT_FOUND)
}

fn asset_matches_format(asset: &crate::models::logo_asset::LogoAsset, format: &str) -> bool {
    match format.to_lowercase().as_str() {
        "png" => asset.mime_type.contains("png") || asset.file_name.ends_with(".png"),
        "jpg" | "jpeg" => {
            asset.mime_type.contains("jpeg")
                || asset.file_name.ends_with(".jpg")
                || asset.file_name.ends_with(".jpeg")
        }
        "svg" => asset.mime_type.contains("svg") || asset.file_name.ends_with(".svg"),
        "webp" => asset.mime_type.contains("webp") || asset.file_name.ends_with(".webp"),
        "gif" => asset.mime_type.contains("gif") || asset.file_name.ends_with(".gif"),
        _ => false,
    }
}

async fn serve_asset(
    state: &AppState,
    asset: crate::models::logo_asset::LogoAsset,
) -> Result<(axum::http::HeaderMap, Vec<u8>), StatusCode> {
    match state.logo_asset_storage.get_file(&asset.file_path).await {
        Ok(file_data) => {
            let mut headers = axum::http::HeaderMap::new();
            headers.insert(
                axum::http::header::CONTENT_TYPE,
                asset
                    .mime_type
                    .parse()
                    .unwrap_or_else(|_| "application/octet-stream".parse().unwrap()),
            );
            headers.insert(
                axum::http::header::CACHE_CONTROL,
                "public, max-age=86400".parse().unwrap(),
            );
            Ok((headers, file_data))
        }
        Err(e) => {
            error!("Failed to read logo file {}: {}", asset.file_path, e);
            Err(StatusCode::NOT_FOUND)
        }
    }
}

pub async fn update_logo_asset(
    Path(id): Path<Uuid>,
    State(state): State<AppState>,
    Json(payload): Json<crate::models::logo_asset::LogoAssetUpdateRequest>,
) -> Result<Json<crate::models::logo_asset::LogoAsset>, StatusCode> {
    match state
        .logo_asset_service
        .update_asset(id, payload.name, payload.description)
        .await
    {
        Ok(asset) => Ok(Json(asset)),
        Err(e) => {
            error!("Failed to update logo asset {}: {}", id, e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

pub async fn delete_logo_asset(
    Path(id): Path<Uuid>,
    State(state): State<AppState>,
) -> Result<StatusCode, StatusCode> {
    // Get asset first to get file path
    let asset = match state.logo_asset_service.get_asset(id).await {
        Ok(asset) => asset,
        Err(_) => return Err(StatusCode::NOT_FOUND),
    };

    // Get linked assets before deleting from database
    let linked_assets = match state.logo_asset_service.get_linked_assets(id).await {
        Ok(linked) => linked,
        Err(e) => {
            error!("Failed to get linked assets for {}: {}", id, e);
            Vec::new()
        }
    };

    // Delete from database
    match state.logo_asset_service.delete_asset(id).await {
        Ok(_) => {
            // Delete main asset file from storage
            if let Err(e) = state.logo_asset_storage.delete_file(&asset.file_path).await {
                error!("Failed to delete logo file {}: {}", asset.file_path, e);
            }

            // Delete linked asset files from storage
            for linked_asset in linked_assets {
                if let Err(e) = state
                    .logo_asset_storage
                    .delete_file(&linked_asset.file_path)
                    .await
                {
                    error!(
                        "Failed to delete linked logo file {}: {}",
                        linked_asset.file_path, e
                    );
                }
            }

            Ok(StatusCode::NO_CONTENT)
        }
        Err(e) => {
            error!("Failed to delete logo asset {}: {}", id, e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

/// Get cached logo asset by cache ID
/// This endpoint serves logos cached by the WASM plugin system using the sandboxed file manager
/// Normalized format: cache_id.png, with fallback to legacy formats
pub async fn get_cached_logo_asset(
    Path(cache_id): Path<String>,
    State(state): State<AppState>,
) -> Result<(axum::http::HeaderMap, Vec<u8>), StatusCode> {
    // The file manager's base is already the cached logo directory, so use direct paths
    let mut file_extension = "png".to_string();
    let file_data = match state
        .logo_file_manager
        .read(&format!("{}.png", cache_id))
        .await
    {
        Ok(data) => {
            debug!("Serving normalized cached logo: {}.png", cache_id);
            data
        }
        Err(_) => {
            // Fall back to legacy format with other extensions
            let mut found_data = None;
            for ext in &["jpg", "jpeg", "gif", "webp", "svg"] {
                match state
                    .logo_file_manager
                    .read(&format!("{}.{}", cache_id, ext))
                    .await
                {
                    Ok(data) => {
                        file_extension = ext.to_string();
                        found_data = Some(data);
                        debug!("Serving legacy cached logo: {}.{}", cache_id, ext);
                        break;
                    }
                    Err(_) => continue,
                }
            }
            found_data.ok_or_else(|| {
                debug!("Cached logo not found: {}", cache_id);
                StatusCode::NOT_FOUND
            })?
        }
    };

    // Determine content type from extension
    let content_type = match file_extension.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        _ => "application/octet-stream",
    };

    // Set response headers
    let mut headers = axum::http::HeaderMap::new();
    headers.insert(
        axum::http::header::CONTENT_TYPE,
        content_type.parse().unwrap(),
    );
    headers.insert(
        axum::http::header::CACHE_CONTROL,
        "public, max-age=2592000".parse().unwrap(), // 30 days cache
    );

    Ok((headers, file_data))
}

pub async fn health_check() -> Result<Json<serde_json::Value>, StatusCode> {
    Ok(Json(json!({
        "status": "healthy",
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "service": "m3u-proxy"
    })))
}

/// Refresh stream source
pub async fn refresh_stream_source(
    Path(id): Path<Uuid>,
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    match state.database.get_stream_source(id).await {
        Ok(Some(source)) => {
            tokio::spawn({
                let database = state.database.clone();
                let state_manager = state.state_manager.clone();
                async move {
                    use crate::ingestor::IngestorService;
                    use crate::ingestor::ProcessingTrigger;

                    let ingestor = IngestorService::new(state_manager);
                    let _ = ingestor
                        .refresh_stream_source(database, &source, ProcessingTrigger::Manual)
                        .await;
                }
            });

            Ok(Json(serde_json::json!({
                "success": true,
                "message": "Stream source refresh started",
                "source_id": id
            })))
        }
        Ok(None) => Ok(Json(serde_json::json!({
            "success": false,
            "message": "Stream source not found"
        }))),
        Err(e) => {
            error!("Failed to get stream source {}: {}", id, e);
            Ok(Json(serde_json::json!({
                "success": false,
                "message": "Failed to get stream source"
            })))
        }
    }
}

/// Cancel stream source ingestion
pub async fn cancel_stream_source_ingestion(
    Path(id): Path<Uuid>,
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    state.state_manager.cancel_ingestion(id).await;
    Ok(Json(json!({"message": "Ingestion cancelled"})))
}

/// Get stream source progress
pub async fn get_stream_source_progress(
    Path(id): Path<Uuid>,
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let progress = state.state_manager.get_progress(id).await;
    let processing_info = state.state_manager.get_processing_info(id).await;

    let result = serde_json::json!({
        "success": true,
        "message": "Stream source progress retrieved",
        "source_id": id,
        "progress": progress,
        "processing_info": processing_info
    });

    Ok(Json(result))
}

/// Get stream source processing info
pub async fn get_stream_source_processing_info(
    Path(id): Path<Uuid>,
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let info = state.state_manager.get_progress(id).await;
    Ok(Json(json!(info)))
}

/// Get stream source channels
pub async fn get_stream_source_channels(
    Path(id): Path<Uuid>,
    Query(params): Query<ChannelQueryParams>,
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let page = params.page.unwrap_or(1);
    let limit = params.limit.unwrap_or(50);
    let filter = params.filter.as_deref();

    match state
        .database
        .get_source_channels_paginated(id, page, limit, filter)
        .await
    {
        Ok(result) => Ok(Json(json!(result))),
        Err(e) => {
            error!("Failed to get channels for stream source {}: {}", id, e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

/// List EPG sources only

/// Refresh EPG source
pub async fn refresh_epg_source_unified(
    Path(id): Path<Uuid>,
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    match state.database.get_epg_source(id).await {
        Ok(Some(source)) => {
            tokio::spawn({
                let database = state.database.clone();
                let state_manager = state.state_manager.clone();
                async move {
                    use crate::ingestor::IngestorService;
                    use crate::ingestor::ProcessingTrigger;

                    let ingestor = IngestorService::new(state_manager);
                    let _ = ingestor
                        .ingest_epg_source(database, &source, ProcessingTrigger::Manual)
                        .await;
                }
            });

            Ok(Json(serde_json::json!({
                "success": true,
                "message": "EPG source refresh started",
                "source_id": id
            })))
        }
        Ok(None) => Ok(Json(serde_json::json!({
            "success": false,
            "message": "EPG source not found"
        }))),
        Err(e) => {
            error!("Failed to get EPG source {}: {}", id, e);
            Ok(Json(serde_json::json!({
                "success": false,
                "message": "Failed to get EPG source"
            })))
        }
    }
}

/// Get EPG source channels
pub async fn get_epg_source_channels_unified(
    Path(id): Path<Uuid>,
    Query(params): Query<ChannelQueryParams>,
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let _page = params.page.unwrap_or(1);
    let _limit = params.limit.unwrap_or(50);
    let _filter = params.filter;

    match state.database.get_epg_source_channels(id).await {
        Ok(result) => Ok(Json(json!(result))),
        Err(e) => {
            error!("Failed to get channels for EPG source {}: {}", id, e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

// Unified Sources API
pub async fn list_all_sources(
    State(state): State<AppState>,
) -> Result<Json<Vec<UnifiedSourceWithStats>>, StatusCode> {
    let mut unified_sources = Vec::new();

    // Get stream sources
    match state.database.list_stream_sources_with_stats().await {
        Ok(stream_sources) => {
            for stream_source in stream_sources {
                unified_sources.push(UnifiedSourceWithStats::from_stream(stream_source));
            }
        }
        Err(e) => {
            error!("Failed to list stream sources: {}", e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    }

    // Get EPG sources
    match state.database.list_epg_sources_with_stats().await {
        Ok(epg_sources) => {
            for epg_source in epg_sources {
                unified_sources.push(UnifiedSourceWithStats::from_epg(epg_source));
            }
        }
        Err(e) => {
            error!("Failed to list EPG sources: {}", e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    }

    // Sort by name for consistent ordering
    unified_sources.sort_by(|a, b| a.get_name().cmp(b.get_name()));

    Ok(Json(unified_sources))
}

/// Search logo assets
#[utoipa::path(
    get,
    path = "/logos/search",
    tag = "logos",
    summary = "Search logo assets",
    description = "Search for logo assets using various criteria",
    params(
        ("q" = Option<String>, Query, description = "Search query"),
        ("include_cached" = Option<bool>, Query, description = "Include cached logos"),
    ),
    responses(
        (status = 200, description = "Search results"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn search_logo_assets(
    Query(params): Query<crate::models::logo_asset::LogoAssetSearchRequest>,
    State(state): State<AppState>,
) -> Result<Json<crate::models::logo_asset::LogoAssetSearchResult>, StatusCode> {
    let include_cached = params.include_cached.unwrap_or(true); // Default to include cached

    if include_cached && state.logo_cache_scanner.is_some() {
        // Use the enhanced search with cached logos
        match state
            .logo_asset_service
            .search_assets_with_cached(
                params,
                &state.config.web.base_url,
                state.logo_cache_scanner.as_ref(),
                include_cached,
            )
            .await
        {
            Ok(result) => Ok(Json(result)),
            Err(e) => {
                error!("Failed to search logo assets with cached: {}", e);
                Err(StatusCode::INTERNAL_SERVER_ERROR)
            }
        }
    } else {
        // Fall back to database-only search
        match state
            .logo_asset_service
            .search_assets(params, &state.config.web.base_url)
            .await
        {
            Ok(result) => Ok(Json(result)),
            Err(e) => {
                error!("Failed to search logo assets: {}", e);
                Err(StatusCode::INTERNAL_SERVER_ERROR)
            }
        }
    }
}

pub async fn get_logo_asset_with_formats(
    Path(id): Path<Uuid>,
    State(state): State<AppState>,
) -> Result<Json<crate::models::logo_asset::LogoAssetWithLinked>, StatusCode> {
    match state.logo_asset_service.get_asset(id).await {
        Ok(asset) => {
            // Get linked assets (PNG conversions, etc.)
            let linked_assets = match state.logo_asset_service.get_linked_assets(id).await {
                Ok(linked) => linked,
                Err(e) => {
                    error!("Failed to get linked assets for {}: {}", id, e);
                    Vec::new()
                }
            };

            // Convert linked assets to LogoAssetWithUrl (using relative URLs for web UI)
            let linked_with_urls: Vec<crate::models::logo_asset::LogoAssetWithUrl> = linked_assets
                .into_iter()
                .map(|linked| crate::models::logo_asset::LogoAssetWithUrl {
                    url: crate::utils::logo::LogoUrlGenerator::relative(linked.id),
                    asset: linked,
                })
                .collect();

            // Build available formats list
            let mut available_formats = vec![
                asset
                    .mime_type
                    .split('/')
                    .last()
                    .unwrap_or("unknown")
                    .to_string(),
            ];
            for linked in &linked_with_urls {
                if let Some(format) = linked.asset.mime_type.split('/').last() {
                    if !available_formats.contains(&format.to_string()) {
                        available_formats.push(format.to_string());
                    }
                }
            }

            let response = crate::models::logo_asset::LogoAssetWithLinked {
                asset,
                url: crate::utils::logo::LogoUrlGenerator::relative(id),
                linked_assets: linked_with_urls,
                available_formats,
            };

            Ok(Json(response))
        }
        Err(_) => Err(StatusCode::NOT_FOUND),
    }
}

pub async fn get_logo_cache_stats(
    State(state): State<AppState>,
) -> Result<Json<crate::models::logo_asset::LogoCacheStats>, StatusCode> {
    match state
        .logo_asset_service
        .get_cache_stats_with_filesystem(state.logo_cache_scanner.as_ref())
        .await
    {
        Ok(stats) => Ok(Json(stats)),
        Err(e) => {
            error!("Failed to get logo cache stats: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

/// Generate missing metadata for cached logos
pub async fn generate_cached_logo_metadata(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    if let Some(scanner) = &state.logo_cache_scanner {
        match state
            .logo_asset_service
            .ensure_cached_logo_metadata(scanner)
            .await
        {
            Ok(generated_count) => Ok(Json(serde_json::json!({
                "success": true,
                "message": "Metadata generation completed",
                "generated_count": generated_count
            }))),
            Err(e) => {
                error!("Failed to generate cached logo metadata: {}", e);
                Err(StatusCode::INTERNAL_SERVER_ERROR)
            }
        }
    } else {
        error!("Logo cache scanner not available");
        Err(StatusCode::SERVICE_UNAVAILABLE)
    }
}

// EPG Sources API

pub async fn get_epg_viewer_data(
    Query(request): Query<EpgViewerRequest>,
    State(state): State<AppState>,
) -> Result<Json<EpgViewerResponse>, StatusCode> {
    // Parse datetime strings
    let start_time = match DateTime::parse_from_rfc3339(&request.start_time) {
        Ok(dt) => dt.with_timezone(&Utc),
        Err(_) => {
            error!("Invalid start_time format: {}", request.start_time);
            return Err(StatusCode::BAD_REQUEST);
        }
    };

    let end_time = match DateTime::parse_from_rfc3339(&request.end_time) {
        Ok(dt) => dt.with_timezone(&Utc),
        Err(_) => {
            error!("Invalid end_time format: {}", request.end_time);
            return Err(StatusCode::BAD_REQUEST);
        }
    };

    let parsed_request = EpgViewerRequestParsed {
        start_time,
        end_time,
        channel_filter: request.channel_filter,
        source_ids: request.source_ids,
    };

    match state
        .database
        .get_epg_data_for_viewer(&parsed_request)
        .await
    {
        Ok(response) => Ok(Json(response)),
        Err(e) => {
            error!("Failed to get EPG viewer data: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

// Linked Xtream Sources API
pub async fn list_linked_xtream_sources(
    State(state): State<AppState>,
) -> Result<Json<Vec<LinkedXtreamSources>>, StatusCode> {
    match state.database.list_linked_xtream_sources().await {
        Ok(sources) => Ok(Json(sources)),
        Err(e) => {
            error!("Failed to list linked Xtream sources: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

pub async fn create_linked_xtream_source(
    State(state): State<AppState>,
    Json(payload): Json<XtreamCodesCreateRequest>,
) -> Result<Json<XtreamCodesCreateResponse>, StatusCode> {
    match state.database.create_linked_xtream_sources(&payload).await {
        Ok(response) => {
            if response.success {
                // Invalidate scheduler cache since we may have added new sources
                let _ = state.cache_invalidation_tx.send(());
            }
            Ok(Json(response))
        }
        Err(e) => {
            error!("Failed to create linked Xtream source: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

pub async fn get_linked_xtream_source(
    Path(link_id): Path<String>,
    State(state): State<AppState>,
) -> Result<Json<LinkedXtreamSources>, StatusCode> {
    match state.database.get_linked_xtream_source(&link_id).await {
        Ok(Some(sources)) => Ok(Json(sources)),
        Ok(None) => Err(StatusCode::NOT_FOUND),
        Err(e) => {
            error!("Failed to get linked Xtream source {}: {}", link_id, e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

pub async fn update_linked_xtream_source(
    Path(link_id): Path<String>,
    State(state): State<AppState>,
    Json(payload): Json<XtreamCodesUpdateRequest>,
) -> Result<StatusCode, StatusCode> {
    match state
        .database
        .update_linked_xtream_sources(&link_id, &payload)
        .await
    {
        Ok(true) => {
            // Invalidate scheduler cache since sources were updated
            let _ = state.cache_invalidation_tx.send(());
            Ok(StatusCode::NO_CONTENT)
        }
        Ok(false) => Err(StatusCode::NOT_FOUND),
        Err(e) => {
            error!("Failed to update linked Xtream source {}: {}", link_id, e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

pub async fn delete_linked_xtream_source(
    Path(link_id): Path<String>,
    Query(params): Query<std::collections::HashMap<String, String>>,
    State(state): State<AppState>,
) -> Result<StatusCode, StatusCode> {
    let _delete_sources = params
        .get("delete_sources")
        .map(|s| s == "true")
        .unwrap_or(false);

    match state.database.delete_linked_xtream_sources(&link_id).await {
        Ok(true) => {
            // Invalidate scheduler cache since sources may have been deleted
            let _ = state.cache_invalidation_tx.send(());
            Ok(StatusCode::NO_CONTENT)
        }
        Ok(false) => Err(StatusCode::NOT_FOUND),
        Err(e) => {
            error!("Failed to delete linked Xtream source {}: {}", link_id, e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

// Channel Mapping API Endpoints
#[derive(Deserialize)]
pub struct CreateChannelMappingRequest {
    pub stream_channel_id: Uuid,
    pub epg_channel_id: Uuid,
    pub mapping_type: String,
}

#[derive(Deserialize)]
pub struct ChannelMappingQueryParams {
    pub stream_channel_id: Option<Uuid>,
    pub epg_channel_id: Option<Uuid>,
    pub mapping_type: Option<String>,
}

pub async fn list_channel_mappings(
    Query(params): Query<ChannelMappingQueryParams>,
    State(state): State<AppState>,
) -> Result<Json<Vec<ChannelEpgMapping>>, StatusCode> {
    match state
        .database
        .get_channel_mappings(
            params.stream_channel_id,
            params.epg_channel_id,
            params.mapping_type.as_deref(),
        )
        .await
    {
        Ok(mappings) => Ok(Json(mappings)),
        Err(e) => {
            error!("Failed to get channel mappings: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

pub async fn create_channel_mapping(
    State(state): State<AppState>,
    Json(request): Json<CreateChannelMappingRequest>,
) -> Result<Json<ChannelEpgMapping>, StatusCode> {
    let mapping_type = match request.mapping_type.as_str() {
        "manual" => EpgMappingType::Manual,
        "auto_name" => EpgMappingType::AutoName,
        "auto_tvg_id" => EpgMappingType::AutoTvgId,
        _ => return Err(StatusCode::BAD_REQUEST),
    };

    match state
        .database
        .create_channel_mapping(
            request.stream_channel_id,
            request.epg_channel_id,
            mapping_type,
        )
        .await
    {
        Ok(mapping) => {
            info!(
                "Created channel mapping: {} -> {}",
                request.stream_channel_id, request.epg_channel_id
            );
            Ok(Json(mapping))
        }
        Err(e) => {
            error!("Failed to create channel mapping: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

pub async fn delete_channel_mapping(
    Path(mapping_id): Path<Uuid>,
    State(state): State<AppState>,
) -> Result<StatusCode, StatusCode> {
    match state.database.delete_channel_mapping(mapping_id).await {
        Ok(true) => {
            info!("Deleted channel mapping: {}", mapping_id);
            Ok(StatusCode::NO_CONTENT)
        }
        Ok(false) => Err(StatusCode::NOT_FOUND),
        Err(e) => {
            error!("Failed to delete channel mapping: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

#[derive(Deserialize)]
pub struct AutoMapChannelsRequest {
    pub source_id: Option<Uuid>,
    pub mapping_type: String,
    pub dry_run: Option<bool>,
}

pub async fn auto_map_channels(
    State(state): State<AppState>,
    Json(request): Json<AutoMapChannelsRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let mapping_type = match request.mapping_type.as_str() {
        "name" => EpgMappingType::AutoName,
        "tvg_id" => EpgMappingType::AutoTvgId,
        _ => return Err(StatusCode::BAD_REQUEST),
    };

    let dry_run = request.dry_run.unwrap_or(false);

    match state
        .database
        .auto_map_channels(request.source_id, mapping_type, dry_run)
        .await
    {
        Ok(result) => {
            info!(
                "Auto-mapped channels: {} matches found, {} created",
                result.potential_matches, result.mappings_created
            );
            Ok(Json(serde_json::json!({
                "success": true,
                "potential_matches": result.potential_matches,
                "mappings_created": result.mappings_created,
                "dry_run": dry_run
            })))
        }
        Err(e) => {
            error!("Failed to auto-map channels: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

/// Count conditions in a condition tree
fn count_conditions_in_tree(tree: &crate::models::ConditionTree) -> usize {
    count_conditions_in_node(&tree.root)
}

/// Count conditions in a condition node recursively
fn count_conditions_in_node(node: &crate::models::ConditionNode) -> usize {
    match node {
        crate::models::ConditionNode::Condition { .. } => 1,
        crate::models::ConditionNode::Group { children, .. } => {
            children.iter().map(count_conditions_in_node).sum()
        }
    }
}

/// Generate JSON representation of condition tree for frontend rendering (shared by data mapping and filters)
pub fn generate_expression_tree_json(tree: &crate::models::ConditionTree) -> serde_json::Value {
    generate_condition_node_json(&tree.root)
}

/// Generate JSON representation of a condition node
fn generate_condition_node_json(node: &crate::models::ConditionNode) -> serde_json::Value {
    match node {
        crate::models::ConditionNode::Condition {
            field,
            operator,
            value,
            negate,
            case_sensitive,
        } => {
            json!({
                "type": "condition",
                "field": field,
                "operator": format!("{:?}", operator),
                "value": value,
                "negate": negate,
                "case_sensitive": case_sensitive
            })
        }
        crate::models::ConditionNode::Group { operator, children } => {
            let operator_str = match operator {
                crate::models::LogicalOperator::And => "AND",
                crate::models::LogicalOperator::Or => "OR",
            };

            json!({
                "type": "group",
                "operator": operator_str,
                "children": children.iter().map(|child| generate_condition_node_json(child)).collect::<Vec<_>>()
            })
        }
    }
}

/// Get progress for all sources (used by frontend polling)
pub async fn get_sources_progress(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    // Get progress for all active stream sources
    let stream_sources = match state.database.list_stream_sources().await {
        Ok(sources) => sources
            .into_iter()
            .filter(|s| s.is_active)
            .collect::<Vec<_>>(),
        Err(e) => {
            error!("Failed to list stream sources for progress: {}", e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    let mut sources_progress = Vec::new();
    for source in stream_sources {
        let progress = state.state_manager.get_progress(source.id).await;
        let processing_info = state.state_manager.get_processing_info(source.id).await;

        sources_progress.push(json!({
            "source_id": source.id,
            "source_name": source.name,
            "source_type": "stream",
            "progress": progress,
            "processing_info": processing_info
        }));
    }

    Ok(Json(json!({
        "success": true,
        "sources": sources_progress
    })))
}

/// Get progress for all EPG sources (used by frontend polling)
pub async fn get_epg_progress(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    // Get progress for all active EPG sources
    let epg_sources = match state.database.list_epg_sources().await {
        Ok(sources) => sources
            .into_iter()
            .filter(|s| s.is_active)
            .collect::<Vec<_>>(),
        Err(e) => {
            error!("Failed to list EPG sources for progress: {}", e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    let mut sources_progress = Vec::new();
    for source in epg_sources {
        let progress = state.state_manager.get_progress(source.id).await;
        let processing_info = state.state_manager.get_processing_info(source.id).await;

        sources_progress.push(json!({
            "source_id": source.id,
            "source_name": source.name,
            "source_type": "epg",
            "progress": progress,
            "processing_info": processing_info
        }));
    }

    Ok(Json(json!({
        "success": true,
        "sources": sources_progress
    })))
}

/// Get stream fields for data mapping
pub async fn get_stream_fields(
    _state: State<AppState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    Ok(Json(json!({
        "fields": [
            {"name": "channel_name", "type": "string", "description": "Channel name"},
            {"name": "tvg_id", "type": "string", "description": "TVG ID"},
            {"name": "tvg_name", "type": "string", "description": "TVG name"},
            {"name": "tvg_logo", "type": "string", "description": "TVG logo URL"},
            {"name": "tvg_shift", "type": "string", "description": "TVG time shift"},
            {"name": "group_title", "type": "string", "description": "Group title"},
            {"name": "url", "type": "string", "description": "Stream URL"}
        ]
    })))
}

/// Get EPG fields for data mapping
pub async fn get_epg_fields(
    _state: State<AppState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    Ok(Json(json!({
        "fields": [
            {"name": "channel_id", "type": "string", "description": "Channel ID"},
            {"name": "channel_name", "type": "string", "description": "Channel display name"},
            {"name": "programme_title", "type": "string", "description": "Programme title"},
            {"name": "programme_description", "type": "string", "description": "Programme description"},
            {"name": "start_time", "type": "datetime", "description": "Programme start time"},
            {"name": "end_time", "type": "datetime", "description": "Programme end time"}
        ]
    })))
}

/// Preview proxies (placeholder implementation)
pub async fn preview_proxies(
    _state: State<AppState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    Ok(Json(json!({
        "success": true,
        "proxies": [],
        "message": "Proxy preview not yet implemented"
    })))
}

/// Regenerate all active proxies
pub async fn regenerate_all_proxies(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    info!("Starting regeneration of all active proxies");

    // Get all active proxies
    let active_proxies = match state.database.get_all_active_proxies().await {
        Ok(proxies) => proxies,
        Err(e) => {
            error!("Failed to get active proxies: {}", e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    if active_proxies.is_empty() {
        return Ok(Json(json!({
            "success": true,
            "message": "No active proxies found to regenerate",
            "count": 0
        })));
    }

    info!(
        "Found {} active proxies to regenerate",
        active_proxies.len()
    );

    // Create repositories for dependency injection
    use crate::repositories::{
        FilterRepository, StreamProxyRepository, StreamSourceRepository,
    };
    let proxy_repo = StreamProxyRepository::new(state.database.pool());
    let stream_source_repo = StreamSourceRepository::new(state.database.pool());
    let filter_repo = FilterRepository::new(state.database.pool());

    let config_resolver = crate::proxy::config_resolver::ProxyConfigResolver::new(
        proxy_repo,
        stream_source_repo,
        filter_repo,
        state.database.clone(),
    );

    let mut successful_count = 0;
    let mut failed_count = 0;
    let mut errors = Vec::new();

    // Regenerate each proxy using ProxyService
    for proxy in active_proxies.iter() {
        info!("Regenerating proxy '{}'", proxy.name);

        // Resolve proxy configuration
        match config_resolver.resolve_config(proxy.id).await {
            Ok(resolved_config) => {
                // Validate configuration
                if let Err(e) = config_resolver.validate_config(&resolved_config) {
                    error!("Invalid proxy configuration for {}: {}", proxy.id, e);
                    failed_count += 1;
                    errors.push(format!("Proxy '{}': Invalid configuration: {}", proxy.name, e));
                    continue;
                }

                // Create production output destination
                let output = crate::models::GenerationOutput::InMemory;

                // Generate using dependency injection
                match state.proxy_service
                    .generate_proxy_with_config(
                        resolved_config,
                        output,
                        &state.database,
                        &state.data_mapping_service,
                        &state.logo_asset_service,
                        &state.config.web.base_url,
                        state.config.data_mapping_engine.clone(),
                        &state.config,
                    )
                    .await
                {
                    Ok(generation) => {
                        // Save the M3U file using the proxy service
                        match state.proxy_service
                            .save_m3u_file_with_manager(
                                &proxy.id.to_string(),
                                &generation.m3u_content,
                                None,
                            )
                            .await
                        {
                            Ok(_) => {
                                // Update the last_generated_at timestamp for the proxy
                                if let Err(e) = state.database.update_proxy_last_generated(proxy.id).await {
                                    error!("Failed to update last_generated_at for proxy {}: {}", proxy.id, e);
                                }
                                
                                info!(
                                    "Successfully regenerated proxy '{}' with {} channels using ProxyService",
                                    proxy.name, generation.channel_count
                                );
                                successful_count += 1;
                            }
                            Err(e) => {
                                error!(
                                    "Failed to save regenerated M3U for proxy '{}': {}",
                                    proxy.name, e
                                );
                                failed_count += 1;
                                errors.push(format!("Proxy '{}': {}", proxy.name, e));
                            }
                        }
                    }
                    Err(e) => {
                        error!(
                            "Failed to regenerate proxy '{}' using ProxyService: {}",
                            proxy.name, e
                        );
                        failed_count += 1;
                        errors.push(format!("Proxy '{}': {}", proxy.name, e));
                    }
                }
            }
            Err(e) => {
                error!(
                    "Failed to resolve proxy configuration for '{}': {}",
                    proxy.name, e
                );
                failed_count += 1;
                errors.push(format!("Proxy '{}': Config resolution failed: {}", proxy.name, e));
            }
        }
    }

    let total_count = active_proxies.len();
    let message = if failed_count == 0 {
        format!(
            "Successfully regenerated all {} active proxies",
            successful_count
        )
    } else if successful_count == 0 {
        format!("Failed to regenerate all {} proxies", failed_count)
    } else {
        format!(
            "Regenerated {}/{} proxies ({} succeeded, {} failed)",
            successful_count, total_count, successful_count, failed_count
        )
    };

    info!("{}", message);

    Ok(Json(json!({
        "success": failed_count == 0,
        "message": message,
        "count": successful_count,
        "total": total_count,
        "failed": failed_count,
        "errors": if errors.is_empty() { serde_json::Value::Null } else { json!(errors) }
    })))
}

// Relay Configuration API Endpoints - implemented in web/api/relay.rs

// Metrics API structures
#[derive(Debug, Serialize)]
pub struct DashboardMetrics {
    pub total_channels: u64,
    pub active_streams: u64,
    pub active_clients: u64,
    pub total_proxies: u64,
    pub system_uptime: String,
    pub bytes_served_today: u64,
    pub popular_channels: Vec<PopularChannel>,
}

#[derive(Debug, Serialize)]
pub struct PopularChannel {
    pub channel_id: String,
    pub channel_name: String,
    pub session_count: u64,
    pub bytes_served: u64,
}

#[derive(Debug, Serialize)]
pub struct RealtimeMetrics {
    pub active_sessions: u64,
    pub active_clients: u64,
    pub bytes_per_second: u64,
    pub proxy_breakdown: Vec<ProxyMetrics>,
}

#[derive(Debug, Serialize)]
pub struct ProxyMetrics {
    pub proxy_id: String,
    pub proxy_name: String,
    pub active_sessions: u64,
    pub active_clients: u64,
    pub bytes_per_second: u64,
}

#[derive(Debug, Serialize)]
pub struct UsageMetrics {
    pub hourly_sessions: Vec<HourlyUsage>,
    pub daily_sessions: Vec<DailyUsage>,
}

#[derive(Debug, Serialize)]
pub struct HourlyUsage {
    pub hour: String,
    pub total_sessions: u64,
    pub unique_clients: u64,
    pub bytes_served: u64,
}

#[derive(Debug, Serialize)]
pub struct DailyUsage {
    pub date: String,
    pub total_sessions: u64,
    pub unique_clients: u64,
    pub bytes_served: u64,
}

#[derive(Debug, Deserialize)]
pub struct UsageQuery {
    pub proxy_id: Option<String>,
    pub days: Option<u32>,
    pub hours: Option<u32>,
}

#[utoipa::path(
    get,
    path = "/dashboard/metrics",
    tag = "metrics",
    summary = "Get dashboard metrics",
    description = "Retrieve overview metrics for the dashboard",
    responses(
        (status = 200, description = "Dashboard metrics overview"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_dashboard_metrics(
    State(state): State<AppState>,
) -> Result<Json<DashboardMetrics>, StatusCode> {
    let db = &state.database.pool();

    // Get total channels across all proxies
    let total_channels = match sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM channels")
        .fetch_one(db)
        .await
    {
        Ok(count) => count as u64,
        Err(e) => {
            error!("Failed to get total channels: {}", e);
            0
        }
    };

    // Get total proxies
    let total_proxies = match sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM stream_proxies")
        .fetch_one(db)
        .await
    {
        Ok(count) => count as u64,
        Err(e) => {
            error!("Failed to get total proxies: {}", e);
            0
        }
    };

    Ok(Json(DashboardMetrics {
        total_channels,
        active_streams: 0,
        active_clients: 0,
        total_proxies,
        system_uptime: "99.8%".to_string(),
        bytes_served_today: 0,
        popular_channels: Vec::new(),
    }))
}

#[utoipa::path(
    get,
    path = "/metrics/realtime",
    tag = "metrics",
    summary = "Get real-time metrics",
    description = "Retrieve real-time performance metrics",
    responses(
        (status = 200, description = "Real-time metrics data"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_realtime_metrics(
    State(_state): State<AppState>,
) -> Result<Json<RealtimeMetrics>, StatusCode> {
    Ok(Json(RealtimeMetrics {
        active_sessions: 0,
        active_clients: 0,
        bytes_per_second: 0,
        proxy_breakdown: Vec::new(),
    }))
}

#[utoipa::path(
    get,
    path = "/metrics/usage",
    tag = "metrics",
    summary = "Get usage metrics",
    description = "Retrieve usage metrics over time",
    params(
        ("days" = Option<u32>, Query, description = "Number of days to include"),
        ("hours" = Option<u32>, Query, description = "Number of hours to include"),
    ),
    responses(
        (status = 200, description = "Usage metrics data"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_usage_metrics(
    Query(_params): Query<UsageQuery>,
    State(_state): State<AppState>,
) -> Result<Json<UsageMetrics>, StatusCode> {
    Ok(Json(UsageMetrics {
        hourly_sessions: Vec::new(),
        daily_sessions: Vec::new(),
    }))
}

/// Get popular channels
pub async fn get_popular_channels(
    Query(_params): Query<UsageQuery>,
    State(_state): State<AppState>,
) -> Result<Json<Vec<PopularChannel>>, StatusCode> {
    Ok(Json(Vec::new()))
}

