use m3u_proxy::config::file_categories::{FileManagerConfig, FileCategoryConfig};
use sandboxed_file_manager::TimeMatch;
use std::collections::HashMap;
use tempfile::TempDir;

/// Test duplicate base directory detection with various path variants
#[tokio::test]
async fn test_duplicate_base_directory_detection() {
    let temp_dir = TempDir::new().unwrap();
    let base_path = temp_dir.path();

    // Create subdirectories for testing
    let subdir1 = base_path.join("shared");
    let subdir2 = base_path.join("different");
    tokio::fs::create_dir_all(&subdir1).await.unwrap();
    tokio::fs::create_dir_all(&subdir2).await.unwrap();

    // Test 1: Direct duplicate paths
    {
        let mut categories = HashMap::new();
        
        categories.insert(
            "category1".to_string(),
            FileCategoryConfig {
                subdirectory: "shared".to_string(),
                retention_duration: humantime::parse_duration("5m").unwrap(),
                time_match: TimeMatch::LastAccess,
                enabled: true,
                cleanup_interval: None,
            },
        );
        
        categories.insert(
            "category2".to_string(),
            FileCategoryConfig {
                subdirectory: "shared".to_string(), // Same subdirectory = same final path
                retention_duration: humantime::parse_duration("10m").unwrap(),
                time_match: TimeMatch::LastAccess,
                enabled: true,
                cleanup_interval: None,
            },
        );

        let config = FileManagerConfig {
            base_directory: base_path.to_path_buf(),
            categories,
        };

        let result = validate_no_duplicate_paths(&config);
        assert!(result.is_err(), "Should detect direct duplicate paths");
        let error_message = result.err().unwrap().to_string();
        assert!(error_message.contains("Duplicate base directory"));
        assert!(error_message.contains("category1"));
        assert!(error_message.contains("category2"));
    }

    // Test 2: Path variants that resolve to same location
    {
        let mut categories = HashMap::new();
        
        categories.insert(
            "category1".to_string(),
            FileCategoryConfig {
                subdirectory: "shared".to_string(),
                retention_duration: humantime::parse_duration("5m").unwrap(),
                time_match: TimeMatch::LastAccess,
                enabled: true,
                cleanup_interval: None,
            },
        );
        
        categories.insert(
            "category2".to_string(),
            FileCategoryConfig {
                subdirectory: "different/../shared".to_string(), // Resolves to same as "shared"
                retention_duration: humantime::parse_duration("10m").unwrap(),
                time_match: TimeMatch::LastAccess,
                enabled: true,
                cleanup_interval: None,
            },
        );

        let config = FileManagerConfig {
            base_directory: base_path.to_path_buf(),
            categories,
        };

        let result = validate_no_duplicate_paths(&config);
        assert!(result.is_err(), "Should detect path variants that resolve to same location");
        let error_message = result.err().unwrap().to_string();
        assert!(error_message.contains("Duplicate base directory"));
    }

    // Test 3: Different paths should be allowed
    {
        let mut categories = HashMap::new();
        
        categories.insert(
            "category1".to_string(),
            FileCategoryConfig {
                subdirectory: "shared".to_string(),
                retention_duration: humantime::parse_duration("5m").unwrap(),
                time_match: TimeMatch::LastAccess,
                enabled: true,
                cleanup_interval: None,
            },
        );
        
        categories.insert(
            "category2".to_string(),
            FileCategoryConfig {
                subdirectory: "different".to_string(), // Actually different path
                retention_duration: humantime::parse_duration("10m").unwrap(),
                time_match: TimeMatch::LastAccess,
                enabled: true,
                cleanup_interval: None,
            },
        );

        let config = FileManagerConfig {
            base_directory: base_path.to_path_buf(),
            categories,
        };

        let result = validate_no_duplicate_paths(&config);
        assert!(result.is_ok(), "Should allow different paths");
    }
}

/// Test duplicate detection with logo paths
#[tokio::test]
async fn test_duplicate_with_logo_paths() {
    let temp_dir = TempDir::new().unwrap();
    let base_path = temp_dir.path();

    // Create subdirectories
    let logos_dir = base_path.join("logos");
    let shared_dir = base_path.join("shared");
    tokio::fs::create_dir_all(&logos_dir).await.unwrap();
    tokio::fs::create_dir_all(&shared_dir).await.unwrap();

    let mut categories = HashMap::new();
    categories.insert(
        "temp".to_string(),
        FileCategoryConfig {
            subdirectory: "shared".to_string(),
            retention_duration: humantime::parse_duration("5m").unwrap(),
            time_match: TimeMatch::LastAccess,
            enabled: true,
            cleanup_interval: None,
        },
    );

    let config = FileManagerConfig {
        base_directory: base_path.to_path_buf(),
        categories,
    };

    // Test that logo path conflicts are detected
    let shared_path = base_path.join("shared");
    let result = validate_paths_including_logos(&config, &shared_path);
    assert!(result.is_err(), "Should detect conflict between file manager category and logo path");
    
    // Test that different logo path is allowed
    let result = validate_paths_including_logos(&config, &logos_dir);
    assert!(result.is_ok(), "Should allow different logo path");
}

