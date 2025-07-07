//! Trait interface for the external sandboxed-file-manager crate
//!
//! This defines the interface we expect from the sandboxed file manager crate,
//! which provides std::io-like operations in a sandboxed environment.

use anyhow::Result;
use async_trait::async_trait;
use std::path::PathBuf;

/// Trait for sandboxed file operations
///
/// This trait defines the interface for the external sandboxed-file-manager crate
/// which provides secure file operations within designated sandbox directories.
#[async_trait]
pub trait SandboxedFileManager: Send + Sync {
    /// Store a file in the sandbox
    ///
    /// # Arguments
    /// * `category` - The category name (e.g., "logo_cached", "preview")
    /// * `file_id` - Unique identifier for the file
    /// * `content` - File content as bytes
    /// * `extension` - File extension (e.g., "png", "jpg")
    async fn store_file(
        &self,
        category: &str,
        file_id: &str,
        content: &[u8],
        extension: &str,
    ) -> Result<()>;
    
    /// Store a linked file (e.g., a converted version with same ID but different extension)
    async fn store_linked_file(
        &self,
        category: &str,
        file_id: &str,
        content: &[u8],
        extension: &str,
    ) -> Result<()>;
    
    /// Read a file from the sandbox
    async fn read_file(
        &self,
        category: &str,
        file_id: &str,
        extension: &str,
    ) -> Result<Option<Vec<u8>>>;
    
    /// Check if a file exists
    async fn file_exists(
        &self,
        category: &str,
        file_id: &str,
        extension: &str,
    ) -> Result<bool>;
    
    /// Get the full path to a file (for serving)
    async fn get_file_path(
        &self,
        category: &str,
        file_id: &str,
        extension: &str,
    ) -> Result<Option<PathBuf>>;
    
    /// Delete a file
    async fn delete_file(
        &self,
        category: &str,
        file_id: &str,
        extension: &str,
    ) -> Result<()>;
    
    /// List all files in a category
    async fn list_files(
        &self,
        category: &str,
    ) -> Result<Vec<FileInfo>>;
}

/// Information about a file in the sandbox
#[derive(Debug, Clone)]
pub struct FileInfo {
    pub file_id: String,
    pub extension: String,
    pub size_bytes: u64,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub last_accessed: chrono::DateTime<chrono::Utc>,
}