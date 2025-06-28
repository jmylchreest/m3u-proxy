use chrono::{DateTime, NaiveDateTime, Utc};
use sqlx;
use uuid::Uuid;

pub mod time;

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

/// Generate a full logo URL using the base URL and logo asset ID
pub fn generate_logo_url(base_url: &str, logo_id: Uuid) -> String {
    let sanitized_base = sanitize_base_url(base_url);
    format!("{}/api/logos/{}", sanitized_base, logo_id)
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

        assert_eq!(
            generate_logo_url("http://localhost:8080", logo_id),
            "http://localhost:8080/api/logos/c63d556e-7b3c-4a85-accd-214c32663482"
        );

        assert_eq!(
            generate_logo_url("http://localhost:8080/", logo_id),
            "http://localhost:8080/api/logos/c63d556e-7b3c-4a85-accd-214c32663482"
        );

        assert_eq!(
            generate_logo_url("https://example.com", logo_id),
            "https://example.com/api/logos/c63d556e-7b3c-4a85-accd-214c32663482"
        );
    }
}
