//! Unified memory context for centralized monitoring and stage-to-stage analysis
//!
//! This module provides a centralized memory management context that combines
//! memory monitoring, pressure calculation, and inter-stage analysis capabilities.

use anyhow::Result;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tracing::{debug, info, warn};

use crate::proxy::stage_strategy::{
    DynamicStrategySelector, MemoryPressureLevel, MemoryThresholds,
};
use crate::utils::memory_config::{
    MemoryMonitoringConfig, MemoryVerbosity, get_global_memory_config,
};
use crate::utils::{
    MemoryLimitStatus, MemorySnapshot, MemoryStats, SimpleMemoryMonitor, format_duration,
    format_memory,
};

/// Unified memory context for the entire processing pipeline
pub struct MemoryContext {
    /// The underlying memory monitor for system memory tracking
    monitor: SimpleMemoryMonitor,
    /// Strategy selector for memory pressure calculation
    pressure_calculator: DynamicStrategySelector,
    /// Current memory pressure level
    current_pressure: MemoryPressureLevel,
    /// Progression of stages and their memory impact
    stage_progression: Vec<StageMemoryInfo>,
    /// Last memory observation for delta calculation
    last_observation: Option<MemorySnapshot>,
    /// Stage timing information
    stage_timings: HashMap<String, StageTiming>,
    /// Memory monitoring configuration
    config: MemoryMonitoringConfig,
}

/// Detailed memory information for a specific stage
#[derive(Debug, Clone)]
pub struct StageMemoryInfo {
    pub stage_name: String,
    pub memory_before_mb: f64,
    pub memory_after_mb: f64,
    pub memory_delta_mb: f64,
    pub stage_to_stage_delta_mb: f64, // Delta from previous stage
    pub peak_during_stage_mb: f64,
    pub pressure_level: MemoryPressureLevel,
    pub timestamp: Instant,
    pub duration_ms: u64,
}

/// Stage timing information
#[derive(Debug, Clone)]
pub struct StageTiming {
    pub start_time: Instant,
    pub end_time: Option<Instant>,
    pub duration_ms: Option<u64>,
}

/// Memory analysis results between stages
#[derive(Debug, Clone)]
pub struct MemoryAnalysis {
    pub total_stages: usize,
    pub total_memory_growth_mb: f64,
    pub largest_stage_impact_mb: f64,
    pub largest_impact_stage: String,
    pub memory_efficiency_trend: MemoryEfficiencyTrend,
    pub pressure_escalations: Vec<(String, MemoryPressureLevel, MemoryPressureLevel)>,
    pub cleanup_opportunities: Vec<String>,
}

/// Memory efficiency trend analysis
#[derive(Debug, Clone, PartialEq)]
pub enum MemoryEfficiencyTrend {
    Improving, // Memory usage decreasing or stable
    Stable,    // Memory usage consistent
    Degrading, // Memory usage increasing moderately
    Critical,  // Memory usage increasing rapidly
}

impl MemoryContext {
    /// Create a new memory context with shared system instance
    pub fn new(
        memory_limit_mb: Option<usize>,
        memory_thresholds: Option<MemoryThresholds>,
        system: Arc<tokio::sync::RwLock<sysinfo::System>>,
    ) -> Self {
        let config = get_global_memory_config();
        let monitor = SimpleMemoryMonitor::new(memory_limit_mb, config.clone(), system);

        // Create a minimal strategy selector just for pressure calculation
        let registry = crate::proxy::stage_strategy::StageStrategyRegistry::new();
        let mut pressure_calculator = DynamicStrategySelector::new(registry);

        // Use custom thresholds if provided
        if let Some(thresholds) = memory_thresholds {
            pressure_calculator.set_memory_thresholds(thresholds);
        }

        Self {
            monitor,
            pressure_calculator,
            current_pressure: MemoryPressureLevel::Optimal,
            stage_progression: Vec::new(),
            last_observation: None,
            stage_timings: HashMap::new(),
            config,
        }
    }

