//! File categories used by m3u-proxy with the sandboxed file service
//!
//! This module defines the file categories and their retention policies
//! for the sandboxed file service integration.

use std::path::PathBuf;

/// File categories used in m3u-proxy
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum FileCategory {
    /// Preview files for M3U/EPG generation (5 minutes retention)
    Preview,
    /// Cached logo files downloaded from external sources (30 days retention)
    LogoCached,
    /// Uploaded logo files (no expiry)
    LogoUploaded,
    /// Proxy output files (30 days retention)
    ProxyOutput,
    /// General cache files (7 days retention)
    Cache,
    /// Temporary files (1 hour retention)
    Temp,
}

impl FileCategory {
    /// Get the string key for this category
    pub fn as_str(&self) -> &'static str {
        match self {
            FileCategory::Preview => "preview",
            FileCategory::LogoCached => "logo_cached",
            FileCategory::LogoUploaded => "logo_uploaded",
            FileCategory::ProxyOutput => "proxy_output",
            FileCategory::Cache => "cache",
            FileCategory::Temp => "temp",
        }
    }

    /// Get the subdirectory for this category
    pub fn subdirectory(&self) -> &'static str {
        match self {
            FileCategory::Preview => "previews",
            FileCategory::LogoCached => "logos/cached",
            FileCategory::LogoUploaded => "logos/uploaded",
            FileCategory::ProxyOutput => "proxy-output",
            FileCategory::Cache => "cache",
            FileCategory::Temp => "temp",
        }
    }

    /// Get the default retention in minutes
    pub fn default_retention_minutes(&self) -> Option<i64> {
        match self {
            FileCategory::Preview => Some(5),                // 5 minutes
            FileCategory::LogoCached => Some(30 * 24 * 60),  // 30 days
            FileCategory::LogoUploaded => None,              // No expiry
            FileCategory::ProxyOutput => Some(30 * 24 * 60), // 30 days
            FileCategory::Cache => Some(7 * 24 * 60),        // 7 days
            FileCategory::Temp => Some(60),                  // 1 hour
        }
    }

    /// Whether to use last accessed time for cleanup (vs created time)
    pub fn cleanup_on_last_access(&self) -> bool {
        match self {
            FileCategory::Preview => true,
            FileCategory::LogoCached => true, // Clean up based on when last accessed
            FileCategory::LogoUploaded => true,
            FileCategory::ProxyOutput => true,
            FileCategory::Cache => true,
            FileCategory::Temp => false, // Clean up based on creation time
        }
    }
}

/// Get the default base directory for file storage
pub fn get_default_base_directory() -> PathBuf {
    if let Some(xdg_cache) = std::env::var_os("XDG_CACHE_HOME") {
        PathBuf::from(xdg_cache).join("m3u-proxy")
    } else if let Some(home) = std::env::var_os("HOME") {
        PathBuf::from(home).join(".cache").join("m3u-proxy")
    } else {
        std::env::temp_dir().join("m3u-proxy")
    }
}

/// Generate a deterministic cache ID from a URL with normalization
/// This allows us to check if a logo is already cached without database lookups
pub fn generate_logo_cache_id(url: &str) -> String {
    let normalized_url = normalize_logo_url(url);

    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(normalized_url.as_bytes());
    let result = hasher.finalize();

    // Use first 16 bytes of hash as hex string for a reasonable length ID
    hex::encode(&result[..16])
}

/// Normalize a logo URL for consistent caching
/// - Removes scheme (http/https)
/// - Removes file extension from path
/// - Sorts query parameters alphabetically
fn normalize_logo_url(url: &str) -> String {
    // Try to parse as proper URL first
    if let Ok(parsed) = url::Url::parse(url) {
        let host = parsed.host_str().unwrap_or("");
        let path_without_extension = remove_file_extension(parsed.path());

        // Sort query parameters for consistency
        let mut query_pairs: Vec<_> = parsed.query_pairs().collect();
        query_pairs.sort_by(|a, b| a.0.cmp(&b.0));

        let query_string = if query_pairs.is_empty() {
            String::new()
        } else {
            format!(
                "?{}",
                query_pairs
                    .iter()
                    .map(|(k, v)| format!("{k}={v}"))
                    .collect::<Vec<_>>()
                    .join("&")
            )
        };

        format!("{host}{path_without_extension}{query_string}")
    } else {
        // Fallback for malformed URLs - just remove common prefixes and normalize
        let cleaned = url
            .trim_start_matches("http://")
            .trim_start_matches("https://")
            .to_lowercase();

        remove_file_extension(&cleaned)
    }
}

