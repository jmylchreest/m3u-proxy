//! # Sandboxed File Manager
//!
//! A secure, sandboxed file management library with configurable retention policies
//! and magic number-based file type detection.
//!
//! This library provides safe file operations within a sandboxed directory structure
//! with automatic cleanup based on configurable retention policies using file timestamps.
//! It includes robust security features to prevent path traversal attacks, supports
//! nested directory structures, handles symlinks safely, and provides file type validation
//! using magic number detection.
//!
//! ## Features
//!
//! - **Sandboxed Operations**: All file operations are restricted to a base directory
//! - **Nested Path Support**: Full support for subdirectories (e.g., `config/app/settings.json`)
//! - **Path Canonicalization**: Resolves `../`, symlinks, and relative paths safely
//! - **Path Validation**: Protection against directory traversal attacks
//! - **Magic Number Detection**: File type validation using file signatures (via `infer` crate)
//! - **Configurable File Type Restrictions**: Allow only specific MIME types
//! - **Custom File Type Matchers**: Support for custom file type detection
//! - **Configurable Retention**: File cleanup based on atime, mtime, or ctime
//! - **Automatic Cleanup**: Background cleanup with configurable intervals
//! - **Security First**: Symlink validation and path sanitization
//!
//! ## Basic Usage
//!
//! ```rust
//! use sandboxed_file_manager::{SandboxedManager, CleanupPolicy, TimeMatch};
//! use std::time::Duration;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let manager = SandboxedManager::builder()
//!     .base_directory("/app/cache")
//!     .cleanup_policy(
//!         CleanupPolicy::new()
//!             .remove_after(Duration::from_secs(7 * 24 * 60 * 60)) // 7 days
//!             .time_match(TimeMatch::LastAccess)
//!     )
//!     .cleanup_interval(Duration::from_secs(60 * 60)) // 1 hour
//!     .build()
//!     .await?;
//!
//! // Write file (like std::fs::write)
//! manager.write("docs/hello.txt", "Hello World").await?;
//!
//! // Read file (like std::fs::read_to_string)
//! let content = manager.read_to_string("docs/hello.txt").await?;
//! # Ok(())
//! # }
//! ```
//!
//! ## Nested Directory Support
//!
//! ```rust
//! use sandboxed_file_manager::{SandboxedManager, CleanupPolicy};
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let manager = SandboxedManager::builder()
//!     .base_directory("/var/cache/myapp")
//!     .build()
//!     .await?;
//!
//! // Write files in nested directories (like std::fs::write)
//! manager.write("config/app/settings.json", r#"{"debug": true}"#).await?;
//! let image_bytes = b"fake image data";
//! manager.write("assets/images/logo.png", image_bytes).await?;
//! manager.write("data/cache/temp.txt", "temporary data").await?;
//!
//! // Create and open files (like std::fs::File operations)
//! let mut file = manager.create("logs/app.log").await?;
//! let existing_file = manager.open("config/app/settings.json").await?;
//!
//! // Directory operations
//! manager.create_dir_all("nested/deep/structure").await?;
//! # Ok(())
//! # }
//! ```
//!
//! ## Standard File Operations
//!
//! ```rust
//! use sandboxed_file_manager::SandboxedManager;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let manager = SandboxedManager::builder()
//!     .base_directory("/var/cache/myapp")
//!     .build()
//!     .await?;
//!
//! // Standard Rust file operations, but sandboxed
//! manager.write("file.txt", "content").await?;           // std::fs::write
//! let content = manager.read_to_string("file.txt").await?; // std::fs::read_to_string
//! let bytes = manager.read("file.txt").await?;             // std::fs::read
//! let metadata = manager.metadata("file.txt").await?;      // std::fs::metadata
//! manager.copy("file.txt", "backup.txt").await?;          // std::fs::copy
//! manager.remove_file("file.txt").await?;                 // std::fs::remove_file
//! # Ok(())
//! # }
//! ```
//!
//! ## File Type Validation
//!
//! ```rust
//! use sandboxed_file_manager::{SandboxedManager, file_types::{FileTypeValidator, FileTypeConfigBuilder}};
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Create a validator that only allows images and JSON
//! let validator = FileTypeValidator::with_config(
//!     FileTypeConfigBuilder::new()
//!         .allow_mime_type("image/png")
//!         .allow_mime_type("image/jpeg")
//!         .allow_mime_type("application/json")
//!         .build()
//! );
//!
//! let manager = SandboxedManager::builder()
//!     .base_directory("/var/cache/myapp")
//!     .build()
//!     .await?;
//!
//! // Write file then validate its type
//! let jpeg_bytes = b"fake jpeg data";
//! manager.write("image.jpg", jpeg_bytes).await?;
//! let file_info = manager.validate_file_type("image.jpg", &validator).await?;
//! println!("Detected: {} ({})", file_info.mime_type, file_info.extension);
//! # Ok(())
//! # }
//! ```
//!
//! ## Real-World Usage Example
//!
//! ```rust
//! use sandboxed_file_manager::{SandboxedManager, CleanupPolicy, TimeMatch};
//! use std::time::Duration;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Set up manager with cleanup policy
//! let manager = SandboxedManager::builder()
//!     .base_directory("/var/cache/myapp")
//!     .cleanup_policy(
//!         CleanupPolicy::new()
//!             .remove_after(Duration::from_secs(3600)) // 1 hour
//!             .time_match(TimeMatch::LastAccess)
//!             .enabled(true)
//!     )
//!     .build()
//!     .await?;
//!
//! // Use like standard file operations
//! manager.create_dir_all("configs/app").await?;
//! manager.write("configs/app/settings.json", r#"{"port": 8080}"#).await?;
//!
//! let config = manager.read_to_string("configs/app/settings.json").await?;
//! println!("Config: {}", config);
//! # Ok(())
//! # }
//! ```
//!
//! ## Security Features
//!
//! - **Path Canonicalization**: Resolves `../`, `.`, symlinks and relative paths before validation
//! - **Sandbox Enforcement**: All resolved paths must be within the base directory
//! - **Path Traversal Prevention**: Blocks attempts to escape sandbox via `../` sequences
//! - **Symlink Safety**: Allows symlinks that resolve within sandbox, blocks external ones
//! - **Magic Number Detection**: Files validated by content, not just extensions
//! - **Nested Directory Support**: Full support for subdirectories with security validation
//! - **Filename Preservation**: Maintains original filenames and directory structure
//! - **Secure Permissions**: Automatic secure directory permissions (Unix)
//!
//! ## Path Resolution Examples
//!
//! Given sandbox base: `/var/cache/myapp`
//!
//! **✅ Allowed operations:**
//! ```rust
//! use sandboxed_file_manager::SandboxedManager;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! # let manager = SandboxedManager::builder().base_directory("/var/cache/myapp").build().await?;
//! manager.write("file.txt", "content").await?;                    // Simple file
//! manager.write("config/app.json", "{}").await?;                 // Nested path
//! manager.write("dir/../other/file.txt", "content").await?;      // Resolves to other/file.txt
//! manager.copy("source.txt", "backup/copy.txt").await?;          // Copy with nesting
//! # Ok(())
//! # }
//! ```
//!
//! **❌ Blocked operations:**
//! ```rust,ignore
//! // These operations would fail with security errors:
//! // manager.write("../../../etc/passwd", "evil").await?;           // Escapes sandbox
//! // manager.write("/etc/passwd", "evil").await?;                   // Absolute path  
//! // manager.write("file\0.txt", "evil").await?;                    // Null bytes
//! ```

pub mod error;
pub mod file_types;
pub mod manager;
pub mod policy;
pub mod security;

pub use error::{Result, SandboxedFileError};
pub use file_types::{
    DetectionMethod, FileTypeConfig, FileTypeConfigBuilder, FileTypeInfo, FileTypeValidator,
};
pub use manager::{FileInfo, SandboxedManager, SandboxedManagerBuilder};
pub use policy::{CleanupPolicy, TimeMatch};

// Re-export commonly used types
pub use std::time::Duration;
