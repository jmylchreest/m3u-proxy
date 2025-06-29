use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::Json,
};
use chrono::{DateTime, Utc};
use std::collections::HashMap;

use serde_json::json;
use tracing::{error, info};
use uuid::Uuid;

use super::AppState;

use crate::data_mapping::DataMappingService;
use crate::models::*;
use crate::proxy::generator::ProxyGenerator;
use serde::Deserialize;

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
        "mapped_group_title": mc.mapped_group_title
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

// Stream Proxies API
pub async fn list_proxies(
    State(_state): State<AppState>,
) -> Result<Json<Vec<StreamProxy>>, StatusCode> {
    // TODO: Implement list proxies
    Ok(Json(vec![]))
}

pub async fn create_proxy(
    State(_state): State<AppState>,
    Json(_payload): Json<StreamProxy>,
) -> Result<Json<StreamProxy>, StatusCode> {
    // TODO: Implement create proxy
    Err(StatusCode::NOT_IMPLEMENTED)
}

pub async fn get_proxy(
    Path(_id): Path<Uuid>,
    State(_state): State<AppState>,
) -> Result<Json<StreamProxy>, StatusCode> {
    // TODO: Implement get proxy
    Err(StatusCode::NOT_FOUND)
}

pub async fn update_proxy(
    Path(_id): Path<Uuid>,
    State(_state): State<AppState>,
    Json(_payload): Json<StreamProxy>,
) -> Result<Json<StreamProxy>, StatusCode> {
    // TODO: Implement update proxy
    Err(StatusCode::NOT_IMPLEMENTED)
}

