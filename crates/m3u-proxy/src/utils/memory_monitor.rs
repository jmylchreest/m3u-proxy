//! Simple passive memory monitor for proxy generation
//!
//! This module provides a lightweight memory monitoring utility that:
//! 1. Uses system memory tracking (/proc/self/status on Linux)
//! 2. Tracks memory usage at different stages passively
//! 3. Enforces memory limits when configured

use crate::utils::memory_config::{MemoryMonitoringConfig, get_global_memory_config};
use anyhow::Result;
use std::collections::HashMap;
use std::time::Instant;
use tracing::{debug, warn};

/// Simple memory monitor for tracking generation stages
#[derive(Debug)]
pub struct SimpleMemoryMonitor {
    baseline_rss: u64,
    peak_rss: u64,
    stage_memories: HashMap<String, u64>,
    pub memory_limit_mb: Option<usize>,
    enabled: bool,
    last_warning_time: Option<Instant>,
    config: MemoryMonitoringConfig,
}

/// Memory usage snapshot
#[derive(Debug, Clone)]
pub struct MemorySnapshot {
    pub stage: String,
    pub rss_mb: f64,
    pub delta_mb: f64,
    pub timestamp: Instant,
}

/// Memory limit status for decision making
#[derive(Debug, Clone, PartialEq)]
pub enum MemoryLimitStatus {
    /// Memory usage is within normal limits
    Ok,
    /// Memory usage is approaching the limit (warning threshold reached)
    Warning,
    /// Memory limit has been exceeded
    Exceeded,
}

/// Final memory statistics
#[derive(Debug)]
pub struct MemoryStats {
    pub baseline_mb: f64,
    pub peak_mb: f64,
    pub stages: HashMap<String, MemorySnapshot>,
    pub within_limits: bool,
}

impl SimpleMemoryMonitor {
    /// Create a new memory monitor
    pub fn new(memory_limit_mb: Option<usize>) -> Self {
        let enabled = Self::is_system_tracking_available();

        if !enabled {
            warn!("System memory tracking not available on this platform");
        }

        let mut config = get_global_memory_config();
        if let Some(limit) = memory_limit_mb {
            config.memory_limit_mb = Some(limit);
        }

        Self {
            baseline_rss: 0,
            peak_rss: 0,
            stage_memories: HashMap::new(),
            memory_limit_mb,
            enabled,
            last_warning_time: None,
            config,
        }
    }

    /// Create a new memory monitor with custom configuration
    pub fn new_with_config(memory_limit_mb: Option<usize>, config: MemoryMonitoringConfig) -> Self {
        let enabled = Self::is_system_tracking_available();

        if !enabled {
            warn!("System memory tracking not available on this platform");
        }

        let mut final_config = config;
        if let Some(limit) = memory_limit_mb {
            final_config.memory_limit_mb = Some(limit);
        }

        Self {
            baseline_rss: 0,
            peak_rss: 0,
            stage_memories: HashMap::new(),
            memory_limit_mb,
            enabled,
            last_warning_time: None,
            config: final_config,
        }
    }

