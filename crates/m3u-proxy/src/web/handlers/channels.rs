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
    database::repositories::{LastKnownCodecSeaOrmRepository, ChannelSeaOrmRepository},
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
    pub total_pages: u32,
}

/// Get all channels with filtering and pagination
#[utoipa::path(
    get,
    path = "/api/v1/channels",
    tag = "channels",
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

        // Use SeaORM connection for read operations
        let channel_repo = ChannelSeaOrmRepository::new(state.database.connection().clone());
        
        // Simple implementation using SeaORM repository
        let all_channels = channel_repo.find_all().await.map_err(|e| AppError::Validation { message: e.to_string() })?;
        
        // Apply basic filtering (client-side for now)
        let mut filtered_channels = all_channels;
        
        if let Some(search) = &params.search
            && !search.trim().is_empty() {
                let search_lower = search.trim().to_lowercase();
                filtered_channels.retain(|ch| ch.channel_name.to_lowercase().contains(&search_lower));
            }
        
        // Apply group title filtering
        if let Some(group) = &params.group
            && !group.trim().is_empty() {
                filtered_channels.retain(|ch| {
                    ch.group_title.as_ref().map(|g| g.eq_ignore_ascii_case(group.trim())).unwrap_or(false)
                });
            }
        
        // Parse and apply source ID filtering (support both source_id and legacy proxy_id)
        let mut source_ids = Vec::new();
        if let Some(source_id_str) = &params.source_id {
            for id_str in source_id_str.split(',') {
                if let Ok(uuid) = parse_uuid_flexible(id_str.trim()) {
                    source_ids.push(uuid);
                }
            }
        }
        if let Some(proxy_id_str) = &params.proxy_id {
            for id_str in proxy_id_str.split(',') {
                if let Ok(uuid) = parse_uuid_flexible(id_str.trim()) {
                    source_ids.push(uuid);
                }
            }
        }
        
        if !source_ids.is_empty() {
            filtered_channels.retain(|ch| source_ids.contains(&ch.source_id));
        }

        // Calculate pagination info
        let total_count = filtered_channels.len() as u64;
        let offset = ((page - 1) * limit) as usize;
        let end = std::cmp::min(offset + limit as usize, filtered_channels.len());
        
        let paginated_channels = if offset < filtered_channels.len() {
            &filtered_channels[offset..end]
        } else {
            &[]
        };

        // Get source names and codec info in bulk to avoid N+1 queries
        let source_ids: Vec<uuid::Uuid> = paginated_channels
            .iter()
            .filter(|c| c.source_id != uuid::Uuid::nil())
            .map(|c| c.source_id)
            .collect();
        
        
        let source_names = if !source_ids.is_empty() {
            // Use SeaORM repository to get source names
            let stream_source_repo = crate::database::repositories::StreamSourceSeaOrmRepository::new(state.database.connection().clone());
            let mut names = std::collections::HashMap::new();
            
            // Get source names by fetching sources by ID
            for source_id in &source_ids {
                if let Ok(Some(source)) = stream_source_repo.find_by_id(source_id).await {
                    names.insert(*source_id, source.name);
                }
            }
            
            names
        } else {
            std::collections::HashMap::new()
        };
        
        // Get codec information for all channels by stream URL
        let codec_repo = LastKnownCodecSeaOrmRepository::new(state.database.connection().clone());
        let mut codec_info = std::collections::HashMap::new();
        for channel in paginated_channels.iter() {
            if let Ok(Some(codec)) = codec_repo.get_latest_codec_info(&channel.stream_url).await {
                codec_info.insert(channel.id, codec);
            }
        }
        
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
            
            let codec = codec_info.get(&channel.id);
            
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
                // Codec information from last_known_codecs table
                video_codec: codec.as_ref().and_then(|c| c.video_codec.clone()),
                audio_codec: codec.as_ref().and_then(|c| c.audio_codec.clone()),
                resolution: codec.as_ref().and_then(|c| c.resolution.clone()),
                last_probed_at: codec.as_ref().map(|c| c.detected_at.to_rfc3339()),
                probe_method: codec.as_ref().map(|c| format!("{:?}", c.probe_method)),
            });
        }

        let has_more = end < filtered_channels.len();
        
        let total_pages = (total_count as f64 / limit as f64).ceil() as u32;
        
        let response = ChannelsListResponse {
            channels: channel_responses,
            total: total_count,
            page,
            limit,
            has_more,
            total_pages,
        };

        Ok(response)
    }
    
    handle_result(inner(state, params).await)
}