    /// Initialize the memory context
    pub async fn initialize(&mut self) -> Result<()> {
        self.monitor.initialize().await?;
        debug!("Memory monitoring initialized");
        Ok(())
    }

    /// Start timing for a stage
    pub async fn start_stage(&mut self, stage_name: &str) -> Result<MemorySnapshot> {
        // Record start timing
        self.stage_timings.insert(
            stage_name.to_string(),
            StageTiming {
                start_time: Instant::now(),
                end_time: None,
                duration_ms: None,
            },
        );

        // Observe memory at stage start
        let snapshot = self
            .monitor
            .observe_stage(&format!("{}_start", stage_name))
            .await?;

        debug!("Stage started: {}", stage_name);

        Ok(snapshot)
    }

    /// Complete a stage and analyze memory impact
    pub async fn complete_stage(&mut self, stage_name: &str) -> Result<StageMemoryInfo> {
        // Get end timing
        let start_timing = self
            .stage_timings
            .get(stage_name)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Stage '{}' was not started", stage_name))?;

        let end_time = Instant::now();
        let duration_ms = end_time.duration_since(start_timing.start_time).as_millis() as u64;

        // Update timing
        self.stage_timings.insert(
            stage_name.to_string(),
            StageTiming {
                start_time: start_timing.start_time,
                end_time: Some(end_time),
                duration_ms: Some(duration_ms),
            },
        );

        // Update config from global in case it changed
        self.config = get_global_memory_config();

        // Observe memory at stage end
        let end_snapshot = self
            .monitor
            .observe_stage(&format!("{}_end", stage_name))
            .await?;

        // Calculate memory pressure
        let pressure = self
            .pressure_calculator
            .assess_memory_pressure(end_snapshot.rss_mb as usize);

        // Check for pressure escalation - only warn on significant escalations
        if pressure as u8 > self.current_pressure as u8 {
            let is_significant = matches!(
                (self.current_pressure, pressure),
                (_, MemoryPressureLevel::Emergency)
                    | (MemoryPressureLevel::Optimal, MemoryPressureLevel::High)
            );

            if self.config.should_log_pressure_escalation(is_significant) {
                match (self.current_pressure, pressure) {
                    (_, MemoryPressureLevel::Emergency) => {
                        warn!(
                            "Memory pressure escalated to Emergency during stage '{}'",
                            stage_name
                        );
                    }
                    (MemoryPressureLevel::Optimal, MemoryPressureLevel::High) => {
                        debug!(
                            "Memory pressure escalated from Optimal to High during stage '{}'",
                            stage_name
                        );
                    }
                    _ => {
                        debug!(
                            "Memory pressure escalated from {:?} to {:?} during stage '{}'",
                            self.current_pressure, pressure, stage_name
                        );
                    }
                }
            }
        }
        self.current_pressure = pressure;

        // Calculate stage-to-stage delta
        let stage_to_stage_delta_mb = if let Some(ref last_obs) = self.last_observation {
            end_snapshot.rss_mb - last_obs.rss_mb
        } else {
            // First stage, delta from baseline
            end_snapshot.delta_mb
        };

        // Get peak memory during stage (approximation)
        let peak_during_stage_mb = self.monitor.get_statistics().peak_mb;

        // Create stage memory info
        let stage_info = StageMemoryInfo {
            stage_name: stage_name.to_string(),
            memory_before_mb: self
                .last_observation
                .as_ref()
                .map(|obs| obs.rss_mb)
                .unwrap_or(end_snapshot.rss_mb - end_snapshot.delta_mb), // baseline
            memory_after_mb: end_snapshot.rss_mb,
            memory_delta_mb: end_snapshot.delta_mb,
            stage_to_stage_delta_mb,
            peak_during_stage_mb,
            pressure_level: pressure,
            timestamp: end_snapshot.timestamp,
            duration_ms,
        };

        // Log stage completion based on configuration
        if self
            .config
            .should_log_stage_completion(stage_to_stage_delta_mb)
        {
            let direction = if stage_to_stage_delta_mb > 0.0 {
                "increased"
            } else {
                "decreased"
            };

            if stage_to_stage_delta_mb.abs() > self.config.min_stage_delta_mb {
                info!(
                    "Stage '{}' completed: Memory {} by {} ({} → {}) in {} | Pressure: {:?}",
                    stage_name,
                    direction,
                    format_memory(stage_to_stage_delta_mb.abs() * 1024.0 * 1024.0),
                    format_memory(stage_info.memory_before_mb * 1024.0 * 1024.0),
                    format_memory(stage_info.memory_after_mb * 1024.0 * 1024.0),
                    format_duration(duration_ms),
                    pressure
                );
            } else {
                // Only log pressure escalation without memory details
                debug!(
                    "Stage '{}' completed: Memory stable at {} in {} | Pressure: {:?}",
                    stage_name,
                    format_memory(stage_info.memory_after_mb * 1024.0 * 1024.0),
                    format_duration(duration_ms),
                    pressure
                );
            }
        } else if self.config.verbosity == MemoryVerbosity::Debug {
            // Always log in debug mode
            debug!(
                "Stage '{}' completed: Memory stable at {} in {} | Pressure: {:?}",
                stage_name,
                format_memory(stage_info.memory_after_mb * 1024.0 * 1024.0),
                format_duration(duration_ms),
                pressure
            );
        }

        // Store stage progression
        self.stage_progression.push(stage_info.clone());
        self.last_observation = Some(end_snapshot);

        Ok(stage_info)
    }

