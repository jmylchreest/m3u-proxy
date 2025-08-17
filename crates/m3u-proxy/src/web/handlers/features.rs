//! Feature flags HTTP handlers
//!
//! This module provides endpoints for managing runtime feature flags.

use axum::{extract::State, response::IntoResponse, Json};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use utoipa::{self, ToSchema};

use crate::web::{
    AppState,
    extractors::RequestContext,
    responses::ok,
    utils::log_request,
};

/// Get current feature flags configuration
///
/// Returns the current feature flags as configured in the system
#[utoipa::path(
    get,
    path = "/api/v1/features",
    tag = "features",
    summary = "Get feature flags",
    description = "Retrieve current feature flags configuration",
    responses(
        (status = 200, description = "Feature flags configuration"),
    )
)]
pub async fn get_features(
    State(state): State<AppState>,
    context: RequestContext,
) -> impl IntoResponse {
    log_request(
        &axum::http::Method::GET,
        &"/api/v1/features".parse().unwrap(),
        &context,
    );

    // Get feature flags and config from runtime store (with fallback to static config)
    let runtime_flags = state.runtime_settings_store.get_feature_flags().await;
    let runtime_config = state.runtime_settings_store.get_feature_config().await;
    
    // If runtime store is empty, fall back to static config
    let (flags, config) = if runtime_flags.is_empty() && runtime_config.is_empty() {
        match &state.config.features {
            Some(features_config) => (
                features_config.flags.clone(),
                features_config.config.clone()
            ),
            None => (
                std::collections::HashMap::new(),
                std::collections::HashMap::new()
            )
        }
    } else {
        (runtime_flags, runtime_config)
    };

    ok(serde_json::json!({
        "flags": flags,
        "config": config,
        "timestamp": chrono::Utc::now()
    }))
}

#[derive(Debug, Deserialize, Serialize, ToSchema)]
pub struct UpdateFeaturesRequest {
    pub flags: HashMap<String, bool>,
    pub config: HashMap<String, HashMap<String, serde_json::Value>>,
}

/// Update feature flags configuration
///
/// Updates the feature flags and their configuration. Note: This updates the runtime
/// configuration but does not persist changes to the config file.
#[utoipa::path(
    put,
    path = "/api/v1/features",
    tag = "features",
    summary = "Update feature flags",
    description = "Update feature flags configuration at runtime",
    request_body = UpdateFeaturesRequest,
    responses(
        (status = 200, description = "Feature flags updated successfully"),
        (status = 400, description = "Invalid request data"),
    )
)]
pub async fn update_features(
    State(state): State<AppState>,
    context: RequestContext,
    Json(request): Json<UpdateFeaturesRequest>,
) -> impl IntoResponse {
    log_request(
        &axum::http::Method::PUT,
        &"/api/v1/features".parse().unwrap(),
        &context,
    );

    // Update the runtime configuration using RuntimeSettingsStore
    let success = state.runtime_settings_store
        .update_feature_flags(request.flags.clone(), request.config.clone())
        .await;

    if success {
        ok(serde_json::json!({
            "success": true,
            "message": "Feature flags updated successfully in runtime memory. Changes will be lost on restart unless saved to config file.",
            "flags_updated": request.flags.len(),
            "configs_updated": request.config.len(),
            "timestamp": chrono::Utc::now()
        })).into_response()
    } else {
        axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response()
    }
}