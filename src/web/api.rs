use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
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
