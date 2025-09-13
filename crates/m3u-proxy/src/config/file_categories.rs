//! Configuration for file categories in M3U Proxy application.

use super::duration_serde;
use sandboxed_file_manager::{CleanupPolicy, TimeMatch};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, path::PathBuf, time::Duration};

/// Configuration for file management with multiple categories specific to M3U Proxy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileManagerConfig {
    /// Base directory for all file storage
    pub base_directory: PathBuf,

    /// Configuration for different file categories
    pub categories: HashMap<String, FileCategoryConfig>,
}

/// Configuration for a specific file category.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileCategoryConfig {
    /// Subdirectory within base_directory for this category
    pub subdirectory: String,

    /// How long to keep files before they're eligible for cleanup
    /// Can be specified as seconds (number) or human-readable string (e.g., "3months", "5m", "1h30m")
    #[serde(with = "duration_serde::duration")]
    pub retention_duration: Duration,

    /// Which timestamp to use for cleanup decisions
    #[serde(default)]
    pub time_match: TimeMatch,

    /// Whether cleanup is enabled for this category
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Optional override for cleanup interval (if not specified, uses recommended)
    /// Can be specified as seconds (number) or human-readable string (e.g., "5m", "1h")
    #[serde(default, with = "duration_serde::option_duration")]
    pub cleanup_interval: Option<Duration>,
}

impl FileManagerConfig {
    /// Create a new configuration with sensible defaults for M3U Proxy categories.
    pub fn with_defaults(base_directory: PathBuf) -> Self {
        let mut categories = HashMap::new();

        // Temp files - short retention for temporary files (replaces preview)
        categories.insert(
            "temp".to_string(),
            FileCategoryConfig {
                subdirectory: "temp".to_string(),
                retention_duration: humantime::parse_duration("5m").unwrap(), // 5 minutes
                time_match: TimeMatch::LastAccess,
                enabled: true,
                cleanup_interval: None, // Use recommended (every minute)
            },
        );

        // Proxy output - medium retention for generated proxy files
        categories.insert(
            "proxy_output".to_string(),
            FileCategoryConfig {
                subdirectory: "proxy-output".to_string(),
                retention_duration: humantime::parse_duration("30days").unwrap(), // 30 days
                time_match: TimeMatch::LastAccess,
                enabled: true,
                cleanup_interval: None, // Use recommended (every 12 hours)
            },
        );

        Self {
            base_directory,
            categories,
        }
    }

    /// Get configuration for a specific category.
    pub fn get_category(&self, category: &str) -> Option<&FileCategoryConfig> {
        self.categories.get(category)
    }

    /// Get the full path for a category's storage directory.
    pub fn category_path(&self, category: &str) -> Option<PathBuf> {
        self.get_category(category)
            .map(|config| self.base_directory.join(&config.subdirectory))
    }

    /// Convert a category config to a CleanupPolicy.
    pub fn cleanup_policy_for_category(&self, category: &str) -> Option<CleanupPolicy> {
        self.get_category(category).map(|config| {
            if !config.enabled {
                // Use infinite retention for disabled cleanup
                CleanupPolicy::infinite_retention().time_match(config.time_match)
            } else {
                CleanupPolicy::new()
                    .remove_after(config.retention_duration)
                    .time_match(config.time_match)
                    .enabled(config.enabled)
            }
        })
    }

    /// Get the cleanup interval for a category (uses recommended if not specified).
    pub fn cleanup_interval_for_category(&self, category: &str) -> Option<Duration> {
        self.get_category(category).map(|config| {
            config.cleanup_interval.unwrap_or_else(|| {
                let policy = CleanupPolicy::new()
                    .remove_after(config.retention_duration)
                    .time_match(config.time_match)
                    .enabled(config.enabled);
                policy.recommended_cleanup_interval()
            })
        })
    }

    /// Add or update a category configuration.
    pub fn set_category(&mut self, name: String, config: FileCategoryConfig) {
        self.categories.insert(name, config);
    }

    /// List all configured categories.
    pub fn category_names(&self) -> Vec<&String> {
        self.categories.keys().collect()
    }

    /// Create a sandboxed manager for logo storage using direct paths from storage config
    pub async fn create_logo_manager(
        path: &std::path::Path,
        infinite_retention: bool,
    ) -> Result<sandboxed_file_manager::SandboxedManager, sandboxed_file_manager::SandboxedFileError>
    {
        use sandboxed_file_manager::{CleanupPolicy, SandboxedManager, TimeMatch};

        let policy = if infinite_retention {
            CleanupPolicy::infinite_retention()
        } else {
            CleanupPolicy::new()
                .remove_after(humantime::parse_duration("3months").unwrap())
                .time_match(TimeMatch::LastAccess)
                .enabled(true)
        };

        let mut builder = SandboxedManager::builder()
            .base_directory(path)
            .cleanup_policy(policy);

        if !infinite_retention {
            builder = builder.cleanup_interval(std::time::Duration::from_secs(43200)); // 12 hours
        }

        builder.build().await
    }
}

impl FileCategoryConfig {
    /// Create a new category configuration.
    pub fn new(subdirectory: String, retention_duration: Duration) -> Self {
        Self {
            subdirectory,
            retention_duration,
            time_match: TimeMatch::LastAccess,
            enabled: true,
            cleanup_interval: None,
        }
    }

    /// Set the time match strategy.
    pub fn time_match(mut self, time_match: TimeMatch) -> Self {
        self.time_match = time_match;
        self
    }

    /// Enable or disable cleanup for this category.
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Set a custom cleanup interval (overrides recommended).
    pub fn cleanup_interval(mut self, interval: Duration) -> Self {
        self.cleanup_interval = Some(interval);
        self
    }
}

fn default_true() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_default_config() {
        let config = FileManagerConfig::with_defaults("/tmp/test".into());

        // Check that default categories exist
        assert!(config.get_category("temp").is_some());
        assert!(config.get_category("proxy_output").is_some());

        // Check that the config has exactly the expected categories
        assert_eq!(config.categories.len(), 2);

        // Check temp category has short retention
        let temp = config.get_category("temp").unwrap();
        assert_eq!(temp.retention_duration, Duration::from_secs(5 * 60));
        assert_eq!(temp.time_match, TimeMatch::LastAccess);
        assert!(temp.enabled);
    }

    #[test]
    fn test_cleanup_frequency_precision() {
        let config = FileManagerConfig::with_defaults("/tmp/test".into());

        // Temp files (5 min retention) should check every minute
        // This gives max 1 minute drift (5 min + 1 min = 6 min max)
        let temp_interval = config.cleanup_interval_for_category("temp").unwrap();
        assert_eq!(temp_interval, Duration::from_secs(60));

        // Proxy output (30 day retention) cleanup interval has been updated
        let proxy_interval = config
            .cleanup_interval_for_category("proxy_output")
            .unwrap();
        assert_eq!(proxy_interval, Duration::from_secs(14400)); // 4 hours
    }
}
