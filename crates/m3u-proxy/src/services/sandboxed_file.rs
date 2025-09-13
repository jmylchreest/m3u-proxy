//! Sandboxed file service integration
//!
//! This module provides the integration with the external sandboxed-file-manager crate.

use anyhow::Result;
use async_trait::async_trait;
use sandboxed_file_manager::SandboxedManager;
use std::path::PathBuf;

// Re-export the trait and types
pub use super::sandboxed_file_trait::{FileInfo, SandboxedFileManager};

/// Adapter to make SandboxedManager implement our SandboxedFileManager trait
pub struct SandboxedManagerAdapter {
    inner: SandboxedManager,
}

impl SandboxedManagerAdapter {
    pub fn new(manager: SandboxedManager) -> Self {
        Self { inner: manager }
    }
}

#[async_trait]
impl SandboxedFileManager for SandboxedManagerAdapter {
    async fn store_file(
        &self,
        category: &str,
        file_id: &str,
        content: &[u8],
        extension: &str,
    ) -> Result<()> {
        let path = format!("{category}/{file_id}.{extension}");
        self.inner
            .write(&path, content)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to write file: {}", e))
    }

    async fn store_linked_file(
        &self,
        category: &str,
        file_id: &str,
        content: &[u8],
        extension: &str,
    ) -> Result<()> {
        let path = format!("{category}/{file_id}_linked.{extension}");
        self.inner
            .write(&path, content)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to write linked file: {}", e))
    }

    async fn read_file(&self, category: &str, file_id: &str, extension: &str) -> Result<Vec<u8>> {
        let path = format!("{category}/{file_id}.{extension}");
        self.inner
            .read(&path)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to read file: {}", e))
    }

    async fn file_exists(&self, category: &str, file_id: &str, extension: &str) -> Result<bool> {
        let path = format!("{category}/{file_id}.{extension}");
        self.inner
            .exists(&path)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to check file existence: {}", e))
    }

    async fn get_file_path(
        &self,
        category: &str,
        file_id: &str,
        extension: &str,
    ) -> Result<Option<PathBuf>> {
        let path = format!("{category}/{file_id}.{extension}");
        if self
            .inner
            .exists(&path)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to check existence: {}", e))?
        {
            let full_path = self
                .inner
                .get_full_path(&path)
                .map_err(|e| anyhow::anyhow!("Failed to get full path: {}", e))?;
            Ok(Some(full_path))
        } else {
            Ok(None)
        }
    }

    async fn delete_file(&self, category: &str, file_id: &str, extension: &str) -> Result<()> {
        let path = format!("{category}/{file_id}.{extension}");
        self.inner
            .remove_file(&path)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to delete file: {}", e))
    }

    async fn list_files(&self, category: &str) -> Result<Vec<FileInfo>> {
        let files = self
            .inner
            .list_files(category)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to list files: {}", e))?;

        let mut result = Vec::new();
        for file_path in files {
            // Extract file_id and extension from the path
            if let Some(file_name) = std::path::Path::new(&file_path)
                .file_name()
                .and_then(|n| n.to_str())
                && let Some((name, ext)) = file_name.rsplit_once('.')
            {
                // Get file metadata if possible
                let now = chrono::Utc::now();
                result.push(FileInfo {
                    file_id: name.to_string(),
                    extension: ext.to_string(),
                    size_bytes: 0, // Would need to get actual metadata
                    created_at: now,
                    last_accessed: now,
                });
            }
        }

        Ok(result)
    }
}
