//! Logo cache scanner for discovering cached logos in the sandboxed file manager
//!
//! This module provides functionality to scan the sandboxed file manager for cached logos
//! and integrate them with the existing logo search system.

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tokio::fs;
use tracing::{debug, warn};
use uuid::Uuid;

use crate::services::file_categories::FileCategory;
use sandboxed_file_manager::SandboxedManager;

/// Information about a cached logo discovered in the filesystem
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedLogoInfo {
    pub cache_id: String,
    pub file_name: String,
    pub file_extension: String,
    pub file_path: PathBuf,
    pub size_bytes: u64,
    pub content_type: String,
    pub created_at: DateTime<Utc>,
    pub last_accessed: DateTime<Utc>,
    /// Inferred from cache_id if this represents a cached logo from a URL
    pub inferred_source_url: Option<String>,
}

impl CachedLogoInfo {
    /// Convert to a LogoAsset-compatible structure for search results
    pub fn to_logo_asset_like(&self, base_url: &str) -> serde_json::Value {
        serde_json::json!({
            "id": self.cache_id, // Use cache_id as the ID
            "name": format!("Cached: {}", self.file_name),
            "description": self.inferred_source_url
                .as_ref()
                .map(|url| format!("Cached from: {}", url))
                .unwrap_or_else(|| "Cached logo".to_string()),
            "file_name": self.file_name,
            "file_path": format!("cached/{}", self.file_name), // Relative path
            "size_bytes": self.size_bytes,
            "mime_type": self.content_type,
            "asset_type": "cached",
            "source_url": self.inferred_source_url,
            "width": null, // We don't store dimensions for cached logos
            "height": null,
            "created_at": self.created_at,
            "updated_at": self.last_accessed,
            // Generate serving URL for cached logos
            "serving_url": format!("{}/api/v1/logos/cached/{}", base_url.trim_end_matches('/'), self.cache_id)
        })
    }
}

/// Scanner for cached logos in the sandboxed file manager
#[derive(Clone)]
pub struct LogoCacheScanner {
    file_manager: SandboxedManager,
    base_path: PathBuf,
}

impl LogoCacheScanner {
    /// Create a new logo cache scanner
    pub fn new(file_manager: SandboxedManager, base_path: PathBuf) -> Self {
        Self {
            file_manager,
            base_path,
        }
    }
    
    /// Scan for all cached logos in the sandboxed file manager
    pub async fn scan_cached_logos(&self) -> Result<Vec<CachedLogoInfo>> {
        let logos_subdir = FileCategory::LogoCached.subdirectory();
        let scan_path = self.base_path.join(logos_subdir);
        
        debug!("Scanning for cached logos in: {:?}", scan_path);
        
        if !scan_path.exists() {
            debug!("Cached logos directory does not exist: {:?}", scan_path);
            return Ok(Vec::new());
        }
        
        let mut cached_logos = Vec::new();
        let mut entries = fs::read_dir(&scan_path).await?;
        
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            
            // Skip directories and non-files
            if !path.is_file() {
                continue;
            }
            
            // Only process known image extensions
            if let Some(cached_logo) = self.process_cached_logo_file(&path).await? {
                cached_logos.push(cached_logo);
            }
        }
        
