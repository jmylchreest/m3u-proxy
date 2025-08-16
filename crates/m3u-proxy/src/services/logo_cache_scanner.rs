//! Logo cache scanner for discovering cached logos in the sandboxed file manager
//!
//! This module provides functionality to scan the sandboxed file manager for cached logos
//! and integrate them with the existing logo search system.

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tracing::{debug, warn};

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
    /// Type of logo: "cached" (from URL) or "uploaded" (manual upload)
    pub logo_type: String,
}

impl CachedLogoInfo {
    /// Convert to a LogoAsset-compatible structure for search results
    pub fn to_logo_asset_like(&self, base_url: &str) -> serde_json::Value {
        let display_name = if let Some(metadata) = &self.metadata {
            if let Some(channel_name) = &metadata.channel_name {
                format!("Cached: {channel_name}")
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
                        .map(|url| format!("Cached from: {url}"))
                })
                .unwrap_or_else(|| "Cached logo".to_string())
        } else {
            self.inferred_source_url
                .as_ref()
                .map(|url| format!("Cached from: {url}"))
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
            "id": self.cache_id, // Use cache_id directly as string ID
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

/// Scanner for logos in the sandboxed file manager
/// Supports both cached logos (from URLs) and uploaded logos (manual uploads)
#[derive(Clone)]
pub struct LogoCacheScanner {
    /// File manager for cached logos (downloaded from URLs)
    cached_file_manager: SandboxedManager,
    /// File manager for uploaded logos (manually uploaded)
    uploaded_file_manager: SandboxedManager,
}


impl LogoCacheScanner {
    /// Create a new logo cache scanner with separate managers for cached and uploaded logos
    pub fn new(cached_file_manager: SandboxedManager, uploaded_file_manager: SandboxedManager) -> Self {
        Self {
            cached_file_manager,
            uploaded_file_manager,
        }
    }

    /// Scan for all cached logos using the cached file manager
    pub async fn scan_cached_logos(&self) -> Result<Vec<CachedLogoInfo>> {
        debug!("Scanning for cached logos using file manager");
        self.scan_logos_in_manager(&self.cached_file_manager, "cached").await
    }
    
    /// Scan for all uploaded logos using the uploaded file manager  
    pub async fn scan_uploaded_logos(&self) -> Result<Vec<CachedLogoInfo>> {
        debug!("Scanning for uploaded logos using file manager");
        self.scan_logos_in_manager(&self.uploaded_file_manager, "uploaded").await
    }
    
    /// Scan for logos in a specific file manager
    async fn scan_logos_in_manager(&self, file_manager: &SandboxedManager, logo_type: &str) -> Result<Vec<CachedLogoInfo>> {
        debug!("Scanning for {} logos using sandboxed file manager", logo_type);
        
        // Use sandboxed list_files operation to scan root directory (use "." for current directory)
        let file_list = match file_manager.list_files(".").await {
            Ok(files) => files,
            Err(e) => {
                warn!("Failed to list files in {} file manager: {}", logo_type, e);
                return Ok(Vec::new());
            }
        };
        
        let mut logos = Vec::new();
        
        for file_name in file_list {
            // Process each file and extract logo information
            match self.process_logo_file_sandboxed(file_manager, &file_name, logo_type).await {
                Ok(Some(logo_info)) => logos.push(logo_info),
                Ok(None) => {}, // File skipped (not an image, etc.)
                Err(e) => {
                    warn!("Failed to process logo file {}: {}", file_name, e);
                    continue;
                }
            }
        }
        
        debug!("Found {} {} logos", logos.len(), logo_type);
        Ok(logos)
    }

    /// Process a single logo file using sandboxed operations
    async fn process_logo_file_sandboxed(&self, file_manager: &SandboxedManager, file_name: &str, logo_type: &str) -> Result<Option<CachedLogoInfo>> {
        // Extract file name
        let file_name = file_name.to_string();
        
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
        
        // Get file metadata using sandboxed operations
        let metadata = match file_manager.metadata(&file_name).await {
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
        
        // Try to load metadata from .json sidecar file
        let metadata_info = Self::load_metadata_sandboxed(file_manager, &cache_id).await;
        
        // Try to infer original URL (this is optional)
        let inferred_source_url = None; // Could implement reverse URL mapping if needed
        
        Ok(Some(CachedLogoInfo {
            cache_id: cache_id.clone(),
            file_name,
            file_extension,
            file_path: PathBuf::from(&cache_id), // Use cache_id as relative path
            size_bytes,
            content_type,
            created_at,
            last_accessed,
            metadata: metadata_info,
            inferred_source_url,
            logo_type: logo_type.to_string(),
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

    /// Get a specific cached logo by cache ID using sandboxed operations
    pub async fn get_cached_logo(&self, cache_id: &str) -> Result<Option<CachedLogoInfo>> {
        // Try PNG first (normalized format: cache_id.png)
        let png_file = format!("{cache_id}.png");
        if self.cached_file_manager.exists(&png_file).await.unwrap_or(false) {
            return self.process_logo_file_sandboxed(&self.cached_file_manager, &png_file, "cached").await;
        }
        
        // Fall back to legacy formats with other extensions
        for ext in &["jpg", "jpeg", "gif", "webp", "svg"] {
            let file_name = format!("{cache_id}.{ext}");
            if self.cached_file_manager.exists(&file_name).await.unwrap_or(false) {
                return self.process_logo_file_sandboxed(&self.cached_file_manager, &file_name, "cached").await;
            }
        }
        
        Ok(None)
    }

    /// Load metadata from .json sidecar file using sandboxed operations
    async fn load_metadata_sandboxed(file_manager: &SandboxedManager, cache_id: &str) -> Option<CachedLogoMetadata> {
        let metadata_file = format!("{cache_id}.json");
        
        // Check if metadata file exists using sandboxed operations
        let exists = match file_manager.exists(&metadata_file).await {
            Ok(exists) => exists,
            Err(_) => return None,
        };
        
        if !exists {
            return None;
        }
        
        // Read metadata file using sandboxed operations
        match file_manager.read_to_string(&metadata_file).await {
            Ok(content) => match serde_json::from_str::<CachedLogoMetadata>(&content) {
                Ok(metadata) => Some(metadata),
                Err(e) => {
                    warn!("Failed to parse metadata file {}: {}", metadata_file, e);
                    None
                }
            },
            Err(e) => {
                warn!("Failed to read metadata file {}: {}", metadata_file, e);
                None
            }
        }
    }
    

    /// Save metadata to .json sidecar file using sandboxed operations  
    pub async fn save_metadata(&self, cache_id: &str, metadata: &CachedLogoMetadata) -> Result<()> {
        let metadata_file = format!("{cache_id}.json");
        let json_content = serde_json::to_string_pretty(metadata)?;
        
        // Use cached file manager for metadata (metadata goes with the cached logos)
        self.cached_file_manager.write(&metadata_file, json_content).await
            .map_err(|e| anyhow::anyhow!("Failed to save metadata: {}", e))?;
        
        debug!("Saved metadata for cache_id: {} using sandboxed operations", cache_id);
        Ok(())
    }

    /// Create or update cached logo with metadata using sandboxed operations
    pub async fn create_cached_logo_with_metadata(
        &self,
        cache_id: &str,
        file_data: Vec<u8>,
        file_extension: &str,
        metadata: CachedLogoMetadata,
    ) -> Result<CachedLogoInfo> {
        // Determine which file manager to use based on logo type
        let (file_manager, logo_type) = if metadata.original_url.is_some() {
            (&self.cached_file_manager, "cached")
        } else {
            (&self.uploaded_file_manager, "uploaded")
        };
        
        // Save the image file using sandboxed operations
        let file_name = format!("{cache_id}.{file_extension}");
        file_manager.write(&file_name, &file_data).await
            .map_err(|e| anyhow::anyhow!("Failed to write logo file: {}", e))?;
        
        // Save the metadata using the appropriate manager
        let metadata_file = format!("{cache_id}.json");
        let json_content = serde_json::to_string_pretty(&metadata)?;
        file_manager.write(&metadata_file, json_content).await
            .map_err(|e| anyhow::anyhow!("Failed to write metadata file: {}", e))?;
        
        // Get file metadata using sandboxed operations
        let file_metadata = file_manager.metadata(&file_name).await
            .map_err(|e| anyhow::anyhow!("Failed to get file metadata: {}", e))?;
        
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
            file_path: PathBuf::from(cache_id), // Use cache_id as relative path
            size_bytes,
            content_type,
            created_at,
            last_accessed,
            metadata: Some(metadata),
            inferred_source_url: None,
            logo_type: logo_type.to_string(),
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

    /// Delete a cached logo and its metadata using sandboxed operations
    pub async fn delete_cached_logo(&self, cache_id: &str) -> Result<bool> {
        let mut deleted = false;
        
        // Try both file managers to find and delete the logo
        for (file_manager, manager_type) in [(&self.cached_file_manager, "cached"), (&self.uploaded_file_manager, "uploaded")] {
            // Try to delete the image file (try different extensions)
            for ext in &["png", "jpg", "jpeg", "gif", "webp", "svg"] {
                let file_name = format!("{cache_id}.{ext}");
                if file_manager.exists(&file_name).await.unwrap_or(false) {
                    file_manager.remove_file(&file_name).await
                        .map_err(|e| anyhow::anyhow!("Failed to delete logo file: {}", e))?;
                    deleted = true;
                    debug!("Deleted {} logo file: {}", manager_type, file_name);
                    break;
                }
            }
            
            // Delete the metadata file
            let metadata_file = format!("{cache_id}.json");
            if file_manager.exists(&metadata_file).await.unwrap_or(false) {
                file_manager.remove_file(&metadata_file).await
                    .map_err(|e| anyhow::anyhow!("Failed to delete metadata file: {}", e))?;
                deleted = true;
                debug!("Deleted {} metadata file: {}", manager_type, metadata_file);
            }
        }
        
        Ok(deleted)
    }

    
    /// Generate metadata for existing cached logo without .json file using sandboxed operations
    pub async fn generate_metadata_for_existing_logo(
        &self,
        cache_id: &str,
        original_url: Option<String>,
        channel_name: Option<String>,
        additional_info: Option<std::collections::HashMap<String, String>>,
    ) -> Result<bool> {
        // Check if metadata already exists in either manager
        let metadata_file = format!("{cache_id}.json");
        
        if self.cached_file_manager.exists(&metadata_file).await.unwrap_or(false) ||
           self.uploaded_file_manager.exists(&metadata_file).await.unwrap_or(false) {
            return Ok(false); // Already has metadata
        }
        
        // Try to find the image file in either manager
        let mut found_file_manager: Option<&SandboxedManager> = None;
        
        for (file_manager, _) in [(&self.cached_file_manager, "cached"), (&self.uploaded_file_manager, "uploaded")] {
            for ext in &["png", "jpg", "jpeg", "gif", "webp", "svg"] {
                let image_file = format!("{cache_id}.{ext}");
                if file_manager.exists(&image_file).await.unwrap_or(false) {
                    found_file_manager = Some(file_manager);
                    break;
                }
            }
            if found_file_manager.is_some() {
                break;
            }
        }
        
        let file_manager = match found_file_manager {
            Some(manager) => manager,
            None => return Ok(false), // No image file found
        };
        
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
        
        // Save metadata using sandboxed operations
        let json_content = serde_json::to_string_pretty(&metadata)?;
        file_manager.write(&metadata_file, json_content).await
            .map_err(|e| anyhow::anyhow!("Failed to write metadata: {}", e))?;
        
        debug!("Generated metadata for existing cached logo: {}", cache_id);
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
