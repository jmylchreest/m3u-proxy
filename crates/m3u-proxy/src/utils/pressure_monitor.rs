//! Cross-platform memory monitor for proxy generation
//!
//! This module provides a lightweight memory monitoring utility that:
//! 1. Uses sysinfo for cross-platform memory tracking
//! 2. Tracks memory usage at different stages passively
//! 3. Enforces memory limits when configured
//! 4. Provides unified pressure assessment for app/system/CPU

// use crate::utils::memory_pressure_calculator::MemoryPressureLevel; // Removed

// Simplified replacement for MemoryPressureLevel
#[derive(Debug, Clone, PartialEq)]
pub enum MemoryPressureLevel {
    Low,
    Medium,
    High,
    Optimal,
    Moderate,
    Critical,
    Emergency,
}
use crate::utils::human_format::{format_memory, format_memory_delta};
use crate::utils::memory_config::{MemoryMonitoringConfig, get_global_memory_config};
use anyhow::Result;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use sysinfo::{CpuExt, Pid, PidExt, ProcessExt, System, SystemExt};
use tracing::{debug, warn};

/// Cross-platform memory monitor for tracking generation stages
#[derive(Debug, Clone)]
pub struct SimpleMemoryMonitor {
    baseline_rss: u64,
    peak_rss: u64,
    stage_memories: HashMap<String, u64>,
    pub memory_limit_mb: Option<usize>,
    enabled: bool,
    last_warning_time: Option<Instant>,
    config: MemoryMonitoringConfig,
    /// Shared system instance for monitoring
    system: Arc<tokio::sync::RwLock<System>>,
    /// Process ID for self-monitoring
    current_pid: Pid,
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

/// Unified system pressure assessment
#[derive(Debug, Clone)]
pub struct SystemPressureAssessment {
    /// Memory pressure based on configured application limits
    pub memory_pressure: MemoryPressureLevel,
    /// Memory pressure based on system-wide memory availability
    pub memory_pressure_system: MemoryPressureLevel,
    /// CPU pressure based on system-wide CPU usage
    pub cpu_pressure_system: MemoryPressureLevel,
    /// Raw metrics used for assessment
    pub metrics: SystemPressureMetrics,
}

/// Raw system metrics for pressure assessment
#[derive(Debug, Clone)]
pub struct SystemPressureMetrics {
    /// Application memory usage in MB
    pub app_memory_mb: f64,
    /// System memory usage percentage (0-100)
    pub system_memory_percent: f64,
    /// System CPU usage percentage (0-100)
    pub system_cpu_percent: f64,
    /// System load average (1-minute)
    pub system_load_avg: f64,
    /// Total system memory in MB
    pub total_system_memory_mb: f64,
    /// Available system memory in MB
    pub available_system_memory_mb: f64,
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
    /// Create a new memory monitor with shared system instance
    pub fn new(
        memory_limit_mb: Option<usize>,
        config: MemoryMonitoringConfig,
        system: Arc<tokio::sync::RwLock<System>>,
    ) -> Self {
        let current_pid = Pid::from_u32(std::process::id());
        let enabled = true;

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
            system,
            current_pid,
        }
    }