/// Get channels for a specific proxy
#[utoipa::path(
    get,
    path = "/api/v1/channels/proxy/{proxy_id}",
    tag = "channels",
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

        // Use SeaORM connection for read operations
        let channel_repo = ChannelSeaOrmRepository::new(state.database.connection().clone());
        
        // Get channels for the specific source using SeaORM repository
        let source_channels = channel_repo.find_by_source_id(&proxy_id).await.map_err(|e| AppError::Validation { message: e.to_string() })?;
        
        // Apply filtering
        let mut filtered_channels = source_channels;
        
        if let Some(search) = &params.search
            && !search.trim().is_empty() {
                let search_lower = search.trim().to_lowercase();
                filtered_channels.retain(|ch| ch.channel_name.to_lowercase().contains(&search_lower));
            }
        
        if let Some(group) = &params.group
            && !group.trim().is_empty() {
                filtered_channels.retain(|ch| {
                    ch.group_title.as_ref().map(|g| g.eq_ignore_ascii_case(group.trim())).unwrap_or(false)
                });
            }

        // Apply pagination 
        let total = filtered_channels.len() as u32;
        let total_pages = (total as f64 / limit as f64).ceil() as u32;
        let start = ((page - 1) * limit) as usize;
        let end = (start + limit as usize).min(filtered_channels.len());
        
        let paginated_channels = if start < filtered_channels.len() {
            filtered_channels[start..end].to_vec()
        } else {
            Vec::new()
        };

        // Convert Channel models to ChannelResponse
        let channel_responses: Vec<ChannelResponse> = paginated_channels
            .into_iter()
            .map(|channel| ChannelResponse {
                id: channel.id.to_string(),
                name: channel.channel_name,
                logo_url: channel.tvg_logo,
                group: channel.group_title,
                stream_url: channel.stream_url,
                proxy_id: None,
                source_type: "source".to_string(),
                source_name: None,
                tvg_id: channel.tvg_id,
                tvg_name: channel.tvg_name,
                tvg_chno: channel.tvg_chno,
                tvg_shift: channel.tvg_shift,
                video_codec: channel.video_codec,
                audio_codec: channel.audio_codec,
                resolution: channel.resolution,
                last_probed_at: channel.last_probed_at.map(|dt| dt.to_rfc3339()),
                probe_method: channel.probe_method,
            })
            .collect();
        
        let has_more = end < filtered_channels.len();

        let response = ChannelsListResponse {
            channels: channel_responses,
            total: total as u64,
            page,
            limit,
            has_more,
            total_pages,
        };
        
        Ok(response)
    }
    
    handle_result(inner(state, proxy_id_str, params).await)
}