/// Test absolute vs relative path variants
#[tokio::test]
async fn test_absolute_vs_relative_paths() {
    let temp_dir = TempDir::new().unwrap();
    let base_path = temp_dir.path();
    
    // Create subdirectory
    let subdir = base_path.join("test");
    tokio::fs::create_dir_all(&subdir).await.unwrap();

    let mut categories = HashMap::new();
    
    // Use relative path
    categories.insert(
        "category1".to_string(),
        FileCategoryConfig {
            subdirectory: "test".to_string(),
            retention_duration: humantime::parse_duration("5m").unwrap(),
            time_match: TimeMatch::LastAccess,
            enabled: true,
            cleanup_interval: None,
        },
    );

    let config = FileManagerConfig {
        base_directory: base_path.to_path_buf(),
        categories,
    };

    // Create another config with absolute path to same location
    let absolute_path = subdir.canonicalize().unwrap();
    let result = validate_paths_including_logos(&config, &absolute_path);
    assert!(result.is_err(), "Should detect absolute vs relative path to same location");
}

/// Helper function to validate file manager paths (extracted from main.rs logic)
fn validate_no_duplicate_paths(config: &FileManagerConfig) -> Result<(), anyhow::Error> {
    let mut all_paths = std::collections::HashMap::new();
    
    // Check file manager config categories
    for category in config.category_names() {
        if let Some(path) = config.category_path(category) {
            let canonical_path = path.canonicalize().unwrap_or(path.clone());
            if let Some(existing_category) = all_paths.insert(canonical_path.clone(), category.clone()) {
                return Err(anyhow::anyhow!(
                    "Duplicate base directory detected! Categories '{}' and '{}' both use path: {:?}. This would cause cleanup conflicts.",
                    existing_category, category, canonical_path
                ));
            }
        }
    }
    
    Ok(())
}

/// Helper function to validate paths including logos
fn validate_paths_including_logos(config: &FileManagerConfig, logo_path: &std::path::Path) -> Result<(), anyhow::Error> {
    let mut all_paths = std::collections::HashMap::new();
    
    // Check file manager config categories
    for category in config.category_names() {
        if let Some(path) = config.category_path(category) {
            let canonical_path = path.canonicalize().unwrap_or(path.clone());
            if let Some(existing_category) = all_paths.insert(canonical_path.clone(), category.clone()) {
                return Err(anyhow::anyhow!(
                    "Duplicate base directory detected! Categories '{}' and '{}' both use path: {:?}",
                    existing_category, category, canonical_path
                ));
            }
        }
    }
    
    // Check logo path
    let logo_canonical = logo_path.canonicalize().unwrap_or(logo_path.to_path_buf());
    if let Some(existing_category) = all_paths.get(&logo_canonical) {
        return Err(anyhow::anyhow!(
            "Duplicate base directory detected! Category '{}' and logos both use path: {:?}",
            existing_category, logo_canonical
        ));
    }
    
    Ok(())
}

/// Test edge cases for duplicate detection
#[tokio::test]
async fn test_edge_cases_duplicate_detection() {
    let temp_dir = TempDir::new().unwrap();
    let base_path = temp_dir.path();

    // Create nested directories for testing
    let nested = base_path.join("level1").join("level2");
    let other_dir = base_path.join("level1").join("other");
    tokio::fs::create_dir_all(&nested).await.unwrap();
    tokio::fs::create_dir_all(&other_dir).await.unwrap();

    // Test case: Complex path resolution should be detected
    {
        let mut categories = HashMap::new();
        
        categories.insert(
            "category1".to_string(),
            FileCategoryConfig {
                subdirectory: "level1/level2".to_string(),
                retention_duration: humantime::parse_duration("5m").unwrap(),
                time_match: TimeMatch::LastAccess,
                enabled: true,
                cleanup_interval: None,
            },
        );
        
        categories.insert(
            "category2".to_string(),
            FileCategoryConfig {
                subdirectory: "level1/other/../level2".to_string(), // Complex path resolution
                retention_duration: humantime::parse_duration("10m").unwrap(),
                time_match: TimeMatch::LastAccess,
                enabled: true,
                cleanup_interval: None,
            },
        );

        let config = FileManagerConfig {
            base_directory: base_path.to_path_buf(),
            categories,
        };

        let result = validate_no_duplicate_paths(&config);
        assert!(result.is_err(), "Should detect duplicate paths with complex resolution");
    }
}

/// Test that symlinks are properly resolved for duplicate detection
#[tokio::test]
#[cfg(unix)] // Symlinks work differently on Windows
async fn test_symlink_duplicate_detection() {
    let temp_dir = TempDir::new().unwrap();
    let base_path = temp_dir.path();

    // Create target directory and symlink
    let target_dir = base_path.join("target");
    let symlink_dir = base_path.join("symlink");
    tokio::fs::create_dir_all(&target_dir).await.unwrap();
    
    // Create symlink pointing to target
    std::os::unix::fs::symlink(&target_dir, &symlink_dir).unwrap();

    let mut categories = HashMap::new();
    
    categories.insert(
        "category1".to_string(),
        FileCategoryConfig {
            subdirectory: "target".to_string(),
            retention_duration: humantime::parse_duration("5m").unwrap(),
            time_match: TimeMatch::LastAccess,
            enabled: true,
            cleanup_interval: None,
        },
    );
    
    categories.insert(
        "category2".to_string(),
        FileCategoryConfig {
            subdirectory: "symlink".to_string(), // Symlink to same target
            retention_duration: humantime::parse_duration("10m").unwrap(),
            time_match: TimeMatch::LastAccess,
            enabled: true,
            cleanup_interval: None,
        },
    );

    let config = FileManagerConfig {
        base_directory: base_path.to_path_buf(),
        categories,
    };

    let result = validate_no_duplicate_paths(&config);
    assert!(result.is_err(), "Should detect duplicate paths through symlinks");
    let error_message = result.err().unwrap().to_string();
    assert!(error_message.contains("Duplicate base directory"));
}