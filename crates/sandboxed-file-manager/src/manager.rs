//! Core sandboxed file manager implementation.

use crate::{
    error::{Result, SandboxedFileError},
    file_types::{FileTypeInfo, FileTypeValidator},
    policy::CleanupPolicy,
    security::set_secure_permissions,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::{fs, sync::RwLock, time::interval};

/// Information about a managed file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileInfo {
    pub id: String,
    pub file_path: PathBuf,
    pub created_at: DateTime<Utc>,
    pub last_accessed: DateTime<Utc>,
    pub size_bytes: u64,
    pub content_type: String,
    pub original_name: Option<String>,
}

/// Statistics about managed files.
#[derive(Debug, Serialize)]
pub struct ManagerStats {
    pub total_files: usize,
    pub total_size_bytes: u64,
    pub base_directory: PathBuf,
}

/// Main sandboxed file manager.
#[derive(Clone)]
pub struct SandboxedManager {
    base_dir: PathBuf,
    file_registry: Arc<RwLock<HashMap<String, FileInfo>>>,
    cleanup_policy: CleanupPolicy,
    cleanup_interval: Duration,
}

impl SandboxedManager {
    /// Create a new builder for configuring the manager.
    pub fn builder() -> SandboxedManagerBuilder {
        SandboxedManagerBuilder::new()
    }

    /// Sandboxed version of `std::fs::write` - writes data to a file within the sandbox.
    pub async fn write<P: AsRef<str>, C: AsRef<[u8]>>(&self, path: P, contents: C) -> Result<()> {
        let path_str = path.as_ref();
        let file_path = self.validate_and_get_path(path_str)?;

        fs::write(&file_path, contents.as_ref()).await?;

        // Update registry for tracking
        let file_info = FileInfo {
            id: path_str.to_string(),
            file_path: file_path.clone(),
            created_at: Utc::now(),
            last_accessed: Utc::now(),
            size_bytes: contents.as_ref().len() as u64,
            content_type: "application/octet-stream".to_string(),
            original_name: Some(path_str.to_string()),
        };

        self.file_registry
            .write()
            .await
            .insert(path_str.to_string(), file_info);

        Ok(())
    }

    /// Sandboxed version of `std::fs::read` - reads entire file into a Vec<u8>.
    pub async fn read<P: AsRef<str>>(&self, path: P) -> Result<Vec<u8>> {
        let path_str = path.as_ref();
        let file_path = self.validate_and_get_path(path_str)?;

        // Update access time in registry
        if let Some(file_info) = self.file_registry.write().await.get_mut(path_str) {
            file_info.last_accessed = Utc::now();
        }

        let content = fs::read(&file_path).await?;
        Ok(content)
    }

    /// Sandboxed version of `std::fs::read_to_string` - reads entire file into a String.
    pub async fn read_to_string<P: AsRef<str>>(&self, path: P) -> Result<String> {
        let path_str = path.as_ref();
        let file_path = self.validate_and_get_path(path_str)?;

        // Update access time in registry
        if let Some(file_info) = self.file_registry.write().await.get_mut(path_str) {
            file_info.last_accessed = Utc::now();
        }

        let content = fs::read_to_string(&file_path).await?;
        Ok(content)
    }

    /// Sandboxed version of `std::fs::File::create` - creates a file within the sandbox.
    /// Returns a standard tokio::fs::File that's guaranteed to be within the sandbox.
    pub async fn create<P: AsRef<str>>(&self, path: P) -> Result<fs::File> {
        let path_str = path.as_ref();
        let file_path = self.validate_and_get_path(path_str)?;

        let file = fs::File::create(&file_path).await?;

        // Update registry
        let file_info = FileInfo {
            id: path_str.to_string(),
            file_path: file_path.clone(),
            created_at: Utc::now(),
            last_accessed: Utc::now(),
            size_bytes: 0,
            content_type: "application/octet-stream".to_string(),
            original_name: Some(path_str.to_string()),
        };

        self.file_registry
            .write()
            .await
            .insert(path_str.to_string(), file_info);

        Ok(file)
    }

