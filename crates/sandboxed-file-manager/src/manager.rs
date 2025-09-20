//! Core sandboxed file manager implementation.

use crate::{
    error::{Result, SandboxedFileError},
    file_types::{FileTypeInfo, FileTypeValidator},
    policy::CleanupPolicy,
    security::set_secure_permissions,
};

use chrono::{DateTime, Utc};
// removed futures::FutureExt
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

/// Internal snapshot entry used for two‑phase cleanup evaluation (Phase 1 snapshot).
#[derive(Debug, Clone)]
struct SnapshotEntry {
    id: String,
    path: PathBuf,
    last_accessed: DateTime<Utc>,
    created_at: DateTime<Utc>,
}

impl SnapshotEntry {
    fn from_registry(id: &str, info: &FileInfo) -> Self {
        Self {
            id: id.to_string(),
            path: info.file_path.clone(),
            last_accessed: info.last_accessed,
            created_at: info.created_at,
        }
    }
}

/// Main sandboxed file manager.
#[derive(Clone, Debug)]
pub struct SandboxedManager {
    base_dir: PathBuf,
    file_registry: Arc<RwLock<HashMap<String, FileInfo>>>,
    cleanup_policy: CleanupPolicy,
    cleanup_interval: Duration,
    cleanup_suspension: Arc<RwLock<Option<std::time::Instant>>>,
}

impl SandboxedManager {
    /// Create a new builder for configuring the manager.
    #[must_use]
    pub fn builder() -> SandboxedManagerBuilder {
        SandboxedManagerBuilder::new()
    }

    /// Sandboxed version of `std::fs::write` - writes data to a file within the sandbox.
    ///
    /// # Errors
    /// Returns an error if:
    /// - The path is invalid or escapes the sandbox
    /// - The underlying write operation fails
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
    ///
    /// # Errors
    /// Returns an error if:
    /// - The path is invalid or outside the sandbox
    /// - The file cannot be opened or read
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

    /// Sandboxed version of `std::fs::read_to_string` - reads entire file into a `String`.
    ///
    /// # Errors
    /// Returns an error if:
    /// - The path is invalid or outside the sandbox
    /// - The file cannot be opened or read as UTF-8 text
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
    /// Returns a standard `tokio::fs::File` that's guaranteed to be within the sandbox.
    ///
    /// # Errors
    /// Returns an error if:
    /// - The path is invalid / escapes the sandbox
    /// - Parent directories cannot be created or the file cannot be created
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
    ///
    /// # Errors
    /// Returns an error if:
    /// - The path is invalid / outside sandbox
    /// - The file cannot be opened
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
    ///
    /// # Errors
    /// Returns an error if:
    /// - The path is invalid / outside sandbox
    /// - The underlying file removal fails
    pub async fn remove_file<P: AsRef<str>>(&self, path: P) -> Result<()> {
        let path_str = path.as_ref();
        let file_path = self.validate_and_get_path(path_str)?;

        fs::remove_file(&file_path).await?;

        // Remove from registry
        self.file_registry.write().await.remove(path_str);

        Ok(())
    }

    /// Sandboxed version of `std::fs::metadata` - gets metadata for a file within the sandbox.
    ///
    /// # Errors
    /// Returns an error if:
    /// - Path validation fails
    /// - Metadata cannot be retrieved
    pub async fn metadata<P: AsRef<str>>(&self, path: P) -> Result<std::fs::Metadata> {
        let path_str = path.as_ref();
        let file_path = self.validate_and_get_path(path_str)?;

        let metadata = fs::metadata(&file_path).await?;
        Ok(metadata)
    }

    /// Sandboxed version of `std::fs::create_dir` - creates a directory within the sandbox.
    ///
    /// # Errors
    /// Returns an error if:
    /// - Path validation fails
    /// - Directory creation fails
    pub async fn create_dir<P: AsRef<str>>(&self, path: P) -> Result<()> {
        let path_str = path.as_ref();
        let dir_path = self.validate_and_get_path(path_str)?;

        fs::create_dir(&dir_path).await?;
        Ok(())
    }

