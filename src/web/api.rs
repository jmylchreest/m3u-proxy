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
        Ok(source) => Ok(Json(source)),
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
        Ok(Some(source)) => Ok(Json(source)),
        Ok(None) => Err(StatusCode::NOT_FOUND),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

pub async fn delete_source(
    Path(id): Path<Uuid>,
    State(state): State<AppState>,
) -> Result<StatusCode, StatusCode> {
    match state.database.delete_stream_source(id).await {
        Ok(true) => Ok(StatusCode::NO_CONTENT),
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

    // Update state to indicate saving phase
    state
        .state_manager
        .update_progress(
            source.id,
            crate::models::IngestionState::Saving,
            crate::models::ProgressInfo {
                current_step: "Saving channels to database".to_string(),
                total_bytes: None,
                downloaded_bytes: None,
                channels_parsed: None,
                channels_saved: None,
                percentage: Some(90.0),
            },
        )
        .await;

    match ingestor.ingest_source(&source).await {
        Ok(channels) => {
            info!(
                "Ingestion completed for '{}': {} channels",
                source.name,
                channels.len()
            );

            // Update the channels in database
            match state.database.update_source_channels(id, &channels).await {
                Ok(_) => {
                    // Update last_ingested_at timestamp
                    if let Err(e) = state.database.update_source_last_ingested(id).await {
                        error!(
                            "Failed to update last_ingested_at for source '{}': {}",
                            source.name, e
                        );
                    }

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
pub async fn list_filters(State(_state): State<AppState>) -> Result<Json<Vec<Filter>>, StatusCode> {
    // TODO: Implement list filters
    Ok(Json(vec![]))
}

pub async fn create_filter(
    State(_state): State<AppState>,
    Json(_payload): Json<Filter>,
) -> Result<Json<Filter>, StatusCode> {
    // TODO: Implement create filter
    Err(StatusCode::NOT_IMPLEMENTED)
}

pub async fn get_filter(
    Path(_id): Path<Uuid>,
    State(_state): State<AppState>,
) -> Result<Json<Filter>, StatusCode> {
    // TODO: Implement get filter
    Err(StatusCode::NOT_FOUND)
}

pub async fn update_filter(
    Path(_id): Path<Uuid>,
    State(_state): State<AppState>,
    Json(_payload): Json<Filter>,
) -> Result<Json<Filter>, StatusCode> {
    // TODO: Implement update filter
    Err(StatusCode::NOT_IMPLEMENTED)
}

pub async fn delete_filter(
    Path(_id): Path<Uuid>,
    State(_state): State<AppState>,
) -> Result<StatusCode, StatusCode> {
    // TODO: Implement delete filter
    Err(StatusCode::NOT_IMPLEMENTED)
}
