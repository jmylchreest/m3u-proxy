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

use sandboxed_file_manager::SandboxedManager;

/// Metadata stored in .json files next to cached logos
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedLogoMetadata {
    /// Original URL this logo was cached from
    pub original_url: Option<String>,
    /// Channel name or identifier this logo belongs to
    pub channel_name: Option<String>,
    /// Channel group or category
    pub channel_group: Option<String>,
    /// Description of the logo
    pub description: Option<String>,
    /// Tags for searching
    pub tags: Option<Vec<String>>,
    /// Image dimensions if known
    pub width: Option<i32>,
    pub height: Option<i32>,
    /// Additional metadata fields
    pub extra_fields: Option<std::collections::HashMap<String, String>>,
    /// When this logo was first cached
    pub cached_at: DateTime<Utc>,
    /// Last time this metadata was updated
    pub updated_at: DateTime<Utc>,
}

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
    /// Metadata from .json sidecar file
    pub metadata: Option<CachedLogoMetadata>,
    /// Inferred from cache_id if this represents a cached logo from a URL
    pub inferred_source_url: Option<String>,
}

impl CachedLogoInfo {
    /// Convert to a LogoAsset-compatible structure for search results
    pub fn to_logo_asset_like(&self, base_url: &str) -> serde_json::Value {
        let display_name = if let Some(metadata) = &self.metadata {
            if let Some(channel_name) = &metadata.channel_name {
                format!("Cached: {}", channel_name)
            } else {
                format!("Cached: {}", self.file_name)
            }
        } else {
            format!("Cached: {}", self.file_name)
        };

        let description = if let Some(metadata) = &self.metadata {
            metadata
                .description
                .clone()
                .or_else(|| {
                    metadata
                        .original_url
                        .as_ref()
                        .map(|url| format!("Cached from: {}", url))
                })
                .unwrap_or_else(|| "Cached logo".to_string())
        } else {
            self.inferred_source_url
                .as_ref()
                .map(|url| format!("Cached from: {}", url))
                .unwrap_or_else(|| "Cached logo".to_string())
        };

        let source_url = self
            .metadata
            .as_ref()
            .and_then(|m| m.original_url.clone())
            .or_else(|| self.inferred_source_url.clone());

        let (width, height) = if let Some(metadata) = &self.metadata {
            (metadata.width, metadata.height)
        } else {
            (None, None)
        };

        serde_json::json!({
            // LogoAsset fields (flattened)
            "id": Uuid::new_v4(), // Generate a synthetic UUID
            "name": display_name,
            "description": description,
            "file_name": self.file_name,
            "file_path": format!("cached/{}", self.file_name), // Relative path
            "file_size": self.size_bytes as i64,
            "mime_type": self.content_type,
            "asset_type": "cached",
            "source_url": source_url,
            "width": width,
            "height": height,
            "parent_asset_id": null,
            "format_type": "original",
            "created_at": self.created_at,
            "updated_at": self.last_accessed,
            // LogoAssetWithUrl url field
            "url": format!("{}/api/v1/logos/cached/{}", base_url.trim_end_matches('/'), self.cache_id)
        })
    }
}

/// Scanner for cached logos in the sandboxed file manager
#[derive(Clone)]
pub struct LogoCacheScanner {
    #[allow(dead_code)]
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
        // Use the base_path directly as it should already point to the cached logos directory
        let scan_path = &self.base_path;

        debug!("Scanning for cached logos in: {:?}", scan_path);

        if !scan_path.exists() {
            debug!("Cached logos directory does not exist: {:?}", scan_path);
            return Ok(Vec::new());
        }

        let mut cached_logos = Vec::new();
        let mut entries = fs::read_dir(scan_path).await?;

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
        let created_at = metadata
            .created()
            .map(DateTime::from)
            .unwrap_or_else(|_| Utc::now());
        let last_accessed = metadata
            .accessed()
            .map(DateTime::from)
            .unwrap_or_else(|_| created_at);

        // Determine content type
        let content_type = Self::get_content_type(&file_extension);

        // Try to infer original URL (this is optional - we don't store this mapping)
        let inferred_source_url = None; // Could implement reverse URL mapping if needed

