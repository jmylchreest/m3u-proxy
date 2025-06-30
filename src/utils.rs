//! Utility functions for the M3U Proxy application
//!
//! This module provides various helper functions including:
//! - URL normalization and sanitization  
//! - DateTime parsing utilities (DEPRECATED - use utils::datetime)
//! - Logo URL Generation (DEPRECATED - use utils::logo)
//! - Validation utilities (use utils::validation)
//!
//! ## Migration Notice
//!
//! This module is being refactored. Use the new sub-modules:
//! - `utils::datetime` for datetime operations
//! - `utils::logo` for logo URL generation  
//! - `utils::validation` for input validation
//!
//! The functions in this module are maintained for backward compatibility
//! but should be migrated to use the new modules.

use chrono::{DateTime, NaiveDateTime, Utc};
use sqlx;
use uuid::Uuid;

pub mod time;

// New refactored modules
pub mod datetime;
pub mod logo;
pub mod validation;

/// Normalize a URL by ensuring it has a proper scheme (http:// or https://)
/// If the URL already has a scheme, it returns it unchanged.
/// If the URL lacks a scheme, it prepends "http://"
pub fn normalize_url_scheme(url: &str) -> String {
    let trimmed_url = url.trim_end_matches('/');

    if trimmed_url.starts_with("http://") || trimmed_url.starts_with("https://") {
        trimmed_url.to_string()
    } else {
        format!("http://{}", trimmed_url)
    }
}

/// Parse datetime from SQLite format or RFC3339 format
pub fn parse_datetime(datetime_str: &str) -> Result<DateTime<Utc>, sqlx::Error> {
    // Try parsing as RFC3339 first (with timezone info)
    if let Ok(dt) = DateTime::parse_from_rfc3339(datetime_str) {
        return Ok(dt.with_timezone(&Utc));
    }

    // Try parsing as naive datetime and assume UTC
    if let Ok(naive_dt) = NaiveDateTime::parse_from_str(datetime_str, "%Y-%m-%d %H:%M:%S") {
        return Ok(DateTime::from_naive_utc_and_offset(naive_dt, Utc));
    }

    // If both fail, return a decode error
    Err(sqlx::Error::Decode(
        format!("Unable to parse datetime: {}", datetime_str).into(),
    ))
}

/// Sanitize a base URL by removing trailing slashes and ensuring proper format
pub fn sanitize_base_url(base_url: &str) -> String {
    let mut url = base_url.trim().to_string();

    // Remove trailing slashes
    while url.ends_with('/') {
        url.pop();
    }

    // Ensure we have a scheme
    if !url.starts_with("http://") && !url.starts_with("https://") {
        url = format!("http://{}", url);
    }

    url
}

/// Options for logo URL generation
pub struct LogoUrlOptions {
    /// Whether to include the full scheme and domain
    pub include_domain: bool,
    /// Optional format specifier (e.g., "original", "thumbnail")
    pub format: Option<String>,
}

impl Default for LogoUrlOptions {
    fn default() -> Self {
        Self {
            include_domain: true,
            format: None,
        }
    }
}

/// Generate a logo URL with flexible options
pub fn logo_uuid_to_url(logo_id: Uuid, base_url: &str, options: LogoUrlOptions) -> String {
    let base = if options.include_domain {
        sanitize_base_url(base_url)
    } else {
        String::new()
    };

    let path = if let Some(format) = options.format {
        format!("/api/logos/{}/formats/{}", logo_id, format)
    } else {
        format!("/api/logos/{}", logo_id)
    };

    if options.include_domain {
        format!("{}{}", base, path)
    } else {
        path
    }
}

/// Generate a logo URL with optional base URL (convenience function)
/// If base_url is None, returns a relative URL; if Some(base_url), returns a full URL
pub fn generate_logo_url(logo_id: Uuid, base_url: Option<&str>) -> String {
    match base_url {
        Some(base) => logo_uuid_to_url(logo_id, base, LogoUrlOptions::default()),
        None => logo_uuid_to_url(logo_id, "", LogoUrlOptions { 
            include_domain: false, 
            format: None 
        }),
    }
}


