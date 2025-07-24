//! Relay API Endpoints
//!
//! This module provides HTTP API endpoints for managing relay profiles,
//! channel configurations, and relay process control.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde_json::json;
use sqlx::Row;
use uuid::Uuid;

use crate::database::Database;
use crate::models::relay::*;
use crate::utils::uuid_parser::parse_uuid_flexible;
use crate::web::AppState;

/// Create relay API routes
pub fn relay_routes() -> Router<AppState> {
    Router::new()
        // Profile management
        .route("/relay/profiles", get(list_profiles).post(create_profile))
        .route("/relay/profiles/{id}", get(get_profile).put(update_profile).delete(delete_profile))
        
        // Channel relay configuration
        .route("/proxies/{proxy_id}/channels/{channel_id}/relay", 
               get(get_channel_relay_config).post(create_channel_relay_config).delete(delete_channel_relay_config))
        
        // Relay content serving
        .route("/relay/{config_id}/playlist.m3u8", get(serve_relay_playlist))
        .route("/relay/{config_id}/segments/{segment_name}", get(serve_relay_segment))
        
        // Relay status and control
        .route("/relay/{config_id}/status", get(get_relay_status))
        .route("/proxy/{proxy_id}/relay/{channel_id}/start", post(start_relay))
        .route("/relay/{config_id}/stop", post(stop_relay))
        
        // Metrics and monitoring
        .route("/relay/metrics", get(get_relay_metrics))
        .route("/relay/metrics/{config_id}", get(get_relay_metrics_for_config))
        .route("/relay/health", get(get_relay_health))
        .route("/relay/health/{config_id}", get(get_relay_health_for_config))
}