        debug!("Found {} cached logos", cached_logos.len());
        Ok(cached_logos)
    }
    
    /// Process a single cached logo file
    async fn process_cached_logo_file(&self, file_path: &Path) -> Result<Option<CachedLogoInfo>> {
        // Extract file name 
        let file_name = match file_path.file_name().and_then(|n| n.to_str()) {
            Some(name) => name.to_string(),
            None => {
                warn!("Invalid file name: {:?}", file_path);
                return Ok(None);
            }
        };
        
        // Parse cache_id and extension from filename (format: {cache_id}.{extension})
        let (cache_id, file_extension) = match file_name.rsplit_once('.') {
            Some((id, ext)) => {
                if !Self::is_supported_image_extension(ext) {
                    debug!("Skipping non-image file: {}", file_name);
                    return Ok(None);
                }
                (id.to_string(), ext.to_string())
            }
            None => {
                warn!("File without extension: {}", file_name);
                return Ok(None);
            }
        };
        
        // Get file metadata
        let metadata = match fs::metadata(&file_path).await {
            Ok(meta) => meta,
            Err(e) => {
                warn!("Failed to get metadata for {}: {}", file_name, e);
                return Ok(None);
            }
        };
        
        let size_bytes = metadata.len();
        let created_at = metadata.created()
            .map(DateTime::from)
            .unwrap_or_else(|_| Utc::now());
        let last_accessed = metadata.accessed()
            .map(DateTime::from)
            .unwrap_or_else(|_| created_at);
        
        // Determine content type
        let content_type = Self::get_content_type(&file_extension);
        
        // Try to infer original URL (this is optional - we don't store this mapping)
        let inferred_source_url = None; // Could implement reverse URL mapping if needed
        
        Ok(Some(CachedLogoInfo {
            cache_id,
            file_name,
            file_extension,
            file_path: file_path.to_path_buf(),
            size_bytes,
            content_type,
            created_at,
            last_accessed,
            inferred_source_url,
        }))
    }
    
    /// Check if the file extension is a supported image type
    fn is_supported_image_extension(ext: &str) -> bool {
        matches!(ext.to_lowercase().as_str(), "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg")
    }
    
    /// Get MIME content type for file extension
    fn get_content_type(extension: &str) -> String {
        match extension.to_lowercase().as_str() {
            "png" => "image/png",
            "jpg" | "jpeg" => "image/jpeg",
            "gif" => "image/gif",
            "webp" => "image/webp",
            "svg" => "image/svg+xml",
            _ => "application/octet-stream",
        }.to_string()
    }
    
    /// Search cached logos by query string
    pub async fn search_cached_logos(&self, query: Option<&str>, limit: Option<usize>) -> Result<Vec<CachedLogoInfo>> {
        let mut all_logos = self.scan_cached_logos().await?;
        
        // Filter by query if provided
        if let Some(query_str) = query {
            let query_lower = query_str.to_lowercase();
            all_logos.retain(|logo| {
                logo.file_name.to_lowercase().contains(&query_lower) ||
                logo.cache_id.to_lowercase().contains(&query_lower) ||
                logo.inferred_source_url.as_ref()
                    .map(|url| url.to_lowercase().contains(&query_lower))
                    .unwrap_or(false)
            });
        }
        
        // Sort by last accessed time (most recent first)
        all_logos.sort_by(|a, b| b.last_accessed.cmp(&a.last_accessed));
        
        // Apply limit if specified
        if let Some(limit_count) = limit {
            all_logos.truncate(limit_count);
        }
        
        Ok(all_logos)
    }
    
    /// Get a specific cached logo by cache ID
    pub async fn get_cached_logo(&self, cache_id: &str) -> Result<Option<CachedLogoInfo>> {
        let logos_subdir = FileCategory::LogoCached.subdirectory();
        let scan_path = self.base_path.join(logos_subdir);
        
        // Try PNG first (normalized format: cache_id.png)
        let png_path = scan_path.join(format!("{}.png", cache_id));
        if png_path.exists() {
            return self.process_cached_logo_file(&png_path).await;
        }
        
        // Fall back to legacy formats with other extensions
        for ext in &["jpg", "jpeg", "gif", "webp", "svg"] {
            let file_path = scan_path.join(format!("{}.{}", cache_id, ext));
            if file_path.exists() {
                return self.process_cached_logo_file(&file_path).await;
            }
        }
        
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    
    #[test]
    fn test_is_supported_image_extension() {
        assert!(LogoCacheScanner::is_supported_image_extension("png"));
        assert!(LogoCacheScanner::is_supported_image_extension("JPG"));
        assert!(LogoCacheScanner::is_supported_image_extension("webp"));
        assert!(!LogoCacheScanner::is_supported_image_extension("txt"));
        assert!(!LogoCacheScanner::is_supported_image_extension("pdf"));
    }
    
    #[test]
    fn test_get_content_type() {
        assert_eq!(LogoCacheScanner::get_content_type("png"), "image/png");
        assert_eq!(LogoCacheScanner::get_content_type("JPG"), "image/jpeg");
        assert_eq!(LogoCacheScanner::get_content_type("unknown"), "application/octet-stream");
    }
}