    /// Sandboxed version of `std::fs::File::open` - opens a file within the sandbox.
    pub async fn open<P: AsRef<str>>(&self, path: P) -> Result<fs::File> {
        let path_str = path.as_ref();
        let file_path = self.validate_and_get_path(path_str)?;

        // Update access time in registry
        if let Some(file_info) = self.file_registry.write().await.get_mut(path_str) {
            file_info.last_accessed = Utc::now();
        }

        let file = fs::File::open(&file_path).await?;
        Ok(file)
    }

    /// Sandboxed version of `std::fs::remove_file` - removes a file within the sandbox.
    pub async fn remove_file<P: AsRef<str>>(&self, path: P) -> Result<()> {
        let path_str = path.as_ref();
        let file_path = self.validate_and_get_path(path_str)?;

        fs::remove_file(&file_path).await?;

        // Remove from registry
        self.file_registry.write().await.remove(path_str);

        Ok(())
    }

    /// Sandboxed version of `std::fs::metadata` - gets metadata for a file within the sandbox.
    pub async fn metadata<P: AsRef<str>>(&self, path: P) -> Result<std::fs::Metadata> {
        let path_str = path.as_ref();
        let file_path = self.validate_and_get_path(path_str)?;

        let metadata = fs::metadata(&file_path).await?;
        Ok(metadata)
    }

    /// Sandboxed version of `std::fs::create_dir` - creates a directory within the sandbox.
    pub async fn create_dir<P: AsRef<str>>(&self, path: P) -> Result<()> {
        let path_str = path.as_ref();
        let dir_path = self.validate_and_get_path(path_str)?;

        fs::create_dir(&dir_path).await?;
        Ok(())
    }

    /// Sandboxed version of `std::fs::create_dir_all` - creates directories recursively within the sandbox.
    pub async fn create_dir_all<P: AsRef<str>>(&self, path: P) -> Result<()> {
        let path_str = path.as_ref();
        let dir_path = self.validate_and_get_path(path_str)?;

        fs::create_dir_all(&dir_path).await?;
        Ok(())
    }

    /// Sandboxed version of `std::fs::remove_dir` - removes an empty directory within the sandbox.
    pub async fn remove_dir<P: AsRef<str>>(&self, path: P) -> Result<()> {
        let path_str = path.as_ref();
        let dir_path = self.validate_and_get_path(path_str)?;

        fs::remove_dir(&dir_path).await?;
        Ok(())
    }

    /// Sandboxed version of `std::fs::remove_dir_all` - removes a directory and all contents within the sandbox.
    pub async fn remove_dir_all<P: AsRef<str>>(&self, path: P) -> Result<()> {
        let path_str = path.as_ref();
        let dir_path = self.validate_and_get_path(path_str)?;

        fs::remove_dir_all(&dir_path).await?;

        // Remove all files in this directory from registry
        let prefix = format!("{}/", path_str);
        let mut registry = self.file_registry.write().await;
        registry.retain(|key, _| !key.starts_with(&prefix) && key != path_str);

        Ok(())
    }

    /// Copy a file within the sandbox - equivalent to `std::fs::copy`.
    pub async fn copy<P: AsRef<str>, Q: AsRef<str>>(&self, from: P, to: Q) -> Result<u64> {
        let from_str = from.as_ref();
        let to_str = to.as_ref();
        let from_path = self.validate_and_get_path(from_str)?;
        let to_path = self.validate_and_get_path(to_str)?;

        let bytes_copied = fs::copy(&from_path, &to_path).await?;

        // Update registry for destination file
        if let Some(source_info) = self.file_registry.read().await.get(from_str).cloned() {
            let dest_info = FileInfo {
                id: to_str.to_string(),
                file_path: to_path,
                created_at: Utc::now(),
                last_accessed: Utc::now(),
                size_bytes: bytes_copied,
                content_type: source_info.content_type,
                original_name: Some(to_str.to_string()),
            };

            self.file_registry
                .write()
                .await
                .insert(to_str.to_string(), dest_info);
        }

        Ok(bytes_copied)
    }

