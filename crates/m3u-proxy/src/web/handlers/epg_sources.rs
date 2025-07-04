//! EPG sources HTTP handlers
//!
//! This module contains HTTP handlers for EPG source operations.
//! These are placeholder implementations that follow the same pattern
//! as stream sources but for EPG-specific functionality.

use axum::{extract::State, response::IntoResponse};

use crate::web::{
    extractors::RequestContext,
    responses::ok,
    utils::log_request,
    AppState,
};

/// List all EPG sources
pub async fn list_epg_sources(
    State(_state): State<AppState>,
    context: RequestContext,
) -> impl IntoResponse {
    log_request(&axum::http::Method::GET, &"/api/sources/epg".parse().unwrap(), &context);
    
    // TODO: Implement EPG source listing using service layer
    let empty_response = crate::web::responses::PaginatedResponse::new(
        Vec::<serde_json::Value>::new(),
        0,
        1,
        50,
    );
    ok(empty_response)
}

/// Get a specific EPG source by ID
pub async fn get_epg_source(
    State(_state): State<AppState>,
    context: RequestContext,
) -> impl IntoResponse {
    log_request(&axum::http::Method::GET, &"/api/sources/epg/[id]".parse().unwrap(), &context);
    
    // TODO: Implement EPG source retrieval
    crate::web::responses::bad_request("EPG source handlers not yet implemented")
}

/// Create a new EPG source
pub async fn create_epg_source(
    State(_state): State<AppState>,
    context: RequestContext,
) -> impl IntoResponse {
    log_request(&axum::http::Method::POST, &"/api/sources/epg".parse().unwrap(), &context);
    
    // TODO: Implement EPG source creation
    crate::web::responses::bad_request("EPG source handlers not yet implemented")
}

/// Update an existing EPG source
pub async fn update_epg_source(
    State(_state): State<AppState>,
    context: RequestContext,
) -> impl IntoResponse {
    log_request(&axum::http::Method::PUT, &"/api/sources/epg/[id]".parse().unwrap(), &context);
    
    // TODO: Implement EPG source update
    crate::web::responses::bad_request("EPG source handlers not yet implemented")
}

/// Delete an EPG source
pub async fn delete_epg_source(
    State(_state): State<AppState>,
    context: RequestContext,
) -> impl IntoResponse {
    log_request(&axum::http::Method::DELETE, &"/api/sources/epg/[id]".parse().unwrap(), &context);
    
    // TODO: Implement EPG source deletion
    crate::web::responses::bad_request("EPG source handlers not yet implemented")
}