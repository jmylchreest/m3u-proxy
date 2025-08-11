//! Centralized logo URL generation utilities
//!
//! This module provides a unified approach to generating logo URLs throughout
//! the application, replacing the multiple overlapping functions that were
//! previously scattered across the codebase.
//!
//! # Features
//!
//! - Flexible URL generation (relative vs absolute)
//! - Support for different logo formats
//! - Consistent URL sanitization
//! - Builder pattern for complex configurations
//!
//! # Usage
//!
//! ```rust
//! use crate::utils::logo::LogoUrlGenerator;
//! use uuid::Uuid;
//!
//! let logo_id = Uuid::new_v4();
//!
//! // Simple relative URL
//! let relative = LogoUrlGenerator::relative(logo_id);
//!
//! // Full URL with base
//! let full = LogoUrlGenerator::full(logo_id, "https://api.example.com");
//!
//! // Complex configuration
//! let url = LogoUrlGenerator::builder(logo_id)
//!     .base_url("https://api.example.com")
//!     .format("thumbnail")
//!     .build();
//! ```

use uuid::Uuid;

/// Configuration for logo URL generation
///
/// This struct holds all the parameters needed to generate a logo URL.
/// It provides a flexible way to specify different URL generation options.
#[derive(Debug, Clone, PartialEq)]
#[derive(Default)]
pub struct LogoUrlConfig {
    /// Base URL for absolute URLs (None for relative URLs)
    pub base_url: Option<String>,
    /// Optional format specifier (e.g., "original", "thumbnail")
    pub format: Option<String>,
    /// Whether to include domain in the URL
    pub include_domain: bool,
    /// Optional query parameters
    pub query_params: Vec<(String, String)>,
}


/// Builder for creating LogoUrlConfig with fluent interface
///
/// This builder provides a convenient way to construct complex URL configurations
/// using method chaining.
///
/// # Examples
///
/// ```rust
/// let config = LogoUrlConfigBuilder::new()
///     .base_url("https://api.example.com")
///     .format("thumbnail")
///     .query_param("size", "small")
///     .build();
/// ```
#[derive(Debug)]
pub struct LogoUrlConfigBuilder {
    config: LogoUrlConfig,
}

impl Default for LogoUrlConfigBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl LogoUrlConfigBuilder {
    /// Create a new builder with default configuration
    pub fn new() -> Self {
        Self {
            config: LogoUrlConfig::default(),
        }
    }
    
    /// Set the base URL for absolute URLs
    pub fn base_url<S: Into<String>>(mut self, base_url: S) -> Self {
        self.config.base_url = Some(base_url.into());
        self.config.include_domain = true;
        self
    }
    
    /// Set the format specifier
    pub fn format<S: Into<String>>(mut self, format: S) -> Self {
        self.config.format = Some(format.into());
        self
    }
    
    /// Set whether to include domain (only relevant if base_url is set)
    pub fn include_domain(mut self, include: bool) -> Self {
        self.config.include_domain = include;
        self
    }
    
    /// Add a query parameter
    pub fn query_param<K: Into<String>, V: Into<String>>(mut self, key: K, value: V) -> Self {
        self.config.query_params.push((key.into(), value.into()));
        self
    }
    
    /// Build the final configuration
    pub fn build(self) -> LogoUrlConfig {
        self.config
    }
}

/// Centralized logo URL generation
///
/// This struct provides all methods for generating logo URLs in a consistent
/// manner throughout the application. It replaces the multiple functions
/// that were previously doing similar work.
pub struct LogoUrlGenerator;

impl LogoUrlGenerator {
    /// Generate a logo URL with the specified configuration
    ///
    /// This is the core method that all other convenience methods delegate to.
    /// It handles all the logic for building URLs based on the configuration.
    ///
    /// # Arguments
    ///
    /// * `logo_id` - The UUID of the logo
    /// * `config` - Configuration specifying how to build the URL
    ///
    /// # Returns
    ///
    /// The generated URL as a string
    ///
    /// # Examples
    ///
    /// ```rust
    /// let config = LogoUrlConfig {
    ///     base_url: Some("https://api.example.com".to_string()),
    ///     format: Some("thumbnail".to_string()),
    ///     include_domain: true,
    ///     query_params: vec![("size".to_string(), "small".to_string())],
    /// };
    ///
    /// let url = LogoUrlGenerator::generate(logo_id, config);
    /// // Result: "https://api.example.com/api/v1/logos/{uuid}/formats/thumbnail?size=small"
    /// ```
    pub fn generate(logo_id: Uuid, config: LogoUrlConfig) -> String {
        // Build the path component
        let path = match config.format {
            Some(format) => format!("/api/v1/logos/{logo_id}/formats/{format}"),
            None => format!("/api/v1/logos/{logo_id}"),
        };
        
        // Add query parameters if any
        let path_with_query = if config.query_params.is_empty() {
            path
        } else {
            let query_string = config
                .query_params
                .iter()
                .map(|(k, v)| format!("{}={}", urlencoding::encode(k), urlencoding::encode(v)))
                .collect::<Vec<_>>()
                .join("&");
            format!("{path}?{query_string}")
        };
        
        // Combine with base URL if needed
        match (&config.base_url, config.include_domain) {
            (Some(base), true) => format!("{}{}", Self::sanitize_base_url(base), path_with_query),
            _ => path_with_query,
        }
    }
    