/// Remove file extension from a path, being careful about URLs that might not have extensions
fn remove_file_extension(path: &str) -> String {
    // Find the last dot that's after the last slash (to avoid removing dots from domain names)
    if let Some(last_slash_pos) = path.rfind('/') {
        // Look for dot after the last slash
        let filename_part = &path[last_slash_pos..];
        if let Some(dot_pos) = filename_part.rfind('.') {
            // Check if this looks like a file extension (not too long, alphanumeric)
            let potential_ext = &filename_part[dot_pos + 1..];
            if potential_ext.len() <= 5 && potential_ext.chars().all(|c| c.is_alphanumeric()) {
                // Remove the extension
                return format!("{}{}", &path[..last_slash_pos], &filename_part[..dot_pos]);
            }
        }
    } else {
        // No slash found, check if the whole thing might have an extension
        if let Some(dot_pos) = path.rfind('.') {
            let potential_ext = &path[dot_pos + 1..];
            if potential_ext.len() <= 5 && potential_ext.chars().all(|c| c.is_alphanumeric()) {
                return path[..dot_pos].to_string();
            }
        }
    }

    // No extension found or not a valid extension
    path.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_category_properties() {
        assert_eq!(FileCategory::Preview.as_str(), "preview");
        assert_eq!(FileCategory::LogoCached.subdirectory(), "logos/cached");
        assert_eq!(FileCategory::LogoUploaded.default_retention_minutes(), None);
        assert!(FileCategory::LogoCached.cleanup_on_last_access());
    }

    #[test]
    fn test_url_normalization() {
        // These should all normalize to the same result
        let urls = vec![
            "http://cdn.tv.com/logos/channel.png",
            "https://cdn.tv.com/logos/channel.jpg",
            "HTTP://cdn.tv.com/logos/channel.gif",
            "https://cdn.tv.com/logos/channel.webp",
            "https://cdn.tv.com/logos/channel", // No extension
        ];

        let expected = "cdn.tv.com/logos/channel";
        for url in urls {
            assert_eq!(normalize_logo_url(url), expected, "Failed for URL: {url}");
        }

        // Parameter sorting
        let url1 = "https://cdn.tv.com/logo?b=2&a=1&c=3";
        let url2 = "http://cdn.tv.com/logo?c=3&a=1&b=2";
        assert_eq!(normalize_logo_url(url1), normalize_logo_url(url2));
        assert_eq!(normalize_logo_url(url1), "cdn.tv.com/logo?a=1&b=2&c=3");
    }

    #[test]
    fn test_file_extension_removal() {
        assert_eq!(remove_file_extension("/path/to/file.png"), "/path/to/file");
        assert_eq!(remove_file_extension("/path/to/file.jpg"), "/path/to/file");
        assert_eq!(remove_file_extension("/path/to/file"), "/path/to/file"); // No extension
        assert_eq!(
            remove_file_extension("domain.com/file.png"),
            "domain.com/file"
        );
        assert_eq!(remove_file_extension("domain.com/path"), "domain.com/path"); // No extension

        // Don't remove dots from domain names
        assert_eq!(
            remove_file_extension("sub.domain.com/path"),
            "sub.domain.com/path"
        );

        // Very long "extensions" should not be removed
        assert_eq!(
            remove_file_extension("/file.verylongextension"),
            "/file.verylongextension"
        );
    }

    #[test]
    fn test_logo_cache_id_generation() {
        // These should produce the same cache ID due to normalization
        let url1 = "https://example.com/logo.png";
        let url2 = "http://example.com/logo.jpg";
        let url3 = "HTTP://EXAMPLE.COM/logo.gif";

        let id1 = generate_logo_cache_id(url1);
        let id2 = generate_logo_cache_id(url2);
        let id3 = generate_logo_cache_id(url3);

        assert_eq!(id1, id2);
        assert_eq!(id2, id3);

        // Should be reasonable length
        assert_eq!(id1.len(), 32); // 16 bytes as hex = 32 chars

        // Different normalized URLs should produce different IDs
        let id4 = generate_logo_cache_id("https://example.com/other");
        assert_ne!(id1, id4);

        // Parameter order shouldn't matter
        let url_params1 = "https://example.com/logo?b=2&a=1";
        let url_params2 = "http://example.com/logo?a=1&b=2";
        assert_eq!(
            generate_logo_cache_id(url_params1),
            generate_logo_cache_id(url_params2)
        );
    }
}