/// List all relay profiles
#[utoipa::path(
    get,
    path = "/relay/profiles",
    tag = "relay",
    summary = "List relay profiles",
    description = "Retrieve all relay profiles for stream transcoding",
    responses(
        (status = 200, description = "List of relay profiles"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn list_profiles(State(state): State<AppState>) -> impl IntoResponse {
    match get_relay_profiles(&state.database).await {
        Ok(profiles) => Json(profiles).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("Database error: {}", e)).into_response(),
    }
}

/// Get a specific relay profile
#[utoipa::path(
    get,
    path = "/relay/profiles/{id}",
    tag = "relay",
    summary = "Get relay profile",
    description = "Retrieve a specific relay profile by ID",
    params(
        ("id" = String, Path, description = "Relay profile ID (UUID)"),
    ),
    responses(
        (status = 200, description = "Relay profile details"),
        (status = 404, description = "Relay profile not found"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_profile(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    match get_relay_profile_by_id(&state.database, id).await {
        Ok(Some(profile)) => Json(profile).into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, "Profile not found").into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("Database error: {}", e)).into_response(),
    }
}

/// Create a new relay profile
#[utoipa::path(
    post,
    path = "/relay/profiles",
    tag = "relay",
    summary = "Create relay profile",
    description = "Create a new relay profile for stream transcoding",
    responses(
        (status = 201, description = "Relay profile created successfully"),
        (status = 400, description = "Invalid request"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn create_profile(
    State(state): State<AppState>,
    Json(request): Json<CreateRelayProfileRequest>,
) -> impl IntoResponse {

    // Create the profile
    match RelayProfile::new(request) {
        Ok(profile) => {
            match create_relay_profile(&state.database, &profile).await {
                Ok(_) => (StatusCode::CREATED, Json(profile)).into_response(),
                Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("Database error: {}", e)).into_response(),
            }
        }
        Err(e) => (StatusCode::BAD_REQUEST, format!("Invalid profile: {}", e)).into_response(),
    }
}

/// Update an existing relay profile
#[utoipa::path(
    put,
    path = "/relay/profiles/{id}",
    tag = "relay",
    summary = "Update relay profile",
    description = "Update an existing relay profile",
    params(
        ("id" = String, Path, description = "Relay profile ID (UUID)"),
    ),
    responses(
        (status = 200, description = "Relay profile updated successfully"),
        (status = 400, description = "Invalid request"),
        (status = 404, description = "Relay profile not found"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn update_profile(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(request): Json<UpdateRelayProfileRequest>,
) -> impl IntoResponse {

    match update_relay_profile(&state.database, id, request).await {
        Ok(Some(profile)) => Json(profile).into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, "Profile not found").into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("Database error: {}", e)).into_response(),
    }
}

/// Delete a relay profile
#[utoipa::path(
    delete,
    path = "/relay/profiles/{id}",
    tag = "relay",
    summary = "Delete relay profile",
    description = "Delete a relay profile",
    params(
        ("id" = String, Path, description = "Relay profile ID (UUID)"),
    ),
    responses(
        (status = 204, description = "Relay profile deleted successfully"),
        (status = 404, description = "Relay profile not found"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn delete_profile(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    match delete_relay_profile(&state.database, id).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => (StatusCode::NOT_FOUND, "Profile not found").into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("Database error: {}", e)).into_response(),
    }
}

/// Get channel relay configuration
async fn get_channel_relay_config(
    State(state): State<AppState>,
    Path((proxy_id, channel_id)): Path<(String, Uuid)>,
) -> impl IntoResponse {
    let proxy_uuid = match proxy_id.parse::<Uuid>() {
        Ok(uuid) => uuid,
        Err(_) => return (StatusCode::BAD_REQUEST, "Invalid proxy ID").into_response(),
    };

    // Since relay configs are now at proxy level, we check if the proxy has a relay profile
    match get_proxy_relay_profile(&state.database, proxy_uuid).await {
        Ok(Some(profile_id)) => {
            // Create a synthetic channel config based on the proxy's relay profile
            let synthetic_config = ChannelRelayConfig {
                id: Uuid::new_v4(),
                proxy_id: proxy_uuid,
                channel_id,
                profile_id,
                name: format!("Relay for Channel {}", channel_id),
                description: Some("Auto-generated from proxy relay profile".to_string()),
                custom_args: None,
                is_active: true,
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            };
            Json(synthetic_config).into_response()
        },
        Ok(None) => (StatusCode::NOT_FOUND, "Proxy has no relay profile configured").into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("Database error: {}", e)).into_response(),
    }
}

/// Create channel relay configuration
async fn create_channel_relay_config(
    State(state): State<AppState>,
    Path((proxy_id, channel_id)): Path<(String, Uuid)>,
    Json(request): Json<CreateChannelRelayConfigRequest>,
) -> impl IntoResponse {
    let proxy_uuid = match proxy_id.parse::<Uuid>() {
        Ok(uuid) => uuid,
        Err(_) => return (StatusCode::BAD_REQUEST, "Invalid proxy ID").into_response(),
    };

    // Validate that the profile exists
    match get_relay_profile_by_id(&state.database, request.profile_id).await {
        Ok(Some(_)) => {},
        Ok(None) => return (StatusCode::BAD_REQUEST, "Profile not found").into_response(),
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, format!("Database error: {}", e)).into_response(),
    }

    // Update the proxy's relay profile instead of creating channel-specific config
    match update_proxy_relay_profile(&state.database, proxy_uuid, request.profile_id).await {
        Ok(_) => {
            // Return a synthetic config for the response
            let config = ChannelRelayConfig {
                id: Uuid::new_v4(),
                proxy_id: proxy_uuid,
                channel_id,
                profile_id: request.profile_id,
                name: request.name,
                description: request.description,
                custom_args: request.custom_args.map(|args| args.join(" ")),
                is_active: true,
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            };
            (StatusCode::CREATED, Json(config)).into_response()
        },
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("Database error: {}", e)).into_response(),
    }
}

/// Delete channel relay configuration
async fn delete_channel_relay_config(
    State(state): State<AppState>,
    Path((proxy_id, _channel_id)): Path<(String, Uuid)>,
) -> impl IntoResponse {
    let proxy_uuid = match proxy_id.parse::<Uuid>() {
        Ok(uuid) => uuid,
        Err(_) => return (StatusCode::BAD_REQUEST, "Invalid proxy ID").into_response(),
    };

    // Remove relay profile from proxy (affects all channels)
    match remove_proxy_relay_profile(&state.database, proxy_uuid).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => (StatusCode::NOT_FOUND, "Proxy has no relay profile configured").into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("Database error: {}", e)).into_response(),
    }
}