    /// Generate a relative logo URL
    ///
    /// This is a convenience method for generating relative URLs without
    /// any domain or base URL information.
    ///
    /// # Arguments
    ///
    /// * `logo_id` - The UUID of the logo
    ///
    /// # Returns
    ///
    /// Relative URL string (e.g., "/api/v1/logos/{uuid}")
    ///
    /// # Examples
    ///
    /// ```rust
    /// let url = LogoUrlGenerator::relative(logo_id);
    /// // Result: "/api/v1/logos/12345678-1234-5678-9abc-123456789abc"
    /// ```
    pub fn relative(logo_id: Uuid) -> String {
        Self::generate(logo_id, LogoUrlConfig::default())
    }
    
    /// Generate a full (absolute) logo URL
    ///
    /// This is a convenience method for generating absolute URLs with
    /// a specified base URL.
    ///
    /// # Arguments
    ///
    /// * `logo_id` - The UUID of the logo
    /// * `base_url` - The base URL to prepend
    ///
    /// # Returns
    ///
    /// Absolute URL string
    ///
    /// # Examples
    ///
    /// ```rust
    /// let url = LogoUrlGenerator::full(logo_id, "https://api.example.com");
    /// // Result: "https://api.example.com/api/v1/logos/12345678-1234-5678-9abc-123456789abc"
    /// ```
    pub fn full<S: Into<String>>(logo_id: Uuid, base_url: S) -> String {
        Self::generate(
            logo_id,
            LogoUrlConfig {
                base_url: Some(base_url.into()),
                include_domain: true,
                ..Default::default()
            },
        )
    }
    
    /// Generate a logo URL with a specific format
    ///
    /// This is a convenience method for generating URLs with format specifiers.
    ///
    /// # Arguments
    ///
    /// * `logo_id` - The UUID of the logo
    /// * `base_url` - Optional base URL (None for relative URLs)
    /// * `format` - The format specifier (e.g., "thumbnail", "original")
    ///
    /// # Returns
    ///
    /// URL string with format path
    ///
    /// # Examples
    ///
    /// ```rust
    /// // Relative URL with format
    /// let url = LogoUrlGenerator::with_format(logo_id, None, "thumbnail");
    /// // Result: "/api/v1/logos/{uuid}/formats/thumbnail"
    ///
    /// // Absolute URL with format
    /// let url = LogoUrlGenerator::with_format(logo_id, Some("https://api.example.com"), "thumbnail");
    /// // Result: "https://api.example.com/api/v1/logos/{uuid}/formats/thumbnail"
    /// ```
    pub fn with_format<S: Into<String>>(
        logo_id: Uuid,
        base_url: Option<S>,
        format: S,
    ) -> String {
        let has_base_url = base_url.is_some();
        Self::generate(
            logo_id,
            LogoUrlConfig {
                base_url: base_url.map(|s| s.into()),
                format: Some(format.into()),
                include_domain: has_base_url,
                ..Default::default()
            },
        )
    }
    
    /// Create a builder for complex URL generation
    ///
    /// This method returns a builder that can be used to create complex
    /// URL configurations with method chaining.
    ///
    /// # Arguments
    ///
    /// * `logo_id` - The UUID of the logo
    ///
    /// # Returns
    ///
    /// A LogoUrlBuilder for method chaining
    ///
    /// # Examples
    ///
    /// ```rust
    /// let url = LogoUrlGenerator::builder(logo_id)
    ///     .base_url("https://api.example.com")
    ///     .format("thumbnail")
    ///     .query_param("size", "small")
    ///     .build();
    /// ```
    pub fn builder(logo_id: Uuid) -> LogoUrlBuilder {
        LogoUrlBuilder::new(logo_id)
    }
    
