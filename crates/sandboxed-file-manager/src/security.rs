//! Security utilities for path validation and sandboxing.

use crate::error::{Result, SandboxedFileError};
use std::path::Path;

/// Sets secure permissions on a directory (Unix only).
pub async fn set_secure_permissions(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o700);
        tokio::fs::set_permissions(path, perms)
            .await
            .map_err(|_e| SandboxedFileError::Permission {
                operation: "set secure permissions".to_string(),
                path: path.to_path_buf(),
            })?;
    }

    #[cfg(not(unix))]
    {
        // On non-Unix systems, we can't set specific permissions
        // but we can still validate the directory exists
        if !path.exists() {
            return Err(SandboxedFileError::PathValidation {
                path: path.to_path_buf(),
                reason: "Directory does not exist".to_string(),
            });
        }
    }

    Ok(())
}

/// Validates that a file path doesn't contain null bytes or other basic issues.
pub fn validate_file_path_security(path: &Path) -> Result<()> {
    let path_str = path.to_string_lossy();

    // Check for null bytes
    if path_str.contains('\0') {
        return Err(SandboxedFileError::PathValidation {
            path: path.to_path_buf(),
            reason: "Path contains null bytes".to_string(),
        });
    }

    Ok(())
}

/// Validates that a resolved path is within the specified sandbox directory.
/// Uses OS path resolution to handle symlinks, .., ., etc. properly.
pub fn validate_path_within_sandbox(resolved_path: &Path, sandbox_base: &Path) -> Result<()> {
    // Get canonical sandbox base
    let canonical_base =
        sandbox_base
            .canonicalize()
            .map_err(|e| SandboxedFileError::PathValidation {
                path: sandbox_base.to_path_buf(),
                reason: format!("Failed to resolve sandbox base: {e}"),
            })?;

    // Resolve the target path (may or may not exist)
    let canonical_path = if resolved_path.exists() {
        resolved_path
            .canonicalize()
            .map_err(|e| SandboxedFileError::PathValidation {
                path: resolved_path.to_path_buf(),
                reason: format!("Failed to resolve path: {e}"),
            })?
    } else {
        // Path doesn't exist - resolve parent and append filename
        let parent = resolved_path
            .parent()
            .ok_or_else(|| SandboxedFileError::PathValidation {
                path: resolved_path.to_path_buf(),
                reason: "Path has no parent directory".to_string(),
            })?;

        let canonical_parent =
            parent
                .canonicalize()
                .map_err(|e| SandboxedFileError::PathValidation {
                    path: parent.to_path_buf(),
                    reason: format!("Failed to resolve parent: {e}"),
                })?;

        let filename =
            resolved_path
                .file_name()
                .ok_or_else(|| SandboxedFileError::PathValidation {
                    path: resolved_path.to_path_buf(),
                    reason: "Invalid filename".to_string(),
                })?;

        canonical_parent.join(filename)
    };

    // Verify the resolved path is within the sandbox
    if !canonical_path.starts_with(&canonical_base) {
        return Err(SandboxedFileError::PathValidation {
            path: resolved_path.to_path_buf(),
            reason: format!(
                "Path escapes sandbox: resolves to '{}' (outside '{}')",
                canonical_path.display(),
                canonical_base.display()
            ),
        });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_file_path_security() {
        // Valid relative paths
        assert!(validate_file_path_security(Path::new("file.txt")).is_ok());
        assert!(validate_file_path_security(Path::new("subdir/file.txt")).is_ok());

        // Invalid paths with null bytes
        assert!(validate_file_path_security(Path::new("file\0.txt")).is_err());
    }

    #[tokio::test]
    async fn test_validate_path_within_sandbox() {
        let temp_dir = tempfile::tempdir().unwrap();
        let base = temp_dir.path();

        // Create a test file within the sandbox
        let test_file = base.join("test.txt");
        std::fs::write(&test_file, "test content").unwrap();

        // Valid path within sandbox
        assert!(validate_path_within_sandbox(&test_file, base).is_ok());

        // Create a file outside the sandbox
        let outside_temp = tempfile::tempdir().unwrap();
        let outside_file = outside_temp.path().join("outside.txt");
        std::fs::write(&outside_file, "outside content").unwrap();

        // Invalid path outside sandbox
        assert!(validate_path_within_sandbox(&outside_file, base).is_err());
    }
}