    /// Get file information from the manager's registry.
    pub async fn file_info<P: AsRef<str>>(&self, path: P) -> Result<Option<FileInfo>> {
        let path_str = path.as_ref();
        let registry = self.file_registry.read().await;
        Ok(registry.get(path_str).cloned())
    }

    /// Get statistics about managed files.
    pub async fn stats(&self) -> ManagerStats {
        let registry = self.file_registry.read().await;
        let total_files = registry.len();
        let total_size_bytes = registry.values().map(|f| f.size_bytes).sum();

        ManagerStats {
            total_files,
            total_size_bytes,
            base_directory: self.base_dir.clone(),
        }
    }

    /// Validate file type using magic number detection with sandbox security checks.
    pub async fn validate_file_type<P: AsRef<str>>(
        &self,
        path: P,
        validator: &FileTypeValidator,
    ) -> Result<FileTypeInfo> {
        let path_str = path.as_ref();
        let file_path = self.validate_and_get_path(path_str)?;

        validator
            .validate_file_type_sandboxed(&file_path, &self.base_dir)
            .await
    }

    /// Validate a filepath and construct the full path within the sandbox.
    ///
    /// Uses OS syscalls to properly resolve paths including symlinks, .., ., etc.
    fn validate_and_get_path(&self, filepath: &str) -> Result<PathBuf> {
        // Basic security validation
        if filepath.is_empty() {
            return Err(SandboxedFileError::PathValidation {
                path: PathBuf::from(filepath),
                reason: "Filepath cannot be empty".to_string(),
            });
        }

        if filepath.contains('\0') {
            return Err(SandboxedFileError::PathValidation {
                path: PathBuf::from(filepath),
                reason: "Filepath contains null bytes".to_string(),
            });
        }

        // Reject absolute paths - use relative paths within sandbox
        let path_obj = Path::new(filepath);
        if path_obj.is_absolute() {
            return Err(SandboxedFileError::PathValidation {
                path: PathBuf::from(filepath),
                reason: "Absolute paths not allowed - use relative paths within sandbox"
                    .to_string(),
            });
        }

        // Construct full path within sandbox
        let full_path = self.base_dir.join(filepath);

        // Get canonical base directory (must exist)
        let canonical_base =
            self.base_dir
                .canonicalize()
                .map_err(|e| SandboxedFileError::PathValidation {
                    path: self.base_dir.clone(),
                    reason: format!("Failed to canonicalize base directory: {}", e),
                })?;

        // Create parent directories if they don't exist
        if let Some(parent) = full_path.parent() {
            if !parent.exists() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    SandboxedFileError::DirectoryCreation {
                        path: parent.to_path_buf(),
                        source: e,
                    }
                })?;
            }
        }

        // Use OS to resolve the actual path the file would have
        let resolved_path = if full_path.exists() {
            // File exists - use canonicalize to resolve everything
            full_path
                .canonicalize()
                .map_err(|e| SandboxedFileError::PathValidation {
                    path: full_path.clone(),
                    reason: format!("Failed to resolve existing file path: {}", e),
                })?
        } else {
            // File doesn't exist - resolve parent and construct final path
            let parent = full_path
                .parent()
                .ok_or_else(|| SandboxedFileError::PathValidation {
                    path: full_path.clone(),
                    reason: "Path has no parent directory".to_string(),
                })?;

            let canonical_parent =
                parent
                    .canonicalize()
                    .map_err(|e| SandboxedFileError::PathValidation {
                        path: parent.to_path_buf(),
                        reason: format!("Failed to resolve parent directory: {}", e),
                    })?;

            let filename =
                full_path
                    .file_name()
                    .ok_or_else(|| SandboxedFileError::PathValidation {
                        path: full_path.clone(),
                        reason: "Invalid filename".to_string(),
                    })?;

            canonical_parent.join(filename)
        };

        // Security check: ensure resolved path is within sandbox
        if !resolved_path.starts_with(&canonical_base) {
            return Err(SandboxedFileError::PathValidation {
                path: full_path,
                reason: format!(
                    "Path escapes sandbox: '{}' resolves to '{}' (outside '{}')",
                    filepath,
                    resolved_path.display(),
                    canonical_base.display()
                ),
            });
        }

        tracing::debug!(
            "Path validated: '{}' -> '{}' (within '{}')",
            filepath,
            resolved_path.display(),
            canonical_base.display()
        );

        Ok(full_path)
    }

    /// Manually trigger cleanup of expired files.
    pub async fn cleanup_expired_files(&self) -> Result<usize> {
        if !self.cleanup_policy.enabled {
            return Ok(0);
        }

        let mut registry = self.file_registry.write().await;
        let mut files_to_remove = Vec::new();

        // Find expired files
        for (id, file_info) in registry.iter() {
            // Get file timestamps
            let metadata = match fs::metadata(&file_info.file_path).await {
                Ok(meta) => meta,
                Err(_) => {
                    // File doesn't exist, mark for removal from registry
                    files_to_remove.push((id.clone(), None));
                    continue;
                }
            };

            let modified = DateTime::from(metadata.modified().unwrap_or(std::time::UNIX_EPOCH));
            let created = DateTime::from(metadata.created().unwrap_or(std::time::UNIX_EPOCH));

            // Try to get filesystem atime (access time) - convert to std::fs::Metadata
            let std_metadata = std::fs::metadata(&file_info.file_path)
                .map_err(|e| anyhow::anyhow!("Failed to get std metadata: {}", e))
                .ok();
            let filesystem_atime = if let Some(std_meta) = std_metadata {
                self.get_filesystem_atime(&std_meta).await
            } else {
                None
            };

            if self.cleanup_policy.should_cleanup(
                filesystem_atime,
                file_info.last_accessed,
                modified,
                created,
            ) {
                files_to_remove.push((id.clone(), Some(file_info.file_path.clone())));
            }
        }

        // Remove expired files
        let mut removed_count = 0;
        for (id, file_path_opt) in files_to_remove {
            if let Some(file_path) = file_path_opt {
                // Remove from filesystem
                if let Err(e) = fs::remove_file(&file_path).await {
                    tracing::warn!("Failed to remove expired file {:?}: {}", file_path, e);
                } else {
                    tracing::debug!("Removed expired file: {}", id);
                    removed_count += 1;
                }
            }

            // Remove from registry
            registry.remove(&id);
        }

        if removed_count > 0 {
            tracing::info!("Cleaned up {} expired files", removed_count);
        }

        Ok(removed_count)
    }

    /// Start the background cleanup task.
    fn start_cleanup_task(&self) {
        if !self.cleanup_policy.enabled || self.cleanup_interval.is_zero() {
            return;
        }

        let manager = self.clone();

        tokio::spawn(async move {
            let mut cleanup_interval = interval(manager.cleanup_interval);

            loop {
                cleanup_interval.tick().await;

                if let Err(e) = manager.cleanup_expired_files().await {
                    tracing::error!("Error during file cleanup: {}", e);
                }
            }
        });
    }

    /// Load existing files from disk on startup (for cleanup after restart).
    async fn load_existing_files(&self) -> Result<()> {
        let mut entries = fs::read_dir(&self.base_dir).await?;
        let mut loaded_count = 0;

        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.is_file() {
                if let Some(file_name) = path.file_name().and_then(|n| n.to_str()) {
                    // Use the filename as both ID and original name
                    let filename = file_name.to_string();
                    let metadata = entry.metadata().await?;
                    let created_at =
                        DateTime::from(metadata.created().unwrap_or(std::time::UNIX_EPOCH));

                    let content_type = "application/octet-stream".to_string();

                    let file_info = FileInfo {
                        id: filename.clone(),
                        file_path: path.clone(),
                        created_at,
                        last_accessed: created_at,
                        size_bytes: metadata.len(),
                        content_type,
                        original_name: Some(filename.clone()),
                    };

                    self.file_registry.write().await.insert(filename, file_info);
                    loaded_count += 1;
                }
            }
        }

        if loaded_count > 0 {
            tracing::info!("Loaded {} existing files from disk", loaded_count);
        }

        Ok(())
    }

    /// Try to get filesystem access time (atime) from metadata.
    /// Returns None if atime is not available or unreliable on this filesystem.
    async fn get_filesystem_atime(&self, metadata: &std::fs::Metadata) -> Option<DateTime<Utc>> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;

            // Get atime from filesystem metadata
            let atime_secs = metadata.atime();
            let atime_nsecs = metadata.atime_nsec();

            // Check if atime seems valid (not Unix epoch, not too far in the future)
            if atime_secs > 0 {
                let atime = std::time::UNIX_EPOCH
                    + std::time::Duration::new(atime_secs as u64, atime_nsecs as u32);
                let atime_utc = DateTime::from(atime);

                // Sanity check: atime should be reasonable (after 2000, before far future)
                let year_2000: DateTime<Utc> = DateTime::parse_from_rfc3339("2000-01-01T00:00:00Z")
                    .unwrap()
                    .with_timezone(&chrono::Utc);
                let far_future = Utc::now() + chrono::Duration::days(365); // One year from now

                if atime_utc > year_2000 && atime_utc < far_future {
                    tracing::debug!("Using filesystem atime: {}", atime_utc);
                    return Some(atime_utc);
                } else {
                    tracing::debug!("Filesystem atime seems invalid: {}, ignoring", atime_utc);
                }
            }
        }

        #[cfg(not(unix))]
        {
            // On non-Unix systems, atime might not be available or reliable
            tracing::debug!("Filesystem atime not available on this platform");
        }

        None
    }
}