/// Serve HLS playlist (playlist.m3u8)
async fn serve_relay_playlist(
    State(state): State<AppState>,
    Path(config_id): Path<Uuid>,
) -> impl IntoResponse {
    let client_info = ClientInfo {
        ip: "playlist_request".to_string(),
        user_agent: None,
        referer: None,
    };

    match state.relay_manager.serve_relay_content(config_id, "", &client_info).await {
        Ok(RelayContent::Playlist(content)) => {
            use axum::http::header;
            (
                [
                    (header::CONTENT_TYPE, "application/vnd.apple.mpegurl"),
                    (header::CACHE_CONTROL, "no-cache, no-store, must-revalidate"),
                    (header::EXPIRES, "0"),
                ],
                content,
            ).into_response()
        }
        Ok(_) => (StatusCode::INTERNAL_SERVER_ERROR, "Unexpected content type").into_response(),
        Err(e) => (StatusCode::NOT_FOUND, format!("Playlist not found: {}", e)).into_response(),
    }
}

/// Serve relay segment (for HLS)
async fn serve_relay_segment(
    State(state): State<AppState>,
    Path((config_id, segment_name)): Path<(Uuid, String)>,
) -> impl IntoResponse {
    let client_info = ClientInfo {
        ip: "segment_request".to_string(),
        user_agent: None,
        referer: None,
    };

    match state.relay_manager.serve_relay_content(config_id, &segment_name, &client_info).await {
        Ok(RelayContent::Segment(data)) => {
            use axum::http::header;
            (
                [
                    (header::CONTENT_TYPE, "video/mp2t"),
                    (header::CACHE_CONTROL, "no-cache, no-store, must-revalidate"),
                    (header::EXPIRES, "0"),
                ],
                data,
            ).into_response()
        }
        Ok(RelayContent::Playlist(content)) => {
            // Sometimes segments might be served as playlists for sub-manifests
            use axum::http::header;
            (
                [
                    (header::CONTENT_TYPE, "application/vnd.apple.mpegurl"),
                    (header::CACHE_CONTROL, "no-cache, no-store, must-revalidate"),
                    (header::EXPIRES, "0"),
                ],
                content,
            ).into_response()
        }
        Ok(_) => (StatusCode::INTERNAL_SERVER_ERROR, "Unexpected content type").into_response(),
        Err(e) => (StatusCode::NOT_FOUND, format!("Segment not found: {}", e)).into_response(),
    }
}


/// Get relay status
async fn get_relay_status(
    State(state): State<AppState>,
    Path(config_id): Path<Uuid>,
) -> impl IntoResponse {
    match get_relay_status_from_db(&state.database, config_id).await {
        Ok(Some(status)) => Json(status).into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, "Relay not found").into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("Database error: {}", e)).into_response(),
    }
}

/// Start relay process (now uses proxy_id and channel_id instead of config_id)
async fn start_relay(
    State(state): State<AppState>,
    Path((proxy_id, channel_id)): Path<(String, Uuid)>,
) -> impl IntoResponse {
    let proxy_uuid = match proxy_id.parse::<Uuid>() {
        Ok(uuid) => uuid,
        Err(_) => return (StatusCode::BAD_REQUEST, "Invalid proxy ID").into_response(),
    };

    // Get the relay configuration from relay manager
    let resolved_config = match state.relay_manager.get_relay_config_for_channel(proxy_uuid, channel_id).await {
        Ok(Some(config)) => config,
        Ok(None) => return (StatusCode::NOT_FOUND, "Proxy has no relay profile configured").into_response(),
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to get relay config: {}", e)).into_response(),
    };

    // Get channel information to get the input URL
    let channel = match state.database.get_channel_for_proxy(&proxy_uuid.to_string(), channel_id).await {
        Ok(Some(channel)) => channel,
        Ok(None) => return (StatusCode::NOT_FOUND, "Channel not found").into_response(),
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, format!("Database error: {}", e)).into_response(),
    };

    // Start the relay process
    match state.relay_manager.ensure_relay_running(&resolved_config, &channel.stream_url).await {
        Ok(_) => Json(json!({
            "message": "Relay started successfully",
            "proxy_id": proxy_uuid,
            "channel_id": channel_id,
            "profile_name": resolved_config.profile.name,
            "channel_name": channel.channel_name
        })).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to start relay: {}", e)).into_response(),
    }
}

