//! Request extractors and validation
//!
//! This module provides custom extractors for request validation,
//! pagination parameters, and other common request processing needs.

use axum::{
    async_trait,
    extract::{FromRequestParts, Query},
    http::{request::Parts, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use uuid::Uuid;

use super::responses::{ApiResponse, ValidationErrorResponse, validation_error};

/// Pagination parameters from query string
#[derive(Debug, Clone, Deserialize)]
pub struct PaginationParams {
    #[serde(default = "default_page")]
    pub page: u32,
    #[serde(default = "default_limit")]
    pub limit: u32,
}

fn default_page() -> u32 {
    1
}

fn default_limit() -> u32 {
    50
}

impl Default for PaginationParams {
    fn default() -> Self {
        Self {
            page: default_page(),
            limit: default_limit(),
        }
    }
}

impl PaginationParams {
    /// Validate pagination parameters
    pub fn validate(&self) -> Result<(), Vec<ValidationErrorResponse>> {
        let mut errors = Vec::new();

        if self.page < 1 {
            errors.push(ValidationErrorResponse {
                field: "page".to_string(),
                message: "Page must be >= 1".to_string(),
            });
        }

        if self.limit < 1 || self.limit > 1000 {
            errors.push(ValidationErrorResponse {
                field: "limit".to_string(),
                message: "Limit must be between 1 and 1000".to_string(),
            });
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    /// Calculate offset for database queries (0-based)
    pub fn offset(&self) -> u32 {
        (self.page.saturating_sub(1)) * self.limit
    }
}

#[async_trait]
impl<S> FromRequestParts<S> for PaginationParams
where
    S: Send + Sync,
{
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let Query(params): Query<PaginationParams> = Query::from_request_parts(parts, state)
            .await
            .map_err(|_| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(ApiResponse::<()>::error("Invalid pagination parameters".to_string())),
                ).into_response()
            })?;

        params.validate().map_err(|errors| validation_error(errors).into_response())?;

        Ok(params)
    }
}

/// Search parameters from query string
#[derive(Debug, Clone, Deserialize)]
pub struct SearchParams {
    #[serde(default)]
    pub search: Option<String>,
    #[serde(default)]
    pub sort_by: Option<String>,
    #[serde(default = "default_sort_ascending")]
    pub sort_ascending: bool,
}

fn default_sort_ascending() -> bool {
    true
}

impl Default for SearchParams {
    fn default() -> Self {
        Self {
            search: None,
            sort_by: None,
            sort_ascending: default_sort_ascending(),
        }
    }
}

#[async_trait]
impl<S> FromRequestParts<S> for SearchParams
where
    S: Send + Sync,
{
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let Query(params): Query<SearchParams> = Query::from_request_parts(parts, state)
            .await
            .map_err(|_| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(ApiResponse::<()>::error("Invalid search parameters".to_string())),
                ).into_response()
            })?;

        Ok(params)
    }
}

/// Combined pagination and search parameters
#[derive(Debug, Clone)]
pub struct ListParams {
    pub pagination: PaginationParams,
    pub search: SearchParams,
}

#[async_trait]
impl<S> FromRequestParts<S> for ListParams
where
    S: Send + Sync,
{
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let pagination = PaginationParams::from_request_parts(parts, state).await?;
        let search = SearchParams::from_request_parts(parts, state).await?;

        Ok(Self { pagination, search })
    }
}

/// Validated JSON extractor that provides better error messages
pub struct ValidatedJson<T>(pub T);

#[async_trait]
impl<T, S> FromRequestParts<S> for ValidatedJson<T>
where
    T: for<'de> Deserialize<'de> + Send,
    S: Send + Sync,
{
    type Rejection = Response;

    async fn from_request_parts(_parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        // This would be implemented with the request body, but FromRequestParts
        // doesn't have access to the body. In practice, we'd use FromRequest instead.
        // For now, this is a placeholder that demonstrates the pattern.
        Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse::<()>::error("ValidatedJson not implemented for FromRequestParts".to_string())),
        ).into_response())
    }
}

/// UUID path parameter extractor with validation
#[derive(Debug, Clone)]
pub struct ValidatedUuid(pub Uuid);

impl ValidatedUuid {
    pub fn into_inner(self) -> Uuid {
        self.0
    }
}

/// Query parameter validation trait
pub trait ValidateQuery {
    type Error;
    fn validate(&self) -> Result<(), Self::Error>;
}

/// Stream source filter parameters
#[derive(Debug, Clone, Deserialize)]
pub struct StreamSourceFilterParams {
    #[serde(default)]
    pub source_type: Option<String>,
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub healthy: Option<bool>,
}

impl ValidateQuery for StreamSourceFilterParams {
    type Error = Vec<ValidationErrorResponse>;

    fn validate(&self) -> Result<(), Self::Error> {
        let mut errors = Vec::new();

        if let Some(ref source_type) = self.source_type {
            if !["m3u", "xtream"].contains(&source_type.to_lowercase().as_str()) {
                errors.push(ValidationErrorResponse {
                    field: "source_type".to_string(),
                    message: "Source type must be 'm3u' or 'xtream'".to_string(),
                });
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

#[async_trait]
impl<S> FromRequestParts<S> for StreamSourceFilterParams
where
    S: Send + Sync,
{
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let Query(params): Query<StreamSourceFilterParams> = Query::from_request_parts(parts, state)
            .await
            .map_err(|_| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(ApiResponse::<()>::error("Invalid filter parameters".to_string())),
                ).into_response()
            })?;

        params.validate().map_err(|errors| validation_error(errors).into_response())?;

        Ok(params)
    }
}

/// Request context information
#[derive(Debug, Clone)]
pub struct RequestContext {
    pub user_agent: Option<String>,
    pub real_ip: Option<String>,
    pub request_id: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

impl Default for RequestContext {
    fn default() -> Self {
        Self {
            user_agent: None,
            real_ip: None,
            request_id: Uuid::new_v4().to_string(),
            timestamp: chrono::Utc::now(),
        }
    }
}

#[async_trait]
impl<S> FromRequestParts<S> for RequestContext
where
    S: Send + Sync,
{
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let user_agent = parts
            .headers
            .get("user-agent")
            .and_then(|h| h.to_str().ok())
            .map(|s| s.to_string());

        let real_ip = parts
            .headers
            .get("x-real-ip")
            .or_else(|| parts.headers.get("x-forwarded-for"))
            .and_then(|h| h.to_str().ok())
            .map(|s| s.split(',').next().unwrap_or(s).trim().to_string());

        Ok(Self {
            user_agent,
            real_ip,
            request_id: Uuid::new_v4().to_string(),
            timestamp: chrono::Utc::now(),
        })
    }
}