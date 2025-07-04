//! Sandboxed file service for secure file operations
//!
//! This module provides sandboxed file management with configurable retention policies.
//! It's used for previews, cached logos, proxy files, and other application file storage.

use anyhow::{Result, Context, bail};
use chrono::{DateTime, Utc, Duration};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::fs;
use tokio::sync::RwLock;
use tokio::time::{sleep, Duration as TokioDuration};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

/// File category with specific retention policy
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum FileCategory {
    Preview,        // 5 minutes
    Logo,          // 90 days  
    ProxyOutput,   // 30 days
    Cache,         // 7 days
    Temp,          // 1 hour
}

impl FileCategory {
    pub fn default_ttl_minutes(&self) -> i64 {
        match self {
            FileCategory::Preview => 5,
            FileCategory::Logo => 90 * 24 * 60,      // 90 days
            FileCategory::ProxyOutput => 30 * 24 * 60, // 30 days  
            FileCategory::Cache => 7 * 24 * 60,       // 7 days
            FileCategory::Temp => 60,                 // 1 hour
        }
    }
    
    pub fn subdirectory(&self) -> &'static str {
        match self {
            FileCategory::Preview => "previews",
            FileCategory::Logo => "logos", 
            FileCategory::ProxyOutput => "proxy-output",
            FileCategory::Cache => "cache",
            FileCategory::Temp => "temp",
        }
    }
}

/// Metadata for a managed file
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManagedFileInfo {
    pub id: String,
    pub category: String, // Store as string for serialization
    pub file_path: PathBuf,
    pub created_at: DateTime<Utc>,
    pub last_accessed: DateTime<Utc>,
    pub size_bytes: u64,
    pub content_type: String,
    pub original_name: Option<String>,
}

/// Configuration for retention policies per category
#[derive(Debug, Clone)]
pub struct RetentionConfig {
    pub category: FileCategory,
    pub ttl_minutes: i64,
}

/// Service for managing sandboxed files with configurable retention
#[derive(Clone)]
pub struct SandboxedFileService {
    base_dir: PathBuf,
    file_registry: Arc<RwLock<HashMap<String, ManagedFileInfo>>>,
    retention_configs: Arc<RwLock<HashMap<FileCategory, i64>>>,
    cleanup_interval_minutes: u64,
}

impl SandboxedFileService {
    /// Create a new sandboxed file service
    pub async fn new(
        cleanup_interval_minutes: Option<u64>, 
        custom_retention_configs: Option<Vec<RetentionConfig>>
    ) -> Result<Self> {
        let base_dir = Self::get_base_directory()?;
        
        // Ensure cache directory exists and set secure permissions
        fs::create_dir_all(&cache_dir).await
            .with_context(|| format!("Failed to create cache directory: {:?}", cache_dir))?;
        
        // Set restrictive permissions (owner read/write/execute only)
        #[cfg(unix)]
        {
            use std::fs::Permissions;
            use std::os::unix::fs::PermissionsExt;
            let perms = Permissions::from_mode(0o700);
            std::fs::set_permissions(&cache_dir, perms)
                .with_context(|| format!("Failed to set permissions on cache directory: {:?}", cache_dir))?;
        }
        
        let service = Self {
            cache_dir,
            file_registry: Arc::new(RwLock::new(HashMap::new())),
            cleanup_interval_minutes: cleanup_interval_minutes.unwrap_or(1), // Check every minute
            file_ttl_minutes: file_ttl_minutes.unwrap_or(5), // Files expire after 5 minutes
        };
        
        // Load existing files from disk on startup
        service.load_existing_files().await?;
        
        // Start cleanup task
        service.start_cleanup_task();
        
        info!(
            "TempFileService initialized - cache_dir: {:?}, cleanup_interval: {}min, ttl: {}min", 
            service.cache_dir, 
            service.cleanup_interval_minutes,
            service.file_ttl_minutes
        );
        
        Ok(service)
    }
    
    /// Get the system cache directory for temporary files
    fn get_cache_directory() -> Result<PathBuf> {
        // Try to use XDG cache directory or fallback to temp
        let cache_dir = if let Some(xdg_cache) = std::env::var_os("XDG_CACHE_HOME") {
            PathBuf::from(xdg_cache).join("m3u-proxy").join("previews")
        } else if let Some(home) = std::env::var_os("HOME") {
            PathBuf::from(home).join(".cache").join("m3u-proxy").join("previews")
        } else {
            std::env::temp_dir().join("m3u-proxy-previews")
        };
        
        debug!("Using cache directory: {:?}", cache_dir);
        Ok(cache_dir)
    }
    
    /// Validate that a file ID is safe and doesn't contain path traversal attempts
    fn validate_file_id(file_id: &str) -> Result<()> {
        // Only allow alphanumeric characters, hyphens, and underscores
        if !file_id.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_') {
            bail!("Invalid file ID: contains unsafe characters");
        }
        
