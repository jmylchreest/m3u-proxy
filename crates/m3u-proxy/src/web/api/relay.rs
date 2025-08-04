//! Relay API Endpoints
//!
//! This module provides HTTP API endpoints for managing relay profiles,
//! channel configurations, and relay process control.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::get,
    Json, Router,
};
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
        
        // System monitoring
        .route("/relay/health", get(get_relay_health))
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












/// Get comprehensive relay system health and metrics
#[utoipa::path(
    get,
    path = "/relay/health",
    tag = "relay",
    summary = "Get relay system health",
    description = "Retrieve comprehensive health status, metrics, and connected client information for the relay system",
    responses(
        (status = 200, description = "Relay system health with detailed metrics and client information", body = RelayHealth),
        (status = 500, description = "Failed to get health status")
    )
)]
pub async fn get_relay_health(State(state): State<AppState>) -> impl IntoResponse {
    match state.relay_manager.get_relay_health().await {
        Ok(health) => Json(health).into_response(),
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





