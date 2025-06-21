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
    match engine.test_mapping_rule(channels, conditions, actions, logo_assets) {
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

// Logo Assets API
pub async fn list_logo_assets(
    Query(params): Query<crate::models::logo_asset::LogoAssetListRequest>,
    State(state): State<AppState>,
) -> Result<Json<crate::models::logo_asset::LogoAssetListResponse>, StatusCode> {
    match state.logo_asset_service.list_assets(params).await {
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
    State(state): State<AppState>,
) -> Result<(axum::http::HeaderMap, Vec<u8>), StatusCode> {
    match state.logo_asset_service.get_asset(id).await {
        Ok(asset) => match state.logo_asset_storage.get_file(&asset.file_path).await {
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
        },
        Err(e) => {
            error!("Failed to get logo asset {}: {}", id, e);
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
    match state.logo_asset_service.search_assets(params).await {
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