    /// Initialize baseline memory reading
    pub async fn initialize(&mut self) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }

        let rss = self.get_current_rss().await?;
        self.baseline_rss = rss;
        self.peak_rss = rss;

        debug!(
            "Memory monitor initialized: baseline {:.1}MB",
            rss as f64 / 1024.0 / 1024.0
        );
        Ok(())
    }

    /// Record memory usage for a stage (passive observation)
    pub async fn observe_stage(&mut self, stage_name: &str) -> Result<MemorySnapshot> {
        if !self.enabled {
            return Ok(MemorySnapshot {
                stage: stage_name.to_string(),
                rss_mb: 0.0,
                delta_mb: 0.0,
                timestamp: Instant::now(),
            });
        }

        let current_rss = self.get_current_rss().await?;

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
                "Memory observation for '{}': {} ({})",
                stage_name,
                format_memory(rss_mb * 1024.0 * 1024.0),
                format_memory_delta(delta_mb * 1024.0 * 1024.0)
            );
        }

        Ok(snapshot)
    }

    /// Check if current memory usage exceeds limits and suggest actions
    pub async fn check_memory_limit(&self) -> Result<MemoryLimitStatus> {
        if !self.enabled {
            return Ok(MemoryLimitStatus::Ok);
        }

        if let Some(limit_mb) = self.memory_limit_mb {
            let current_rss = self.get_current_rss().await?;
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

    /// Get current RSS memory from system using sysinfo
    pub async fn get_current_rss(&self) -> Result<u64> {
        let mut system = self.system.write().await;
        system.refresh_process(self.current_pid);

        if let Some(process) = system.process(self.current_pid) {
            Ok(process.memory()) // sysinfo returns bytes
        } else {
            Ok(0)
        }
    }

    /// Get comprehensive system pressure assessment
    pub async fn get_system_pressure_assessment(&self) -> Result<SystemPressureAssessment> {
        let mut system = self.system.write().await;
        system.refresh_all();

        // Get application memory
        let app_memory_mb = if let Some(process) = system.process(self.current_pid) {
            process.memory() as f64 / 1024.0 / 1024.0 // Convert bytes to MB
        } else {
            0.0
        };

        // Get system memory
        let total_memory_mb = system.total_memory() as f64 / (1024.0 * 1024.0); // Convert from bytes to MB
        let used_memory_mb = system.used_memory() as f64 / (1024.0 * 1024.0);
        let available_memory_mb = system.available_memory() as f64 / (1024.0 * 1024.0);
        let system_memory_percent = (used_memory_mb / total_memory_mb) * 100.0;

        // Get CPU usage
        let cpu_count = system.cpus().len();
        let cpu_usage_sum: f32 = system.cpus().iter().map(|cpu| cpu.cpu_usage()).sum();
        let system_cpu_percent = (cpu_usage_sum / cpu_count as f32) as f64;

        // Get system load
        let load_avg = system.load_average();
        let system_load_avg = load_avg.one;

        let metrics = SystemPressureMetrics {
            app_memory_mb,
            system_memory_percent,
            system_cpu_percent,
            system_load_avg,
            total_system_memory_mb: total_memory_mb,
            available_system_memory_mb: available_memory_mb,
        };

        // Assess pressure levels
        let memory_pressure = self.assess_app_memory_pressure(app_memory_mb);
        let memory_pressure_system = self.assess_system_memory_pressure(system_memory_percent);
        let cpu_pressure_system = self.assess_cpu_pressure(system_cpu_percent, system_load_avg);

        Ok(SystemPressureAssessment {
            memory_pressure,
            memory_pressure_system,
            cpu_pressure_system,
            metrics,
        })
    }

    /// Assess application memory pressure based on configured limits
    fn assess_app_memory_pressure(&self, app_memory_mb: f64) -> MemoryPressureLevel {
        if let Some(limit_mb) = self.memory_limit_mb {
            let usage_percent = (app_memory_mb / limit_mb as f64) * 100.0;

            if usage_percent < 50.0 {
                MemoryPressureLevel::Optimal
            } else if usage_percent < 75.0 {
                MemoryPressureLevel::Moderate
            } else if usage_percent < 90.0 {
                MemoryPressureLevel::High
            } else if usage_percent < 100.0 {
                MemoryPressureLevel::Critical
            } else {
                MemoryPressureLevel::Emergency
            }
        } else {
            // No limit configured, use system-wide assessment
            self.assess_system_memory_pressure(app_memory_mb)
        }
    }

    /// Assess system memory pressure
    fn assess_system_memory_pressure(&self, system_memory_percent: f64) -> MemoryPressureLevel {
        if system_memory_percent < 60.0 {
            MemoryPressureLevel::Optimal
        } else if system_memory_percent < 75.0 {
            MemoryPressureLevel::Moderate
        } else if system_memory_percent < 85.0 {
            MemoryPressureLevel::High
        } else if system_memory_percent < 95.0 {
            MemoryPressureLevel::Critical
        } else {
            MemoryPressureLevel::Emergency
        }
    }

    /// Assess CPU pressure
    fn assess_cpu_pressure(&self, cpu_percent: f64, load_avg: f64) -> MemoryPressureLevel {
        // Consider both CPU usage and load average
        let cpu_pressure = if cpu_percent < 50.0 {
            0
        } else if cpu_percent < 70.0 {
            1
        } else if cpu_percent < 85.0 {
            2
        } else if cpu_percent < 95.0 {
            3
        } else {
            4
        };

        let load_pressure = if load_avg < 1.0 {
            0
        } else if load_avg < 2.0 {
            1
        } else if load_avg < 4.0 {
            2
        } else if load_avg < 8.0 {
            3
        } else {
            4
        };

        // Take the maximum of both pressures
        let max_pressure = cpu_pressure.max(load_pressure);

        match max_pressure {
            0 => MemoryPressureLevel::Optimal,
            1 => MemoryPressureLevel::Moderate,
            2 => MemoryPressureLevel::High,
            3 => MemoryPressureLevel::Critical,
            _ => MemoryPressureLevel::Emergency,
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
                        "Memory usage high: {:.1}% of limit ({} / {}) - Stage: {}",
                        usage_ratio * 100.0,
                        format_memory(snapshot.rss_mb * 1024.0 * 1024.0),
                        format_memory(limit_mb as f64 * 1024.0 * 1024.0),
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
        use std::sync::Arc;
        use tokio::sync::RwLock;
        use sysinfo::System;
        
        let config = MemoryMonitoringConfig::default();
        let system = Arc::new(RwLock::new(System::new()));
        let monitor = SimpleMemoryMonitor::new(Some(512), config, system);
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

