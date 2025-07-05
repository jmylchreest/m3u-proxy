//! Memory monitoring configuration for controlling verbosity and behavior
//!
//! This module provides configuration options for memory monitoring to reduce
//! log spam while maintaining important monitoring capabilities.

use serde::{Deserialize, Serialize};
use std::sync::{Arc, RwLock};
use std::time::Duration;

/// Memory monitoring configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryMonitoringConfig {
    /// Whether memory monitoring is enabled
    pub enabled: bool,

    /// Memory limit in MB (None means no limit)
    pub memory_limit_mb: Option<usize>,

    /// Threshold for memory warnings (0.0 to 1.0, default 0.85)
    pub warning_threshold: f64,

    /// Minimum time between memory warnings to prevent spam
    pub warning_cooldown_seconds: u64,

    /// Minimum memory delta (MB) to log stage completions
    pub min_stage_delta_mb: f64,

    /// Memory monitoring verbosity level
    pub verbosity: MemoryVerbosity,

    /// Whether to log pressure escalations only for significant changes
    pub log_pressure_escalations_only: bool,
}

/// Memory monitoring verbosity levels
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum MemoryVerbosity {
    /// No memory logging except critical errors
    Silent,
    /// Only log significant memory changes and warnings
    Minimal,
    /// Log important memory events and stage completions
    Normal,
    /// Log all memory observations and detailed stage info
    Verbose,
    /// Log everything including debug information
    Debug,
}

impl Default for MemoryMonitoringConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            memory_limit_mb: Some(512),
            warning_threshold: 0.85,
            warning_cooldown_seconds: 10,
            min_stage_delta_mb: 10.0,
            verbosity: MemoryVerbosity::Minimal,
            log_pressure_escalations_only: true,
        }
    }
}

impl MemoryMonitoringConfig {
    /// Create a new config with production-friendly defaults
    pub fn production() -> Self {
        Self {
            enabled: true,
            memory_limit_mb: Some(512),
            warning_threshold: 0.90,
            warning_cooldown_seconds: 30,
            min_stage_delta_mb: 20.0,
            verbosity: MemoryVerbosity::Minimal,
            log_pressure_escalations_only: true,
        }
    }

    /// Create a config for development with more verbose logging
    pub fn development() -> Self {
        Self {
            enabled: true,
            memory_limit_mb: Some(1024),
            warning_threshold: 0.80,
            warning_cooldown_seconds: 5,
            min_stage_delta_mb: 5.0,
            verbosity: MemoryVerbosity::Normal,
            log_pressure_escalations_only: false,
        }
    }

    /// Create a config for debugging with maximum verbosity
    pub fn debug() -> Self {
        Self {
            enabled: true,
            memory_limit_mb: Some(512),
            warning_threshold: 0.75,
            warning_cooldown_seconds: 1,
            min_stage_delta_mb: 1.0,
            verbosity: MemoryVerbosity::Debug,
            log_pressure_escalations_only: false,
        }
    }

    /// Create a minimal config with very quiet logging
    pub fn minimal() -> Self {
        Self {
            enabled: true,
            memory_limit_mb: Some(512),
            warning_threshold: 0.95,
            warning_cooldown_seconds: 60,
            min_stage_delta_mb: 50.0,
            verbosity: MemoryVerbosity::Silent,
            log_pressure_escalations_only: true,
        }
    }

    /// Get warning cooldown as Duration
    pub fn warning_cooldown_duration(&self) -> Duration {
        Duration::from_secs(self.warning_cooldown_seconds)
    }

    /// Check if a stage completion should be logged based on memory delta
    pub fn should_log_stage_completion(&self, memory_delta_mb: f64) -> bool {
        match self.verbosity {
            MemoryVerbosity::Silent => false,
            MemoryVerbosity::Minimal => memory_delta_mb.abs() >= self.min_stage_delta_mb,
            MemoryVerbosity::Normal => memory_delta_mb.abs() >= (self.min_stage_delta_mb / 2.0),
            MemoryVerbosity::Verbose => memory_delta_mb.abs() >= 1.0,
            MemoryVerbosity::Debug => true,
        }
    }

    /// Check if memory warnings should be logged
    pub fn should_log_memory_warnings(&self) -> bool {
        match self.verbosity {
            MemoryVerbosity::Silent => false,
            _ => true,
        }
    }