pub async fn delete_proxy(
    Path(_id): Path<Uuid>,
    State(_state): State<AppState>,
) -> Result<StatusCode, StatusCode> {
    // TODO: Implement delete proxy
    Err(StatusCode::NOT_IMPLEMENTED)
}

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

    // Use the existing proxy generator to regenerate with current data mapping rules
    let proxy_generator = ProxyGenerator::new(state.config.storage.clone());

    // Generate the updated proxy
    match proxy_generator
        .generate(
            &proxy,
            &state.database,
            &state.data_mapping_service,
            &state.logo_asset_service,
            &state.config.web.base_url,
            state.config.data_mapping_engine.clone(),
        )
        .await
    {
        Ok(generation) => {
            // Save the new M3U file
            match proxy_generator
                .save_m3u_file(proxy.id, &generation.m3u_content)
                .await
            {
                Ok(_) => {
                    info!(
                        "Successfully regenerated proxy '{}' with {} channels",
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
            error!("Failed to regenerate proxy {}: {}", proxy_id, e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

// Filters API
pub async fn list_filters(
    State(state): State<AppState>,
) -> Result<Json<Vec<FilterWithUsage>>, StatusCode> {
    match state.database.get_filters_with_usage().await {
        Ok(filters) => Ok(Json(filters)),
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

pub async fn get_filter(
    Path(id): Path<Uuid>,
    State(state): State<AppState>,
) -> Result<Json<Filter>, StatusCode> {
    match state.database.get_filter(id).await {
        Ok(Some(filter)) => Ok(Json(filter)),
        Ok(None) => Err(StatusCode::NOT_FOUND),
        Err(e) => {
            error!("Failed to get filter {}: {}", id, e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

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
pub async fn list_data_mapping_rules(
    State(state): State<AppState>,
) -> Result<Json<Vec<crate::models::data_mapping::DataMappingRuleWithDetails>>, StatusCode> {
    match state.data_mapping_service.get_all_rules().await {
        Ok(rules) => Ok(Json(rules)),
        Err(e) => {
            error!("Failed to list data mapping rules: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

pub async fn create_data_mapping_rule(
    State(state): State<AppState>,
    Json(payload): Json<crate::models::data_mapping::DataMappingRuleCreateRequest>,
) -> Result<Json<crate::models::data_mapping::DataMappingRuleWithDetails>, StatusCode> {
    match state.data_mapping_service.create_rule(payload).await {
        Ok(rule) => Ok(Json(rule)),
        Err(e) => {
            error!("Failed to create data mapping rule: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

pub async fn get_data_mapping_rule(
    Path(id): Path<Uuid>,
    State(state): State<AppState>,
) -> Result<Json<crate::models::data_mapping::DataMappingRuleWithDetails>, StatusCode> {
    match state.data_mapping_service.get_rule_with_details(id).await {
        Ok(rule) => Ok(Json(rule)),
        Err(e) => {
            error!("Failed to get data mapping rule {}: {}", id, e);
            Err(StatusCode::NOT_FOUND)
        }
    }
}

pub async fn update_data_mapping_rule(
    Path(id): Path<Uuid>,
    State(state): State<AppState>,
    Json(payload): Json<crate::models::data_mapping::DataMappingRuleUpdateRequest>,
) -> Result<Json<crate::models::data_mapping::DataMappingRuleWithDetails>, StatusCode> {
    match state.data_mapping_service.update_rule(id, payload).await {
        Ok(rule) => Ok(Json(rule)),
        Err(e) => {
            error!("Failed to update data mapping rule {}: {}", id, e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

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

pub async fn test_data_mapping_rule(
    State(state): State<AppState>,
    Json(payload): Json<crate::models::data_mapping::DataMappingTestRequest>,
) -> Result<Json<crate::models::data_mapping::DataMappingTestResult>, StatusCode> {
    use crate::data_mapping::DataMappingEngine;
    use crate::filter_parser::FilterParser;

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

    // Parse the expression to get conditions and actions
    let available_fields = match payload.source_type {
        crate::models::data_mapping::DataMappingSourceType::Stream => {
            crate::models::data_mapping::StreamMappingFields::available_fields()
                .into_iter()
                .map(|s| s.to_string())
                .collect()
        }
        crate::models::data_mapping::DataMappingSourceType::Epg => {
            crate::models::data_mapping::EpgMappingFields::available_fields()
                .into_iter()
                .map(|s| s.to_string())
                .collect()
        }
    };

    let parser = FilterParser::new().with_fields(available_fields);
    let parsed_expression = match parser.parse_extended(&payload.expression) {
        Ok(expr) => expr,
        Err(e) => {
            return Ok(Json(crate::models::data_mapping::DataMappingTestResult {
                is_valid: false,
                error: Some(format!("Expression parsing error: {}", e)),
                matching_channels: vec![],
                total_channels: total_channels_count,
                matched_count: 0,
            }));
        }
    };

    // For testing, create temporary conditions and actions from the parsed expression
    let (conditions, actions) = match parsed_expression {
        crate::models::ExtendedExpression::ConditionOnly(_) => {
            // No actions to test
            return Ok(Json(crate::models::data_mapping::DataMappingTestResult {
                is_valid: true,
                error: None,
                matching_channels: vec![],
                total_channels: total_channels_count,
                matched_count: 0,
            }));
        }
        crate::models::ExtendedExpression::ConditionWithActions { condition, actions } => {
            // Convert condition tree to flat conditions for testing
            let flat_conditions = flatten_condition_tree_for_test(&condition);
            let test_actions = convert_actions_for_test(&actions);
            (flat_conditions, test_actions)
        }
        crate::models::ExtendedExpression::ConditionalActionGroups(groups) => {
            // For testing, use the first group
            if let Some(first_group) = groups.first() {
                let flat_conditions = flatten_condition_tree_for_test(&first_group.conditions);
                let test_actions = convert_actions_for_test(&first_group.actions);
                (flat_conditions, test_actions)
            } else {
                return Ok(Json(crate::models::data_mapping::DataMappingTestResult {
                    is_valid: false,
                    error: Some("No conditional action groups found".to_string()),
                    matching_channels: vec![],
                    total_channels: total_channels_count,
                    matched_count: 0,
                }));
            }
        }
    };

    // Convert to internal types for testing
    let test_conditions: Vec<crate::models::data_mapping::DataMappingCondition> = conditions
        .into_iter()
        .enumerate()
        .map(|(i, c)| crate::models::data_mapping::DataMappingCondition {
            id: Uuid::new_v4(),
            rule_id: Uuid::new_v4(),
            field_name: c.field_name,
            operator: c.operator,
            value: c.value,
            logical_operator: c.logical_operator,
            sort_order: i as i32,
            created_at: chrono::Utc::now(),
        })
        .collect();

    let test_actions: Vec<crate::models::data_mapping::DataMappingAction> = actions
        .into_iter()
        .enumerate()
        .map(|(i, a)| crate::models::data_mapping::DataMappingAction {
            id: Uuid::new_v4(),
            rule_id: Uuid::new_v4(),
            action_type: a.action_type,
            target_field: a.target_field,
            value: a.value,
            logo_asset_id: a.logo_asset_id,
            timeshift_minutes: a.timeshift_minutes,
            sort_order: i as i32,
            created_at: chrono::Utc::now(),
        })
        .collect();

    // Get logo assets for the actions (including those from @logo: references)
    let mut logo_assets = HashMap::new();
    for action in &test_actions {
        if let Some(logo_id) = action.logo_asset_id {
            if let Ok(asset) = state.logo_asset_service.get_asset(logo_id).await {
                logo_assets.insert(logo_id, asset);
            }
        }
        // Also check for @logo: references in values
        if let Some(value) = &action.value {
            if value.starts_with("@logo:") {
                if let Ok(logo_uuid) = uuid::Uuid::parse_str(&value[6..]) {
                    if let Ok(asset) = state.logo_asset_service.get_asset(logo_uuid).await {
                        logo_assets.insert(logo_uuid, asset);
                    }
                }
            }
        }
    }

    let mut engine = DataMappingEngine::new();
    match engine.test_mapping_rule(
        channels,
        test_conditions.clone(),
        test_actions.clone(),
        logo_assets.clone(),
        &state.config.web.base_url,
    ) {
        Ok(mapped_channels) => {
            let test_channels: Vec<crate::models::data_mapping::DataMappingTestChannel> =
                mapped_channels
                    .into_iter()
                    .map(|mc| {
                        let (original_values, mapped_values) = mapped_channel_to_test_format(&mc);

                        // Create meaningful action descriptions with resolved values
                        let applied_actions = if mc.applied_rules.is_empty() {
                            vec![]
                        } else {
                            test_actions.iter().map(|action| {
                                match action.action_type {
                                    crate::models::data_mapping::DataMappingActionType::SetValue => {
                                        let template = action.value.as_deref().unwrap_or("");
                                        let resolved_value = get_resolved_value(&mc, &action.target_field);

                                        // Check if template contains @logo: reference
                                        let display_template = if template.starts_with("@logo:") {
                                            if let Ok(logo_uuid) = uuid::Uuid::parse_str(&template[6..]) {
                                                if let Some(logo_asset) = logo_assets.get(&logo_uuid) {
                                                    let logo_url = crate::utils::generate_logo_url(&state.config.web.base_url, logo_uuid);
                                                    format!("@logo:{} ({})", &logo_asset.name, logo_url)
                                                } else {
                                                    format!("{} (logo not found)", template)
                                                }
                                            } else {
                                                template.to_string()
                                            }
                                        } else {
                                            template.to_string()
                                        };

                                        if template.contains('$') && resolved_value.is_some() {
                                            format!("Set {} = {} ('{}')", action.target_field, display_template, resolved_value.unwrap())
                                        } else {
                                            format!("Set {} = {}", action.target_field, display_template)
                                        }
                                    },
                                    crate::models::data_mapping::DataMappingActionType::SetDefaultIfEmpty => {
                                        let template = action.value.as_deref().unwrap_or("");
                                        let resolved_value = get_resolved_value(&mc, &action.target_field);

                                        // Check if template contains @logo: reference
                                        let display_template = if template.starts_with("@logo:") {
                                            if let Ok(logo_uuid) = uuid::Uuid::parse_str(&template[6..]) {
                                                if let Some(logo_asset) = logo_assets.get(&logo_uuid) {
                                                    let logo_url = crate::utils::generate_logo_url(&state.config.web.base_url, logo_uuid);
                                                    format!("@logo:{} ({})", &logo_asset.name, logo_url)
                                                } else {
                                                    format!("{} (logo not found)", template)
                                                }
                                            } else {
                                                template.to_string()
                                            }
                                        } else {
                                            template.to_string()
                                        };

                                        if template.contains('$') && resolved_value.is_some() {
                                            format!("Set default {} = {} ('{}')", action.target_field, display_template, resolved_value.unwrap())
                                        } else {
                                            format!("Set default {} = {}", action.target_field, display_template)
                                        }
                                    },
                                    crate::models::data_mapping::DataMappingActionType::SetLogo => {
                                        if let Some(logo_id) = action.logo_asset_id {
                                            if let Some(logo_asset) = logo_assets.get(&logo_id) {
                                                let logo_url = crate::utils::generate_logo_url(&state.config.web.base_url, logo_id);
                                                format!("Set logo for {} = {} ({})", action.target_field, logo_asset.name, logo_url)
                                            } else {
                                                format!("Set logo for {} (logo not found)", action.target_field)
                                            }
                                        } else {
                                            format!("Set logo for {}", action.target_field)
                                        }
                                    },
                                    crate::models::data_mapping::DataMappingActionType::TimeshiftEpg => {
                                        format!("Timeshift EPG by {} minutes", action.timeshift_minutes.unwrap_or(0))
                                    },
                                    crate::models::data_mapping::DataMappingActionType::DeduplicateStreamUrls => {
                                        "Deduplicate stream URLs".to_string()
                                    },
                                    crate::models::data_mapping::DataMappingActionType::RemoveChannel => {
                                        "Remove channel".to_string()
                                    },
                                }
                            }).collect()
                        };

                        crate::models::data_mapping::DataMappingTestChannel {
                            channel_name: mc.original.channel_name,
                            group_title: mc.original.group_title,
                            original_values,
                            mapped_values,
                            applied_actions,
                        }
                    })
                    .collect();

            let result = crate::models::data_mapping::DataMappingTestResult {
                is_valid: true,
                error: None,
                matching_channels: test_channels.clone(),
                total_channels: total_channels_count,
                matched_count: test_channels.len(),
            };

            Ok(Json(result))
        }
        Err(e) => {
            let result = crate::models::data_mapping::DataMappingTestResult {
                is_valid: false,
                error: Some(e.to_string()),
                matching_channels: vec![],
                total_channels: total_channels_count,
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
                r.rule.is_active
                    && r.rule.source_type
                        == crate::models::data_mapping::DataMappingSourceType::Stream
            })
            .map(|rule| {
                let affected_count = modified_channels
                    .iter()
                    .filter(|mc| mc.applied_rules.contains(&rule.rule.id))
                    .count();
                let condition_count = rule.conditions.len();
                let action_count = rule.actions.len();

                // Get performance stats for this rule
                let (total_execution_time, avg_execution_time) = rule_performance
                    .get(&rule.rule.id)
                    .map(|(total_micros, avg_micros, _)| (*total_micros, *avg_micros))
                    .unwrap_or((0, 0));

                // Debug logging for performance data
                info!(
                    "Looking up rule '{}' (ID: {}) in performance data...",
                    rule.rule.name, rule.rule.id
                );
                info!(
                    "Rule '{}' (ID: {}): performance stats lookup - found: {}, total_time: {}μs, avg_time: {}μs",
                    rule.rule.name,
                    rule.rule.id,
                    rule_performance.contains_key(&rule.rule.id),
                    total_execution_time,
                    avg_execution_time
                );
                if !rule_performance.contains_key(&rule.rule.id) {
                    info!("Available IDs in performance data: {:?}", rule_performance.keys().collect::<Vec<_>>());
                }

                serde_json::json!({
                    "rule_id": rule.rule.id,
                    "rule_name": rule.rule.name,
                    "rule_description": rule.rule.description,
                    "affected_channels_count": affected_count,
                    "condition_count": condition_count,
                    "action_count": action_count,
                    "avg_execution_time": avg_execution_time,
                    "total_execution_time": total_execution_time,
                    "sort_order": rule.rule.sort_order,
                    "conditions": rule.conditions.iter().map(|c| {
                        serde_json::json!({
                            "field": c.field_name,
                            "operator": format!("{:?}", c.operator),
                            "value": c.value
                        })
                    }).collect::<Vec<_>>(),
                    "actions": rule.actions.iter().map(|a| {
                        serde_json::json!({
                            "field": a.target_field,
                            "value": a.value
                        })
                    }).collect::<Vec<_>>()
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
                        let entry = combined_performance_data.entry(rule_id).or_insert((0u128, 0u128, 0usize));
                        entry.0 += total_time; // Sum total execution times
                        entry.1 = if entry.2 + processed_count > 0 {
                            (entry.1 * entry.2 as u128 + avg_time * processed_count as u128) / (entry.2 + processed_count) as u128
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
                        r.rule.is_active
                            && r.rule.source_type
                                == crate::models::data_mapping::DataMappingSourceType::Stream
                    })
                    .map(|rule| {
                        let affected_count = limited_channels
                            .iter()
                            .filter(|mc| mc.applied_rules.contains(&rule.rule.id))
                            .count();
                        let condition_count = rule.conditions.len();
                        let action_count = rule.actions.len();

                        // Get actual performance data for this rule
                        let (total_execution_time, avg_execution_time, _processed_count) = 
                            combined_performance_data.get(&rule.rule.id)
                                .map(|(total, avg, count)| (*total, *avg, *count))
                                .unwrap_or((0, 0, 0));

                        serde_json::json!({
                                "rule_id": rule.rule.id,
                                "rule_name": rule.rule.name,
                                "rule_description": rule.rule.description,
                                "affected_channels_count": affected_count,
                                "condition_count": condition_count,
                                "action_count": action_count,
                                "avg_execution_time": avg_execution_time,
                        "total_execution_time": total_execution_time,
                                "sort_order": rule.rule.sort_order,
                                "conditions": rule.conditions.iter().map(|c| {
                                    serde_json::json!({
                                        "field": c.field_name,
                                        "operator": format!("{:?}", c.operator),
                                        "value": c.value
                                    })
                                }).collect::<Vec<_>>(),
                                "actions": rule.actions.iter().map(|a| {
                                    serde_json::json!({
                                        "field": a.target_field,
                                        "value": a.value
                                    })
                                }).collect::<Vec<_>>()
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
pub async fn list_logo_assets(
    Query(params): Query<crate::models::logo_asset::LogoAssetListRequest>,
    State(state): State<AppState>,
) -> Result<Json<crate::models::logo_asset::LogoAssetListResponse>, StatusCode> {
    match state
        .logo_asset_service
        .list_assets(params, &state.config.web.base_url)
        .await
    {
        Ok(response) => Ok(Json(response)),
        Err(e) => {
            error!("Failed to list logo assets: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

pub async fn upload_logo_asset(
    State(state): State<AppState>,
    mut multipart: axum::extract::Multipart,
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
    };

    match state
        .logo_asset_service
        .create_asset_from_upload_with_conversion(create_request, &data, &content_type)
        .await
    {
        Ok(asset) => Ok(Json(LogoAssetUploadResponse {
            id: asset.id,
            name: asset.name,
            file_name: asset.file_name,
            file_size: asset.file_size,
            url: format!("/logos/{}", asset.id),
        })),
        Err(e) => {
            error!("Failed to create logo asset: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

pub async fn get_logo_asset(
    Path(id): Path<Uuid>,
    Query(params): Query<std::collections::HashMap<String, String>>,
    State(state): State<AppState>,
) -> Result<(axum::http::HeaderMap, Vec<u8>), StatusCode> {
    let requested_format = params.get("format").map(|s| s.as_str());

    // Get the main asset first
    let main_asset = match state.logo_asset_service.get_asset(id).await {
        Ok(asset) => asset,
        Err(e) => {
            error!("Failed to get logo asset {}: {}", id, e);
            return Err(StatusCode::NOT_FOUND);
        }
    };

    // If a specific format is requested, try to find that format
    if let Some(format) = requested_format {
        // First check if the main asset matches the requested format
        if asset_matches_format(&main_asset, format) {
            return serve_asset(&state, main_asset).await;
        }

        // Look for linked assets with the requested format
        if let Ok(linked_assets) = state.logo_asset_service.get_linked_assets(id).await {
            for linked_asset in linked_assets {
                if asset_matches_format(&linked_asset, format) {
                    return serve_asset(&state, linked_asset).await;
                }
            }
        }

        // Requested format not found
        return Err(StatusCode::NOT_FOUND);
    }

    // No specific format requested - use preference order: png > svg > webp > original
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
    match state.logo_asset_service.update_asset(id, payload).await {
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

pub async fn health_check() -> Result<Json<serde_json::Value>, StatusCode> {
    Ok(Json(json!({
        "status": "healthy",
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "service": "m3u-proxy"
    })))
}

/// List stream sources only
pub async fn list_stream_sources(
    State(state): State<AppState>,
) -> Result<Json<Vec<UnifiedSourceWithStats>>, StatusCode> {
    match state.database.list_stream_sources_with_stats().await {
        Ok(sources) => {
            let unified_sources: Vec<UnifiedSourceWithStats> = sources
                .into_iter()
                .map(UnifiedSourceWithStats::from_stream)
                .collect();
            Ok(Json(unified_sources))
        }
        Err(e) => {
            error!("Failed to list stream sources: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

/// Create stream source
pub async fn create_stream_source(
    State(state): State<AppState>,
    Json(payload): Json<StreamSourceCreateRequest>,
) -> Result<Json<UnifiedSourceWithStats>, StatusCode> {
    match state.database.create_stream_source(&payload).await {
        Ok(source) => {
            // Invalidate scheduler cache since we added a new source
            let _ = state.cache_invalidation_tx.send(());

            // Trigger immediate refresh
            info!(
                "Triggering immediate refresh for new stream source: {} ({})",
                source.name, source.id
            );

            tokio::spawn({
                let database = state.database.clone();
                let state_manager = state.state_manager.clone();
                let source_id = source.id;
                let source_name = source.name.clone();
                async move {
                    use crate::ingestor::IngestorService;

                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    let ingestor = IngestorService::new(state_manager.clone());

                    if let Ok(Some(stream_source)) = database.get_stream_source(source_id).await {
                        match ingestor.ingest_source(&stream_source).await {
                            Ok(channels) => {
                                info!(
                                    "Initial stream ingestion completed for '{}': {} channels",
                                    source_name,
                                    channels.len()
                                );

                                if let Err(e) = database
                                    .update_source_channels(
                                        source_id,
                                        &channels,
                                        Some(&state_manager),
                                    )
                                    .await
                                {
                                    error!(
                                        "Failed to save initial stream data for source '{}': {}",
                                        source_name, e
                                    );
                                } else if let Err(e) =
                                    database.update_source_last_ingested(source_id).await
                                {
                                    error!("Failed to update last_ingested_at for stream source '{}': {}", source_name, e);
                                } else {
                                    // Mark ingestion as completed with final channel count
                                    state_manager
                                        .complete_ingestion(source_id, channels.len())
                                        .await;
                                }
                            }
                            Err(e) => {
                                error!(
                                    "Initial stream ingestion failed for '{}': {}",
                                    source_name, e
                                );
                            }
                        }
                    }
                }
            });

            // Return unified response
            let source_with_stats = StreamSourceWithStats {
                source,
                channel_count: 0,
                next_scheduled_update: None,
            };
            Ok(Json(UnifiedSourceWithStats::from_stream(source_with_stats)))
        }
        Err(e) => {
            error!("Failed to create stream source: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

/// Get stream source by ID
pub async fn get_stream_source(
    Path(id): Path<Uuid>,
    State(state): State<AppState>,
) -> Result<Json<UnifiedSourceWithStats>, StatusCode> {
    match state.database.get_stream_source(id).await {
        Ok(Some(source)) => {
            // Get stats separately
            let channel_count = state
                .database
                .get_source_channel_count(id)
                .await
                .unwrap_or(0);
            let source_with_stats = StreamSourceWithStats {
                source,
                channel_count,
                next_scheduled_update: None,
            };
            Ok(Json(UnifiedSourceWithStats::from_stream(source_with_stats)))
        }
        Ok(None) => Err(StatusCode::NOT_FOUND),
        Err(e) => {
            error!("Failed to get stream source {}: {}", id, e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

/// Update stream source
pub async fn update_stream_source(
    Path(id): Path<Uuid>,
    State(state): State<AppState>,
    Json(payload): Json<StreamSourceUpdateRequest>,
) -> Result<Json<UnifiedSourceWithStats>, StatusCode> {
    match state.database.update_stream_source(id, &payload).await {
        Ok(Some(source)) => {
            let _ = state.cache_invalidation_tx.send(());
            let source_with_stats = StreamSourceWithStats {
                source,
                channel_count: 0,
                next_scheduled_update: None,
            };
            Ok(Json(UnifiedSourceWithStats::from_stream(source_with_stats)))
        }
        Ok(None) => Err(StatusCode::NOT_FOUND),
        Err(e) => {
            error!("Failed to update stream source {}: {}", id, e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

/// Delete stream source
pub async fn delete_stream_source(
    Path(id): Path<Uuid>,
    State(state): State<AppState>,
) -> Result<StatusCode, StatusCode> {
    match state.database.delete_stream_source(id).await {
        Ok(_) => {
            let _ = state.cache_invalidation_tx.send(());
            Ok(StatusCode::NO_CONTENT)
        }
        Err(e) => {
            error!("Failed to delete stream source {}: {}", id, e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
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
pub async fn list_epg_sources_unified(
    State(state): State<AppState>,
) -> Result<Json<Vec<UnifiedSourceWithStats>>, StatusCode> {
    match state.database.list_epg_sources_with_stats().await {
        Ok(sources) => {
            let unified_sources: Vec<UnifiedSourceWithStats> = sources
                .into_iter()
                .map(UnifiedSourceWithStats::from_epg)
                .collect();
            Ok(Json(unified_sources))
        }
        Err(e) => {
            error!("Failed to list EPG sources: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

/// Create EPG source
pub async fn create_epg_source_unified(
    State(state): State<AppState>,
    Json(payload): Json<EpgSourceCreateRequest>,
) -> Result<Json<UnifiedSourceWithStats>, StatusCode> {
    match state.database.create_epg_source(&payload).await {
        Ok(source) => {
            let _ = state.cache_invalidation_tx.send(());

            // Trigger immediate refresh
            info!(
                "Triggering immediate refresh for new EPG source: {} ({})",
                source.name, source.id
            );

            tokio::spawn({
                let database = state.database.clone();
                let state_manager = state.state_manager.clone();
                let source_id = source.id;
                let source_name = source.name.clone();
                async move {
                    use crate::ingestor::ingest_epg::EpgIngestor;
                    use crate::ingestor::state_manager::ProcessingTrigger;

                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

                    let ingestor = EpgIngestor::new_with_state_manager(
                        database.clone(),
                        state_manager.clone(),
                    );

                    if let Ok(Some(epg_source)) = database.get_epg_source(source_id).await {
                        match ingestor
                            .ingest_epg_source_with_trigger(&epg_source, ProcessingTrigger::Manual)
                            .await
                        {
                            Ok((channels, mut programs, detected_timezone)) => {
                                // Update channel names in programs
                                ingestor.update_channel_names(&channels, &mut programs);

                                // Update detected timezone if found
                                if let Some(ref detected_tz) = detected_timezone {
                                    if detected_tz != &epg_source.timezone
                                        && !epg_source.timezone_detected
                                    {
                                        info!(
                                            "Updating EPG source '{}' timezone from '{}' to detected '{}'",
                                            source_name, epg_source.timezone, detected_tz
                                        );
                                        let _ = database
                                            .update_epg_source_detected_timezone(
                                                source_id,
                                                detected_tz,
                                            )
                                            .await;
                                    }
                                }

                                // Save to database using cancellation-aware method
                                match ingestor
                                    .save_epg_data_with_cancellation(source_id, channels, programs)
                                    .await
                                {
                                    Ok((channel_count, program_count)) => {
                                        // Update last ingested timestamp
                                        if let Err(e) = database
                                            .update_epg_source_last_ingested(source_id)
                                            .await
                                        {
                                            error!("Failed to update last_ingested_at for EPG source '{}': {}", source_name, e);
                                        }

                                        info!(
                                            "Initial EPG ingestion completed for '{}': {} channels, {} programs",
                                            source_name, channel_count, program_count
                                        );
                                    }
                                    Err(e) => {
                                        error!(
                                            "Failed to save initial EPG data for source '{}': {}",
                                            source_name, e
                                        );
                                    }
                                }
                            }
                            Err(e) => {
                                error!("Initial EPG ingestion failed for '{}': {}", source_name, e);
                            }
                        }
                    }
                }
            });

            let source_with_stats = EpgSourceWithStats {
                source,
                channel_count: 0,
                program_count: 0,
                next_scheduled_update: None,
            };
            Ok(Json(UnifiedSourceWithStats::from_epg(source_with_stats)))
        }
        Err(e) => {
            error!("Failed to create EPG source: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

/// Get EPG source by ID
pub async fn get_epg_source_unified(
    Path(id): Path<Uuid>,
    State(state): State<AppState>,
) -> Result<Json<UnifiedSourceWithStats>, StatusCode> {
    match state.database.get_epg_source(id).await {
        Ok(Some(source)) => {
            // Get stats separately
            let channel_count = state
                .database
                .get_epg_source_channel_count(id)
                .await
                .unwrap_or(0);
            let source_with_stats = EpgSourceWithStats {
                source,
                channel_count,
                program_count: 0, // TODO: Add program count method
                next_scheduled_update: None,
            };
            Ok(Json(UnifiedSourceWithStats::from_epg(source_with_stats)))
        }
        Ok(None) => Err(StatusCode::NOT_FOUND),
        Err(e) => {
            error!("Failed to get EPG source {}: {}", id, e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

/// Update EPG source
pub async fn update_epg_source_unified(
    Path(id): Path<Uuid>,
    State(state): State<AppState>,
    Json(payload): Json<EpgSourceUpdateRequest>,
) -> Result<Json<UnifiedSourceWithStats>, StatusCode> {
    match state.database.update_epg_source(id, &payload).await {
        Ok(true) => {
            let _ = state.cache_invalidation_tx.send(());
            // Get the updated source
            match state.database.get_epg_source(id).await {
                Ok(Some(source)) => {
                    let channel_count = state
                        .database
                        .get_epg_source_channel_count(id)
                        .await
                        .unwrap_or(0);
                    let source_with_stats = EpgSourceWithStats {
                        source,
                        channel_count,
                        program_count: 0,
                        next_scheduled_update: None,
                    };
                    Ok(Json(UnifiedSourceWithStats::from_epg(source_with_stats)))
                }
                Ok(None) => Err(StatusCode::NOT_FOUND),
                Err(e) => {
                    error!("Failed to get updated EPG source {}: {}", id, e);
                    Err(StatusCode::INTERNAL_SERVER_ERROR)
                }
            }
        }
        Ok(false) => Err(StatusCode::NOT_FOUND),
        Err(e) => {
            error!("Failed to update EPG source {}: {}", id, e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

/// Delete EPG source
pub async fn delete_epg_source_unified(
    Path(id): Path<Uuid>,
    State(state): State<AppState>,
) -> Result<StatusCode, StatusCode> {
    match state.database.delete_epg_source(id).await {
        Ok(_) => {
            let _ = state.cache_invalidation_tx.send(());
            Ok(StatusCode::NO_CONTENT)
        }
        Err(e) => {
            error!("Failed to delete EPG source {}: {}", id, e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

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

pub async fn search_logo_assets(
    Query(params): Query<crate::models::logo_asset::LogoAssetSearchRequest>,
    State(state): State<AppState>,
) -> Result<Json<crate::models::logo_asset::LogoAssetSearchResult>, StatusCode> {
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

            // Convert linked assets to LogoAssetWithUrl
            let linked_with_urls: Vec<crate::models::logo_asset::LogoAssetWithUrl> = linked_assets
                .into_iter()
                .map(|linked| crate::models::logo_asset::LogoAssetWithUrl {
                    url: format!("/api/logos/{}", linked.id),
                    asset: linked,
                })
                .collect();

            // Build available formats list
            let mut available_formats = vec![asset
                .mime_type
                .split('/')
                .last()
                .unwrap_or("unknown")
                .to_string()];
            for linked in &linked_with_urls {
                if let Some(format) = linked.asset.mime_type.split('/').last() {
                    if !available_formats.contains(&format.to_string()) {
                        available_formats.push(format.to_string());
                    }
                }
            }

            let response = crate::models::logo_asset::LogoAssetWithLinked {
                asset,
                url: format!("/api/logos/{}", id),
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
    match state.logo_asset_service.get_cache_stats().await {
        Ok(stats) => Ok(Json(stats)),
        Err(e) => {
            error!("Failed to get logo cache stats: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
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

// Helper function to flatten condition tree for testing
fn flatten_condition_tree_for_test(
    condition_tree: &crate::models::ConditionTree,
) -> Vec<crate::models::data_mapping::DataMappingConditionRequest> {
    let mut conditions = vec![];
    flatten_condition_node_for_test(&condition_tree.root, &mut conditions, None);
    conditions
}

fn flatten_condition_node_for_test(
    node: &crate::models::ConditionNode,
    conditions: &mut Vec<crate::models::data_mapping::DataMappingConditionRequest>,
    logical_op: Option<crate::models::LogicalOperator>,
) {
    match node {
        crate::models::ConditionNode::Condition {
            field,
            operator,
            value,
            case_sensitive: _,
            negate,
        } => {
            let final_operator = if *negate {
                match operator {
                    crate::models::FilterOperator::Equals => {
                        crate::models::FilterOperator::NotEquals
                    }
                    crate::models::FilterOperator::Contains => {
                        crate::models::FilterOperator::NotContains
                    }
                    crate::models::FilterOperator::Matches => {
                        crate::models::FilterOperator::NotMatches
                    }
                    crate::models::FilterOperator::NotEquals => {
                        crate::models::FilterOperator::Equals
                    }
                    crate::models::FilterOperator::NotContains => {
                        crate::models::FilterOperator::Contains
                    }
                    crate::models::FilterOperator::NotMatches => {
                        crate::models::FilterOperator::Matches
                    }
                    _ => operator.clone(),
                }
            } else {
                operator.clone()
            };

            conditions.push(crate::models::data_mapping::DataMappingConditionRequest {
                field_name: field.clone(),
                operator: final_operator,
                value: value.clone(),
                logical_operator: logical_op,
            });
        }
        crate::models::ConditionNode::Group { operator, children } => {
            for (i, child) in children.iter().enumerate() {
                let child_logical_op = if i == 0 {
                    logical_op.clone()
                } else {
                    Some(operator.clone())
                };
                flatten_condition_node_for_test(child, conditions, child_logical_op);
            }
        }
    }
}

// Helper function to convert actions for testing
fn convert_actions_for_test(
    actions: &[crate::models::Action],
) -> Vec<crate::models::data_mapping::DataMappingActionRequest> {
    actions
        .iter()
        .filter_map(|action| {
            let action_type = match action.operator {
                crate::models::ActionOperator::Set => {
                    crate::models::data_mapping::DataMappingActionType::SetValue
                }
                crate::models::ActionOperator::SetIfEmpty => {
                    crate::models::data_mapping::DataMappingActionType::SetDefaultIfEmpty
                }
                _ => return None, // Skip unsupported operators for now
            };

            let (value, logo_asset_id) = match &action.value {
                crate::models::ActionValue::Literal(v) => {
                    // Check if this is a @logo: reference
                    if v.starts_with("@logo:") {
                        if let Ok(logo_uuid) = uuid::Uuid::parse_str(&v[6..]) {
                            (Some(v.clone()), Some(logo_uuid))
                        } else {
                            (Some(v.clone()), None)
                        }
                    } else {
                        (Some(v.clone()), None)
                    }
                }
                _ => (None, None), // Skip complex values for testing
            };

            Some(crate::models::data_mapping::DataMappingActionRequest {
                action_type,
                target_field: action.field.clone(),
                value,
                logo_asset_id,
                timeshift_minutes: None,
            })
        })
        .collect()
}