/// Stop relay process
async fn stop_relay(
    State(state): State<AppState>,
    Path(config_id): Path<Uuid>,
) -> impl IntoResponse {
    match state.relay_manager.stop_relay(config_id).await {
        Ok(_) => Json(json!({"message": "Relay stopped successfully"})).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to stop relay: {}", e)).into_response(),
    }
}

/// Get relay metrics
async fn get_relay_metrics(State(state): State<AppState>) -> impl IntoResponse {
    match state.relay_manager.get_relay_status().await {
        Ok(metrics) => Json(metrics).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to get metrics: {}", e)).into_response(),
    }
}

/// Get relay metrics for specific config
async fn get_relay_metrics_for_config(
    State(_state): State<AppState>,
    Path(_config_id): Path<Uuid>,
) -> impl IntoResponse {
    // TODO: Implement config-specific metrics
    (StatusCode::NOT_IMPLEMENTED, "Config-specific metrics not implemented yet").into_response()
}

/// Get overall relay system health
async fn get_relay_health(State(state): State<AppState>) -> impl IntoResponse {
    match state.relay_manager.get_relay_health().await {
        Ok(health) => Json(health).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to get health: {}", e)).into_response(),
    }
}

/// Get health for specific relay config
async fn get_relay_health_for_config(
    State(state): State<AppState>,
    Path(config_id): Path<Uuid>,
) -> impl IntoResponse {
    match state.relay_manager.get_relay_health_for_config(config_id).await {
        Ok(Some(health)) => Json(health).into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, "Relay config not found").into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to get health: {}", e)).into_response(),
    }
}

// Database helper functions

/// Get all relay profiles from database
async fn get_relay_profiles(database: &Database) -> Result<Vec<RelayProfile>, sqlx::Error> {
    let query = r#"
        SELECT id, name, description, video_codec, audio_codec, video_profile, video_preset,
               video_bitrate, audio_bitrate, audio_sample_rate, audio_channels,
               enable_hardware_acceleration, preferred_hwaccel,
               manual_args, output_format, segment_duration,
               max_segments, input_timeout, is_system_default,
               is_active, created_at, updated_at
        FROM relay_profiles
        WHERE is_active = true
        ORDER BY is_system_default DESC, name ASC
    "#;

    let rows = sqlx::query(query)
        .fetch_all(&database.pool())
        .await?;

    let mut profiles = Vec::new();
    for row in rows {
        let profile = RelayProfile {
            id: parse_uuid_flexible(&row.get::<String, _>("id")).expect("Invalid UUID in relay_profiles table"),
            name: row.get("name"),
            description: row.get("description"),
            
            // Codec settings
            video_codec: row.get::<String, _>("video_codec").parse().unwrap_or(VideoCodec::Copy),
            audio_codec: row.get::<String, _>("audio_codec").parse().unwrap_or(AudioCodec::Copy),
            video_profile: row.get("video_profile"),
            video_preset: row.get("video_preset"),
            video_bitrate: row.get::<Option<i32>, _>("video_bitrate").map(|v| v as u32),
            audio_bitrate: row.get::<Option<i32>, _>("audio_bitrate").map(|v| v as u32),
            audio_sample_rate: row.get::<Option<i32>, _>("audio_sample_rate").map(|v| v as u32),
            audio_channels: row.get::<Option<i32>, _>("audio_channels").map(|v| v as u32),
            
            // Hardware acceleration
            enable_hardware_acceleration: row.get("enable_hardware_acceleration"),
            preferred_hwaccel: row.get("preferred_hwaccel"),
            
            // Manual override
            manual_args: row.get("manual_args"),
            
            // Container settings
            output_format: row.get::<String, _>("output_format").parse().unwrap_or(RelayOutputFormat::TransportStream),
            segment_duration: row.get("segment_duration"),
            max_segments: row.get("max_segments"),
            input_timeout: row.get("input_timeout"),
            
            // System flags
            is_system_default: row.get("is_system_default"),
            is_active: row.get("is_active"),
            created_at: row.get("created_at"),
            updated_at: row.get("updated_at"),
        };
        profiles.push(profile);
    }

    Ok(profiles)
}

