//! EPG (Electronic Program Guide) API handlers
//!
//! Provides endpoints for browsing EPG data from XMLTV sources

use axum::{
    extract::{Query, State, Path},
    response::IntoResponse,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::{
    repositories::{
        EpgProgramRepository,
        EpgSourceRepository,
        EpgProgramQuery,
        epg_source::EpgSourceQuery,
        traits::{Repository, PaginatedRepository},
    },
    web::{AppState, responses::handle_result},
    utils::uuid_parser::parse_uuid_flexible,
    errors::{AppResult, AppError},
};

#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct EpgQuery {
    /// Filter by EPG source ID
    pub source_id: Option<String>,
    /// Filter by channel ID
    pub channel_id: Option<String>,
    /// Start time filter (ISO 8601)
    pub start_time: Option<DateTime<Utc>>,
    /// End time filter (ISO 8601)
    pub end_time: Option<DateTime<Utc>>,
    /// Search term for program title/description
    pub search: Option<String>,
    /// Filter by category/genre
    pub category: Option<String>,
    /// Pagination: page number (0-based)
    pub page: Option<u32>,
    /// Pagination: items per page
    pub limit: Option<u32>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct EpgProgramResponse {
    pub id: String,
    pub channel_id: String,
    pub channel_name: String,
    pub channel_logo: Option<String>,
    pub title: String,
    pub description: Option<String>,
    pub start_time: DateTime<Utc>,
    pub end_time: DateTime<Utc>,
    pub category: Option<String>,
    pub rating: Option<String>,
    pub source_id: Option<String>,
    pub metadata: Option<HashMap<String, String>>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct EpgListResponse {
    pub programs: Vec<EpgProgramResponse>,
    pub total: u64,
    pub page: u32,
    pub limit: u32,
    pub has_more: bool,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct EpgSourceResponse {
    pub id: String,
    pub name: String,
    pub url: Option<String>,
    pub last_updated: Option<DateTime<Utc>>,
    pub channel_count: u32,
    pub program_count: u32,
}

/// Get all EPG programs with filtering and pagination
#[utoipa::path(
    get,
    path = "/api/v1/epg/programs",
    params(EpgQuery),
    responses(
        (status = 200, description = "EPG programs retrieved successfully", body = EpgListResponse),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn list_epg_programs(
    State(state): State<AppState>,
    Query(params): Query<EpgQuery>,
) -> impl IntoResponse {
    async fn inner(state: AppState, params: EpgQuery) -> AppResult<EpgListResponse> {
        let page = params.page.unwrap_or(1).max(1); // Pages are 1-based
        let limit = params.limit.unwrap_or(50).min(200); // Cap at 200 items per page for EPG

        // Default time range if not specified (next 24 hours)
        let start_time = params.start_time.unwrap_or_else(|| Utc::now());
        let end_time = params.end_time.unwrap_or_else(|| {
            Utc::now() + chrono::Duration::hours(24)
        });

        // Use read pool for read-only operations
        let epg_program_repo = EpgProgramRepository::new(state.database.read_pool());
        
        // Build query from request parameters
        let mut query = EpgProgramQuery::new()
            .time_range(start_time, end_time);
        
        if let Some(search) = params.search {
            query.base.search = Some(search);
        }
        
        if let Some(source_id_str) = params.source_id {
            if let Ok(source_id) = parse_uuid_flexible(&source_id_str) {
                query = query.source_id(source_id);
            }
        }
        
        if let Some(channel_id_str) = params.channel_id {
            if let Ok(channel_id) = parse_uuid_flexible(&channel_id_str) {
                query = query.channel_id(channel_id);
            }
        }
        
        if let Some(category) = params.category {
            query = query.category(category);
        }

        let paginated_result = epg_program_repo
            .find_paginated(query, page, limit)
            .await?;

        let program_responses: Vec<EpgProgramResponse> = paginated_result
            .items
            .into_iter()
            .map(|program| EpgProgramResponse {
                id: program.id.to_string(),
                channel_id: program.channel_id,
                channel_name: program.channel_name,
                channel_logo: None, // Not available in current model
                title: program.program_title,
                description: program.program_description,
                start_time: program.start_time,
                end_time: program.end_time,
                category: program.program_category,
                rating: program.rating,
                source_id: Some(program.source_id.to_string()),
                metadata: None,
            })
            .collect();

        let response = EpgListResponse {
            programs: program_responses,
            total: paginated_result.total_count,
            page: paginated_result.page,
            limit: paginated_result.limit,
            has_more: paginated_result.has_next,
        };

        Ok(response)
    }
    
    handle_result(inner(state, params).await)
}

/// Get EPG programs for a specific source
#[utoipa::path(
    get,
    path = "/api/v1/epg/programs/{source_id}",
    params(
        ("source_id" = String, Path, description = "EPG Source ID")
    ),
    responses(
        (status = 200, description = "EPG source programs retrieved successfully", body = EpgListResponse),
        (status = 404, description = "EPG source not found"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_source_epg_programs(
    State(state): State<AppState>,
    Path(source_id_str): Path<String>,
    Query(params): Query<EpgQuery>,
) -> impl IntoResponse {
    async fn inner(state: AppState, source_id_str: String, mut params: EpgQuery) -> AppResult<EpgListResponse> {
        let source_id = parse_uuid_flexible(&source_id_str)
            .map_err(|e| AppError::Validation { message: format!("Invalid source ID format: {}", e) })?;

        // Use read pool for read-only operations
        let epg_source_repo = EpgSourceRepository::new(state.database.read_pool());
        
        // Verify source exists
        let _source = epg_source_repo
            .find_by_id(source_id)
            .await?
            .ok_or_else(|| AppError::NotFound { 
                resource: "EPG source".to_string(), 
                id: source_id_str 
            })?;

        // Override source_id in params
        params.source_id = Some(source_id.to_string());

        // Call inner logic directly
        let page = params.page.unwrap_or(1).max(1);
        let limit = params.limit.unwrap_or(50).min(200);

        let start_time = params.start_time.unwrap_or_else(|| Utc::now());
        let end_time = params.end_time.unwrap_or_else(|| {
            Utc::now() + chrono::Duration::hours(24)
        });

        let epg_program_repo = EpgProgramRepository::new(state.database.read_pool());
        
        let mut query = EpgProgramQuery::new()
            .time_range(start_time, end_time)
            .source_id(source_id);
        
        if let Some(search) = params.search {
            query.base.search = Some(search);
        }
        
        if let Some(channel_id_str) = params.channel_id {
            if let Ok(channel_id) = parse_uuid_flexible(&channel_id_str) {
                query = query.channel_id(channel_id);
            }
        }
        
        if let Some(category) = params.category {
            query = query.category(category);
        }

        let paginated_result = epg_program_repo
            .find_paginated(query, page, limit)
            .await?;

        let program_responses: Vec<EpgProgramResponse> = paginated_result
            .items
            .into_iter()
            .map(|program| EpgProgramResponse {
                id: program.id.to_string(),
                channel_id: program.channel_id,
                channel_name: program.channel_name,
                channel_logo: None,
                title: program.program_title,
                description: program.program_description,
                start_time: program.start_time,
                end_time: program.end_time,
                category: program.program_category,
                rating: program.rating,
                source_id: Some(program.source_id.to_string()),
                metadata: None,
            })
            .collect();

        let response = EpgListResponse {
            programs: program_responses,
            total: paginated_result.total_count,
            page: paginated_result.page,
            limit: paginated_result.limit,
            has_more: paginated_result.has_next,
        };

        Ok(response)
    }
    
    handle_result(inner(state, source_id_str, params).await)
}

/// Get all available EPG sources
#[utoipa::path(
    get,
    path = "/api/v1/epg/sources",
    responses(
        (status = 200, description = "EPG sources retrieved successfully", body = Vec<EpgSourceResponse>),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn list_epg_sources(
    State(state): State<AppState>,
) -> impl IntoResponse {
    async fn inner(state: AppState) -> AppResult<Vec<EpgSourceResponse>> {
        // Use read pool for read-only operations
        let epg_source_repo = EpgSourceRepository::new(state.database.read_pool());
        
        let query = EpgSourceQuery::new();
        let sources = epg_source_repo
            .find_all(query)
            .await?;

        let source_responses: Vec<EpgSourceResponse> = sources
            .into_iter()
            .map(|source| EpgSourceResponse {
                id: source.id.to_string(),
                name: source.name,
                url: Some(source.url),
                last_updated: source.last_ingested_at,
                channel_count: 0, // Would need additional query
                program_count: 0, // Would need additional query
            })
            .collect();

        Ok(source_responses)
    }
    
    handle_result(inner(state).await)
}

/// Get EPG guide data (time-based grid format)
#[utoipa::path(
    get,
    path = "/api/v1/epg/guide",
    params(EpgQuery),
    responses(
        (status = 200, description = "EPG guide data retrieved successfully"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_epg_guide(
    State(state): State<AppState>,
    Query(params): Query<EpgQuery>,
) -> impl IntoResponse {
    async fn inner(state: AppState, params: EpgQuery) -> AppResult<HashMap<String, serde_json::Value>> {
        // This would return EPG data in a time-grid format suitable for TV guide display
        
        let start_time = params.start_time.unwrap_or_else(|| Utc::now());
        let end_time = params.end_time.unwrap_or_else(|| {
            Utc::now() + chrono::Duration::hours(6)
        });

        // Use read pool for read-only operations
        let epg_program_repo = EpgProgramRepository::new(state.database.read_pool());
        
        // Build query from request parameters
        let mut query = EpgProgramQuery::new()
            .time_range(start_time, end_time);
        
        if let Some(source_id_str) = params.source_id {
            if let Ok(source_id) = parse_uuid_flexible(&source_id_str) {
                query = query.source_id(source_id);
            }
        }
        
        if let Some(channel_id_str) = params.channel_id {
            if let Ok(channel_id) = parse_uuid_flexible(&channel_id_str) {
                query = query.channel_id(channel_id);
            }
        }
        
        // Set high limit for grid view
        query.base.limit = Some(1000);

        let programs = epg_program_repo
            .find_all(query)
            .await?;

        // Group programs by channel for grid display
        let mut grid_data = HashMap::new();
        let mut channels = HashMap::new();
        let mut time_slots = Vec::new();

        for program in programs {
            let channel_id = program.channel_id.clone();
            
            // Track channels
            channels.insert(channel_id.clone(), serde_json::json!({
                "id": channel_id,
                "name": program.channel_name,
                "logo": null // Not available in current model
            }));

            // Add program to channel's schedule
            let programs = grid_data
                .entry(channel_id)
                .or_insert_with(Vec::new);
                
            programs.push(serde_json::json!({
                "id": program.id.to_string(),
                "title": program.program_title,
                "description": program.program_description,
                "start_time": program.start_time,
                "end_time": program.end_time,
                "category": program.program_category
            }));
        }

        // Generate time slots (hourly intervals)
        let mut current_time = start_time;
        while current_time < end_time {
            time_slots.push(current_time);
            current_time = current_time + chrono::Duration::hours(1);
        }

        let response = HashMap::from([
            ("channels".to_string(), serde_json::to_value(channels).unwrap()),
            ("programs".to_string(), serde_json::to_value(grid_data).unwrap()),
            ("time_slots".to_string(), serde_json::to_value(time_slots).unwrap()),
            ("start_time".to_string(), serde_json::to_value(start_time).unwrap()),
            ("end_time".to_string(), serde_json::to_value(end_time).unwrap()),
        ]);

        Ok(response)
    }
    
    handle_result(inner(state, params).await)
}