    /// Initialize baseline memory reading
    pub fn initialize(&mut self) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }

        let rss = self.get_current_rss()?;
        self.baseline_rss = rss;
        self.peak_rss = rss;

        debug!(
            "Memory monitor initialized: baseline {:.1}MB",
            rss as f64 / 1024.0 / 1024.0
        );
        Ok(())
    }

    /// Record memory usage for a stage (passive observation)
    pub fn observe_stage(&mut self, stage_name: &str) -> Result<MemorySnapshot> {
        if !self.enabled {
            return Ok(MemorySnapshot {
                stage: stage_name.to_string(),
                rss_mb: 0.0,
                delta_mb: 0.0,
                timestamp: Instant::now(),
            });
        }

        let current_rss = self.get_current_rss()?;

        // Update peak
        if current_rss > self.peak_rss {
            self.peak_rss = current_rss;
        }

        let rss_mb = current_rss as f64 / 1024.0 / 1024.0;
        let delta_mb = (current_rss as i64 - self.baseline_rss as i64) as f64 / 1024.0 / 1024.0;

        let snapshot = MemorySnapshot {
            stage: stage_name.to_string(),
            rss_mb,
            delta_mb,
            timestamp: Instant::now(),
        };

        self.stage_memories
            .insert(stage_name.to_string(), current_rss);

        // Check for memory warnings
        self.check_memory_warning(&snapshot);

        if self.config.should_log_stage_observations() {
            debug!(
                "Memory observation for '{}': {:.1}MB (Δ{:+.1}MB)",
                stage_name, rss_mb, delta_mb
            );
        }

        Ok(snapshot)
    }

    /// Check if current memory usage exceeds limits and suggest actions
    pub fn check_memory_limit(&self) -> Result<MemoryLimitStatus> {
        if !self.enabled {
            return Ok(MemoryLimitStatus::Ok);
        }

        if let Some(limit_mb) = self.memory_limit_mb {
            let current_rss = self.get_current_rss()?;
            let current_mb = current_rss as f64 / 1024.0 / 1024.0;
            let usage_ratio = current_mb / limit_mb as f64;

            if usage_ratio >= 1.0 {
                if self.config.should_log_memory_warnings() {
                    warn!(
                        "Memory limit exceeded: {:.1}MB > {}MB",
                        current_mb, limit_mb
                    );
                }
                return Ok(MemoryLimitStatus::Exceeded);
            } else if usage_ratio >= self.config.warning_threshold {
                // Only log warning if it's been a while since last warning
                let now = Instant::now();
                let should_warn = self
                    .last_warning_time
                    .map(|last| {
                        now.duration_since(last).as_secs() >= self.config.warning_cooldown_seconds
                    })
                    .unwrap_or(true);

                if should_warn && self.config.should_log_memory_warnings() {
                    warn!(
                        "Memory usage near limit: {:.1}% ({:.1}MB / {}MB)",
                        usage_ratio * 100.0,
                        current_mb,
                        limit_mb
                    );
                }
                return Ok(MemoryLimitStatus::Warning);
            }
        }
        Ok(MemoryLimitStatus::Ok)
    }

    /// Get final memory statistics
    pub fn get_statistics(&self) -> MemoryStats {
        let baseline_mb = self.baseline_rss as f64 / 1024.0 / 1024.0;
        let peak_mb = self.peak_rss as f64 / 1024.0 / 1024.0;

        let stages = self
            .stage_memories
            .iter()
            .map(|(stage, rss)| {
                let rss_mb = *rss as f64 / 1024.0 / 1024.0;
                let delta_mb = (*rss as i64 - self.baseline_rss as i64) as f64 / 1024.0 / 1024.0;

                (
                    stage.clone(),
                    MemorySnapshot {
                        stage: stage.clone(),
                        rss_mb,
                        delta_mb,
                        timestamp: Instant::now(), // Not exact but close enough for final stats
                    },
                )
            })
            .collect();

        let within_limits = self
            .memory_limit_mb
            .map(|limit| peak_mb <= limit as f64)
            .unwrap_or(true);

        MemoryStats {
            baseline_mb,
            peak_mb,
            stages,
            within_limits,
        }
    }

    /// Get current RSS memory from system
    pub fn get_current_rss(&self) -> Result<u64> {
        #[cfg(target_os = "linux")]
        {
            let status = std::fs::read_to_string("/proc/self/status")
                .map_err(|e| anyhow::anyhow!("Failed to read /proc/self/status: {}", e))?;

            for line in status.lines() {
                if line.starts_with("VmRSS:") {
                    if let Some(kb_str) = line.split_whitespace().nth(1) {
                        let kb = kb_str.parse::<u64>().unwrap_or(0);
                        return Ok(kb * 1024); // Convert KB to bytes
                    }
                }
            }

            Ok(0)
        }

        #[cfg(not(target_os = "linux"))]
        {
            Ok(0) // Could implement for other platforms
        }
    }

    /// Check if system memory tracking is available
    fn is_system_tracking_available() -> bool {
        #[cfg(target_os = "linux")]
        {
            std::fs::read_to_string("/proc/self/status").is_ok()
        }

        #[cfg(not(target_os = "linux"))]
        {
            false
        }
    }

    /// Check for memory warnings with cooldown to reduce spam
    fn check_memory_warning(&mut self, snapshot: &MemorySnapshot) {
        // Update config from global in case it changed
        self.config = get_global_memory_config();

        if let Some(limit_mb) = self.memory_limit_mb {
            let usage_ratio = snapshot.rss_mb / limit_mb as f64;

            if usage_ratio >= self.config.warning_threshold {
                let now = Instant::now();
                let should_warn = self
                    .last_warning_time
                    .map(|last| {
                        now.duration_since(last).as_secs() >= self.config.warning_cooldown_seconds
                    })
                    .unwrap_or(true);

                if should_warn && self.config.should_log_memory_warnings() {
                    warn!(
                        "Memory usage high: {:.1}% of limit ({:.1}MB / {}MB) - Stage: {}",
                        usage_ratio * 100.0,
                        snapshot.rss_mb,
                        limit_mb,
                        snapshot.stage
                    );
                    self.last_warning_time = Some(now);
                }
            }
        }
    }
}

impl MemoryLimitStatus {
    /// Get suggested actions based on memory status
    pub fn get_suggestions(&self) -> &'static str {
        match self {
            MemoryLimitStatus::Ok => "Memory usage is normal",
            MemoryLimitStatus::Warning => {
                "Consider: reducing batch sizes, processing in chunks, or freeing unused data"
            }
            MemoryLimitStatus::Exceeded => {
                "Immediate action required: stop current processing, free memory, or increase limits"
            }
        }
    }

    /// Check if processing should continue
    pub fn should_continue(&self) -> bool {
        match self {
            MemoryLimitStatus::Ok | MemoryLimitStatus::Warning => true,
            MemoryLimitStatus::Exceeded => false,
        }
    }
}

impl MemoryStats {
    /// Get a summary string of memory usage
    pub fn summary(&self) -> String {
        format!(
            "Memory: {:.1}MB baseline → {:.1}MB peak (Δ{:+.1}MB), {} stages tracked, within_limits: {}",
            self.baseline_mb,
            self.peak_mb,
            self.peak_mb - self.baseline_mb,
            self.stages.len(),
            self.within_limits
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_monitor_creation() {
        let monitor = SimpleMemoryMonitor::new(Some(512));
        assert_eq!(monitor.memory_limit_mb, Some(512));
    }

    #[test]
    fn test_memory_stats_summary() {
        let stats = MemoryStats {
            baseline_mb: 100.0,
            peak_mb: 150.0,
            stages: HashMap::new(),
            within_limits: true,
        };

        let summary = stats.summary();
        assert!(summary.contains("100.0MB"));
        assert!(summary.contains("150.0MB"));
        assert!(summary.contains("within_limits: true"));
    }
}
