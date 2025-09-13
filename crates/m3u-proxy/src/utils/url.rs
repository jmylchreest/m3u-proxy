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
    /// use m3u_proxy::utils::url::UrlUtils;
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
            format!("http://{trimmed}")
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

    /// Build Xtream XMLTV URL with proper path joining and authentication
    ///
    /// This function creates the XMLTV endpoint URL for Xtream Codes APIs,
    /// ensuring proper URL construction and avoiding common mistakes like
    /// missing slashes between base URL and endpoint path.
    ///
    /// # Arguments
    ///
    /// * `base_url` - The base Xtream URL (e.g., "http://example.com:8080")
    /// * `username` - The username for authentication
    /// * `password` - The password for authentication
    ///
    /// # Returns
    ///
    /// * `Ok(String)` - Successfully built XMLTV URL
    /// * `Err(url::ParseError)` - Parse error if base URL is invalid
    ///
    /// # Examples
    ///
    /// ```rust
    /// use m3u_proxy::utils::url::UrlUtils;
    ///
    /// let url = UrlUtils::build_xtream_xmltv_url("http://example.com", "user", "pass").unwrap();
    /// assert_eq!(url, "http://example.com/xmltv.php?username=user&password=pass");
    /// ```
    pub fn build_xtream_xmltv_url(
        base_url: &str,
        username: &str,
        password: &str,
    ) -> Result<String, url::ParseError> {
        let xmltv_path_with_params =
            format!("xmltv.php?username={}&password={}", username, password);
        Self::join(base_url, &xmltv_path_with_params)
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

    /// Obfuscate sensitive information in URLs for safe logging
    ///
    /// This function masks usernames and passwords in URLs to prevent
    /// sensitive credentials from appearing in logs.
    ///
    /// # Arguments
    ///
    /// * `url` - The URL to obfuscate
    ///
    /// # Returns
    ///
    /// URL string with usernames and passwords replaced with asterisks
    ///
    /// # Examples
    ///
    /// ```rust
    /// use m3u_proxy::utils::url::UrlUtils;
    ///
    /// let url = "http://user:pass@example.com/path?username=user&password=secret";
    /// let obfuscated = UrlUtils::obfuscate_credentials(url);
    /// // Result: "http://****:****@example.com/path?username=****&password=****"
    /// ```
    pub fn obfuscate_credentials(url: &str) -> String {
        use regex::Regex;

        let mut obfuscated = url.to_string();

        // Handle URL auth (user:pass@host)
        if let Ok(parsed) = Url::parse(url)
            && (!parsed.username().is_empty() || parsed.password().is_some())
        {
            let mut new_url = parsed.clone();
            // Clear existing credentials and set obfuscated ones
            let _ = new_url.set_username("****");
            let _ = new_url.set_password(Some("****"));
            obfuscated = new_url.to_string();
        }

        // Handle query parameters with case-insensitive matching
        let sensitive_params = ["username", "password", "user", "pass", "pwd", "passwd"];

        for param in &sensitive_params {
            // Create case-insensitive regex pattern for each parameter
            let pattern = format!(r"(?i)([?&]{}=)[^&]*", regex::escape(param));
            if let Ok(re) = Regex::new(&pattern) {
                obfuscated = re.replace_all(&obfuscated, "${1}****").to_string();
            }
        }

        obfuscated
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_scheme() {
        assert_eq!(
            UrlUtils::normalize_scheme("example.com"),
            "http://example.com"
        );
        assert_eq!(
            UrlUtils::normalize_scheme("https://example.com"),
            "https://example.com"
        );
        assert_eq!(
            UrlUtils::normalize_scheme("http://example.com"),
            "http://example.com"
        );
        assert_eq!(
            UrlUtils::normalize_scheme("  example.com  "),
            "http://example.com"
        );
    }

    #[test]
    fn test_sanitize() {
        assert_eq!(
            UrlUtils::sanitize("https://example.com/"),
            "https://example.com"
        );
        assert_eq!(
            UrlUtils::sanitize("https://example.com///"),
            "https://example.com"
        );
        assert_eq!(UrlUtils::sanitize("example.com/"), "http://example.com");
    }

    #[test]
    fn test_extract_domain() {
        assert_eq!(
            UrlUtils::extract_domain("https://example.com/path"),
            Some("example.com".to_string())
        );
        assert_eq!(
            UrlUtils::extract_domain("http://sub.example.com"),
            Some("sub.example.com".to_string())
        );
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

    #[test]
    fn test_build_xtream_xmltv_url() {
        // Test basic URL construction
        assert_eq!(
            UrlUtils::build_xtream_xmltv_url("http://example.com", "user", "pass").unwrap(),
            "http://example.com/xmltv.php?username=user&password=pass"
        );

        // Test URL with trailing slash
        assert_eq!(
            UrlUtils::build_xtream_xmltv_url("http://example.com/", "user", "pass").unwrap(),
            "http://example.com/xmltv.php?username=user&password=pass"
        );

        // Test URL with port
        assert_eq!(
            UrlUtils::build_xtream_xmltv_url("http://example.com:8080", "user", "pass").unwrap(),
            "http://example.com:8080/xmltv.php?username=user&password=pass"
        );

        // Test HTTPS
        assert_eq!(
            UrlUtils::build_xtream_xmltv_url("https://secure.example.com", "user", "pass").unwrap(),
            "https://secure.example.com/xmltv.php?username=user&password=pass"
        );
    }

    #[test]
    fn test_obfuscate_credentials() {
        // Test URL with auth
        assert_eq!(
            UrlUtils::obfuscate_credentials("http://user:pass@example.com/path"),
            "http://****:****@example.com/path"
        );

        // Test URL with query parameters
        assert_eq!(
            UrlUtils::obfuscate_credentials("http://example.com/api?username=user&password=secret"),
            "http://example.com/api?username=****&password=****"
        );

        // Test URL with mixed case parameters
        assert_eq!(
            UrlUtils::obfuscate_credentials("http://example.com/api?USERNAME=user&PASSWORD=secret"),
            "http://example.com/api?USERNAME=****&PASSWORD=****"
        );

        // Test URL without credentials
        assert_eq!(
            UrlUtils::obfuscate_credentials("http://example.com/path"),
            "http://example.com/path"
        );

        // Test complex URL with both auth and query params
        assert_eq!(
            UrlUtils::obfuscate_credentials(
                "http://admin:secret@example.com/xmltv.php?username=user&password=pass123&other=value"
            ),
            "http://****:****@example.com/xmltv.php?username=****&password=****&other=value"
        );
    }
}