/// Generate a full logo URL with a specific format
pub fn logo_uuid_to_url_with_format(logo_id: Uuid, base_url: &str, format: &str) -> String {
    logo_uuid_to_url(
        logo_id,
        base_url,
        LogoUrlOptions {
            include_domain: true,
            format: Some(format.to_string()),
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn test_sanitize_base_url() {
        assert_eq!(
            sanitize_base_url("http://localhost:8080"),
            "http://localhost:8080"
        );
        assert_eq!(
            sanitize_base_url("http://localhost:8080/"),
            "http://localhost:8080"
        );
        assert_eq!(
            sanitize_base_url("http://localhost:8080//"),
            "http://localhost:8080"
        );
        assert_eq!(sanitize_base_url("localhost:8080"), "http://localhost:8080");
        assert_eq!(
            sanitize_base_url("https://example.com/"),
            "https://example.com"
        );
    }

    #[test]
    fn test_generate_logo_url() {
        let logo_id = Uuid::parse_str("c63d556e-7b3c-4a85-accd-214c32663482").unwrap();

        // Test with full URLs
        assert_eq!(
            generate_logo_url(logo_id, Some("http://localhost:8080")),
            "http://localhost:8080/api/logos/c63d556e-7b3c-4a85-accd-214c32663482"
        );

        assert_eq!(
            generate_logo_url(logo_id, Some("http://localhost:8080/")),
            "http://localhost:8080/api/logos/c63d556e-7b3c-4a85-accd-214c32663482"
        );

        assert_eq!(
            generate_logo_url(logo_id, Some("https://example.com")),
            "https://example.com/api/logos/c63d556e-7b3c-4a85-accd-214c32663482"
        );

        // Test with relative URL
        assert_eq!(
            generate_logo_url(logo_id, None),
            "/api/logos/c63d556e-7b3c-4a85-accd-214c32663482"
        );
    }

    #[test]
    fn test_logo_uuid_to_url() {
        let logo_id = Uuid::parse_str("c63d556e-7b3c-4a85-accd-214c32663482").unwrap();

        // Test with full domain
        assert_eq!(
            logo_uuid_to_url(logo_id, "http://localhost:8080", LogoUrlOptions::default()),
            "http://localhost:8080/api/logos/c63d556e-7b3c-4a85-accd-214c32663482"
        );

        // Test relative URL
        assert_eq!(
            logo_uuid_to_url(
                logo_id,
                "http://localhost:8080",
                LogoUrlOptions {
                    include_domain: false,
                    format: None,
                }
            ),
            "/api/logos/c63d556e-7b3c-4a85-accd-214c32663482"
        );

        // Test with format
        assert_eq!(
            logo_uuid_to_url(logo_id, "http://localhost:8080", LogoUrlOptions {
                include_domain: true,
                format: Some("thumbnail".to_string()),
            }),
            "http://localhost:8080/api/logos/c63d556e-7b3c-4a85-accd-214c32663482/formats/thumbnail"
        );

        // Test relative URL with format
        assert_eq!(
            logo_uuid_to_url(
                logo_id,
                "",
                LogoUrlOptions {
                    include_domain: false,
                    format: Some("original".to_string()),
                }
            ),
            "/api/logos/c63d556e-7b3c-4a85-accd-214c32663482/formats/original"
        );
    }

    #[test]
    fn test_convenience_functions() {
        let logo_id = Uuid::parse_str("c63d556e-7b3c-4a85-accd-214c32663482").unwrap();

        // Test relative URL convenience function
        assert_eq!(
            generate_logo_url(logo_id, None),
            "/api/logos/c63d556e-7b3c-4a85-accd-214c32663482"
        );

        // Test format convenience function
        assert_eq!(
            logo_uuid_to_url_with_format(logo_id, "http://localhost:8080", "thumbnail"),
            "http://localhost:8080/api/logos/c63d556e-7b3c-4a85-accd-214c32663482/formats/thumbnail"
        );
    }
}