    /// Check if pressure escalations should be logged
    pub fn should_log_pressure_escalation(&self, is_significant: bool) -> bool {
        match self.verbosity {
            MemoryVerbosity::Silent => false,
            MemoryVerbosity::Minimal => is_significant,
            _ => !self.log_pressure_escalations_only || is_significant,
        }
    }

    /// Check if stage observations should be logged
    pub fn should_log_stage_observations(&self) -> bool {
        match self.verbosity {
            MemoryVerbosity::Silent | MemoryVerbosity::Minimal => false,
            MemoryVerbosity::Normal => false,
            MemoryVerbosity::Verbose | MemoryVerbosity::Debug => true,
        }
    }
}

impl MemoryVerbosity {
    /// Parse verbosity from string
    pub fn from_str(s: &str) -> Result<Self, String> {
        match s.to_lowercase().as_str() {
            "silent" => Ok(Self::Silent),
            "minimal" => Ok(Self::Minimal),
            "normal" => Ok(Self::Normal),
            "verbose" => Ok(Self::Verbose),
            "debug" => Ok(Self::Debug),
            _ => Err(format!("Unknown verbosity level: {}", s)),
        }
    }

    /// Get verbosity as string
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Silent => "silent",
            Self::Minimal => "minimal",
            Self::Normal => "normal",
            Self::Verbose => "verbose",
            Self::Debug => "debug",
        }
    }
}

/// Global memory configuration instance
static GLOBAL_MEMORY_CONFIG: std::sync::OnceLock<Arc<RwLock<MemoryMonitoringConfig>>> =
    std::sync::OnceLock::new();

/// Initialize the global memory configuration
pub fn init_global_memory_config(config: MemoryMonitoringConfig) {
    GLOBAL_MEMORY_CONFIG.set(Arc::new(RwLock::new(config))).ok();
}

/// Get the global memory configuration
pub fn get_global_memory_config() -> MemoryMonitoringConfig {
    GLOBAL_MEMORY_CONFIG
        .get()
        .map(|config| config.read().unwrap().clone())
        .unwrap_or_else(MemoryMonitoringConfig::default)
}

/// Update the global memory configuration
pub fn update_global_memory_config<F>(f: F)
where
    F: FnOnce(&mut MemoryMonitoringConfig),
{
    if let Some(config) = GLOBAL_MEMORY_CONFIG.get() {
        if let Ok(mut config) = config.write() {
            f(&mut config);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = MemoryMonitoringConfig::default();
        assert!(config.enabled);
        assert_eq!(config.memory_limit_mb, Some(512));
        assert_eq!(config.verbosity, MemoryVerbosity::Minimal);
    }

    #[test]
    fn test_production_config() {
        let config = MemoryMonitoringConfig::production();
        assert_eq!(config.warning_threshold, 0.90);
        assert_eq!(config.warning_cooldown_seconds, 30);
        assert_eq!(config.min_stage_delta_mb, 20.0);
    }

    #[test]
    fn test_verbosity_parsing() {
        assert_eq!(
            MemoryVerbosity::from_str("minimal").unwrap(),
            MemoryVerbosity::Minimal
        );
        assert_eq!(
            MemoryVerbosity::from_str("VERBOSE").unwrap(),
            MemoryVerbosity::Verbose
        );
        assert!(MemoryVerbosity::from_str("invalid").is_err());
    }

    #[test]
    fn test_should_log_stage_completion() {
        let config = MemoryMonitoringConfig::production();
        assert!(config.should_log_stage_completion(25.0));
        assert!(!config.should_log_stage_completion(5.0));

        let debug_config = MemoryMonitoringConfig::debug();
        assert!(debug_config.should_log_stage_completion(0.5));
    }

    #[test]
    fn test_silent_verbosity() {
        let config = MemoryMonitoringConfig::minimal();
        assert!(!config.should_log_memory_warnings());
        assert!(!config.should_log_stage_observations());
    }

    #[test]
    fn test_global_config() {
        let config = MemoryMonitoringConfig::debug();
        init_global_memory_config(config.clone());

        let global_config = get_global_memory_config();
        assert_eq!(global_config.verbosity, config.verbosity);
        assert_eq!(global_config.memory_limit_mb, config.memory_limit_mb);
    }
}