    /// Observe memory without stage boundaries (for custom monitoring points)
    pub async fn observe(
        &mut self,
        observation_point: &str,
    ) -> Result<(MemorySnapshot, MemoryPressureLevel)> {
        let snapshot = self.monitor.observe_stage(observation_point).await?;
        let pressure = self
            .pressure_calculator
            .assess_memory_pressure(snapshot.rss_mb as usize);

        // Update current pressure
        self.current_pressure = pressure;

        Ok((snapshot, pressure))
    }

    /// Get current memory pressure level
    pub fn current_pressure(&self) -> MemoryPressureLevel {
        self.current_pressure
    }

    /// Check if memory cleanup is recommended
    pub async fn should_cleanup(&self) -> Result<bool> {
        // Check pressure level
        if matches!(
            self.current_pressure,
            MemoryPressureLevel::High
                | MemoryPressureLevel::Critical
                | MemoryPressureLevel::Emergency
        ) {
            return Ok(true);
        }

        // Check underlying monitor
        let status = self.monitor.check_memory_limit().await?;
        Ok(matches!(
            status,
            MemoryLimitStatus::Warning | MemoryLimitStatus::Exceeded
        ))
    }

    /// Analyze memory usage patterns across all stages
    pub fn analyze_memory_patterns(&self) -> MemoryAnalysis {
        if self.stage_progression.is_empty() {
            return MemoryAnalysis {
                total_stages: 0,
                total_memory_growth_mb: 0.0,
                largest_stage_impact_mb: 0.0,
                largest_impact_stage: "none".to_string(),
                memory_efficiency_trend: MemoryEfficiencyTrend::Stable,
                pressure_escalations: Vec::new(),
                cleanup_opportunities: Vec::new(),
            };
        }

        let total_stages = self.stage_progression.len();

        // Calculate total memory growth
        let first_stage = &self.stage_progression[0];
        let last_stage = &self.stage_progression[total_stages - 1];
        let total_memory_growth_mb = last_stage.memory_after_mb - first_stage.memory_before_mb;

        // Find largest impact stage
        let (largest_stage_impact_mb, largest_impact_stage) = self
            .stage_progression
            .iter()
            .map(|stage| {
                (
                    stage.stage_to_stage_delta_mb.abs(),
                    stage.stage_name.clone(),
                )
            })
            .max_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal))
            .unwrap_or((0.0, "none".to_string()));

        // Analyze efficiency trend
        let memory_efficiency_trend = self.calculate_efficiency_trend();

        // Find pressure escalations
        let pressure_escalations = self.find_pressure_escalations();

        // Identify cleanup opportunities
        let cleanup_opportunities = self.identify_cleanup_opportunities();

        MemoryAnalysis {
            total_stages,
            total_memory_growth_mb,
            largest_stage_impact_mb,
            largest_impact_stage,
            memory_efficiency_trend,
            pressure_escalations,
            cleanup_opportunities,
        }
    }

    /// Calculate memory efficiency trend
    fn calculate_efficiency_trend(&self) -> MemoryEfficiencyTrend {
        if self.stage_progression.len() < 3 {
            return MemoryEfficiencyTrend::Stable;
        }

        // Look at memory growth rate over last few stages
        let recent_stages =
            &self.stage_progression[self.stage_progression.len().saturating_sub(3)..];
        let growth_rates: Vec<f64> = recent_stages
            .windows(2)
            .map(|pair| pair[1].memory_after_mb - pair[0].memory_after_mb)
            .collect();

        let avg_growth = growth_rates.iter().sum::<f64>() / growth_rates.len() as f64;

        match avg_growth {
            x if x < -5.0 => MemoryEfficiencyTrend::Improving, // Memory decreasing
            x if x < 5.0 => MemoryEfficiencyTrend::Stable,     // Stable within 5MB
            x if x < 20.0 => MemoryEfficiencyTrend::Degrading, // Moderate growth
            _ => MemoryEfficiencyTrend::Critical,              // Rapid growth
        }
    }

    /// Find memory pressure escalations between stages
    fn find_pressure_escalations(&self) -> Vec<(String, MemoryPressureLevel, MemoryPressureLevel)> {
        let mut escalations = Vec::new();

        for window in self.stage_progression.windows(2) {
            let prev_pressure = window[0].pressure_level;
            let curr_pressure = window[1].pressure_level;

            if curr_pressure as u8 > prev_pressure as u8 {
                escalations.push((window[1].stage_name.clone(), prev_pressure, curr_pressure));
            }
        }

        escalations
    }

    /// Identify stages that would benefit from cleanup
    fn identify_cleanup_opportunities(&self) -> Vec<String> {
        let mut opportunities = Vec::new();

        for stage in &self.stage_progression {
            // Identify stages with significant memory growth
            if stage.stage_to_stage_delta_mb > 50.0 {
                opportunities.push(format!(
                    "After '{}': {:.1}MB growth",
                    stage.stage_name, stage.stage_to_stage_delta_mb
                ));
            }

            // Identify stages where pressure increased
            if matches!(
                stage.pressure_level,
                MemoryPressureLevel::High | MemoryPressureLevel::Critical
            ) {
                opportunities.push(format!(
                    "During '{}': Pressure reached {:?}",
                    stage.stage_name, stage.pressure_level
                ));
            }
        }

        opportunities
    }

    /// Get memory statistics from the underlying monitor
    pub fn get_memory_statistics(&self) -> MemoryStats {
        self.monitor.get_statistics()
    }

    /// Get detailed stage progression information
    pub fn get_stage_progression(&self) -> &[StageMemoryInfo] {
        &self.stage_progression
    }

    /// Generate a comprehensive memory report
    pub fn generate_report(&self) -> String {
        let analysis = self.analyze_memory_patterns();
        let stats = self.get_memory_statistics();

        let mut report = Vec::new();

        report.push("=== Memory Context Report ===".to_string());
        report.push(format!("Total Stages: {}", analysis.total_stages));
        report.push(format!(
            "Memory Growth: {:.1}MB",
            analysis.total_memory_growth_mb
        ));
        report.push(format!("Peak Memory: {:.1}MB", stats.peak_mb));
        report.push(format!("Current Pressure: {:?}", self.current_pressure));
        report.push(format!(
            "Efficiency Trend: {:?}",
            analysis.memory_efficiency_trend
        ));

        if !analysis.pressure_escalations.is_empty() {
            report.push("".to_string());
            report.push("Pressure Escalations:".to_string());
            for (stage, from, to) in &analysis.pressure_escalations {
                report.push(format!("  {}: {:?} → {:?}", stage, from, to));
            }
        }

        if !analysis.cleanup_opportunities.is_empty() {
            report.push("".to_string());
            report.push("Cleanup Opportunities:".to_string());
            for opportunity in &analysis.cleanup_opportunities {
                report.push(format!("  {}", opportunity));
            }
        }

        report.push("".to_string());
        report.push("Stage-by-Stage Breakdown:".to_string());
        for stage in &self.stage_progression {
            report.push(format!(
                "  {}: {:.1}MB → {:.1}MB (Δ{:+.1}MB) in {}ms [{:?}]",
                stage.stage_name,
                stage.memory_before_mb,
                stage.memory_after_mb,
                stage.stage_to_stage_delta_mb,
                stage.duration_ms,
                stage.pressure_level
            ));
        }

        report.join("\n")
    }

    /// Reset the context for a new processing run
    pub async fn reset(&mut self) -> Result<()> {
        self.stage_progression.clear();
        self.last_observation = None;
        self.stage_timings.clear();
        self.current_pressure = MemoryPressureLevel::Optimal;
        self.monitor.initialize().await?;
        Ok(())
    }
}