    /// Sanitize a base URL by removing trailing slashes and validating format
    ///
    /// This method ensures that base URLs are consistently formatted for
    /// concatenation with path components.
    ///
    /// # Arguments
    ///
    /// * `base_url` - The base URL to sanitize
    ///
    /// # Returns
    ///
    /// Sanitized base URL string
    ///
    /// # Examples
    ///
    /// ```rust
    /// let sanitized = LogoUrlGenerator::sanitize_base_url("https://api.example.com/");
    /// assert_eq!(sanitized, "https://api.example.com");
    /// ```
    fn sanitize_base_url(base_url: &str) -> String {
        let mut url = base_url.trim().to_string();
        
        // Remove trailing slashes
        while url.ends_with('/') {
            url.pop();
        }
        
        // Ensure we have a scheme if it looks like a domain
        if !url.starts_with("http://") && !url.starts_with("https://") && url.contains('.') {
            url = format!("https://{url}");
        }
        
        url
    }
}

/// Builder for creating logo URLs with fluent interface
///
/// This builder provides a convenient way to construct complex logo URLs
/// using method chaining, similar to the config builder but with the logo ID
/// already specified.
#[derive(Debug)]
pub struct LogoUrlBuilder {
    logo_id: Uuid,
    config_builder: LogoUrlConfigBuilder,
}

impl LogoUrlBuilder {
    /// Create a new builder for the specified logo ID
    fn new(logo_id: Uuid) -> Self {
        Self {
            logo_id,
            config_builder: LogoUrlConfigBuilder::new(),
        }
    }
    
    /// Set the base URL for absolute URLs
    pub fn base_url<S: Into<String>>(mut self, base_url: S) -> Self {
        self.config_builder = self.config_builder.base_url(base_url);
        self
    }
    
    /// Set the format specifier
    pub fn format<S: Into<String>>(mut self, format: S) -> Self {
        self.config_builder = self.config_builder.format(format);
        self
    }
    
    /// Set whether to include domain
    pub fn include_domain(mut self, include: bool) -> Self {
        self.config_builder = self.config_builder.include_domain(include);
        self
    }
    
    /// Add a query parameter
    pub fn query_param<K: Into<String>, V: Into<String>>(mut self, key: K, value: V) -> Self {
        self.config_builder = self.config_builder.query_param(key, value);
        self
    }
    
    /// Build the final URL
    pub fn build(self) -> String {
        LogoUrlGenerator::generate(self.logo_id, self.config_builder.build())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_uuid() -> Uuid {
        Uuid::parse_str("12345678-1234-5678-9abc-123456789abc").unwrap()
    }

    #[test]
    fn test_relative_url() {
        let url = LogoUrlGenerator::relative(test_uuid());
        assert_eq!(url, "/api/v1/logos/12345678-1234-5678-9abc-123456789abc");
    }

    #[test]
    fn test_full_url() {
        let url = LogoUrlGenerator::full(test_uuid(), "https://api.example.com");
        assert_eq!(
            url,
            "https://api.example.com/api/v1/logos/12345678-1234-5678-9abc-123456789abc"
        );
    }

    #[test]
    fn test_url_with_format() {
        let url = LogoUrlGenerator::with_format(test_uuid(), None, "thumbnail");
        assert_eq!(
            url,
            "/api/v1/logos/12345678-1234-5678-9abc-123456789abc/formats/thumbnail"
        );
    }

    #[test]
    fn test_full_url_with_format() {
        let url = LogoUrlGenerator::with_format(
            test_uuid(),
            Some("https://api.example.com"),
            "thumbnail",
        );
        assert_eq!(
            url,
            "https://api.example.com/api/v1/logos/12345678-1234-5678-9abc-123456789abc/formats/thumbnail"
        );
    }

    #[test]
    fn test_builder() {
        let url = LogoUrlGenerator::builder(test_uuid())
            .base_url("https://api.example.com")
            .format("thumbnail")
            .query_param("size", "small")
            .build();
        
        assert!(url.starts_with("https://api.example.com/api/v1/logos/"));
        assert!(url.contains("/formats/thumbnail"));
        assert!(url.contains("size=small"));
    }

    #[test]
    fn test_sanitize_base_url() {
        assert_eq!(
            LogoUrlGenerator::sanitize_base_url("https://api.example.com/"),
            "https://api.example.com"
        );
        assert_eq!(
            LogoUrlGenerator::sanitize_base_url("https://api.example.com//"),
            "https://api.example.com"
        );
        assert_eq!(
            LogoUrlGenerator::sanitize_base_url("api.example.com"),
            "https://api.example.com"
        );
    }

    #[test]
    fn test_config_builder() {
        let config = LogoUrlConfigBuilder::new()
            .base_url("https://api.example.com")
            .format("thumbnail")
            .query_param("size", "small")
            .build();
        
        assert_eq!(config.base_url, Some("https://api.example.com".to_string()));
        assert_eq!(config.format, Some("thumbnail".to_string()));
        assert!(config.include_domain);
        assert_eq!(config.query_params.len(), 1);
        assert_eq!(config.query_params[0], ("size".to_string(), "small".to_string()));
    }
}