/// Builder for configuring a SandboxedManager.
pub struct SandboxedManagerBuilder {
    base_directory: Option<PathBuf>,
    cleanup_policy: CleanupPolicy,
    cleanup_interval: Duration,
}

impl SandboxedManagerBuilder {
    fn new() -> Self {
        Self {
            base_directory: None,
            cleanup_policy: CleanupPolicy::default(),
            cleanup_interval: Duration::from_secs(60 * 60), // 1 hour default
        }
    }

    /// Set the base directory for file storage.
    pub fn base_directory<P: Into<PathBuf>>(mut self, path: P) -> Self {
        self.base_directory = Some(path.into());
        self
    }

    /// Set the cleanup policy.
    pub fn cleanup_policy(mut self, policy: CleanupPolicy) -> Self {
        self.cleanup_policy = policy;
        self
    }

    /// Set the cleanup interval.
    pub fn cleanup_interval(mut self, interval: Duration) -> Self {
        self.cleanup_interval = interval;
        self
    }

    /// Build the SandboxedManager.
    pub async fn build(self) -> Result<SandboxedManager> {
        let base_dir = self
            .base_directory
            .ok_or_else(|| SandboxedFileError::Configuration {
                message: "Base directory is required".to_string(),
            })?;

        // Ensure base directory exists and set secure permissions
        fs::create_dir_all(&base_dir)
            .await
            .map_err(|e| SandboxedFileError::DirectoryCreation {
                path: base_dir.clone(),
                source: e,
            })?;

        set_secure_permissions(&base_dir).await?;

        let manager = SandboxedManager {
            base_dir,
            file_registry: Arc::new(RwLock::new(HashMap::new())),
            cleanup_policy: self.cleanup_policy,
            cleanup_interval: self.cleanup_interval,
        };

        // Load existing files from disk
        manager.load_existing_files().await?;

        // Start cleanup task
        manager.start_cleanup_task();

        tracing::info!(
            "SandboxedManager initialized - base_dir: {:?}, cleanup_interval: {:?}, cleanup_enabled: {}",
            manager.base_dir,
            manager.cleanup_interval,
            manager.cleanup_policy.enabled
        );

        Ok(manager)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration as StdDuration;

    #[tokio::test]
    async fn test_store_and_retrieve_content() {
        let temp_dir = tempfile::tempdir().unwrap();

        let manager = SandboxedManager::builder()
            .base_directory(temp_dir.path())
            .cleanup_policy(CleanupPolicy::disabled())
            .build()
            .await
            .unwrap();

        // Write content (like std::fs::write)
        let filename = "test.txt";
        manager.write(filename, "Hello, World!").await.unwrap();

        // Read content (like std::fs::read_to_string)
        let content = manager.read_to_string(filename).await.unwrap();
        assert_eq!(content, "Hello, World!");

        // Get file info from registry
        let info = manager.file_info(filename).await.unwrap();
        assert!(info.is_some());
        let info = info.unwrap();
        assert_eq!(info.id, filename);
        assert_eq!(info.original_name, Some(filename.to_string()));
    }

    #[tokio::test]
    async fn test_cleanup_policy() {
        let temp_dir = tempfile::tempdir().unwrap();

        let policy = CleanupPolicy::new()
            .remove_after(StdDuration::from_millis(50)) // Very short for testing
            .enabled(true);

        let manager = SandboxedManager::builder()
            .base_directory(temp_dir.path())
            .cleanup_policy(policy)
            .build()
            .await
            .unwrap();

        // Store content
        let filename = "test.txt";
        manager.write(filename, "Test content").await.unwrap();

        // File should exist initially
        assert!(manager.read_to_string(filename).await.is_ok());

        // Check initial stats
        let initial_stats = manager.stats().await;
        println!("Initial files in manager: {}", initial_stats.total_files);
        assert_eq!(
            initial_stats.total_files, 1,
            "File should be tracked initially"
        );

        // Wait long enough for the file to be considered expired
        tokio::time::sleep(StdDuration::from_millis(100)).await;

        // Check stats before cleanup
        let before_cleanup_stats = manager.stats().await;
        println!("Files before cleanup: {}", before_cleanup_stats.total_files);

        // Manually trigger cleanup (should remove the expired file)
        let removed = manager.cleanup_expired_files().await.unwrap();

        // Check stats after cleanup
        let after_cleanup_stats = manager.stats().await;
        println!(
            "Files after cleanup: {}, Removed: {}",
            after_cleanup_stats.total_files, removed
        );

        // The file should have been removed (either filesystem atime or in-memory time should be old enough)
        assert!(removed > 0, "Expected at least 1 file to be removed, but removed: {}. Before cleanup: {}, After cleanup: {}",
            removed, before_cleanup_stats.total_files, after_cleanup_stats.total_files);

        // File should be gone
        assert!(manager.read_to_string(filename).await.is_err());
    }

    #[tokio::test]
    async fn test_manager_stats() {
        let temp_dir = tempfile::tempdir().unwrap();

        let manager = SandboxedManager::builder()
            .base_directory(temp_dir.path())
            .cleanup_policy(CleanupPolicy::disabled())
            .build()
            .await
            .unwrap();

        // Initially no files
        let stats = manager.stats().await;
        assert_eq!(stats.total_files, 0);
        assert_eq!(stats.total_size_bytes, 0);

        // Store some content
        manager.write("test1.txt", "Test content").await.unwrap();
        manager.write("test2.json", "More content").await.unwrap();

        let stats = manager.stats().await;
        assert_eq!(stats.total_files, 2);
        assert!(stats.total_size_bytes > 0);
    }

    #[tokio::test]
    async fn test_nested_paths() {
        let temp_dir = tempfile::tempdir().unwrap();

        let manager = SandboxedManager::builder()
            .base_directory(temp_dir.path())
            .cleanup_policy(CleanupPolicy::disabled())
            .build()
            .await
            .unwrap();

        // Test nested directory creation and file storage
        let nested_file = "config/app/settings.json";
        manager
            .write(nested_file, r#"{"debug": true}"#)
            .await
            .unwrap();

        // Verify the nested file exists and can be retrieved
        let content = manager.read_to_string(nested_file).await.unwrap();
        assert_eq!(content, r#"{"debug": true}"#);

        // Verify the file info shows the correct nested path
        let info = manager.file_info(nested_file).await.unwrap();
        assert!(info.is_some());
        let info = info.unwrap();
        assert_eq!(info.id, nested_file);
        assert!(info.file_path.ends_with("config/app/settings.json"));
    }

    #[tokio::test]
    async fn test_path_traversal_resolution() {
        let temp_dir = tempfile::tempdir().unwrap();

        let manager = SandboxedManager::builder()
            .base_directory(temp_dir.path())
            .cleanup_policy(CleanupPolicy::disabled())
            .build()
            .await
            .unwrap();

        // Test paths with .. that resolve within the sandbox
        let valid_traversal_paths = vec![
            "dir/../file.txt",
            "deep/nested/../other/file.txt",
            "a/b/c/../../d/file.txt",
        ];

        for path in valid_traversal_paths {
            let result = manager.write(path, "test content").await;

            assert!(
                result.is_ok(),
                "Should allow path that resolves within sandbox: {}",
                path
            );

            // Verify we can retrieve the content
            let content = manager.read_to_string(path).await.unwrap();
            assert_eq!(content, "test content");
        }
    }

    #[tokio::test]
    async fn test_symlink_within_sandbox() {
        let temp_dir = tempfile::tempdir().unwrap();
        let base_path = temp_dir.path();

        // Create a real file first
        let real_file = base_path.join("realfile.txt");
        std::fs::write(&real_file, "real content").unwrap();

        // Create a symlink pointing to the real file
        let symlink_path = base_path.join("symlink.txt");
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&real_file, &symlink_path).unwrap();
        }
        #[cfg(windows)]
        {
            std::os::windows::fs::symlink_file(&real_file, &symlink_path).unwrap();
        }

        let manager = SandboxedManager::builder()
            .base_directory(base_path)
            .cleanup_policy(CleanupPolicy::disabled())
            .build()
            .await
            .unwrap();

        // Test writing to and reading from the symlink
        let write_result = manager.write("symlink.txt", "symlink content").await;

        assert!(
            write_result.is_ok(),
            "Should allow writing to symlink within sandbox"
        );

        // Try reading from the symlink
        let read_result = manager.read_to_string("symlink.txt").await;
        assert!(
            read_result.is_ok(),
            "Should allow reading from symlink within sandbox"
        );
    }

    #[tokio::test]
    async fn test_invalid_paths() {
        let temp_dir = tempfile::tempdir().unwrap();

        let manager = SandboxedManager::builder()
            .base_directory(temp_dir.path())
            .cleanup_policy(CleanupPolicy::disabled())
            .build()
            .await
            .unwrap();

        // Test paths that should be rejected
        let invalid_paths = vec![
            "/etc/passwd",    // Absolute path
            "file\0name.txt", // Null byte
            "",               // Empty path
        ];

        for path in invalid_paths {
            let result = manager.write(path, "malicious content").await;

            assert!(result.is_err(), "Should reject invalid path: {:?}", path);
        }
    }

    #[tokio::test]
    async fn test_escape_attempts() {
        let temp_dir = tempfile::tempdir().unwrap();
        let base_path = temp_dir.path();

        // Create a target file outside the sandbox that attackers might want to access
        let outside_file = temp_dir.path().parent().unwrap().join("outside_target.txt");
        std::fs::write(&outside_file, "sensitive data").unwrap();

        let manager = SandboxedManager::builder()
            .base_directory(base_path)
            .cleanup_policy(CleanupPolicy::disabled())
            .build()
            .await
            .unwrap();

        // Test various escape attempts that should be blocked
        let escape_attempts = vec![
            "../outside_target.txt",    // Starts with ../
            "../../outside_target.txt", // Starts with ../
            "/etc/passwd",              // Absolute path
            "file\0.txt",               // Null byte
        ];

        // Test paths that resolve within sandbox but use traversal
        let allowed_traversal = vec![
            "a/b/../file.txt",          // Resolves to "a/file.txt"
            "deep/nested/../other.txt", // Resolves to "deep/other.txt"
            "x/y/z/../../file.txt",     // Resolves to "x/file.txt"
        ];

        for attempt in escape_attempts {
            let result = manager.write(attempt, "attack payload").await;

            // These should fail outright due to validation
            assert!(result.is_err(), "Should reject escape attempt: {}", attempt);
        }

        for allowed in allowed_traversal {
            let result = manager.write(allowed, "content").await;

            // These should succeed because they resolve within the sandbox
            assert!(
                result.is_ok(),
                "Should allow path that resolves within sandbox: {}",
                allowed
            );
        }
    }
}