    /// Sandboxed version of `std::fs::create_dir_all` - creates directories recursively within the sandbox.
    ///
    /// # Errors
    /// Returns an error if:
    /// - Path validation fails
    /// - Recursive directory creation fails
    pub async fn create_dir_all<P: AsRef<str>>(&self, path: P) -> Result<()> {
        let path_str = path.as_ref();
        let dir_path = self.validate_and_get_path(path_str)?;

        fs::create_dir_all(&dir_path).await?;
        Ok(())
    }

    /// Sandboxed version of `std::fs::remove_dir` - removes an empty directory within the sandbox.
    ///
    /// # Errors
    /// Returns an error if:
    /// - Path validation fails
    /// - Directory removal fails
    pub async fn remove_dir<P: AsRef<str>>(&self, path: P) -> Result<()> {
        let path_str = path.as_ref();
        let dir_path = self.validate_and_get_path(path_str)?;

        fs::remove_dir(&dir_path).await?;
        Ok(())
    }

    /// Sandboxed version of `std::fs::remove_dir_all` - removes a directory and all contents within the sandbox.
    ///
    /// # Errors
    /// Returns an error if:
    /// - Path validation fails
    /// - Recursive removal fails
    pub async fn remove_dir_all<P: AsRef<str>>(&self, path: P) -> Result<()> {
        let path_str = path.as_ref();
        let dir_path = self.validate_and_get_path(path_str)?;

        fs::remove_dir_all(&dir_path).await?;

        // Remove all files in this directory from registry
        let prefix = format!("{path_str}/");
        self.file_registry
            .write()
            .await
            .retain(|key, _| !key.starts_with(&prefix) && key != path_str);

        Ok(())
    }

    /// Copy a file within the sandbox - equivalent to `std::fs::copy`.
    ///
    /// # Errors
    /// Returns an error if:
    /// - Either source or destination path is invalid
    /// - The underlying copy operation fails
    pub async fn copy<P: AsRef<str>, Q: AsRef<str>>(&self, from: P, to: Q) -> Result<u64> {
        let from_str = from.as_ref();
        let to_str = to.as_ref();
        let from_path = self.validate_and_get_path(from_str)?;
        let to_path = self.validate_and_get_path(to_str)?;

        let bytes_copied = fs::copy(&from_path, &to_path).await?;

        // Update registry for destination file (limit read lock scope)
        let maybe_source_info = {
            let registry = self.file_registry.read().await;
            registry.get(from_str).cloned()
        };
        if let Some(source_info) = maybe_source_info {
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
    ///
    /// # Errors
    /// Returns an error if:
    /// - Path validation fails
    pub async fn file_info<P: AsRef<str>>(&self, path: P) -> Result<Option<FileInfo>> {
        let path_str = path.as_ref();
        let registry = self.file_registry.read().await;
        Ok(registry.get(path_str).cloned())
    }

    /// Get statistics about managed files.
    pub async fn stats(&self) -> ManagerStats {
        // Hold the read lock only as long as needed, then drop explicitly.
        let registry = self.file_registry.read().await;
        let total_files = registry.len();
        let total_size_bytes = registry.values().map(|f| f.size_bytes).sum();
        drop(registry);

        ManagerStats {
            total_files,
            total_size_bytes,
            base_directory: self.base_dir.clone(),
        }
    }

    /// Validate file type using magic number detection with sandbox security checks.
    ///
    /// # Errors
    /// Returns an error if:
    /// - Path validation fails
    /// - File cannot be read
    /// - Detected MIME type is not allowed or unknown
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
                    reason: format!("Failed to canonicalize base directory: {e}"),
                })?;

