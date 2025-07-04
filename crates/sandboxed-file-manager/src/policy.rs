//! Cleanup and retention policies for file management.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Defines which timestamp to use for cleanup decisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TimeMatch {
    /// Use last access time - tries filesystem atime, falls back to in-memory tracking, then mtime
    LastAccess,
    /// Use modification time (mtime) 
    Modified,
    /// Use creation time (ctime)
    Created,
}

impl Default for TimeMatch {
    fn default() -> Self {
        TimeMatch::LastAccess
    }
}

/// Configuration for automatic file cleanup.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleanupPolicy {
    /// How long to keep files before they're eligible for cleanup
    pub retention_duration: Duration,
    /// Which timestamp to use for cleanup decisions
    pub time_match: TimeMatch,
    /// Whether cleanup is enabled
    pub enabled: bool,
}

impl CleanupPolicy {
    /// Create a new cleanup policy with default settings.
    /// 
    /// Default: 24 hours retention based on last access time.
    pub fn new() -> Self {
        Self {
            retention_duration: Duration::from_secs(24 * 60 * 60), // 24 hours
            time_match: TimeMatch::LastAccess,
            enabled: true,
        }
    }

    /// Set the retention duration.
    pub fn remove_after(mut self, duration: Duration) -> Self {
        self.retention_duration = duration;
        self
    }

    /// Set which timestamp to match against.
    pub fn time_match(mut self, time_match: TimeMatch) -> Self {
        self.time_match = time_match;
        self
    }

    /// Enable or disable cleanup.
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Disable cleanup entirely.
    pub fn disabled() -> Self {
        Self {
            retention_duration: Duration::ZERO,
            time_match: TimeMatch::LastAccess,
            enabled: false,
        }
    }
    
    /// Calculate an appropriate cleanup interval based on retention duration.
    /// For precise cleanup, we want to check more frequently for shorter retention periods.
    /// 
    /// Rules:
    /// - For retention < 1 hour: check every 1 minute (max 1 minute drift)
    /// - For retention < 1 day: check every 10 minutes (max 10 minute drift)  
    /// - For retention < 7 days: check every 1 hour (max 1 hour drift)
    /// - For retention < 30 days: check every 4 hours (max 4 hour drift)
    /// - For retention >= 30 days: check every 12 hours (max 12 hour drift)
    pub fn recommended_cleanup_interval(&self) -> Duration {
        let retention_secs = self.retention_duration.as_secs();
        
        match retention_secs {
            0..=3600 => Duration::from_secs(60), // <= 1 hour: check every minute
            3601..=86400 => Duration::from_secs(600), // <= 1 day: check every 10 minutes
            86401..=604800 => Duration::from_secs(3600), // <= 7 days: check every hour
            604801..=2592000 => Duration::from_secs(14400), // <= 30 days: check every 4 hours
            _ => Duration::from_secs(43200), // > 30 days: check every 12 hours
        }
    }

    /// Check if a file should be cleaned up based on this policy.
    /// For LastAccess mode, prefers filesystem_atime if available, falls back to in_memory_access, then modified.
    pub fn should_cleanup(
        &self, 
        filesystem_atime: Option<DateTime<Utc>>,
        in_memory_access: DateTime<Utc>, 
        modified: DateTime<Utc>, 
        created: DateTime<Utc>
    ) -> bool {
        if !self.enabled {
            return false;
        }

        let cutoff = Utc::now() - chrono::Duration::from_std(self.retention_duration).unwrap_or_default();
        
        let timestamp = match self.time_match {
            TimeMatch::LastAccess => {
                let unix_epoch_utc: DateTime<Utc> = DateTime::from(std::time::UNIX_EPOCH);
                
                // Prefer filesystem atime if available and seems valid (not Unix epoch)
                if let Some(fs_atime) = filesystem_atime {
                    if fs_atime > unix_epoch_utc {
                        fs_atime
                    } else {
                        // Filesystem atime not reliable, use in-memory tracking, fallback to mtime
                        if in_memory_access > unix_epoch_utc {
                            in_memory_access
                        } else {
                            modified
                        }
                    }
                } else {
                    // No filesystem atime available, use in-memory tracking, fallback to mtime
                    if in_memory_access > unix_epoch_utc {
                        in_memory_access
                    } else {
                        modified
                    }
                }
            },
            TimeMatch::Modified => modified,
            TimeMatch::Created => created,
        };

        timestamp < cutoff
    }
}

impl Default for CleanupPolicy {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration as ChronoDuration;

    #[test]
    fn test_cleanup_policy_should_cleanup() {
        let policy = CleanupPolicy::new()
            .remove_after(Duration::from_secs(60 * 60)) // 1 hour
            .time_match(TimeMatch::LastAccess);

        let now = Utc::now();
        let old_time = now - ChronoDuration::hours(2); // 2 hours ago
        let recent_time = now - ChronoDuration::minutes(30); // 30 minutes ago

        // Should cleanup old file (using in-memory access time)
        assert!(policy.should_cleanup(None, old_time, now, now));
        
        // Should not cleanup recent file
        assert!(!policy.should_cleanup(None, recent_time, now, now));
        
        // Should prefer filesystem atime if available
        let old_fs_atime = now - ChronoDuration::hours(3);
        assert!(policy.should_cleanup(Some(old_fs_atime), recent_time, now, now));
    }

    #[test]
    fn test_cleanup_policy_disabled() {
        let policy = CleanupPolicy::disabled();
        let old_time = Utc::now() - ChronoDuration::days(365);
        
        // Should never cleanup when disabled
        assert!(!policy.should_cleanup(None, old_time, old_time, old_time));
    }

    #[test]
    fn test_time_match_variants() {
        let policy_atime = CleanupPolicy::new().time_match(TimeMatch::LastAccess);
        let policy_mtime = CleanupPolicy::new().time_match(TimeMatch::Modified);
        let policy_ctime = CleanupPolicy::new().time_match(TimeMatch::Created);

        let now = Utc::now();
        let old_access = now - ChronoDuration::days(2);
        let old_modified = now - ChronoDuration::days(2); 
        let old_created = now - ChronoDuration::days(2);
        let recent = now - ChronoDuration::minutes(1);

        // Test access time matching
        assert!(policy_atime.should_cleanup(None, old_access, recent, recent));
        assert!(!policy_atime.should_cleanup(None, recent, old_modified, old_created));

        // Test modified time matching  
        assert!(policy_mtime.should_cleanup(None, recent, old_modified, recent));
        assert!(!policy_mtime.should_cleanup(None, old_access, recent, old_created));

        // Test created time matching
        assert!(policy_ctime.should_cleanup(None, recent, recent, old_created));
        assert!(!policy_ctime.should_cleanup(None, old_access, old_modified, recent));
    }
}