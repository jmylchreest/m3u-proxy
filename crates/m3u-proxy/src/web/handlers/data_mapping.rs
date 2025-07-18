//! Data mapping HTTP handlers
//!
//! This module contains HTTP handlers for data mapping rule operations.

use axum::{
    Json,
    extract::{Path, Query, State},
    response::IntoResponse,
};
use serde::Deserialize;
use uuid::Uuid;

use crate::{
    models::data_mapping::{DataMappingRuleCreateRequest, DataMappingRuleUpdateRequest},
    web::{
        AppState,
        api::{
            create_data_mapping_rule as api_create_data_mapping_rule,
            delete_data_mapping_rule as api_delete_data_mapping_rule,
            get_data_mapping_rule as api_get_data_mapping_rule,
            list_data_mapping_rules as api_list_data_mapping_rules,
            reorder_data_mapping_rules as api_reorder_data_mapping_rules,
            test_data_mapping_rule as api_test_data_mapping_rule,
            update_data_mapping_rule as api_update_data_mapping_rule,
            validate_data_mapping_expression as api_validate_data_mapping_expression,
        },
        extractors::RequestContext,
        responses::PaginatedResponse,
        utils::log_request,
    },
};

#[derive(Deserialize)]
pub struct PaginationQuery {
    page: Option<u32>,
    page_size: Option<u32>,
}

#[derive(Deserialize)]
pub struct ReorderRequest {
    rules: Vec<(Uuid, i32)>,
}

/// List all data mapping rules
pub async fn list_data_mapping_rules(
    State(state): State<AppState>,
    Query(query): Query<PaginationQuery>,
    context: RequestContext,
) -> Result<impl IntoResponse, impl IntoResponse> {
    log_request(
        &axum::http::Method::GET,
        &"/api/v1/data-mapping".parse().unwrap(),
        &context,
    );

    // Call the actual API implementation
    match api_list_data_mapping_rules(State(state)).await {
        Ok(Json(rules)) => {
            let page = query.page.unwrap_or(1);
            let page_size = query.page_size.unwrap_or(50);
            let total = rules.len();

            // Simple pagination for now
            let start = ((page - 1) * page_size) as usize;
            let end = (start + page_size as usize).min(total);
            let page_rules = if start < total {
                rules[start..end].to_vec()
            } else {
                Vec::new()
            };

            let response = PaginatedResponse::new(page_rules, total as u64, page, page_size);
            Ok(crate::web::responses::ok(response))
        }
        Err(_status) => Err(crate::web::responses::internal_error(
            "Failed to list data mapping rules",
        )),
    }
}

/// Get a specific data mapping rule by ID
pub async fn get_data_mapping_rule(
    State(state): State<AppState>,
    Path(rule_id): Path<Uuid>,
    context: RequestContext,
) -> impl IntoResponse {
    log_request(
        &axum::http::Method::GET,
        &format!("/api/v1/data-mapping/{}", rule_id).parse().unwrap(),
        &context,
    );

    api_get_data_mapping_rule(Path(rule_id), State(state)).await
}

/// Create a new data mapping rule
pub async fn create_data_mapping_rule(
    State(state): State<AppState>,
    context: RequestContext,
    Json(payload): Json<DataMappingRuleCreateRequest>,
) -> impl IntoResponse {
    log_request(
        &axum::http::Method::POST,
        &"/api/v1/data-mapping".parse().unwrap(),
        &context,
    );

    api_create_data_mapping_rule(State(state), Json(payload)).await
}

/// Update an existing data mapping rule
pub async fn update_data_mapping_rule(
    State(state): State<AppState>,
    Path(rule_id): Path<Uuid>,
    context: RequestContext,
    Json(payload): Json<DataMappingRuleUpdateRequest>,
) -> impl IntoResponse {
    log_request(
        &axum::http::Method::PUT,
        &format!("/api/v1/data-mapping/{}", rule_id).parse().unwrap(),
        &context,
    );

    api_update_data_mapping_rule(Path(rule_id), State(state), Json(payload)).await
}

/// Delete a data mapping rule
pub async fn delete_data_mapping_rule(
    State(state): State<AppState>,
    Path(rule_id): Path<Uuid>,
    context: RequestContext,
) -> impl IntoResponse {
    log_request(
        &axum::http::Method::DELETE,
        &format!("/api/v1/data-mapping/{}", rule_id).parse().unwrap(),
        &context,
    );

    api_delete_data_mapping_rule(Path(rule_id), State(state)).await
}

/// Reorder data mapping rules
pub async fn reorder_data_mapping_rules(
    State(state): State<AppState>,
    context: RequestContext,
    Json(payload): Json<ReorderRequest>,
) -> impl IntoResponse {
    log_request(
        &axum::http::Method::POST,
        &"/api/v1/data-mapping/reorder".parse().unwrap(),
        &context,
    );

    api_reorder_data_mapping_rules(State(state), Json(payload.rules)).await
}

/// Test a data mapping rule
pub async fn test_data_mapping_rule(
    State(state): State<AppState>,
    context: RequestContext,
    Json(payload): Json<crate::models::data_mapping::DataMappingTestRequest>,
) -> impl IntoResponse {
    log_request(
        &axum::http::Method::POST,
        &"/api/v1/data-mapping/test".parse().unwrap(),
        &context,
    );

    api_test_data_mapping_rule(State(state), Json(payload)).await
}

/// Validate a data mapping expression
pub async fn validate_data_mapping_expression(
    State(state): State<AppState>,
    context: RequestContext,
    Json(payload): Json<serde_json::Value>,
) -> impl IntoResponse {
    log_request(
        &axum::http::Method::POST,
        &"/api/v1/data-mapping/validate".parse().unwrap(),
        &context,
    );

    api_validate_data_mapping_expression(Json(payload)).await
}