/// Get stream URL for a specific channel
#[utoipa::path(
    get,
    path = "/api/v1/channels/{channel_id}/stream",
    tag = "streaming",
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
            let channel_repo = ChannelSeaOrmRepository::new(state.database.connection().clone());
            
            let channel = channel_repo
                .find_by_id(&channel_uuid)
                .await.map_err(|e| AppError::Validation { message: e.to_string() })?
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
        let channel_repo = ChannelSeaOrmRepository::new(state.database.connection().clone());
        
        let channel = channel_repo
            .find_by_id(&channel_uuid)
            .await.map_err(|e| AppError::Validation { message: e.to_string() })?
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
                // Store failed probe attempt in database
                let codec_repo = LastKnownCodecSeaOrmRepository::new(state.database.connection().clone());
                let failed_codec_request = crate::models::last_known_codec::CreateLastKnownCodecRequest {
                    video_codec: None,
                    audio_codec: None,
                    container_format: None,
                    resolution: None,
                    framerate: None,
                    bitrate: None,
                    video_bitrate: None,
                    audio_bitrate: None,
                    audio_channels: None,
                    audio_sample_rate: None,
                    probe_method: crate::models::last_known_codec::ProbeMethod::FfprobeManual,
                    probe_source: Some(format!("admin_failed: {}", e)),
                };
                
                // Store the failed probe attempt (ignore errors from this operation, with timeout)
                let _ = tokio::time::timeout(
                    std::time::Duration::from_secs(5),
                    codec_repo.upsert_codec_info(&stream_url, failed_codec_request)
                ).await;

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

        // Store codec information using SeaORM connection
        let codec_repo = LastKnownCodecSeaOrmRepository::new(state.database.connection().clone());
        
        let codec_request = crate::models::last_known_codec::CreateLastKnownCodecRequest {
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
        if let Err(e) = codec_repo.upsert_codec_info(&stream_url, codec_request).await {
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

/// Proxy a channel stream directly (solves CORS issues)
#[utoipa::path(
    get,
    path = "/channel/{channel_id}/stream",
    params(
        ("channel_id" = String, Path, description = "Channel ID (UUID) or EPG Channel ID (tvg_id)")
    ),
    responses(
        (status = 200, description = "Stream content proxied successfully", content_type = "video/mp2t"),
        (status = 404, description = "Channel not found"),
        (status = 502, description = "Stream source unavailable"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn proxy_channel_stream(
    State(state): State<AppState>,
    Path(channel_id): Path<String>,
    headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    use axum::http::StatusCode;
    use tracing::{debug, error, info, warn};

    let client_ip = headers
        .get("x-forwarded-for")
        .or_else(|| headers.get("x-real-ip"))
        .and_then(|h| h.to_str().ok())
        .unwrap_or("unknown")
        .to_string();

    let user_agent = headers
        .get("user-agent")
        .and_then(|h| h.to_str().ok())
        .map(|s| s.to_string());

    let referer = headers
        .get("referer")
        .and_then(|h| h.to_str().ok())
        .map(|s| s.to_string());

    debug!(
        "Direct channel stream request: channel_id={}, client_ip={}, user_agent={}",
        channel_id,
        client_ip,
        user_agent.as_deref().unwrap_or("unknown")
    );

    // Smart channel resolution: try UUID first, then tvg_id
    let channel_repo = ChannelSeaOrmRepository::new(state.database.connection().clone());
    
    let channel = if let Ok(channel_uuid) = parse_uuid_flexible(&channel_id) {
        // Try direct UUID lookup first
        match channel_repo.find_by_id(&channel_uuid).await {
            Ok(Some(channel)) => channel,
            Ok(None) => {
                debug!("Channel not found by UUID {}, trying tvg_id lookup", channel_id);
                // Fallback to tvg_id lookup
                match channel_repo.find_by_tvg_id(&channel_id).await {
                    Ok(Some(channel)) => channel,
                    Ok(None) => {
                        warn!("Channel {} not found by UUID or tvg_id", channel_id);
                        return (StatusCode::NOT_FOUND, "Channel not found".to_string()).into_response();
                    }
                    Err(e) => {
                        error!("Failed to lookup channel by tvg_id {}: {}", channel_id, e);
                        return (StatusCode::INTERNAL_SERVER_ERROR, "Database error".to_string()).into_response();
                    }
                }
            }
            Err(e) => {
                error!("Failed to lookup channel by UUID {}: {}", channel_id, e);
                return (StatusCode::INTERNAL_SERVER_ERROR, "Database error".to_string()).into_response();
            }
        }
    } else {
        // Not a valid UUID, try tvg_id lookup directly
        debug!("Invalid UUID format for {}, trying tvg_id lookup", channel_id);
        match channel_repo.find_by_tvg_id(&channel_id).await {
            Ok(Some(channel)) => channel,
            Ok(None) => {
                warn!("Channel {} not found by tvg_id", channel_id);
                return (StatusCode::NOT_FOUND, "Channel not found".to_string()).into_response();
            }
            Err(e) => {
                error!("Failed to lookup channel by tvg_id {}: {}", channel_id, e);
                return (StatusCode::INTERNAL_SERVER_ERROR, "Database error".to_string()).into_response();
            }
        }
    };

    info!(
        "Proxying direct channel stream: '{}' from URL: {}",
        channel.channel_name, channel.stream_url
    );

    // Create simplified session tracking for direct channel access
    let session_id = format!("direct_{}_{}", channel.id, uuid::Uuid::new_v4());
    
    // Create session stats for the proxy function
    let client_info = crate::proxy::session_tracker::ClientInfo {
        ip: client_ip.clone(),
        user_agent: user_agent.clone(),
        referer: referer.clone(),
    };

    let session_stats = crate::proxy::session_tracker::SessionStats::new(
        session_id.clone(),
        client_info,
        "direct_channel".to_string(),
        "direct_channel".to_string(),
        channel.id.to_string(),
        channel.channel_name.clone(),
        channel.stream_url.clone(),
    );

    // Start session tracking
    state.session_tracker.start_session(session_stats.clone()).await;

    // Use the existing proven proxy implementation from proxies.rs
    proxy_http_stream_for_channel(
        channel.stream_url.clone(),
        headers,
        state.session_tracker.clone(),
        session_stats,
    ).await
}

/// HTTP stream proxy implementation for channel streams (reuses proxy logic)
async fn proxy_http_stream_for_channel(
    stream_url: String,
    _headers: axum::http::HeaderMap,
    session_tracker: std::sync::Arc<crate::proxy::session_tracker::SessionTracker>,
    session_stats: crate::proxy::session_tracker::SessionStats,
) -> axum::response::Response<axum::body::Body> {
    use axum::response::Response;
    use axum::body::Body;
    use axum::http::{header, StatusCode};
    use tracing::{error, info};
    
    info!("Proxying HTTP channel stream from: {}", stream_url);
    
    // Create HTTP client for proxying
    let client = reqwest::Client::builder()
        .user_agent("m3u-proxy/channel-stream")
        .timeout(std::time::Duration::from_secs(30))
        .build();
    
    let client = match client {
        Ok(client) => client,
        Err(e) => {
            error!("Failed to create HTTP client: {}", e);
            session_tracker.end_session(&session_stats.session_id).await;
            return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to create HTTP client").into_response();
        }
    };
    
    // Make request to source stream
    let response = match client.get(&stream_url).send().await {
        Ok(response) => response,
        Err(e) => {
            error!("Failed to connect to stream URL {}: {}", stream_url, e);
            session_tracker.end_session(&session_stats.session_id).await;
            return (StatusCode::BAD_GATEWAY, format!("Failed to connect to stream: {}", e)).into_response();
        }
    };
    
    // Check if source responded successfully
    if !response.status().is_success() {
        error!("Stream source returned error: {}", response.status());
        session_tracker.end_session(&session_stats.session_id).await;
        return (StatusCode::BAD_GATEWAY, format!("Stream source error: {}", response.status())).into_response();
    }
    
    // Extract content type from source
    let content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|ct| ct.to_str().ok())
        .unwrap_or("video/mp2t") // Default to MPEG-TS for streams
        .to_string();
    
    // Extract content length if available
    let content_length: Option<u64> = response
        .headers()
        .get(header::CONTENT_LENGTH)
        .and_then(|cl| cl.to_str().ok())
        .and_then(|cl| cl.parse().ok());
    
    info!("Proxying stream: content_type={}, content_length={:?}", content_type, content_length);
    
    // Get response body as stream
    let stream_body = response.bytes_stream();
    
    // Convert reqwest stream to axum body
    let body = Body::from_stream(stream_body);
    
    // Create response with proper headers
    let mut response_builder = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CACHE_CONTROL, "no-cache")
        .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
        .header(header::ACCESS_CONTROL_ALLOW_METHODS, "GET, OPTIONS")
        .header(header::ACCESS_CONTROL_ALLOW_HEADERS, "Content-Type, Range");
    
    // Add content length if we have it
    if let Some(length) = content_length {
        response_builder = response_builder.header(header::CONTENT_LENGTH, length);
    }
    
    let response = match response_builder.body(body) {
        Ok(response) => response,
        Err(e) => {
            error!("Failed to build response: {}", e);
            session_tracker.end_session(&session_stats.session_id).await;
            return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to build response").into_response();
        }
    };
    
    info!("Successfully started proxying channel stream");
    response
}