        Ok(Some(CachedLogoInfo {
            cache_id: cache_id.clone(),
            file_name,
            file_extension,
            file_path: file_path.to_path_buf(),
            size_bytes,
            content_type,
            created_at,
            last_accessed,
            metadata: Self::load_metadata(&cache_id, &file_path.parent().unwrap_or(&file_path))
                .await,
            inferred_source_url,
        }))
    }

    /// Check if the file extension is a supported image type
    fn is_supported_image_extension(ext: &str) -> bool {
        matches!(
            ext.to_lowercase().as_str(),
            "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg"
        )
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
        }
        .to_string()
    }

    /// Search cached logos by query string
    pub async fn search_cached_logos(
        &self,
        query: Option<&str>,
        limit: Option<usize>,
    ) -> Result<Vec<CachedLogoInfo>> {
        let mut all_logos = self.scan_cached_logos().await?;

        // Filter by query if provided
        if let Some(query_str) = query {
            let query_lower = query_str.to_lowercase();
            all_logos.retain(|logo| {
                logo.file_name.to_lowercase().contains(&query_lower)
                    || logo.cache_id.to_lowercase().contains(&query_lower)
                    || logo
                        .inferred_source_url
                        .as_ref()
                        .map(|url| url.to_lowercase().contains(&query_lower))
                        .unwrap_or(false)
                    || logo
                        .metadata
                        .as_ref()
                        .map(|m| {
                            m.channel_name
                                .as_ref()
                                .map(|name| name.to_lowercase().contains(&query_lower))
                                .unwrap_or(false)
                                || m.description
                                    .as_ref()
                                    .map(|desc| desc.to_lowercase().contains(&query_lower))
                                    .unwrap_or(false)
                                || m.original_url
                                    .as_ref()
                                    .map(|url| url.to_lowercase().contains(&query_lower))
                                    .unwrap_or(false)
                                || m.tags
                                    .as_ref()
                                    .map(|tags| {
                                        tags.iter()
                                            .any(|tag| tag.to_lowercase().contains(&query_lower))
                                    })
                                    .unwrap_or(false)
                        })
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
        // Use the base_path directly as it should already point to the cached logos directory
        let scan_path = &self.base_path;

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

    /// Load metadata from .json sidecar file
    async fn load_metadata(cache_id: &str, directory: &Path) -> Option<CachedLogoMetadata> {
        let metadata_path = directory.join(format!("{}.json", cache_id));

        if !metadata_path.exists() {
            return None;
        }

        match fs::read_to_string(&metadata_path).await {
            Ok(content) => match serde_json::from_str::<CachedLogoMetadata>(&content) {
                Ok(metadata) => Some(metadata),
                Err(e) => {
                    warn!("Failed to parse metadata file {:?}: {}", metadata_path, e);
                    None
                }
            },
            Err(e) => {
                warn!("Failed to read metadata file {:?}: {}", metadata_path, e);
                None
            }
        }
    }

    /// Save metadata to .json sidecar file
    pub async fn save_metadata(&self, cache_id: &str, metadata: &CachedLogoMetadata) -> Result<()> {
        let metadata_path = self.base_path.join(format!("{}.json", cache_id));

        let json_content = serde_json::to_string_pretty(metadata)?;
        fs::write(&metadata_path, json_content).await?;

        debug!(
            "Saved metadata for cache_id: {} to {:?}",
            cache_id, metadata_path
        );
        Ok(())
    }

    /// Create or update cached logo with metadata
    pub async fn create_cached_logo_with_metadata(
        &self,
        cache_id: &str,
        file_data: Vec<u8>,
        file_extension: &str,
        metadata: CachedLogoMetadata,
    ) -> Result<CachedLogoInfo> {
        // Ensure directory exists
        if !self.base_path.exists() {
            fs::create_dir_all(&self.base_path).await?;
        }

        // Save the image file
        let file_name = format!("{}.{}", cache_id, file_extension);
        let file_path = self.base_path.join(&file_name);
        fs::write(&file_path, &file_data).await?;

        // Save the metadata
        self.save_metadata(cache_id, &metadata).await?;

        // Get file metadata
        let file_metadata = fs::metadata(&file_path).await?;
        let size_bytes = file_metadata.len();
        let created_at = file_metadata
            .created()
            .map(DateTime::from)
            .unwrap_or_else(|_| Utc::now());
        let last_accessed = file_metadata
            .accessed()
            .map(DateTime::from)
            .unwrap_or_else(|_| created_at);

        let content_type = Self::get_content_type(file_extension);

        Ok(CachedLogoInfo {
            cache_id: cache_id.to_string(),
            file_name,
            file_extension: file_extension.to_string(),
            file_path,
            size_bytes,
            content_type,
            created_at,
            last_accessed,
            metadata: Some(metadata),
            inferred_source_url: None,
        })
    }

    /// Update metadata for an existing cached logo
    pub async fn update_metadata(
        &self,
        cache_id: &str,
        metadata: CachedLogoMetadata,
    ) -> Result<()> {
        self.save_metadata(cache_id, &metadata).await
    }

    /// Delete a cached logo and its metadata
    pub async fn delete_cached_logo(&self, cache_id: &str) -> Result<bool> {
        let mut deleted = false;

        // Try to delete the image file (try different extensions)
        for ext in &["png", "jpg", "jpeg", "gif", "webp", "svg"] {
            let file_path = self.base_path.join(format!("{}.{}", cache_id, ext));
            if file_path.exists() {
                fs::remove_file(&file_path).await?;
                deleted = true;
                break;
            }
        }

        // Delete the metadata file
        let metadata_path = self.base_path.join(format!("{}.json", cache_id));
        if metadata_path.exists() {
            fs::remove_file(&metadata_path).await?;
            deleted = true;
        }

        Ok(deleted)
    }

    /// Generate metadata for existing cached logo without .json file
    /// This will create .json files for legacy cached logos
    pub async fn generate_metadata_for_existing_logo(
        &self,
        cache_id: &str,
        original_url: Option<String>,
        channel_name: Option<String>,
        additional_info: Option<std::collections::HashMap<String, String>>,
    ) -> Result<bool> {
        // Check if metadata already exists
        let metadata_path = self.base_path.join(format!("{}.json", cache_id));
        if metadata_path.exists() {
            return Ok(false); // Already has metadata
        }

        // Check if the image file exists
        let mut image_exists = false;
        for ext in &["png", "jpg", "jpeg", "gif", "webp", "svg"] {
            let image_path = self.base_path.join(format!("{}.{}", cache_id, ext));
            if image_path.exists() {
                image_exists = true;
                break;
            }
        }

        if !image_exists {
            return Ok(false); // No image file to generate metadata for
        }

        // Create basic metadata
        let metadata = CachedLogoMetadata {
            original_url,
            channel_name,
            channel_group: None,
            description: None,
            tags: None,
            width: None,
            height: None,
            extra_fields: additional_info,
            cached_at: Utc::now(),
            updated_at: Utc::now(),
        };

        self.save_metadata(cache_id, &metadata).await?;
        debug!("Generated metadata for existing cached logo: {}", cache_id);
        Ok(true)
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
        assert_eq!(
            LogoCacheScanner::get_content_type("unknown"),
            "application/octet-stream"
        );
    }
}
