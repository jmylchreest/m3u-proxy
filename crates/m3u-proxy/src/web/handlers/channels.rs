//! Channel browser API handlers
//!
//! Provides endpoints for browsing channels from database and M3U sources

use axum::{
    extract::{Query, State, Path},
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::{
    repositories::{
        ChannelRepository,
        channel::{ChannelQuery},
        traits::{Repository, PaginatedRepository},
    },
    web::{AppState, responses::handle_result},
    utils::uuid_parser::parse_uuid_flexible,
    errors::{AppResult, AppError},
};

#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct ChannelsQuery {
    /// Filter by source ID (stream source or proxy) - can be comma-separated for multiple values
    pub source_id: Option<String>,
    /// Filter by proxy ID for M3U sources (deprecated - use source_id)
    pub proxy_id: Option<String>,
    /// Search term for channel name
    pub search: Option<String>,
    /// Filter by channel group
    pub group: Option<String>,
    /// Filter by country
    pub country: Option<String>,
    /// Filter by language
    pub language: Option<String>,
    /// Pagination: page number (0-based)
    pub page: Option<u32>,
    /// Pagination: items per page
    pub limit: Option<u32>,
    /// Sort field
    pub sort_by: Option<String>,
    /// Sort direction
    pub sort_order: Option<String>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct ChannelResponse {
    pub id: String,
    pub name: String,
    pub logo_url: Option<String>,
    pub group: Option<String>,
    pub stream_url: String,
    pub proxy_id: Option<String>,
    pub source_type: String, // "database" | "source" | "proxy"
    pub source_name: Option<String>, // Actual name of the source
    // M3U specific fields
    pub tvg_id: Option<String>,
    pub tvg_name: Option<String>,
    pub tvg_chno: Option<String>,
    pub tvg_shift: Option<String>,
    // Codec information
    pub video_codec: Option<String>,
    pub audio_codec: Option<String>,
    pub resolution: Option<String>,
    pub last_probed_at: Option<String>, // ISO 8601 datetime string
    pub probe_method: Option<String>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct ChannelsListResponse {
    pub channels: Vec<ChannelResponse>,
    pub total: u64,
    pub page: u32,
    pub limit: u32,
    pub has_more: bool,
}

/// Get all channels with filtering and pagination
#[utoipa::path(
    get,
    path = "/api/v1/channels",
    params(ChannelsQuery),
    responses(
        (status = 200, description = "Channels retrieved successfully", body = ChannelsListResponse),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn list_channels(
    State(state): State<AppState>,
    Query(params): Query<ChannelsQuery>,
) -> impl IntoResponse {
    async fn inner(state: AppState, params: ChannelsQuery) -> AppResult<ChannelsListResponse> {
        let page = params.page.unwrap_or(1).max(1); // Pages are 1-based
        let limit = params.limit.unwrap_or(50).min(500); // Cap at 500 items per page

        // Use read pool for read-only operations
        let channel_repo = ChannelRepository::new(state.database.read_pool());
        
        // Build query from request parameters
        let mut query = ChannelQuery::new();
        
        if let Some(search) = params.search {
            query.base.search = Some(search);
        }
        
        if let Some(group) = params.group {
            query = query.group_title(group);
        }
        
        // Support filtering by source_id (new) or proxy_id (deprecated)
        if let Some(source_id_str) = &params.source_id {
            if !source_id_str.is_empty() {
                // Split comma-separated values
                let source_id_parts: Vec<&str> = source_id_str.split(',').map(|s| s.trim()).collect();
                
                if source_id_parts.len() == 1 {
                    // Single source ID
                    if let Ok(source_uuid) = parse_uuid_flexible(source_id_parts[0]) {
                        query = query.source_id(source_uuid);
                    }
                } else {
                    // Multiple source IDs
                    let mut source_uuids = Vec::new();
                    for source_id_part in source_id_parts {
                        if let Ok(source_uuid) = parse_uuid_flexible(source_id_part) {
                            source_uuids.push(source_uuid);
                        }
                    }
                    if !source_uuids.is_empty() {
                        query = query.source_ids(source_uuids);
                    }
                }
            }
        } else if let Some(proxy_id_str) = &params.proxy_id {
            // Support legacy proxy_id parameter
            if let Ok(proxy_uuid) = parse_uuid_flexible(proxy_id_str) {
                query = query.source_id(proxy_uuid);
            }
        }

        // Get channels with codec information using the new method
        let channels_with_codecs = channel_repo
            .find_channels_with_codecs(query)
            .await?;

        // Calculate pagination info
        let total_count = channels_with_codecs.len() as u64;
        let offset = ((page - 1) * limit) as usize;
        let end = std::cmp::min(offset + limit as usize, channels_with_codecs.len());
        
        let paginated_channels = if offset < channels_with_codecs.len() {
            &channels_with_codecs[offset..end]
        } else {
            &[]
        };

        // Get source names in bulk to avoid N+1 queries
        let source_ids: Vec<uuid::Uuid> = paginated_channels
            .iter()
            .filter(|c| c.source_id != uuid::Uuid::nil())
            .map(|c| c.source_id)
            .collect();
        
        let source_names = if !source_ids.is_empty() {
            // Use repository to get source names
            match crate::repositories::StreamSourceRepository::new(state.database.read_pool())
                .get_source_names(&source_ids)
                .await
            {
                Ok(names) => names,
                Err(_) => std::collections::HashMap::new(),
            }
        } else {
            std::collections::HashMap::new()
        };
        
        let mut channel_responses: Vec<ChannelResponse> = Vec::new();
        for channel in paginated_channels {
            let (stream_url, proxy_id, source_type, source_name) = if channel.source_id != uuid::Uuid::nil() {
                // Channel from a stream source - always use direct URL
                let source_name = source_names.get(&channel.source_id).cloned();
                (channel.stream_url.clone(), None, "source".to_string(), source_name)
            } else {
                // No source_id - this is a database channel
                (channel.stream_url.clone(), None, "database".to_string(), None)
            };
            
            channel_responses.push(ChannelResponse {
                id: channel.id.to_string(),
                name: channel.channel_name.clone(),
                logo_url: channel.tvg_logo.clone(),
                group: channel.group_title.clone(),
                stream_url,
                proxy_id,
                source_type,
                source_name,
                // M3U specific fields from database
                tvg_id: channel.tvg_id.clone(),
                tvg_name: channel.tvg_name.clone(),
                tvg_chno: channel.tvg_chno.clone(),
                tvg_shift: channel.tvg_shift.clone(),
                // Codec information
                video_codec: channel.video_codec.clone(),
                audio_codec: channel.audio_codec.clone(),
                resolution: channel.resolution.clone(),
                last_probed_at: channel.last_probed_at.map(|dt| dt.to_rfc3339()),
                probe_method: channel.probe_method.as_ref().map(|pm| pm.to_string()),
            });
        }

        let has_more = end < channels_with_codecs.len();
        
        let response = ChannelsListResponse {
            channels: channel_responses,
            total: total_count,
            page,
            limit,
            has_more,
        };

        Ok(response)
    }
    
    handle_result(inner(state, params).await)
}

/// Get channels for a specific proxy
#[utoipa::path(
    get,
    path = "/api/v1/channels/proxy/{proxy_id}",
    params(
        ("proxy_id" = String, Path, description = "Proxy ID")
    ),
    responses(
        (status = 200, description = "Proxy channels retrieved successfully", body = ChannelsListResponse),
        (status = 404, description = "Proxy not found"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_proxy_channels(
    State(state): State<AppState>,
    Path(proxy_id_str): Path<String>,
    Query(params): Query<ChannelsQuery>,
) -> impl IntoResponse {
    async fn inner(state: AppState, proxy_id_str: String, params: ChannelsQuery) -> AppResult<ChannelsListResponse> {
        let proxy_id = parse_uuid_flexible(&proxy_id_str)
            .map_err(|e| AppError::Validation { message: format!("Invalid proxy ID format: {}", e) })?;

        let page = params.page.unwrap_or(1).max(1);
        let limit = params.limit.unwrap_or(50).min(500);

        // Use read pool for read-only operations
        let channel_repo = ChannelRepository::new(state.database.read_pool());
        
        // Get channels for the specific source (which represents the M3U proxy)
        let mut query = ChannelQuery::new().source_id(proxy_id);
        
        if let Some(search) = params.search {
            query.base.search = Some(search);
        }
        
        if let Some(group) = params.group {
            query = query.group_title(group);
        }

        let paginated_result = channel_repo
            .find_paginated(query, page, limit)
            .await?;

        let channel_responses: Vec<ChannelResponse> = paginated_result
            .items
            .into_iter()
            .map(|channel| {
                // For proxy channels, always generate the relay streaming URL
                let relay_url = format!("/stream/{}/{}", proxy_id, channel.id);
                
                ChannelResponse {
                    id: channel.id.to_string(),
                    name: channel.channel_name,
                    logo_url: channel.tvg_logo,
                    group: channel.group_title,
                    stream_url: relay_url,
                    proxy_id: Some(proxy_id.to_string()),
                    source_type: "proxy".to_string(),
                    source_name: None, // TODO: Get proxy name instead of source name for proxy channels
                    // M3U specific fields from database
                    tvg_id: channel.tvg_id,
                    tvg_name: channel.tvg_name,
                    tvg_chno: channel.tvg_chno,
                    tvg_shift: channel.tvg_shift,
                    // Codec information (not available for legacy proxy endpoint)
                    video_codec: None,
                    audio_codec: None,
                    resolution: None,
                    last_probed_at: None,
                    probe_method: None,
                }
            })
            .collect();

        let response = ChannelsListResponse {
            channels: channel_responses,
            total: paginated_result.total_count,
            page: paginated_result.page,
            limit: paginated_result.limit,
            has_more: paginated_result.has_next,
        };

        Ok(response)
    }
    
    handle_result(inner(state, proxy_id_str, params).await)
}

/// Get stream URL for a specific channel
#[utoipa::path(
    get,
    path = "/api/v1/channels/{channel_id}/stream",
    params(
        ("channel_id" = String, Path, description = "Channel ID")
    ),
    responses(
        (status = 200, description = "Stream URL retrieved successfully"),
        (status = 404, description = "Channel not found"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_channel_stream(
    State(state): State<AppState>,
    Path(channel_id): Path<String>,
) -> impl IntoResponse {
    async fn inner(state: AppState, channel_id: String) -> AppResult<HashMap<String, String>> {
        // For database channels
        if let Ok(channel_uuid) = parse_uuid_flexible(&channel_id) {
            // Use read pool for read-only operations
            let channel_repo = ChannelRepository::new(state.database.read_pool());
            
            let channel = channel_repo
                .find_by_id(channel_uuid)
                .await?
                .ok_or_else(|| AppError::NotFound { 
                    resource: "Channel".to_string(), 
                    id: channel_id.clone() 
                })?;

            let mut response = HashMap::new();
            
            // Simplified logic - no proxy checking needed
            if channel.source_id != uuid::Uuid::nil() {
                // Channel from a stream source - use direct URL
                response.insert("stream_url".to_string(), channel.stream_url);
                response.insert("source_type".to_string(), "source".to_string());
            } else {
                // No source_id - this is a database channel
                response.insert("stream_url".to_string(), channel.stream_url);
                response.insert("source_type".to_string(), "database".to_string());
            }

            return Ok(response);
        }

        // For M3U channels, channel_id would be in format "proxy_id:channel_index"
        if channel_id.contains(':') {
            let parts: Vec<&str> = channel_id.split(':').collect();
            if parts.len() == 2 {
                // Parse M3U channel - simplified implementation
                let mut response = HashMap::new();
                response.insert("stream_url".to_string(), "".to_string());
                response.insert("source_type".to_string(), "m3u".to_string());

                return Ok(response);
            }
        }

        Err(AppError::NotFound { 
            resource: "Channel".to_string(), 
            id: channel_id 
        })
    }
    
    handle_result(inner(state, channel_id).await)
}

/// Probe codec information for a specific channel using ffprobe
#[utoipa::path(
    post,
    path = "/api/v1/channels/{channel_id}/probe",
    tag = "channels",
    summary = "Probe channel codec information",
    description = "Run ffprobe on a channel to detect and store codec information",
    params(
        ("channel_id" = String, Path, description = "Channel ID")
    ),
    responses(
        (status = 200, description = "Channel probed successfully", body = HashMap<String, String>),
        (status = 404, description = "Channel not found"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn probe_channel_codecs(
    State(state): State<AppState>,
    Path(channel_id): Path<String>,
) -> impl IntoResponse {
    async fn inner(state: AppState, channel_id: String) -> AppResult<std::collections::HashMap<String, String>> {
        let channel_uuid = parse_uuid_flexible(&channel_id)
            .map_err(|e| AppError::Validation { message: format!("Invalid channel ID format: {}", e) })?;
        
        // Use read pool to get channel information
        let channel_repo = ChannelRepository::new(state.database.read_pool());
        
        let channel = channel_repo
            .find_by_id(channel_uuid)
            .await?
            .ok_or_else(|| AppError::NotFound { 
                resource: "Channel".to_string(), 
                id: channel_id.clone() 
            })?;

        // Get the stream URL for probing
        let stream_url = if channel.source_id != uuid::Uuid::nil() {
            // Channel from a stream source - use direct URL
            channel.stream_url
        } else {
            // Database channel
            channel.stream_url
        };

        // Create a simple StreamProber to probe the stream
        let stream_prober = crate::services::stream_prober::StreamProber::new(None);
        
        let probe_result = match stream_prober.probe_input(&stream_url).await {
            Ok(probe_info) => probe_info,
            Err(e) => {
                return Ok(std::collections::HashMap::from([
                    ("success".to_string(), "false".to_string()),
                    ("error".to_string(), format!("Failed to probe stream: {}", e)),
                ]));
            }
        };

        // Extract codec information from probe result
        let video_codec = probe_result.video_streams.first()
            .map(|s| s.codec_name.clone());
        
        let audio_codec = probe_result.audio_streams.first()
            .map(|s| s.codec_name.clone());
        
        let resolution = probe_result.video_streams.first()
            .and_then(|s| {
                if let (Some(width), Some(height)) = (s.width, s.height) {
                    Some(format!("{}x{}", width, height))
                } else {
                    None
                }
            });

        // Store codec information using write pool
        let write_channel_repo = ChannelRepository::new(state.database.pool());
        
        let codec_request = crate::models::last_known_codec::CreateLastKnownCodecRequest {
            channel_id: channel_uuid,
            video_codec: video_codec.clone(),
            audio_codec: audio_codec.clone(),
            container_format: probe_result.format_name.clone(),
            resolution: resolution.clone(),
            framerate: probe_result.video_streams.first()
                .and_then(|s| s.r_frame_rate.clone()),
            bitrate: probe_result.bit_rate.map(|br| br as i32),
            video_bitrate: probe_result.video_streams.first()
                .and_then(|s| s.bit_rate.map(|br| br as i32)),
            audio_bitrate: probe_result.audio_streams.first()
                .and_then(|s| s.bit_rate.map(|br| br as i32)),
            audio_channels: probe_result.audio_streams.first()
                .and_then(|s| s.channels.map(|c| c.to_string())),
            audio_sample_rate: probe_result.audio_streams.first()
                .and_then(|s| s.sample_rate.map(|sr| sr as i32)),
            probe_method: crate::models::last_known_codec::ProbeMethod::FfprobeManual,
            probe_source: Some("admin".to_string()),
        };

        // Store the codec information
        if let Err(e) = write_channel_repo.upsert_codec_info(channel_uuid, codec_request).await {
            return Ok(std::collections::HashMap::from([
                ("success".to_string(), "false".to_string()),
                ("error".to_string(), format!("Failed to store codec information: {}", e)),
            ]));
        }

        // Return success with codec information
        let mut response = std::collections::HashMap::new();
        response.insert("success".to_string(), "true".to_string());
        response.insert("message".to_string(), "Channel probed successfully".to_string());
        
        if let Some(vc) = video_codec {
            response.insert("video_codec".to_string(), vc);
        }
        if let Some(ac) = audio_codec {
            response.insert("audio_codec".to_string(), ac);
        }
        if let Some(res) = resolution {
            response.insert("resolution".to_string(), res);
        }

        Ok(response)
    }
    
    handle_result(inner(state, channel_id).await)
}