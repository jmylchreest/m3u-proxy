use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::Json,
};
use serde_json::json;
use tracing::{error, info};
use uuid::Uuid;

use super::AppState;
use crate::models::*;
use crate::proxy::generator::ProxyGenerator;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct ChannelQueryParams {
    pub page: Option<u32>,
    pub limit: Option<u32>,
    pub filter: Option<String>,
}

// Stream Sources API
pub async fn list_sources(
    State(state): State<AppState>,
) -> Result<Json<Vec<StreamSourceWithStats>>, StatusCode> {
    match state.database.list_stream_sources_with_stats().await {
        Ok(sources) => Ok(Json(sources)),
        Err(e) => {
            error!("Failed to list stream sources: {}", e);
            error!("Error type: {:?}", e);
            if let Some(sqlx_error) = e.downcast_ref::<sqlx::Error>() {
                error!("SQLx error details: {:?}", sqlx_error);
            }
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

pub async fn create_source(
    State(state): State<AppState>,
    Json(payload): Json<StreamSourceCreateRequest>,
) -> Result<Json<StreamSource>, StatusCode> {
    match state.database.create_stream_source(&payload).await {
        Ok(source) => {
            // Invalidate scheduler cache since we added a new source
            let _ = state.cache_invalidation_tx.send(());
            Ok(Json(source))
        }
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

pub async fn get_source(
    Path(id): Path<Uuid>,
    State(state): State<AppState>,
) -> Result<Json<StreamSource>, StatusCode> {
    match state.database.get_stream_source(id).await {
        Ok(Some(source)) => Ok(Json(source)),
        Ok(None) => Err(StatusCode::NOT_FOUND),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

pub async fn update_source(
    Path(id): Path<Uuid>,
    State(state): State<AppState>,
    Json(payload): Json<StreamSourceUpdateRequest>,
) -> Result<Json<StreamSource>, StatusCode> {
    match state.database.update_stream_source(id, &payload).await {
        Ok(Some(source)) => {
            // Invalidate scheduler cache since source was updated
            let _ = state.cache_invalidation_tx.send(());
            Ok(Json(source))
        }
        Ok(None) => Err(StatusCode::NOT_FOUND),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

pub async fn delete_source(
    Path(id): Path<Uuid>,
    State(state): State<AppState>,
) -> Result<StatusCode, StatusCode> {
    match state.database.delete_stream_source(id).await {
        Ok(true) => {
            // Invalidate scheduler cache since source was deleted
            let _ = state.cache_invalidation_tx.send(());
            Ok(StatusCode::NO_CONTENT)
        }
        Ok(false) => Err(StatusCode::NOT_FOUND),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

pub async fn refresh_source(
    Path(id): Path<Uuid>,
    State(state): State<AppState>,
) -> Result<Json<RefreshResponse>, StatusCode> {
    use crate::ingestor::IngestorService;
    use tracing::{error, info};

    // Get the source first
    let source = match state.database.get_stream_source(id).await {
        Ok(Some(source)) => source,
        Ok(None) => {
            error!("Stream source ({}) not found for refresh", id);
            return Err(StatusCode::NOT_FOUND);
        }
        Err(e) => {
            error!("Database error getting source ({}): {}", id, e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    info!(
        "Starting manual refresh for source '{}' ({})",
        source.name, source.id
    );

    // Create ingestor with state manager and trigger refresh
    let ingestor = IngestorService::new(state.state_manager.clone());

    match ingestor.ingest_source(&source).await {
        Ok(channels) => {
            info!(
                "Ingestion completed for '{}': {} channels",
                source.name,
                channels.len()
            );

            // Update the channels in database
            match state
                .database
                .update_source_channels(id, &channels, Some(&state.state_manager))
                .await
            {
                Ok(_) => {
                    // Update last_ingested_at timestamp
                    match state.database.update_source_last_ingested(id).await {
                        Ok(_last_ingested_timestamp) => {
                            // Timestamp updated successfully
                        }
                        Err(e) => {
                            error!(
                                "Failed to update last_ingested_at for source '{}': {}",
                                source.name, e
                            );
                        }
                    }

                    // Mark ingestion as completed with final channel count
                    state
                        .state_manager
                        .complete_ingestion(source.id, channels.len())
                        .await;

                    // Invalidate scheduler cache since last_ingested_at was updated
                    let _ = state.cache_invalidation_tx.send(());

                    info!(
                        "Manual refresh completed for source '{}': {} channels saved",
                        source.name,
                        channels.len()
                    );

                    Ok(Json(RefreshResponse {
                        success: true,
                        message: format!("Successfully ingested {} channels", channels.len()),
                        channel_count: channels.len(),
                    }))
                }
                Err(e) => {
                    error!(
                        "Failed to save channels for source '{}': {}",
                        source.name, e
                    );
                    state
                        .state_manager
                        .set_error(source.id, format!("Failed to save channels: {}", e))
                        .await;

                    Err(StatusCode::INTERNAL_SERVER_ERROR)
                }
            }
        }
        Err(err) => {
            error!("Ingestion failed for source '{}': {}", source.name, err);

            Ok(Json(RefreshResponse {
                success: false,
                message: format!("Failed to ingest source: {}", err),
                channel_count: 0,
            }))
        }
    }
}

// Progress API
pub async fn get_source_progress(
    Path(id): Path<Uuid>,
    State(state): State<AppState>,
) -> Result<Json<Option<IngestionProgress>>, StatusCode> {
    let progress = state.state_manager.get_progress(id).await;
    Ok(Json(progress))
}

pub async fn get_all_progress(
    State(state): State<AppState>,
) -> Result<Json<std::collections::HashMap<Uuid, IngestionProgress>>, StatusCode> {
    let progress = state.state_manager.get_all_progress().await;
    Ok(Json(progress))
}

pub async fn cancel_source_ingestion(
    Path(id): Path<Uuid>,
    State(state): State<AppState>,
) -> Result<Json<RefreshResponse>, StatusCode> {
    use tracing::{info, warn};

    // Check if source exists
    match state.database.get_stream_source(id).await {
        Ok(Some(source)) => {
            info!(
                "Attempting to cancel ingestion for source '{}' ({})",
                source.name, source.id
            );

            let cancelled = state.state_manager.cancel_ingestion(id).await;

            if cancelled {
                info!(
                    "Successfully cancelled ingestion for source '{}'",
                    source.name
                );
                Ok(Json(RefreshResponse {
                    success: true,
                    message: "Ingestion cancelled successfully".to_string(),
                    channel_count: 0,
                }))
            } else {
                warn!("No active ingestion found for source '{}'", source.name);
                Ok(Json(RefreshResponse {
                    success: false,
                    message: "No active ingestion to cancel".to_string(),
                    channel_count: 0,
                }))
            }
        }
        Ok(None) => {
            warn!("Source ({}) not found for cancellation", id);
            Err(StatusCode::NOT_FOUND)
        }
        Err(e) => {
            error!("Database error getting source ({}): {}", id, e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

pub async fn get_source_channels(
    Path(id): Path<Uuid>,
    Query(params): Query<ChannelQueryParams>,
    State(state): State<AppState>,
) -> Result<Json<ChannelListResponse>, StatusCode> {
    let page = params.page.unwrap_or(1);
    let limit = params.limit.unwrap_or(10000).min(10000); // Cap at 10k per page
    let filter = params.filter.as_deref();

    match state
        .database
        .get_source_channels_paginated(id, page, limit, filter)
        .await
    {
        Ok(response) => Ok(Json(response)),
        Err(e) => {
            error!("Failed to get channels for source ({}): {}", id, e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
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
    
    match proxy_generator
        .generate(
            &proxy,
            &state.database,
            &state.data_mapping_service,
            &state.logo_asset_service,
            &state.config.web.base_url,
        )
        .await
    {
        Ok(generation) => {
            // Save the new M3U file
            match proxy_generator.save_m3u_file(proxy.id, &generation.m3u_content).await {
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
                    error!("Failed to save regenerated M3U for proxy {}: {}", proxy_id, e);
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

pub async fn get_source_processing_info(
    Path(id): Path<Uuid>,
    State(state): State<AppState>,
) -> Result<Json<Option<crate::ingestor::state_manager::ProcessingInfo>>, StatusCode> {
    let processing_info = state.state_manager.get_processing_info(id).await;
    Ok(Json(processing_info))
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

pub async fn test_data_mapping_rule(
    State(state): State<AppState>,
    Json(payload): Json<crate::models::data_mapping::DataMappingTestRequest>,
) -> Result<Json<crate::models::data_mapping::DataMappingTestResult>, StatusCode> {
    use crate::data_mapping::DataMappingEngine;
    use std::collections::HashMap;

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

    // Convert request to internal types
    let conditions: Vec<crate::models::data_mapping::DataMappingCondition> = payload
        .conditions
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

    let actions: Vec<crate::models::data_mapping::DataMappingAction> = payload
        .actions
        .into_iter()
        .enumerate()
        .map(|(i, a)| crate::models::data_mapping::DataMappingAction {
            id: Uuid::new_v4(),
            rule_id: Uuid::new_v4(),
            action_type: a.action_type,
            target_field: a.target_field,
            value: a.value,
            logo_asset_id: a.logo_asset_id,
            label_key: a.label_key,
            label_value: a.label_value,
            sort_order: i as i32,
            created_at: chrono::Utc::now(),
        })
        .collect();

    // Get logo assets for the actions
    let mut logo_assets = HashMap::new();
    for action in &actions {
        if let Some(logo_id) = action.logo_asset_id {
            if let Ok(asset) = state.logo_asset_service.get_asset(logo_id).await {
                logo_assets.insert(logo_id, asset);
            }
        }
    }

    let mut engine = DataMappingEngine::new();
    match engine.test_mapping_rule(
        channels,
        conditions,
        actions,
        logo_assets,
        &state.config.web.base_url,
    ) {
        Ok(mapped_channels) => {
            let test_channels: Vec<crate::models::data_mapping::DataMappingTestChannel> =
                mapped_channels
                    .into_iter()
                    .map(|mc| {
                        let mut original_values = HashMap::new();
                        original_values.insert(
                            "channel_name".to_string(),
                            Some(mc.original.channel_name.clone()),
                        );
                        original_values.insert("tvg_id".to_string(), mc.original.tvg_id.clone());
                        original_values
                            .insert("tvg_name".to_string(), mc.original.tvg_name.clone());
                        original_values
                            .insert("tvg_logo".to_string(), mc.original.tvg_logo.clone());
                        original_values
                            .insert("group_title".to_string(), mc.original.group_title.clone());

                        let mut mapped_values = HashMap::new();
                        mapped_values.insert(
                            "channel_name".to_string(),
                            Some(mc.mapped_channel_name.clone()),
                        );
                        mapped_values.insert("tvg_id".to_string(), mc.mapped_tvg_id.clone());
                        mapped_values.insert("tvg_name".to_string(), mc.mapped_tvg_name.clone());
                        mapped_values.insert("tvg_logo".to_string(), mc.mapped_tvg_logo.clone());
                        mapped_values
                            .insert("group_title".to_string(), mc.mapped_group_title.clone());

                        crate::models::data_mapping::DataMappingTestChannel {
                            channel_name: mc.original.channel_name,
                            group_title: mc.original.group_title,
                            original_values,
                            mapped_values,
                            applied_actions: vec!["Test Rule".to_string()],
                        }
                    })
                    .collect();

            let result = crate::models::data_mapping::DataMappingTestResult {
                is_valid: true,
                error: None,
                matching_channels: test_channels.clone(),
                total_channels: test_channels.len(),
                matched_count: test_channels.len(),
            };

            Ok(Json(result))
        }
        Err(e) => {
            let result = crate::models::data_mapping::DataMappingTestResult {
                is_valid: false,
                error: Some(e.to_string()),
                matching_channels: vec![],
                total_channels: 0,
                matched_count: 0,
            };
            Ok(Json(result))
        }
    }
}

pub async fn preview_data_mapping(
    State(state): State<AppState>,
    Path(source_id): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    info!("Previewing data mapping for source {}", source_id);

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
            error!("Source {} not found", source_uuid);
            return Err(StatusCode::NOT_FOUND);
        }
        Err(e) => {
            error!("Failed to get source {}: {}", source_uuid, e);
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

    // Apply data mapping for preview (doesn't save to database)
    let mapped_channels = match state
        .data_mapping_service
        .apply_mapping_for_proxy(
            channels.clone(),
            source_uuid,
            &state.logo_asset_service,
            &state.config.web.base_url,
        )
        .await
    {
        Ok(mapped) => mapped,
        Err(e) => {
            error!("Data mapping failed for source '{}': {}", source.name, e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    // Return preview data
    let preview_channels: Vec<_> = mapped_channels.iter().take(10).collect();

    let result = serde_json::json!({
        "success": true,
        "message": "Data mapping preview completed",
        "source_name": source.name,
        "original_count": channels.len(),
        "mapped_count": mapped_channels.len(),
        "preview_channels": preview_channels
    });

    info!(
        "Data mapping preview completed for source '{}': {} original -> {} mapped channels",
        source.name,
        channels.len(),
        mapped_channels.len()
    );

    Ok(Json(result))
}

pub async fn preview_data_mapping_rules(
    Query(params): Query<std::collections::HashMap<String, String>>,
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    info!("Generating data mapping rule preview");

    let view_type = params.get("view").map(|s| s.as_str()).unwrap_or("final");

    // Get all active rules
    let rules = match state.data_mapping_service.get_all_rules().await {
        Ok(rules) => rules,
        Err(e) => {
            error!("Failed to load data mapping rules for preview: {}", e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    let active_rules: Vec<_> = rules
        .into_iter()
        .filter(|rule| rule.rule.is_active)
        .collect();

    if active_rules.is_empty() {
        return Ok(Json(serde_json::json!({
            "success": true,
            "message": "No active data mapping rules found",
            "rules": [],
            "total_rules": 0,
            "total_affected_channels": 0,
            "final_channels": []
        })));
    }

    // Get all sources and their channels
    let sources = match state.database.list_stream_sources().await {
        Ok(sources) => sources,
        Err(e) => {
            error!("Failed to list sources for preview: {}", e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    let mut rule_previews = Vec::new();
    let mut total_affected_channels = 0;
    let mut final_channel_map: std::collections::HashMap<String, serde_json::Value> =
        std::collections::HashMap::new();

    for rule_with_details in active_rules {
        let mut rule_affected_channels = 0;
        let mut affected_channels = Vec::new();

        for source in &sources {
            if !source.is_active {
                continue;
            }

            // Get channels for this source
            let channels = match state.database.get_source_channels(source.id).await {
                Ok(channels) => channels,
                Err(e) => {
                    error!(
                        "Failed to get channels for source {} during preview: {}",
                        source.id, e
                    );
                    continue;
                }
            };

            // Test each channel against this rule's conditions
            for channel in channels {
                let mut engine = crate::data_mapping::DataMappingEngine::new();

                if let Ok(matches) =
                    engine.evaluate_rule_conditions(&channel, &rule_with_details.conditions)
                {
                    if matches {
                        rule_affected_channels += 1;

                        // Preview what actions would be applied
                        let mut action_previews = Vec::new();
                        let mut final_values = std::collections::HashMap::new();

                        // Initialize with current values
                        final_values
                            .insert("channel_name".to_string(), channel.channel_name.clone());
                        final_values.insert(
                            "tvg_id".to_string(),
                            channel.tvg_id.clone().unwrap_or_default(),
                        );
                        final_values.insert(
                            "tvg_name".to_string(),
                            channel.tvg_name.clone().unwrap_or_default(),
                        );
                        final_values.insert(
                            "tvg_logo".to_string(),
                            channel.tvg_logo.clone().unwrap_or_default(),
                        );
                        final_values.insert(
                            "group_title".to_string(),
                            channel.group_title.clone().unwrap_or_default(),
                        );

                        for action in &rule_with_details.actions {
                            let current_value = match action.target_field.as_str() {
                                "channel_name" => Some(channel.channel_name.clone()),
                                "tvg_id" => channel.tvg_id.clone(),
                                "tvg_name" => channel.tvg_name.clone(),
                                "tvg_logo" => channel.tvg_logo.clone(),
                                "group_title" => channel.group_title.clone(),
                                _ => None,
                            };

                            let new_value = match &action.action_type {
                                crate::models::data_mapping::DataMappingActionType::SetValue => {
                                    action.value.clone()
                                },
                                crate::models::data_mapping::DataMappingActionType::SetDefaultIfEmpty => {
                                    if current_value.is_none() || current_value.as_ref().map_or(true, |s| s.is_empty()) {
                                        action.value.clone()
                                    } else {
                                        current_value.clone()
                                    }
                                },
                                crate::models::data_mapping::DataMappingActionType::SetLogo => {
                                    if let Some(logo_id) = action.logo_asset_id {
                                        Some(crate::utils::generate_logo_url(&state.config.web.base_url, logo_id))
                                    } else {
                                        current_value.clone()
                                    }
                                },
                                _ => current_value.clone(),
                            };

                            // Update final values for aggregated view
                            if let Some(ref new_val) = new_value {
                                final_values.insert(action.target_field.clone(), new_val.clone());
                            }

                            action_previews.push(serde_json::json!({
                                "action_type": action.action_type,
                                "target_field": action.target_field,
                                "current_value": current_value,
                                "new_value": new_value,
                                "will_change": current_value != new_value
                            }));
                        }

                        // Update final channel map for aggregated view
                        let channel_key = format!("{}:{}", source.id, channel.id);
                        if let Some(existing) = final_channel_map.get_mut(&channel_key) {
                            // Update with new values and add rule
                            let existing_obj = existing.as_object_mut().unwrap();
                            for (field, value) in &final_values {
                                existing_obj.insert(
                                    field.clone(),
                                    serde_json::Value::String(value.clone()),
                                );
                            }
                            if let Some(rules_array) = existing_obj.get_mut("applied_rules") {
                                rules_array.as_array_mut().unwrap().push(
                                    serde_json::Value::String(rule_with_details.rule.name.clone()),
                                );
                            }
                        } else {
                            final_channel_map.insert(channel_key, serde_json::json!({
                                "channel_id": channel.id,
                                "channel_name": final_values.get("channel_name").unwrap_or(&channel.channel_name),
                                "tvg_id": final_values.get("tvg_id").unwrap_or(&"".to_string()),
                                "tvg_name": final_values.get("tvg_name").unwrap_or(&"".to_string()),
                                "tvg_logo": final_values.get("tvg_logo").unwrap_or(&"".to_string()),
                                "group_title": final_values.get("group_title").unwrap_or(&"".to_string()),
                                "source_id": source.id,
                                "source_name": source.name,
                                "applied_rules": vec![rule_with_details.rule.name.clone()],
                                "original_channel_name": channel.channel_name,
                                "original_tvg_id": channel.tvg_id,
                                "original_tvg_name": channel.tvg_name,
                                "original_tvg_logo": channel.tvg_logo,
                                "original_group_title": channel.group_title
                            }));
                        }

                        affected_channels.push(serde_json::json!({
                            "channel_id": channel.id,
                            "channel_name": channel.channel_name,
                            "tvg_id": channel.tvg_id,
                            "tvg_name": channel.tvg_name,
                            "source_id": source.id,
                            "source_name": source.name,
                            "actions_preview": action_previews
                        }));
                    }
                }
            }
        }

        total_affected_channels += rule_affected_channels;

        rule_previews.push(serde_json::json!({
            "rule_id": rule_with_details.rule.id,
            "rule_name": rule_with_details.rule.name,
            "rule_description": rule_with_details.rule.description,
            "affected_channels_count": rule_affected_channels,
            "affected_channels": affected_channels,
            "conditions": rule_with_details.conditions,
            "actions": rule_with_details.actions
        }));
    }

    // Convert final channel map to array
    let final_channels: Vec<_> = final_channel_map.into_values().collect();

    let result = serde_json::json!({
        "success": true,
        "message": "Data mapping rules preview generated",
        "rules": rule_previews,
        "total_rules": rule_previews.len(),
        "total_affected_channels": total_affected_channels,
        "final_channels": final_channels,
        "view_type": view_type
    });

    info!(
        "Data mapping preview completed: {} rules, {} total affected channels, {} final channels",
        rule_previews.len(),
        total_affected_channels,
        final_channels.len()
    );

    Ok(Json(result))
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
