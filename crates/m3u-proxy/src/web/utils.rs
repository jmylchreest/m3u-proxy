//! Web utility functions
//!
//! This module provides utility functions for common web operations
//! like logging, metrics, and request processing.

use axum::http::{HeaderMap, Method, Uri};
use tracing::{info, warn};
use uuid::Uuid;

use super::extractors::RequestContext;

/// Log an incoming HTTP request
pub fn log_request(method: &Method, uri: &Uri, context: &RequestContext) {
    info!(
        method = %method,
        uri = %uri,
        request_id = %context.request_id,
        user_agent = ?context.user_agent,
        real_ip = ?context.real_ip,
        "HTTP request"
    );
}

/// Log the completion of an HTTP request
pub fn log_response(
    method: &Method,
    uri: &Uri,
    status: u16,
    context: &RequestContext,
    duration_ms: u64,
) {
    if status >= 400 {
        warn!(
            method = %method,
            uri = %uri,
            status = status,
            request_id = %context.request_id,
            duration_ms = duration_ms,
            "HTTP request completed with error"
        );
    } else {
        info!(
            method = %method,
            uri = %uri,
            status = status,
            request_id = %context.request_id,
            duration_ms = duration_ms,
            "HTTP request completed"
        );
    }
}

/// Extract UUID from path parameter
pub fn extract_uuid_param(param: &str) -> Result<Uuid, String> {
    Uuid::parse_str(param).map_err(|_| format!("Invalid UUID format: {}", param))
}

/// Validate content type for JSON requests
pub fn validate_json_content_type(headers: &HeaderMap) -> Result<(), String> {
    if let Some(content_type) = headers.get("content-type") {
        let content_type_str = content_type
            .to_str()
            .map_err(|_| "Invalid content-type header")?;
        
        if content_type_str.starts_with("application/json") {
            Ok(())
        } else {
            Err(format!("Expected application/json, got: {}", content_type_str))
        }
    } else {
        Err("Missing content-type header".to_string())
    }
}

/// Generate correlation ID for request tracking
pub fn generate_correlation_id() -> String {
    Uuid::new_v4().to_string()
}

/// Sanitize search query to prevent injection attacks
pub fn sanitize_search_query(query: &str) -> String {
    // Remove SQL injection patterns and limit length
    query
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace() || "-_".contains(*c))
        .take(255)
        .collect::<String>()
        .trim()
        .to_string()
}

/// Convert service layer pagination to web layer
pub fn map_service_list_response<T, U>(
    service_response: crate::services::ServiceListResponse<T>,
    page: u32,
    limit: u32,
    mapper: impl Fn(T) -> U,
) -> crate::web::responses::PaginatedResponse<U> {
    let mapped_items: Vec<U> = service_response.items.into_iter().map(mapper).collect();
    
    crate::web::responses::PaginatedResponse::new(
        mapped_items,
        service_response.total_count,
        page,
        limit,
    )
}

/// Helper to build query parameters for services
pub fn build_service_query_params(
    search: Option<String>,
    sort_by: Option<String>,
    sort_ascending: bool,
    page: u32,
    limit: u32,
) -> (Option<String>, Option<String>, bool, Option<u32>, Option<u32>) {
    let sanitized_search = search.map(|s| sanitize_search_query(&s)).filter(|s| !s.is_empty());
    let page_option = if page > 1 { Some(page) } else { None };
    let limit_option = if limit != 50 { Some(limit) } else { None };
    
    (sanitized_search, sort_by, sort_ascending, page_option, limit_option)
}

/// Convert source type string to enum
pub fn parse_source_type(source_type: &str) -> Result<crate::models::StreamSourceType, String> {
    match source_type.to_lowercase().as_str() {
        "m3u" => Ok(crate::models::StreamSourceType::M3u),
        "xtream" => Ok(crate::models::StreamSourceType::Xtream),
        _ => Err(format!("Unknown source type: {}", source_type)),
    }
}

/// Rate limiting helper (placeholder for future implementation)
pub struct RateLimiter;

impl RateLimiter {
    pub fn new() -> Self {
        Self
    }
    
    pub async fn check_rate_limit(&self, _key: &str) -> Result<(), String> {
        // Rate limiting not implemented yet
        Ok(())
    }
}

/// Request size validation
pub fn validate_request_size(content_length: Option<usize>, max_size: usize) -> Result<(), String> {
    if let Some(size) = content_length {
        if size > max_size {
            return Err(format!("Request too large: {} bytes (max: {})", size, max_size));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_search_query() {
        assert_eq!(sanitize_search_query("normal query"), "normal query");
        assert_eq!(sanitize_search_query("query-with_dashes"), "query-with_dashes");
        assert_eq!(sanitize_search_query("'; DROP TABLE users; --"), " DROP TABLE users ");
        assert_eq!(sanitize_search_query("<script>alert('xss')</script>"), "scriptalertxssscript");
    }

    #[test]
    fn test_parse_source_type() {
        assert!(matches!(parse_source_type("m3u"), Ok(crate::models::StreamSourceType::M3u)));
        assert!(matches!(parse_source_type("M3U"), Ok(crate::models::StreamSourceType::M3u)));
        assert!(matches!(parse_source_type("xtream"), Ok(crate::models::StreamSourceType::Xtream)));
        assert!(matches!(parse_source_type("XTREAM"), Ok(crate::models::StreamSourceType::Xtream)));
        assert!(parse_source_type("invalid").is_err());
    }

    #[test]
    fn test_extract_uuid_param() {
        let uuid = Uuid::new_v4();
        assert_eq!(extract_uuid_param(&uuid.to_string()).unwrap(), uuid);
        assert!(extract_uuid_param("invalid-uuid").is_err());
    }
}