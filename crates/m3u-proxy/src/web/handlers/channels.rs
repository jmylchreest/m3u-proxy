//! Channel browser API handlers
//!
//! Provides endpoints for browsing channels from database and M3U sources

use axum::{
    extract::{Path, Query, State},
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::{
    database::repositories::{ChannelSeaOrmRepository, LastKnownCodecSeaOrmRepository},
    errors::{AppError, AppResult},
    utils::uuid_parser::parse_uuid_flexible,
    web::{AppState, responses::handle_result},
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
    pub source_type: String,         // "database" | "source" | "proxy"
    pub source_name: Option<String>, // Actual name of the source
    pub source_id: Option<String>,   // UUID of underlying stream source if present
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
        let all_channels = channel_repo
            .find_all()
            .await
            .map_err(|e| AppError::Validation {
                message: e.to_string(),
            })?;

        // Apply basic filtering (client-side for now)
        let mut filtered_channels = all_channels;

        if let Some(search) = &params.search
            && !search.trim().is_empty()
        {
            let search_lower = search.trim().to_lowercase();
            filtered_channels.retain(|ch| ch.channel_name.to_lowercase().contains(&search_lower));
        }

        // Apply group title filtering
        if let Some(group) = &params.group
            && !group.trim().is_empty()
        {
            filtered_channels.retain(|ch| {
                ch.group_title
                    .as_ref()
                    .map(|g| g.eq_ignore_ascii_case(group.trim()))
                    .unwrap_or(false)
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
            let stream_source_repo =
                crate::database::repositories::StreamSourceSeaOrmRepository::new(
                    state.database.connection().clone(),
                );
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
            let (stream_url, proxy_id, source_type, source_name) =
                if channel.source_id != uuid::Uuid::nil() {
                    // Channel from a stream source - always use direct URL
                    let source_name = source_names.get(&channel.source_id).cloned();
                    (
                        channel.stream_url.clone(),
                        None,
                        "source".to_string(),
                        source_name,
                    )
                } else {
                    // No source_id - this is a database channel
                    (
                        channel.stream_url.clone(),
                        None,
                        "database".to_string(),
                        None,
                    )
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
                source_id: (channel.source_id != uuid::Uuid::nil())
                    .then(|| channel.source_id.to_string()),
                // M3U specific fields
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
    async fn inner(
        state: AppState,
        proxy_id_str: String,
        params: ChannelsQuery,
    ) -> AppResult<ChannelsListResponse> {
        let proxy_id = parse_uuid_flexible(&proxy_id_str).map_err(|e| AppError::Validation {
            message: format!("Invalid proxy ID format: {}", e),
        })?;

        let page = params.page.unwrap_or(1).max(1);
        let limit = params.limit.unwrap_or(50).min(500);

        // Use SeaORM connection for read operations
        let channel_repo = ChannelSeaOrmRepository::new(state.database.connection().clone());

        // Get channels for the specific source using SeaORM repository
        let source_channels = channel_repo
            .find_by_source_id(&proxy_id)
            .await
            .map_err(|e| AppError::Validation {
                message: e.to_string(),
            })?;

        // Apply filtering
        let mut filtered_channels = source_channels;

        if let Some(search) = &params.search
            && !search.trim().is_empty()
        {
            let search_lower = search.trim().to_lowercase();
            filtered_channels.retain(|ch| ch.channel_name.to_lowercase().contains(&search_lower));
        }

        if let Some(group) = &params.group
            && !group.trim().is_empty()
        {
            filtered_channels.retain(|ch| {
                ch.group_title
                    .as_ref()
                    .map(|g| g.eq_ignore_ascii_case(group.trim()))
                    .unwrap_or(false)
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
                source_id: (channel.source_id != uuid::Uuid::nil())
                    .then(|| channel.source_id.to_string()),
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
                .await
                .map_err(|e| AppError::Validation {
                    message: e.to_string(),
                })?
                .ok_or_else(|| AppError::NotFound {
                    resource: "Channel".to_string(),
                    id: channel_id.clone(),
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
            id: channel_id,
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
    async fn inner(
        state: AppState,
        channel_id: String,
    ) -> AppResult<crate::models::last_known_codec::LastKnownCodec> {
        let channel_uuid = parse_uuid_flexible(&channel_id).map_err(|e| AppError::Validation {
            message: format!("Invalid channel ID format: {}", e),
        })?;
        let channel_repo = ChannelSeaOrmRepository::new(state.database.connection().clone());
        let channel = channel_repo
            .find_by_id(&channel_uuid)
            .await
            .map_err(|e| AppError::Validation {
                message: e.to_string(),
            })?
            .ok_or_else(|| AppError::NotFound {
                resource: "Channel".to_string(),
                id: channel_id.clone(),
            })?;
        let stream_url = channel.stream_url;
        let persistence =
            state
                .probe_persistence_service
                .clone()
                .ok_or_else(|| AppError::Validation {
                    message: "Probe persistence unavailable (ffprobe not configured)".to_string(),
                })?;
        let stored = persistence
            .probe_and_persist(
                &stream_url,
                crate::models::last_known_codec::ProbeMethod::FfprobeManual,
                Some("admin".to_string()),
            )
            .await
            .map_err(|e| AppError::Validation {
                message: format!("Probe failed: {}", e),
            })?;
        Ok(stored)
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
                debug!(
                    "Channel not found by UUID {}, trying tvg_id lookup",
                    channel_id
                );
                // Fallback to tvg_id lookup
                match channel_repo.find_by_tvg_id(&channel_id).await {
                    Ok(Some(channel)) => channel,
                    Ok(None) => {
                        warn!("Channel {} not found by UUID or tvg_id", channel_id);
                        return (StatusCode::NOT_FOUND, "Channel not found".to_string())
                            .into_response();
                    }
                    Err(e) => {
                        error!("Failed to lookup channel by tvg_id {}: {}", channel_id, e);
                        return (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            "Database error".to_string(),
                        )
                            .into_response();
                    }
                }
            }
            Err(e) => {
                error!("Failed to lookup channel by UUID {}: {}", channel_id, e);
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Database error".to_string(),
                )
                    .into_response();
            }
        }
    } else {
        // Not a valid UUID, try tvg_id lookup directly
        debug!(
            "Invalid UUID format for {}, trying tvg_id lookup",
            channel_id
        );
        match channel_repo.find_by_tvg_id(&channel_id).await {
            Ok(Some(channel)) => channel,
            Ok(None) => {
                warn!("Channel {} not found by tvg_id", channel_id);
                return (StatusCode::NOT_FOUND, "Channel not found".to_string()).into_response();
            }
            Err(e) => {
                error!("Failed to lookup channel by tvg_id {}: {}", channel_id, e);
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Database error".to_string(),
                )
                    .into_response();
            }
        }
    };

    info!(
        "Proxying direct channel stream: '{}' from URL: {}",
        channel.channel_name, channel.stream_url
    );

    // -----------------------------------------------------------------------------------------
    // Unified classification (reuse rich hybrid logic from proxy mode)
    // -----------------------------------------------------------------------------------------
    use crate::streaming::classification::{
        ClassificationParams, StreamModeDecision, classify_stream,
    };

    let classification_result = classify_stream(
        &channel.stream_url,
        &reqwest::Client::new(),
        ClassificationParams {
            format: "auto",
            ..Default::default()
        },
    )
    .await
    .ok();

    // Create session tracking (single path regardless of decision)
    let session_id = format!("direct_{}_{}", channel.id, uuid::Uuid::new_v4());
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
    state
        .session_tracker
        .start_session(session_stats.clone())
        .await;

    // Decide streaming strategy
    if let Some(class_res) = &classification_result {
        if matches!(
            class_res.decision,
            StreamModeDecision::CollapsedSingleVariantTs
        ) {
            // Collapsing path (single variant media playlist -> stitch segments)
            use axum::body::Body;
            use axum::http::{Response, StatusCode, header};
            use futures::StreamExt;
            use std::sync::Arc;

            let collapsing_playlist_url = class_res
                .selected_media_playlist_url
                .clone()
                .unwrap_or_else(|| channel.stream_url.clone());

            let handle = crate::streaming::collapsing::spawn_collapsing_session(
                Arc::new(reqwest::Client::new()),
                collapsing_playlist_url,
                class_res.target_duration,
                crate::streaming::collapsing::CollapsingConfig::default(),
            );

            let collapsing_stream = handle.map(|r| match r {
                Ok(bytes) => Ok(bytes),
                Err(e) => {
                    debug!(error=?e, "Collapsing stream error – terminating");
                    Err(std::io::Error::other(format!("collapsing: {e}")))
                }
            });

            let body = Body::from_stream(collapsing_stream);
            let mut resp = Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, "video/mp2t")
                .header(header::CACHE_CONTROL, "no-store")
                .body(body)
                .unwrap();

            let meta = crate::proxy::http_stream::StreamHeaderMeta {
                origin_kind: Some("RAW_TS".into()),
                decision: Some("hls-to-ts".into()),
                mode: Some("hls-to-ts".into()),
                variant_count: class_res.variant_count,
                variant_bandwidth: class_res.selected_variant_bandwidth,
                variant_resolution: class_res.selected_variant_resolution,
                target_duration: class_res.target_duration,
                fallback: if class_res.forced_raw_rejected {
                    Some("forced-raw-rejected".into())
                } else {
                    None
                },
                relay_profile_id: None,
            };
            crate::proxy::http_stream::apply_uniform_stream_headers(&mut resp, &meta);
            return resp;
        }

        // Passthrough via unified HTTP proxy (with playlist rewriting if needed)
        let meta = crate::proxy::http_stream::StreamHeaderMeta {
            origin_kind: Some(
                match class_res.decision {
                    StreamModeDecision::PassthroughRawTs => "RAW_TS",
                    StreamModeDecision::CollapsedSingleVariantTs => "RAW_TS",
                    StreamModeDecision::TransparentHls { .. } => "HLS_PLAYLIST",
                    StreamModeDecision::TransparentUnknown => "UNKNOWN",
                }
                .into(),
            ),
            decision: Some(
                match class_res.decision {
                    StreamModeDecision::PassthroughRawTs => "passthrough-raw-ts",
                    StreamModeDecision::CollapsedSingleVariantTs => "hls-to-ts",
                    StreamModeDecision::TransparentHls { .. } => "hls-playlist-passthrough",
                    StreamModeDecision::TransparentUnknown => "transparent-unknown",
                }
                .into(),
            ),
            mode: Some(
                match class_res.decision {
                    StreamModeDecision::CollapsedSingleVariantTs => "hls-to-ts",
                    _ => "passthrough",
                }
                .into(),
            ),
            variant_count: class_res.variant_count,
            variant_bandwidth: class_res.selected_variant_bandwidth,
            variant_resolution: class_res.selected_variant_resolution,
            target_duration: class_res.target_duration,
            fallback: if class_res.forced_raw_rejected {
                Some("forced-raw-rejected".into())
            } else if class_res
                .reasons
                .iter()
                .any(|r| r.contains("unsupported-non-ts"))
            {
                Some("unsupported-non-ts".into())
            } else {
                None
            },
            relay_profile_id: None,
        };

        return crate::proxy::http_stream::proxy_http_stream(
            &channel.stream_url,
            &headers,
            &state.config,
            state.session_tracker.clone(),
            session_stats,
            Some(meta),
        )
        .await;
    }

    // Classification failed -> transparent unknown passthrough
    let meta = crate::proxy::http_stream::StreamHeaderMeta {
        origin_kind: Some("UNKNOWN".into()),
        decision: Some("transparent-unknown".into()),
        mode: Some("passthrough".into()),
        ..Default::default()
    };
    crate::proxy::http_stream::proxy_http_stream(
        &channel.stream_url,
        &headers,
        &state.config,
        state.session_tracker.clone(),
        session_stats,
        Some(meta),
    )
    .await
}

// Legacy channel-specific proxy implementation removed; using unified proxy::http_stream::proxy_http_stream
