//! EPG (Electronic Program Guide) API handlers
//!
//! Provides endpoints for browsing EPG data from XMLTV sources

use axum::{
    extract::{Path, Query, State},
    response::IntoResponse,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::{
    errors::{AppError, AppResult},
    utils::uuid_parser::parse_uuid_flexible,
    web::{AppState, responses::handle_result},
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
    pub is_streamable: bool,
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
    tag = "epg",
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
        // Note: start_time is used to find programs that END after this time (to include currently running programs)
        let filter_start = params.start_time.unwrap_or_else(Utc::now);
        let filter_end = params
            .end_time
            .unwrap_or_else(|| Utc::now() + chrono::Duration::hours(24));

        // Use clean SeaORM repository (rationalized approach)
        let epg_program_repo = crate::database::repositories::EpgProgramSeaOrmRepository::new(
            state.database.connection().clone(),
        );

        // Determine source filter
        let source_filter = if let Some(source_id_str) = params.source_id {
            parse_uuid_flexible(&source_id_str).ok()
        } else {
            None
        };

        // Get programs by time range (simplified query approach)
        // This should return programs that overlap with the time range, not just those starting within it
        let mut programs = epg_program_repo
            .find_by_time_range(source_filter.as_ref(), &filter_start, &filter_end)
            .await
            .map_err(|e| AppError::Validation {
                message: e.to_string(),
            })?;

        // Apply additional filters in memory (simplified for cleaner code)
        if let Some(channel_id_str) = params.channel_id
            && let Ok(channel_id) = parse_uuid_flexible(&channel_id_str)
        {
            programs.retain(|p| p.channel_id == channel_id.to_string());
        }

        if let Some(search) = params.search {
            let search_lower = search.to_lowercase();
            programs.retain(|p| {
                p.program_title.to_lowercase().contains(&search_lower)
                    || p.program_description
                        .as_ref()
                        .is_some_and(|d| d.to_lowercase().contains(&search_lower))
            });
        }

        if let Some(category) = params.category {
            programs.retain(|p| {
                p.program_category
                    .as_ref()
                    .is_some_and(|c| c.eq_ignore_ascii_case(&category))
            });
        }

        // Apply pagination manually (simplified approach)
        let total = programs.len() as u32;
        let start = ((page - 1) * limit) as usize;
        let end = (start + limit as usize).min(programs.len());
        let paginated_programs = if start < programs.len() {
            programs[start..end].to_vec()
        } else {
            Vec::new()
        };

        // Check streamability for all programs (database-only, no external HTTP requests)
        let channel_repo = crate::database::repositories::ChannelSeaOrmRepository::new(
            state.database.connection().clone(),
        );
        let mut streamable_channels = std::collections::HashSet::new();

        // Get unique channel IDs from programs
        let unique_channel_ids: std::collections::HashSet<_> =
            paginated_programs.iter().map(|p| &p.channel_id).collect();

        // Check each unique channel ID (UUID or tvg_id lookup)
        for channel_id in unique_channel_ids {
            let is_streamable =
                if let Ok(uuid) = crate::utils::uuid_parser::parse_uuid_flexible(channel_id) {
                    // Try UUID lookup first
                    channel_repo
                        .find_by_id(&uuid)
                        .await
                        .unwrap_or(None)
                        .is_some()
                } else {
                    // Try tvg_id lookup
                    channel_repo
                        .find_by_tvg_id(channel_id)
                        .await
                        .unwrap_or(None)
                        .is_some()
                };

            if is_streamable {
                streamable_channels.insert(channel_id.clone());
            }
        }

        let program_responses: Vec<EpgProgramResponse> = paginated_programs
            .into_iter()
            .map(|program| {
                let is_streamable = streamable_channels.contains(&program.channel_id);
                EpgProgramResponse {
                    id: program.id.to_string(),
                    channel_id: program.channel_id.to_string(),
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
                    is_streamable,
                }
            })
            .collect();

        let response = EpgListResponse {
            programs: program_responses,
            total: total as u64,
            page,
            limit,
            has_more: (start + limit as usize) < programs.len(),
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
    async fn inner(
        state: AppState,
        source_id_str: String,
        params: EpgQuery,
    ) -> AppResult<EpgListResponse> {
        let source_id = parse_uuid_flexible(&source_id_str).map_err(|e| AppError::Validation {
            message: format!("Invalid source ID format: {}", e),
        })?;

        // Use clean SeaORM repository (rationalized approach)
        let epg_source_repo = crate::database::repositories::EpgSourceSeaOrmRepository::new(
            state.database.connection().clone(),
        );

        // Verify source exists
        let _source = epg_source_repo
            .find_by_id(&source_id)
            .await
            .map_err(|e| AppError::Validation {
                message: e.to_string(),
            })?
            .ok_or_else(|| AppError::NotFound {
                resource: "EPG source".to_string(),
                id: source_id_str,
            })?;

        // Call rationalized logic using SeaORM repository
        let page = params.page.unwrap_or(1).max(1);
        let limit = params.limit.unwrap_or(50).min(200);

        let start_time = params.start_time.unwrap_or_else(Utc::now);
        let end_time = params
            .end_time
            .unwrap_or_else(|| Utc::now() + chrono::Duration::hours(24));

        let epg_program_repo = crate::database::repositories::EpgProgramSeaOrmRepository::new(
            state.database.connection().clone(),
        );

        // Get programs by time range for specific source
        let mut programs = epg_program_repo
            .find_by_time_range(Some(&source_id), &start_time, &end_time)
            .await
            .map_err(|e| AppError::Validation {
                message: e.to_string(),
            })?;

        // Apply additional filters in memory (simplified approach)
        if let Some(channel_id_str) = params.channel_id
            && let Ok(channel_id) = parse_uuid_flexible(&channel_id_str)
        {
            programs.retain(|p| p.channel_id == channel_id.to_string());
        }

        if let Some(search) = params.search {
            let search_lower = search.to_lowercase();
            programs.retain(|p| {
                p.program_title.to_lowercase().contains(&search_lower)
                    || p.program_description
                        .as_ref()
                        .is_some_and(|d| d.to_lowercase().contains(&search_lower))
            });
        }

        if let Some(category) = params.category {
            programs.retain(|p| {
                p.program_category
                    .as_ref()
                    .is_some_and(|c| c.eq_ignore_ascii_case(&category))
            });
        }

        // Apply pagination manually (simplified approach)
        let total = programs.len() as u32;
        let start = ((page - 1) * limit) as usize;
        let end = (start + limit as usize).min(programs.len());
        let paginated_programs = if start < programs.len() {
            programs[start..end].to_vec()
        } else {
            Vec::new()
        };

        // Check streamability for all programs (database-only, no external HTTP requests)
        let channel_repo = crate::database::repositories::ChannelSeaOrmRepository::new(
            state.database.connection().clone(),
        );
        let mut streamable_channels = std::collections::HashSet::new();

        // Get unique channel IDs from programs
        let unique_channel_ids: std::collections::HashSet<_> =
            paginated_programs.iter().map(|p| &p.channel_id).collect();

        // Check each unique channel ID (UUID or tvg_id lookup)
        for channel_id in unique_channel_ids {
            let is_streamable =
                if let Ok(uuid) = crate::utils::uuid_parser::parse_uuid_flexible(channel_id) {
                    // Try UUID lookup first
                    channel_repo
                        .find_by_id(&uuid)
                        .await
                        .unwrap_or(None)
                        .is_some()
                } else {
                    // Try tvg_id lookup
                    channel_repo
                        .find_by_tvg_id(channel_id)
                        .await
                        .unwrap_or(None)
                        .is_some()
                };

            if is_streamable {
                streamable_channels.insert(channel_id.clone());
            }
        }

        let program_responses: Vec<EpgProgramResponse> = paginated_programs
            .into_iter()
            .map(|program| {
                let is_streamable = streamable_channels.contains(&program.channel_id);
                EpgProgramResponse {
                    id: program.id.to_string(),
                    channel_id: program.channel_id.to_string(),
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
                    is_streamable,
                }
            })
            .collect();

        let response = EpgListResponse {
            programs: program_responses,
            total: total as u64,
            page,
            limit,
            has_more: (start + limit as usize) < programs.len(),
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
pub async fn list_epg_sources(State(state): State<AppState>) -> impl IntoResponse {
    async fn inner(state: AppState) -> AppResult<Vec<EpgSourceResponse>> {
        // Use clean SeaORM repository (rationalized approach)
        let epg_source_repo = crate::database::repositories::EpgSourceSeaOrmRepository::new(
            state.database.connection().clone(),
        );

        let sources = epg_source_repo
            .find_all()
            .await
            .map_err(|e| AppError::Validation {
                message: e.to_string(),
            })?;

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
    tag = "epg",
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
    async fn inner(
        state: AppState,
        params: EpgQuery,
    ) -> AppResult<HashMap<String, serde_json::Value>> {
        // This would return EPG data in a time-grid format suitable for TV guide display

        let start_time = params.start_time.unwrap_or_else(Utc::now);
        let end_time = params
            .end_time
            .unwrap_or_else(|| Utc::now() + chrono::Duration::hours(6));

        // Use clean SeaORM repository (rationalized approach)
        let epg_program_repo = crate::database::repositories::EpgProgramSeaOrmRepository::new(
            state.database.connection().clone(),
        );

        // Determine source filter
        let source_filter = if let Some(source_id_str) = params.source_id {
            parse_uuid_flexible(&source_id_str).ok()
        } else {
            None
        };

        // Get programs by time range
        let mut programs = epg_program_repo
            .find_by_time_range(source_filter.as_ref(), &start_time, &end_time)
            .await
            .map_err(|e| AppError::Validation {
                message: e.to_string(),
            })?;

        // Apply channel filter if specified
        if let Some(channel_id_str) = params.channel_id
            && let Ok(channel_id) = parse_uuid_flexible(&channel_id_str)
        {
            programs.retain(|p| p.channel_id == channel_id.to_string());
        }

        // No truncation - return all matching programs for comprehensive EPG display

        // Group programs by channel for grid display
        let mut grid_data = HashMap::new();
        let mut channels = HashMap::new();
        let mut time_slots = Vec::new();

        for program in programs {
            let channel_id = program.channel_id.to_string();

            // Track channels
            channels.insert(
                channel_id.clone(),
                serde_json::json!({
                    "id": channel_id,
                    "name": program.channel_name,
                    "logo": null // Not available in current model
                }),
            );

            // Add program to channel's schedule
            let programs = grid_data.entry(channel_id).or_insert_with(Vec::new);

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
            current_time += chrono::Duration::hours(1);
        }

        let response = HashMap::from([
            (
                "channels".to_string(),
                serde_json::to_value(channels).unwrap(),
            ),
            (
                "programs".to_string(),
                serde_json::to_value(grid_data).unwrap(),
            ),
            (
                "time_slots".to_string(),
                serde_json::to_value(time_slots).unwrap(),
            ),
            (
                "start_time".to_string(),
                serde_json::to_value(start_time).unwrap(),
            ),
            (
                "end_time".to_string(),
                serde_json::to_value(end_time).unwrap(),
            ),
        ]);

        Ok(response)
    }

    handle_result(inner(state, params).await)
}