/// Extension trait to add memory pressure thresholds configuration
impl DynamicStrategySelector {
    pub fn set_memory_thresholds(&mut self, thresholds: MemoryThresholds) {
        self.memory_thresholds = thresholds;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_context_creation() {
        let context = MemoryContext::new(Some(512), None);
        assert_eq!(context.current_pressure(), MemoryPressureLevel::Optimal);
        assert_eq!(context.stage_progression.len(), 0);
    }

    #[test]
    fn test_stage_progression_tracking() {
        let mut context = MemoryContext::new(Some(512), None);

        // This would fail in real test due to memory monitor, but tests the structure
        let stage_info = StageMemoryInfo {
            stage_name: "test_stage".to_string(),
            memory_before_mb: 100.0,
            memory_after_mb: 120.0,
            memory_delta_mb: 20.0,
            stage_to_stage_delta_mb: 20.0,
            peak_during_stage_mb: 125.0,
            pressure_level: MemoryPressureLevel::Moderate,
            timestamp: Instant::now(),
            duration_ms: 1000,
        };

        context.stage_progression.push(stage_info);
        assert_eq!(context.stage_progression.len(), 1);
    }

    #[test]
    fn test_efficiency_trend_calculation() {
        let mut context = MemoryContext::new(Some(512), None);

        // Add some mock stages with different memory patterns
        for i in 0..5 {
            let stage_info = StageMemoryInfo {
                stage_name: format!("stage_{}", i),
                memory_before_mb: 100.0 + (i as f64 * 10.0),
                memory_after_mb: 110.0 + (i as f64 * 10.0),
                memory_delta_mb: 10.0,
                stage_to_stage_delta_mb: 10.0,
                peak_during_stage_mb: 115.0 + (i as f64 * 10.0),
                pressure_level: MemoryPressureLevel::Moderate,
                timestamp: Instant::now(),
                duration_ms: 1000,
            };
            context.stage_progression.push(stage_info);
        }

        let trend = context.calculate_efficiency_trend();
        assert_eq!(trend, MemoryEfficiencyTrend::Degrading);
    }
}
