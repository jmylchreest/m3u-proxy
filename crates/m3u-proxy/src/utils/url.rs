//! URL utilities for consistent URL handling
//!
//! This module provides utilities for URL manipulation, validation, and normalization
//! that are used throughout the application.

use url::Url;

/// URL utilities for consistent URL handling
pub struct UrlUtils;

impl UrlUtils {
    /// Normalize URL scheme by ensuring it has a proper HTTP/HTTPS prefix
    ///
    /// This function ensures that URLs have a proper scheme. If no scheme is provided,
    /// it defaults to HTTP. This is useful for handling user input where users might
    /// omit the protocol.
    ///
    /// # Arguments
    ///
    /// * `url` - The URL string to normalize
    ///
    /// # Returns
    ///
    /// String with normalized scheme
    ///
    /// # Examples
    ///
    /// ```rust
    /// use crate::utils::url::UrlUtils;
    ///
    /// assert_eq!(UrlUtils::normalize_scheme("example.com"), "http://example.com");
    /// assert_eq!(UrlUtils::normalize_scheme("https://example.com"), "https://example.com");
    /// assert_eq!(UrlUtils::normalize_scheme("http://example.com"), "http://example.com");
    /// ```
    pub fn normalize_scheme(url: &str) -> String {
        let trimmed = url.trim();
        
        if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
            trimmed.to_string()
        } else {
            format!("http://{}", trimmed)
        }
    }
    
    /// Parse and validate a URL
    ///
    /// This function validates that a URL is properly formatted and parseable.
    ///
    /// # Arguments
    ///
    /// * `url` - The URL string to validate
    ///
    /// # Returns
    ///
    /// * `Ok(Url)` - Successfully parsed URL
    /// * `Err(url::ParseError)` - Parse error
    pub fn parse_and_validate(url: &str) -> Result<Url, url::ParseError> {
        Url::parse(url)
    }
    
    /// Join a base URL with a path segment
    ///
    /// This function safely joins URLs, handling trailing slashes and proper encoding.
    ///
    /// # Arguments
    ///
    /// * `base` - The base URL
    /// * `path` - The path to append
    ///
    /// # Returns
    ///
    /// * `Ok(String)` - Successfully joined URL
    /// * `Err(url::ParseError)` - Parse error
    pub fn join(base: &str, path: &str) -> Result<String, url::ParseError> {
        let base_url = Url::parse(base)?;
        let joined = base_url.join(path)?;
        Ok(joined.to_string())
    }
    
    /// Extract the domain from a URL
    ///
    /// # Arguments
    ///
    /// * `url` - The URL to extract domain from
    ///
    /// # Returns
    ///
    /// * `Some(String)` - Domain if successfully parsed
    /// * `None` - If URL is invalid or has no domain
    pub fn extract_domain(url: &str) -> Option<String> {
        Url::parse(url)
            .ok()
            .and_then(|u| u.host_str().map(|h| h.to_string()))
    }
    
    /// Sanitize URL by removing trailing slashes and normalizing
    ///
    /// # Arguments
    ///
    /// * `url` - The URL to sanitize
    ///
    /// # Returns
    ///
    /// Sanitized URL string
    pub fn sanitize(url: &str) -> String {
        let mut sanitized = Self::normalize_scheme(url);
        
        // Remove trailing slashes (but keep the one after the scheme)
        while sanitized.len() > 8 && sanitized.ends_with('/') {
            sanitized.pop();
        }
        
        sanitized
    }
    
    /// Check if a URL is valid
    ///
    /// # Arguments
    ///
    /// * `url` - The URL to check
    ///
    /// # Returns
    ///
    /// `true` if the URL is valid, `false` otherwise
    pub fn is_valid(url: &str) -> bool {
        Self::parse_and_validate(url).is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_scheme() {
        assert_eq!(UrlUtils::normalize_scheme("example.com"), "http://example.com");
        assert_eq!(UrlUtils::normalize_scheme("https://example.com"), "https://example.com");
        assert_eq!(UrlUtils::normalize_scheme("http://example.com"), "http://example.com");
        assert_eq!(UrlUtils::normalize_scheme("  example.com  "), "http://example.com");
    }
    
    #[test]
    fn test_sanitize() {
        assert_eq!(UrlUtils::sanitize("https://example.com/"), "https://example.com");
        assert_eq!(UrlUtils::sanitize("https://example.com///"), "https://example.com");
        assert_eq!(UrlUtils::sanitize("example.com/"), "http://example.com");
    }
    
    #[test]
    fn test_extract_domain() {
        assert_eq!(UrlUtils::extract_domain("https://example.com/path"), Some("example.com".to_string()));
        assert_eq!(UrlUtils::extract_domain("http://sub.example.com"), Some("sub.example.com".to_string()));
        assert_eq!(UrlUtils::extract_domain("invalid-url"), None);
    }
    
    #[test]
    fn test_is_valid() {
        assert!(UrlUtils::is_valid("https://example.com"));
        assert!(UrlUtils::is_valid("http://example.com/path?query=value"));
        assert!(!UrlUtils::is_valid("not-a-url"));
        assert!(!UrlUtils::is_valid(""));
    }
    
    #[test]
    fn test_join() {
        assert_eq!(
            UrlUtils::join("https://example.com", "api/v1").unwrap(),
            "https://example.com/api/v1"
        );
        assert_eq!(
            UrlUtils::join("https://example.com/", "api/v1").unwrap(),
            "https://example.com/api/v1"
        );
    }
}