/// Get a specific relay profile by ID
async fn get_relay_profile_by_id(database: &Database, id: Uuid) -> Result<Option<RelayProfile>, sqlx::Error> {
    let query = r#"
        SELECT id, name, description, video_codec, audio_codec, video_profile, video_preset,
               video_bitrate, audio_bitrate, audio_sample_rate, audio_channels,
               enable_hardware_acceleration, preferred_hwaccel,
               manual_args, output_format, segment_duration,
               max_segments, input_timeout, is_system_default,
               is_active, created_at, updated_at
        FROM relay_profiles
        WHERE id = ? AND is_active = true
    "#;

    let row = sqlx::query(query)
        .bind(id.to_string())
        .fetch_optional(&database.pool())
        .await?;

    if let Some(row) = row {
        let profile = RelayProfile {
            id: parse_uuid_flexible(&row.get::<String, _>("id")).expect("Invalid UUID in relay_profiles table"),
            name: row.get("name"),
            description: row.get("description"),
            
            // Codec settings
            video_codec: row.get::<String, _>("video_codec").parse().unwrap_or(VideoCodec::Copy),
            audio_codec: row.get::<String, _>("audio_codec").parse().unwrap_or(AudioCodec::Copy),
            video_profile: row.get("video_profile"),
            video_preset: row.get("video_preset"),
            video_bitrate: row.get::<Option<i32>, _>("video_bitrate").map(|v| v as u32),
            audio_bitrate: row.get::<Option<i32>, _>("audio_bitrate").map(|v| v as u32),
            audio_sample_rate: row.get::<Option<i32>, _>("audio_sample_rate").map(|v| v as u32),
            audio_channels: row.get::<Option<i32>, _>("audio_channels").map(|v| v as u32),
            
            // Hardware acceleration
            enable_hardware_acceleration: row.get("enable_hardware_acceleration"),
            preferred_hwaccel: row.get("preferred_hwaccel"),
            
            // Manual override
            manual_args: row.get("manual_args"),
            
            // Container settings
            output_format: row.get::<String, _>("output_format").parse().unwrap_or(RelayOutputFormat::TransportStream),
            segment_duration: row.get("segment_duration"),
            max_segments: row.get("max_segments"),
            input_timeout: row.get("input_timeout"),
            
            // System flags
            is_system_default: row.get("is_system_default"),
            is_active: row.get("is_active"),
            created_at: row.get("created_at"),
            updated_at: row.get("updated_at"),
        };
        Ok(Some(profile))
    } else {
        Ok(None)
    }
}