        // Limit length to prevent abuse
        if file_id.len() > 100 {
            bail!("Invalid file ID: too long");
        }
        
        // Check for common path traversal patterns
        if file_id.contains("..") || file_id.contains("/") || file_id.contains("\\") {
            bail!("Invalid file ID: contains path traversal characters");
        }
        
        Ok(())
    }
    
    /// Ensure a path is within the sandbox and resolve it safely
    fn sandbox_path(&self, file_id: &str, extension: &str) -> Result<PathBuf> {
        // Validate file ID first
        Self::validate_file_id(file_id)?;
        
        // Validate extension
        let safe_extension = match extension {
            "m3u" | "xml" | "txt" => extension,
            _ => bail!("Unsupported file extension: {}", extension),
        };
        
        // Create the file path within our sandbox
        let file_path = self.cache_dir.join(format!("{}.{}", file_id, safe_extension));
        
        // Ensure the resulting path is still within our sandbox
        let canonical_cache = self.cache_dir.canonicalize()
            .with_context(|| "Failed to canonicalize cache directory")?;
        
        // For new files that don't exist yet, we check the parent directory
        let path_to_check = if file_path.exists() {
            file_path.canonicalize()
                .with_context(|| format!("Failed to canonicalize file path: {:?}", file_path))?
        } else {
            // For non-existent files, check if the constructed path would be within sandbox
            let parent = file_path.parent()
                .ok_or_else(|| anyhow::anyhow!("File path has no parent directory"))?;
            let canonical_parent = parent.canonicalize()
                .with_context(|| "Failed to canonicalize parent directory")?;
            canonical_parent.join(file_path.file_name().unwrap())
        };
        
        // Verify the path is within our sandbox
        if !path_to_check.starts_with(&canonical_cache) {
            bail!("Path escapes sandbox: {:?}", file_path);
        }
        
        Ok(file_path)
    }
    
    /// Check if a path contains or points to a symlink that could escape the sandbox
    async fn validate_no_symlink_escape(&self, path: &Path) -> Result<()> {
        let metadata = fs::symlink_metadata(path).await?;
        
        if metadata.is_symlink() {
            // For symlinks, check where they point
            let target = fs::read_link(path).await
                .with_context(|| format!("Failed to read symlink: {:?}", path))?;
            
            // Resolve the target relative to the symlink's directory
            let resolved_target = if target.is_absolute() {
                target
            } else {
                path.parent()
                    .ok_or_else(|| anyhow::anyhow!("Symlink has no parent directory"))?
                    .join(target)
            };
            
            // Canonicalize to resolve any .. or . components
            let canonical_target = resolved_target.canonicalize()
                .with_context(|| format!("Failed to canonicalize symlink target: {:?}", resolved_target))?;
            
            let canonical_cache = self.cache_dir.canonicalize()
                .with_context(|| "Failed to canonicalize cache directory")?;
            
            // Ensure the symlink target is within our sandbox
            if !canonical_target.starts_with(&canonical_cache) {
                bail!("Symlink points outside sandbox: {:?} -> {:?}", path, canonical_target);
            }
        }
        
        Ok(())
    }
    
    /// Load existing files from disk on startup (for cleanup after restart)
    async fn load_existing_files(&self) -> Result<()> {
        let mut entries = fs::read_dir(&self.cache_dir).await?;
        let mut loaded_count = 0;
        
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.is_file() {
                if let Some(file_name) = path.file_name().and_then(|n| n.to_str()) {
                    if file_name.ends_with(".m3u") || file_name.ends_with(".xml") {
                        // Extract ID from filename (format: {id}.{extension})
                        if let Some(id) = file_name.split('.').next() {
                            let metadata = entry.metadata().await?;
                            let created_at = metadata.created()
                                .map(|t| DateTime::from(t))
                                .unwrap_or_else(|_| Utc::now());
                            
                            let file_info = TempFileInfo {
                                id: id.to_string(),
                                file_path: path.clone(),
                                created_at,
                                last_accessed: created_at,
                                size_bytes: metadata.len(),
                                content_type: if file_name.ends_with(".m3u") { 
                                    "application/vnd.apple.mpegurl".to_string() 
                                } else { 
                                    "application/xml".to_string() 
                                },
                            };
                            
                            self.file_registry.write().await.insert(id.to_string(), file_info);
                            loaded_count += 1;
                        }
                    }
                }
            }
        }
        
        if loaded_count > 0 {
            info!("Loaded {} existing temporary files from cache directory", loaded_count);
        }
        
        Ok(())
    }
    
    /// Store content as a temporary file and return the file ID
    pub async fn store_content(&self, content: &str, content_type: &str) -> Result<String> {
        let id = Uuid::new_v4().to_string();
        let extension = match content_type {
            "application/vnd.apple.mpegurl" => "m3u",
            "application/xml" => "xml",
            _ => "txt",
        };
        let file_path = self.sandbox_path(&id, extension)?;
        
        // Write content to file
        fs::write(&file_path, content).await
            .with_context(|| format!("Failed to write temporary file: {:?}", file_path))?;
        
        let file_info = TempFileInfo {
            id: id.clone(),
            file_path: file_path.clone(),
            created_at: Utc::now(),
            last_accessed: Utc::now(),
            size_bytes: content.len() as u64,
            content_type: content_type.to_string(),
        };
        
        // Register the file
        self.file_registry.write().await.insert(id.clone(), file_info);
        
        debug!("Stored temporary file: {} -> {:?}", id, file_path);
        Ok(id)
    }
    
    /// Retrieve content by file ID and update last accessed time
    pub async fn get_content(&self, file_id: &str) -> Result<Option<String>> {
        // Validate file ID first
        Self::validate_file_id(file_id)?;
        
        let mut registry = self.file_registry.write().await;
        
        if let Some(file_info) = registry.get_mut(file_id) {
            // Validate the file path is still within sandbox and not a malicious symlink
            self.validate_no_symlink_escape(&file_info.file_path).await?;
            
            // Update last accessed time
            file_info.last_accessed = Utc::now();
            
            // Read file content
            match fs::read_to_string(&file_info.file_path).await {
                Ok(content) => {
                    debug!("Retrieved temporary file content: {}", file_id);
                    Ok(Some(content))
                },
                Err(e) => {
                    warn!("Failed to read temporary file {}: {}", file_id, e);
                    // Remove from registry if file doesn't exist
                    registry.remove(file_id);
                    Ok(None)
                }
            }
        } else {
            Ok(None)
        }
    }
    
    /// Get file info by ID (for serving files directly)
    pub async fn get_file_info(&self, file_id: &str) -> Option<TempFileInfo> {
        // Validate file ID first
        if Self::validate_file_id(file_id).is_err() {
            return None;
        }
        
        let mut registry = self.file_registry.write().await;
        
        if let Some(file_info) = registry.get_mut(file_id) {
            // Validate the file path is still within sandbox
            if self.validate_no_symlink_escape(&file_info.file_path).await.is_err() {
                warn!("Detected unsafe file path for ID {}, removing from registry", file_id);
                registry.remove(file_id);
                return None;
            }
            
            // Update last accessed time
            file_info.last_accessed = Utc::now();
            Some(file_info.clone())
        } else {
            None
        }
    }
    
    /// Start the cleanup task that runs periodically
    fn start_cleanup_task(&self) {
        let service = self.clone();
        
        tokio::spawn(async move {
            let interval = TokioDuration::from_secs(service.cleanup_interval_minutes * 60);
            
            loop {
                sleep(interval).await;
                
                if let Err(e) = service.cleanup_expired_files().await {
                    error!("Error during temporary file cleanup: {}", e);
                }
            }
        });
    }
    
    /// Clean up expired files
    async fn cleanup_expired_files(&self) -> Result<()> {
        let cutoff = Utc::now() - Duration::minutes(self.file_ttl_minutes);
        let mut registry = self.file_registry.write().await;
        let mut files_to_remove = Vec::new();
        
        // Find expired files
        for (id, file_info) in registry.iter() {
            if file_info.last_accessed < cutoff {
                files_to_remove.push((id.clone(), file_info.file_path.clone()));
            }
        }
        
        // Remove expired files
        let mut removed_count = 0;
        for (id, file_path) in files_to_remove {
            // Remove from filesystem
            if let Err(e) = fs::remove_file(&file_path).await {
                warn!("Failed to remove expired file {:?}: {}", file_path, e);
            } else {
                debug!("Removed expired temporary file: {}", id);
                removed_count += 1;
            }
            
            // Remove from registry
            registry.remove(&id);
        }
        
        if removed_count > 0 {
            info!("Cleaned up {} expired temporary files", removed_count);
        }
        
        Ok(())
    }
    
    /// Get statistics about temporary files
    pub async fn get_stats(&self) -> TempFileStats {
        let registry = self.file_registry.read().await;
        let total_files = registry.len();
        let total_size: u64 = registry.values().map(|f| f.size_bytes).sum();
        
        TempFileStats {
            total_files,
            total_size_bytes: total_size,
            cache_directory: self.cache_dir.clone(),
        }
    }
}

/// Statistics about temporary files
#[derive(Debug, Serialize)]
pub struct TempFileStats {
    pub total_files: usize,
    pub total_size_bytes: u64,
    pub cache_directory: PathBuf,
}