        // Create parent directories if they don't exist
        if let Some(parent) = full_path.parent()
            && !parent.exists()
        {
            std::fs::create_dir_all(parent).map_err(|e| SandboxedFileError::DirectoryCreation {
                path: parent.to_path_buf(),
                source: e,
            })?;
        }

        // Use OS to resolve the actual path the file would have
        let resolved_path = if full_path.exists() {
            // File exists - use canonicalize to resolve everything
            full_path
                .canonicalize()
                .map_err(|e| SandboxedFileError::PathValidation {
                    path: full_path.clone(),
                    reason: format!("Failed to resolve existing file path: {e}"),
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
                        reason: format!("Failed to resolve parent directory: {e}"),
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

        tracing::trace!(
            "Path validated: '{}' -> '{}' (within '{}')",
            filepath,
            resolved_path.display(),
            canonical_base.display()
        );

        Ok(full_path)
    }

    /// Suspend cleanup operations for a specified duration.
    /// This prevents automatic cleanup from running until the TTL expires or `resume_cleanup()` is called.
    /// Suspend cleanup operations for a specified duration.
    ///
    /// # Errors
    /// Returns an error only if internal lock acquisition fails (rare).
    pub async fn suspend_cleanup(&self, ttl: Duration) -> Result<()> {
        let suspension_until = std::time::Instant::now() + ttl;
        *self.cleanup_suspension.write().await = Some(suspension_until);
        tracing::debug!(
            "Cleanup suspended for {:?} (until {:?})",
            ttl,
            suspension_until
        );
        Ok(())
    }

    /// Update cleanup suspension to expire the specified duration from now.
    /// This always sets the suspension to "duration from current time", not additive.
    /// If no suspension is active, this behaves like `suspend_cleanup()`.
    /// Update cleanup suspension to expire the specified duration from now.
    ///
    /// # Errors
    /// Returns an error only if internal lock acquisition fails (rare).
    pub async fn update_suspension(&self, duration_from_now: Duration) -> Result<()> {
        let suspension_until = std::time::Instant::now() + duration_from_now;
        *self.cleanup_suspension.write().await = Some(suspension_until);
        tracing::trace!(
            "Cleanup suspension updated to {:?} from now (until {:?})",
            duration_from_now,
            suspension_until
        );
        Ok(())
    }

    /// Resume cleanup operations immediately, clearing any active suspension.
    pub async fn resume_cleanup(&self) {
        let mut guard = self.cleanup_suspension.write().await;
        if guard.take().is_some() {
            tracing::debug!("Cleanup operations resumed");
        }
    }

    /// Check if cleanup is currently suspended (TTL hasn't expired).
    async fn is_cleanup_suspended(&self) -> bool {
        self.cleanup_suspension
            .read()
            .await
            .is_some_and(|until| std::time::Instant::now() < until)
    }

    /// Manually trigger cleanup of expired files.
    ///
    /// Two‑phase algorithm to reduce lock contention & complexity:
    /// 1. Acquire a read snapshot of candidate entries & decide which to remove.
    /// 2. Acquire a write lock only for actual removals (filesystem + registry).
    ///
    /// # Errors
    /// Returns an error if unexpected metadata or path validation issues occur.
    ///
    /// (`SnapshotEntry` moved to module scope for Clippy compliance)
    /// Phase 1: collect a stable snapshot (read lock only).
    async fn collect_cleanup_snapshot(&self) -> Vec<SnapshotEntry> {
        let registry = self.file_registry.read().await;
        registry
            .iter()
            .map(|(id, info)| SnapshotEntry::from_registry(id, info))
            .collect()
    }

    /// Evaluate a single snapshot entry for removal.
    async fn evaluate_cleanup_candidate(
        &self,
        entry: &SnapshotEntry,
    ) -> Option<(String, Option<PathBuf>)> {
        // If file disappeared already -> remove from registry only
        let Ok(meta) = fs::metadata(&entry.path).await else {
            return Some((entry.id.clone(), None));
        };

        let modified = DateTime::from(meta.modified().unwrap_or(std::time::UNIX_EPOCH));
        let created = DateTime::from(meta.created().unwrap_or(std::time::UNIX_EPOCH));

        // Attempt to obtain filesystem atime using helper (best effort; may return None)
        let fs_atime = match std::fs::metadata(&entry.path) {
            Ok(std_meta) => self.get_filesystem_atime(&std_meta).await,
            Err(_) => None,
        };

        if self.cleanup_policy.should_cleanup(
            fs_atime,
            entry.last_accessed,
            modified,
            created.max(entry.created_at),
        ) {
            Some((entry.id.clone(), Some(entry.path.clone())))
        } else {
            None
        }
    }

    /// Phase 2: apply filesystem + registry removals (write lock).
    async fn apply_cleanup_removals(&self, removals: Vec<(String, Option<PathBuf>)>) -> usize {
        if removals.is_empty() {
            return 0;
        }
        let mut removed = 0;
        let mut registry = self.file_registry.write().await;
        for (id, maybe_path) in removals {
            if let Some(path) = maybe_path {
                match fs::remove_file(&path).await {
                    Ok(()) => {
                        tracing::debug!("Removed expired file: {}", id);
                        removed += 1;
                    }
                    Err(e) => {
                        tracing::warn!("Failed to remove expired file {:?}: {}", path, e);
                    }
                }
            }
            registry.remove(&id);
        }
        removed
    }

    /// # Errors
    /// Returns an error if filesystem metadata retrieval or file removal encounters unexpected issues.
    pub async fn cleanup_expired_files(&self) -> Result<usize> {
        if self.cleanup_policy.infinite_retention {
            return Ok(0);
        }
        if self.is_cleanup_suspended().await {
            tracing::trace!("Cleanup skipped due to active suspension");
            return Ok(0);
        }

        // Phase 1
        let snapshot = self.collect_cleanup_snapshot().await;

        // Evaluate candidates (sequential; could be parallelized later)
        let mut removals = Vec::with_capacity(snapshot.len());
        for entry in &snapshot {
            if let Some(r) = self.evaluate_cleanup_candidate(entry).await {
                removals.push(r);
            }
        }

        // Phase 2
        let removed = self.apply_cleanup_removals(removals).await;

        if removed > 0 {
            tracing::info!("Cleaned up {} expired files", removed);
        }
        Ok(removed)
    }

    /// Start the background cleanup task.
    fn start_cleanup_task(&self) {
        if self.cleanup_policy.infinite_retention || self.cleanup_interval.is_zero() {
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
            if path.is_file()
                && let Some(file_name) = path.file_name().and_then(|n| n.to_str())
            {
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

        if loaded_count > 0 {
            tracing::info!("Loaded {} existing files from disk", loaded_count);
        }

        Ok(())
    }

    /// Sandboxed version of `Path::exists` - checks if a file exists within the sandbox.
    ///
    /// # Errors
    /// Returns an error if:
    /// - The path is empty, absolute, or escapes the sandbox root
    /// - Intermediate parent directories cannot be created during validation
    #[allow(clippy::unused_async)]
    pub async fn exists<P: AsRef<str>>(&self, path: P) -> Result<bool> {
        let path_str = path.as_ref();
        let file_path = self.validate_and_get_path(path_str)?;

        Ok(file_path.exists())
    }

    /// Get the full filesystem path for a file within the sandbox.
    /// Returns the absolute path that can be used for serving files.
    ///
    /// # Errors
    /// Returns an error if:
    /// - The provided relative path is empty or attempts path traversal
    /// - The resolved path would escape the sandbox root
    pub fn get_full_path<P: AsRef<str>>(&self, path: P) -> Result<PathBuf> {
        let path_str = path.as_ref();
        self.validate_and_get_path(path_str)
    }

    /// List all files in a directory within the sandbox.
    /// Returns a vector of relative path strings.
    ///
    /// # Errors
    /// Returns an error if:
    /// - The directory path is invalid or escapes the sandbox
    /// - The underlying `read_dir` operation fails
    pub async fn list_files<P: AsRef<str>>(&self, dir_path: P) -> Result<Vec<String>> {
        let dir_str = dir_path.as_ref();
        let full_dir_path = self.validate_and_get_path(dir_str)?;

        let mut files = Vec::new();

        if full_dir_path.is_dir() {
            let mut entries = fs::read_dir(&full_dir_path).await?;

            while let Some(entry) = entries.next_entry().await? {
                let entry_path = entry.path();

                // Get relative path from the sandbox base
                if let Ok(relative) = entry_path.strip_prefix(&self.base_dir)
                    && let Some(path_str) = relative.to_str()
                {
                    // Files in root directory produce an empty relative path; substitute filename
                    if path_str.is_empty() {
                        if let Some(filename) = entry_path.file_name()
                            && let Some(filename_str) = filename.to_str()
                        {
                            files.push(filename_str.to_string());
                        }
                    } else {
                        files.push(path_str.to_string());
                    }
                }
            }
        }

        Ok(files)
    }

    /// Try to get filesystem access time (atime) from metadata.
    /// Returns None if atime is not available or unreliable on this filesystem.
    #[allow(clippy::unused_async)]
    async fn get_filesystem_atime(&self, metadata: &std::fs::Metadata) -> Option<DateTime<Utc>> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;

            // Get atime from filesystem metadata
            let atime_seconds = metadata.atime();
            let atime_subsec_nanos = metadata.atime_nsec();

            // Check if atime seems valid (not Unix epoch, not too far in the future)
            if atime_seconds >= 0 && atime_subsec_nanos >= 0 {
                if let (Ok(sec), Ok(nanos)) = (
                    u64::try_from(atime_seconds),
                    u32::try_from(atime_subsec_nanos),
                ) {
                    let atime = std::time::UNIX_EPOCH + std::time::Duration::new(sec, nanos);
                    let atime_utc = DateTime::from(atime);

                    // Sanity check: atime should be reasonable (after 2000, before far future)
                    let maybe_year_2000 = DateTime::parse_from_rfc3339("2000-01-01T00:00:00Z")
                        .map(|d| d.with_timezone(&Utc));
                    if let Ok(year_2000) = maybe_year_2000 {
                        let far_future = Utc::now() + chrono::Duration::days(365); // One year from now

                        if atime_utc > year_2000 && atime_utc < far_future {
                            tracing::debug!("Using filesystem atime: {}", atime_utc);
                            return Some(atime_utc);
                        }
                        tracing::debug!("Filesystem atime seems invalid: {}, ignoring", atime_utc);
                    }
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

/// Builder for configuring a `SandboxedManager`.
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
    #[must_use]
    pub fn base_directory<P: Into<PathBuf>>(mut self, path: P) -> Self {
        self.base_directory = Some(path.into());
        self
    }

    /// Set the cleanup policy.
    #[must_use]
    #[allow(clippy::missing_const_for_fn)]
    pub fn cleanup_policy(mut self, policy: CleanupPolicy) -> Self {
        self.cleanup_policy = policy;
        self
    }

    /// Set the cleanup interval.
    #[must_use]
    #[allow(clippy::missing_const_for_fn)]
    pub fn cleanup_interval(mut self, interval: Duration) -> Self {
        self.cleanup_interval = interval;
        self
    }

    /// Build the `SandboxedManager`.
    /// Build the `SandboxedManager`.
    ///
    /// # Errors
    /// Returns an error if:
    /// - Base directory is not set
    /// - Base directory cannot be created or secured
    /// - Existing file loading fails
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
            cleanup_suspension: Arc::new(RwLock::new(None)),
        };

        // Load existing files from disk
        manager.load_existing_files().await?;

        // Start cleanup task
        manager.start_cleanup_task();

        tracing::info!(
            "SandboxedManager initialized - base_dir: {:?}, cleanup_interval: {:?}, cleanup_enabled: {}",
            manager.base_dir,
            manager.cleanup_interval,
            !manager.cleanup_policy.infinite_retention
        );

        Ok(manager)
    }
}

#[cfg(test)]
#[allow(clippy::print_stderr)]
mod tests {
    use super::*;
    use std::time::Duration as StdDuration;

    #[tokio::test]
    async fn test_store_and_retrieve_content() -> std::result::Result<(), Box<dyn std::error::Error>>
    {
        let temp_dir = tempfile::tempdir()?;

        let manager = SandboxedManager::builder()
            .base_directory(temp_dir.path())
            .cleanup_policy(CleanupPolicy::disabled())
            .build()
            .await?;

        // Write content (like std::fs::write)
        let filename = "test.txt";
        manager.write(filename, "Hello, World!").await?;

        // Read content (like std::fs::read_to_string)
        let content = manager.read_to_string(filename).await?;
        assert_eq!(content, "Hello, World!");

        // Get file info from registry
        let info_opt = manager.file_info(filename).await?;
        let Some(info) = info_opt else {
            return Err("file info should exist".into());
        };
        assert_eq!(info.id, filename);
        assert_eq!(info.original_name, Some(filename.to_string()));
        Ok(())
    }

    #[tokio::test]
    async fn test_cleanup_policy() -> std::result::Result<(), Box<dyn std::error::Error>> {
        let temp_dir = tempfile::tempdir()?;

        let policy = CleanupPolicy::new()
            .remove_after(StdDuration::from_millis(50)) // Very short for testing
            .enabled(true);

        let manager = SandboxedManager::builder()
            .base_directory(temp_dir.path())
            .cleanup_policy(policy)
            .build()
            .await?;

        // Store content
        let filename = "test.txt";
        manager.write(filename, "Test content").await?;

        // File should exist initially
        assert!(manager.read_to_string(filename).await.is_ok());

        // Wait long enough for the file to be considered expired
        tokio::time::sleep(StdDuration::from_millis(100)).await;

        // Manually trigger cleanup (should remove the expired file)
        let removed = manager.cleanup_expired_files().await?;

        // The file should have been removed
        assert!(
            removed > 0,
            "Expected at least 1 file to be removed, but removed: {removed}"
        );

        // File should be gone
        assert!(manager.read_to_string(filename).await.is_err());
        Ok(())
    }

    #[tokio::test]
    async fn test_manager_stats() -> std::result::Result<(), Box<dyn std::error::Error>> {
        let temp_dir = tempfile::tempdir()?;

        let manager = SandboxedManager::builder()
            .base_directory(temp_dir.path())
            .cleanup_policy(CleanupPolicy::disabled())
            .build()
            .await?;

        // Initially no files
        let stats = manager.stats().await;
        assert_eq!(stats.total_files, 0);
        assert_eq!(stats.total_size_bytes, 0);

        // Store some content
        manager.write("test1.txt", "Test content").await?;
        manager.write("test2.json", "More content").await?;

        let stats = manager.stats().await;
        assert_eq!(stats.total_files, 2);
        assert!(stats.total_size_bytes > 0);
        Ok(())
    }

    #[tokio::test]
    async fn test_nested_paths() -> std::result::Result<(), Box<dyn std::error::Error>> {
        let temp_dir = tempfile::tempdir()?;

        let manager = SandboxedManager::builder()
            .base_directory(temp_dir.path())
            .cleanup_policy(CleanupPolicy::disabled())
            .build()
            .await?;

        // Test nested directory creation and file storage
        let nested_file = "config/app/settings.json";
        manager.write(nested_file, r#"{"debug": true}"#).await?;

        // Verify the nested file exists and can be retrieved
        let content = manager.read_to_string(nested_file).await?;
        assert_eq!(content, r#"{"debug": true}"#);

        // Verify the file info shows the correct nested path
        let info_opt = manager.file_info(nested_file).await?;
        let Some(info) = info_opt else {
            return Err("nested file info should exist".into());
        };
        assert_eq!(info.id, nested_file);
        assert!(info.file_path.ends_with("config/app/settings.json"));
        Ok(())
    }

    #[tokio::test]
    async fn test_path_traversal_resolution() -> std::result::Result<(), Box<dyn std::error::Error>>
    {
        let temp_dir = tempfile::tempdir()?;

        let manager = SandboxedManager::builder()
            .base_directory(temp_dir.path())
            .cleanup_policy(CleanupPolicy::disabled())
            .build()
            .await?;

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
                "Should allow path that resolves within sandbox: {path}",
            );

            // Verify we can retrieve the content
            let content = manager.read_to_string(path).await?;
            assert_eq!(content, "test content");
        }
        Ok(())
    }

    #[tokio::test]
    async fn test_symlink_within_sandbox() -> std::result::Result<(), Box<dyn std::error::Error>> {
        let temp_dir = tempfile::tempdir()?;
        let base_path = temp_dir.path();

        // Create a real file first
        let real_file = base_path.join("realfile.txt");
        std::fs::write(&real_file, "real content")?;

        // Create a symlink pointing to the real file
        let symlink_path = base_path.join("symlink.txt");
        #[cfg(unix)]
        {
            if std::os::unix::fs::symlink(&real_file, &symlink_path).is_err() {
                // Skipping symlink test (unix) due to symlink creation error
                return Ok(());
            }
        }
        #[cfg(windows)]
        {
            if let Err(e) = std::os::windows::fs::symlink_file(&real_file, &symlink_path) {
                eprintln!("Skipping symlink test (windows) due to error: {e}");
                return Ok(());
            }
        }

        let manager = SandboxedManager::builder()
            .base_directory(base_path)
            .cleanup_policy(CleanupPolicy::disabled())
            .build()
            .await?;

        // Test writing to and reading from the symlink
        manager.write("symlink.txt", "symlink content").await?;

        // Try reading from the symlink
        let _content = manager.read_to_string("symlink.txt").await?;
        Ok(())
    }

    #[tokio::test]
    async fn test_invalid_paths() -> std::result::Result<(), Box<dyn std::error::Error>> {
        let temp_dir = tempfile::tempdir()?;

        let manager = SandboxedManager::builder()
            .base_directory(temp_dir.path())
            .cleanup_policy(CleanupPolicy::disabled())
            .build()
            .await?;

        // Test paths that should be rejected
        let invalid_paths = vec![
            "/etc/passwd",    // Absolute path
            "file\0name.txt", // Null byte
            "",               // Empty path
        ];

        for path in invalid_paths {
            let result = manager.write(path, "malicious content").await;
            assert!(result.is_err(), "Should reject invalid path: {path:?}");
        }
        Ok(())
    }

    #[tokio::test]
    async fn test_escape_attempts() -> std::result::Result<(), Box<dyn std::error::Error>> {
        let temp_dir = tempfile::tempdir()?;
        let base_path = temp_dir.path();

        // Create a target file outside the sandbox that attackers might want to access
        if let Some(parent) = temp_dir.path().parent() {
            let outside_file = parent.join("outside_target.txt");
            std::fs::write(&outside_file, "sensitive data")?;
        }

        let manager = SandboxedManager::builder()
            .base_directory(base_path)
            .cleanup_policy(CleanupPolicy::disabled())
            .build()
            .await?;

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
            assert!(result.is_err(), "Should reject escape attempt: {attempt}");
        }

        for allowed in allowed_traversal {
            let result = manager.write(allowed, "content").await;
            assert!(
                result.is_ok(),
                "Should allow path that resolves within sandbox: {allowed}",
            );
        }
        Ok(())
    }
}