/// Create a new relay profile in database
async fn create_relay_profile(database: &Database, profile: &RelayProfile) -> Result<(), sqlx::Error> {
    let query = r#"
        INSERT INTO relay_profiles (
            id, name, description, video_codec, audio_codec, video_profile, video_preset,
            video_bitrate, audio_bitrate, audio_sample_rate, audio_channels,
            enable_hardware_acceleration, preferred_hwaccel,
            manual_args, output_format, segment_duration,
            max_segments, input_timeout, is_system_default,
            is_active, created_at, updated_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
    "#;

    sqlx::query(query)
        .bind(profile.id.to_string())
        .bind(&profile.name)
        .bind(&profile.description)
        .bind(profile.video_codec.to_string())
        .bind(profile.audio_codec.to_string())
        .bind(&profile.video_profile)
        .bind(&profile.video_preset)
        .bind(profile.video_bitrate.map(|v| v as i32))
        .bind(profile.audio_bitrate.map(|v| v as i32))
        .bind(profile.audio_sample_rate.map(|v| v as i32))
        .bind(profile.audio_channels.map(|v| v as i32))
        .bind(profile.enable_hardware_acceleration)
        .bind(&profile.preferred_hwaccel)
        .bind(&profile.manual_args)
        .bind(profile.output_format.to_string())
        .bind(profile.segment_duration)
        .bind(profile.max_segments)
        .bind(profile.input_timeout)
        .bind(profile.is_system_default)
        .bind(profile.is_active)
        .bind(profile.created_at.to_rfc3339())
        .bind(profile.updated_at.to_rfc3339())
        .execute(&database.pool())
        .await?;

    Ok(())
}

/// Update an existing relay profile
async fn update_relay_profile(
    database: &Database,
    id: Uuid,
    request: UpdateRelayProfileRequest,
) -> Result<Option<RelayProfile>, sqlx::Error> {
    // First get the existing profile
    let existing = get_relay_profile_by_id(database, id).await?;
    let mut profile = match existing {
        Some(p) => p,
        None => return Ok(None),
    };

    // Update fields
    if let Some(name) = request.name {
        profile.name = name;
    }
    if let Some(description) = request.description {
        profile.description = Some(description);
    }
    if let Some(output_format) = request.output_format {
        profile.output_format = output_format;
    }
    if let Some(segment_duration) = request.segment_duration {
        profile.segment_duration = Some(segment_duration);
    }
    if let Some(max_segments) = request.max_segments {
        profile.max_segments = Some(max_segments);
    }
    if let Some(input_timeout) = request.input_timeout {
        profile.input_timeout = input_timeout;
    }
    if let Some(enable_hardware_acceleration) = request.enable_hardware_acceleration {
        profile.enable_hardware_acceleration = enable_hardware_acceleration;
    }
    if let Some(preferred_hwaccel) = request.preferred_hwaccel {
        profile.preferred_hwaccel = Some(preferred_hwaccel);
    }
    if let Some(manual_args) = request.manual_args {
        profile.manual_args = Some(manual_args);
    }
    if let Some(video_codec) = request.video_codec {
        profile.video_codec = video_codec;
    }
    if let Some(audio_codec) = request.audio_codec {
        profile.audio_codec = audio_codec;
    }
    if let Some(video_profile) = request.video_profile {
        profile.video_profile = Some(video_profile);
    }
    if let Some(video_preset) = request.video_preset {
        profile.video_preset = Some(video_preset);
    }
    if let Some(video_bitrate) = request.video_bitrate {
        profile.video_bitrate = Some(video_bitrate);
    }
    if let Some(audio_bitrate) = request.audio_bitrate {
        profile.audio_bitrate = Some(audio_bitrate);
    }
    if let Some(audio_sample_rate) = request.audio_sample_rate {
        profile.audio_sample_rate = Some(audio_sample_rate);
    }
    if let Some(audio_channels) = request.audio_channels {
        profile.audio_channels = Some(audio_channels);
    }
    if let Some(is_active) = request.is_active {
        profile.is_active = is_active;
    }

    profile.updated_at = chrono::Utc::now();

    // Update in database
    let query = r#"
        UPDATE relay_profiles SET
            name = ?, description = ?, video_codec = ?, audio_codec = ?, video_profile = ?,
            video_preset = ?, video_bitrate = ?, audio_bitrate = ?, audio_sample_rate = ?, audio_channels = ?,
            enable_hardware_acceleration = ?, preferred_hwaccel = ?, manual_args = ?, output_format = ?,
            segment_duration = ?, max_segments = ?, input_timeout = ?, is_active = ?, updated_at = ?
        WHERE id = ?
    "#;

    sqlx::query(query)
        .bind(&profile.name)
        .bind(&profile.description)
        .bind(profile.video_codec.to_string())
        .bind(profile.audio_codec.to_string())
        .bind(&profile.video_profile)
        .bind(&profile.video_preset)
        .bind(profile.video_bitrate.map(|v| v as i32))
        .bind(profile.audio_bitrate.map(|v| v as i32))
        .bind(profile.audio_sample_rate.map(|v| v as i32))
        .bind(profile.audio_channels.map(|v| v as i32))
        .bind(profile.enable_hardware_acceleration)
        .bind(&profile.preferred_hwaccel)
        .bind(&profile.manual_args)
        .bind(profile.output_format.to_string())
        .bind(profile.segment_duration)
        .bind(profile.max_segments)
        .bind(profile.input_timeout)
        .bind(profile.is_active)
        .bind(profile.updated_at.to_rfc3339())
        .bind(id.to_string())
        .execute(&database.pool())
        .await?;

    Ok(Some(profile))
}

/// Delete a relay profile
async fn delete_relay_profile(database: &Database, id: Uuid) -> Result<bool, sqlx::Error> {
    let query = r#"
        UPDATE relay_profiles SET is_active = false, updated_at = ?
        WHERE id = ? AND is_active = true
    "#;

    let result = sqlx::query(query)
        .bind(chrono::Utc::now().to_rfc3339())
        .bind(id.to_string())
        .execute(&database.pool())
        .await?;

    Ok(result.rows_affected() > 0)
}

/// Get proxy's relay profile ID
async fn get_proxy_relay_profile(
    database: &Database,
    proxy_id: Uuid,
) -> Result<Option<Uuid>, sqlx::Error> {
    let query = r#"
        SELECT relay_profile_id
        FROM stream_proxies
        WHERE id = ? AND is_active = true AND relay_profile_id IS NOT NULL
    "#;

    let row = sqlx::query(query)
        .bind(proxy_id.to_string())
        .fetch_optional(&database.pool())
        .await?;

    if let Some(row) = row {
        let profile_id_str: String = row.get("relay_profile_id");
        Ok(Some(parse_uuid_flexible(&profile_id_str).expect("Invalid UUID in relay_profile_id")))
    } else {
        Ok(None)
    }
}

/// Update proxy's relay profile
async fn update_proxy_relay_profile(
    database: &Database,
    proxy_id: Uuid,
    profile_id: Uuid,
) -> Result<(), sqlx::Error> {
    let query = r#"
        UPDATE stream_proxies 
        SET relay_profile_id = ?, updated_at = ?
        WHERE id = ? AND is_active = true
    "#;

    sqlx::query(query)
        .bind(profile_id.to_string())
        .bind(chrono::Utc::now().to_rfc3339())
        .bind(proxy_id.to_string())
        .execute(&database.pool())
        .await?;

    Ok(())
}

/// Remove relay profile from proxy
async fn remove_proxy_relay_profile(
    database: &Database,
    proxy_id: Uuid,
) -> Result<bool, sqlx::Error> {
    let query = r#"
        UPDATE stream_proxies 
        SET relay_profile_id = NULL, updated_at = ?
        WHERE id = ? AND is_active = true AND relay_profile_id IS NOT NULL
    "#;

    let result = sqlx::query(query)
        .bind(chrono::Utc::now().to_rfc3339())
        .bind(proxy_id.to_string())
        .execute(&database.pool())
        .await?;

    Ok(result.rows_affected() > 0)
}


/// Get relay status from database
async fn get_relay_status_from_db(
    database: &Database,
    config_id: Uuid,
) -> Result<Option<RelayRuntimeStatus>, sqlx::Error> {
    let query = r#"
        SELECT channel_relay_config_id, process_id, sandbox_path, is_running,
               started_at, client_count, bytes_served, error_message,
               last_heartbeat, updated_at
        FROM relay_runtime_status
        WHERE channel_relay_config_id = ?
    "#;

    let row = sqlx::query(query)
        .bind(config_id.to_string())
        .fetch_optional(&database.pool())
        .await?;

    if let Some(row) = row {
        let status = RelayRuntimeStatus {
            channel_relay_config_id: parse_uuid_flexible(&row.get::<String, _>("channel_relay_config_id")).expect("Invalid UUID in channel_relay_config_id"),
            process_id: row.get("process_id"),
            sandbox_path: row.get("sandbox_path"),
            is_running: row.get("is_running"),
            started_at: row.get("started_at"),
            client_count: row.get("client_count"),
            bytes_served: row.get("bytes_served"),
            error_message: row.get("error_message"),
            last_heartbeat: row.get("last_heartbeat"),
            updated_at: row.get("updated_at"),
        };
        Ok(Some(status))
    } else {
        Ok(None)
    }
}