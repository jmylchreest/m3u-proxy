//! Performance tracking for pipeline orchestrator
//!
//! This module provides comprehensive performance monitoring including memory usage
//! and timing tracking for each pipeline stage.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use sysinfo::{ProcessExt, System, SystemExt};
use tracing::{info, debug};

/// Memory information snapshot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemorySnapshot {
    /// Process memory usage in bytes
    pub memory_bytes: u64,
    /// Process memory usage in KB
    pub memory_kb: u64,
    /// Process memory usage in MB
    pub memory_mb: f64,
    /// Virtual memory usage in bytes
    pub virtual_memory_bytes: u64,
    /// System available memory in bytes
    pub system_available_bytes: u64,
    /// System total memory in bytes
    pub system_total_bytes: u64,
    /// Memory usage percentage of system total
    pub memory_usage_percent: f64,
    /// Timestamp when this snapshot was taken
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

/// Performance metrics for a single stage
#[derive(Debug, Clone)]
pub struct StagePerformanceMetrics {
    /// Stage name
    pub stage_name: String,
    /// When the stage started (not serializable)
    #[allow(dead_code)]
    start_time: Instant,
    /// Duration of the stage execution
    pub duration: Option<Duration>,
    /// Memory snapshot before stage execution
    pub memory_before: MemorySnapshot,
    /// Memory snapshot after stage execution
    pub memory_after: Option<MemorySnapshot>,
    /// Memory delta (growth/reduction) in bytes
    pub memory_delta_bytes: Option<i64>,
    /// Memory delta (growth/reduction) in MB
    pub memory_delta_mb: Option<f64>,
    /// Number of artifacts processed
    pub artifacts_processed: usize,
    /// Additional stage-specific metrics
    pub custom_metrics: HashMap<String, serde_json::Value>,
}

/// Complete pipeline performance tracking  
#[derive(Debug)]
pub struct PipelinePerformanceTracker {
    /// Pipeline execution ID
    pub execution_id: String,
    /// Pipeline execution prefix
    pub execution_prefix: String,
    /// System information (not serializable)
    #[allow(dead_code)]
    system: System,
    /// Process ID for memory tracking
    #[allow(dead_code)]
    process_id: u32,
    /// Pipeline start time (not serializable)
    #[allow(dead_code)]
    pipeline_start_time: Instant,
    /// Pipeline total duration
    pub total_duration: Option<Duration>,
    /// Initial memory snapshot
    pub initial_memory: MemorySnapshot,
    /// Final memory snapshot
    pub final_memory: Option<MemorySnapshot>,
    /// Performance metrics for each stage
    pub stage_metrics: HashMap<String, StagePerformanceMetrics>,
    /// Peak memory usage during pipeline execution
    pub peak_memory_mb: f64,
    /// Total memory growth/reduction in MB
    pub total_memory_delta_mb: Option<f64>,
}

impl MemorySnapshot {
    /// Create a new memory snapshot
    pub fn new(system: &System, process_id: u32) -> Self {
        let process = system.process(sysinfo::Pid::from(process_id as usize));
        let memory_bytes = process.map(|p| p.memory()).unwrap_or(0); // sysinfo returns bytes
        let virtual_memory_bytes = process.map(|p| p.virtual_memory()).unwrap_or(0);
        
        let memory_kb = memory_bytes / 1024;
        let memory_mb = memory_bytes as f64 / (1024.0 * 1024.0);
        
        let system_total_bytes = system.total_memory(); // sysinfo returns bytes
        let system_available_bytes = system.available_memory();
        
        let memory_usage_percent = if system_total_bytes > 0 {
            (memory_bytes as f64 / system_total_bytes as f64) * 100.0
        } else {
            0.0
        };

        Self {
            memory_bytes,
            memory_kb,
            memory_mb,
            virtual_memory_bytes,
            system_available_bytes,
            system_total_bytes,
            memory_usage_percent,
            timestamp: chrono::Utc::now(),
        }
    }
}

impl StagePerformanceMetrics {
    /// Create new stage performance metrics
    pub fn new(stage_name: String, system: &System, process_id: u32) -> Self {
        Self {
            stage_name: stage_name.clone(),
            start_time: Instant::now(),
            duration: None,
            memory_before: MemorySnapshot::new(system, process_id),
            memory_after: None,
            memory_delta_bytes: None,
            memory_delta_mb: None,
            artifacts_processed: 0,
            custom_metrics: HashMap::new(),
        }
    }

    /// Complete stage metrics measurement
    pub fn complete(&mut self, system: &System, process_id: u32, artifacts_processed: usize) {
        self.duration = Some(self.start_time.elapsed());
        self.memory_after = Some(MemorySnapshot::new(system, process_id));
        self.artifacts_processed = artifacts_processed;

        // Calculate memory deltas
        if let Some(ref memory_after) = self.memory_after {
            let delta_bytes = memory_after.memory_bytes as i64 - self.memory_before.memory_bytes as i64;
            let delta_mb = memory_after.memory_mb - self.memory_before.memory_mb;
            
            self.memory_delta_bytes = Some(delta_bytes);
            self.memory_delta_mb = Some(delta_mb);
        }
    }

    /// Add custom metric
    pub fn add_metric<T: Into<serde_json::Value>>(&mut self, key: String, value: T) {
        self.custom_metrics.insert(key, value.into());
    }
}

impl PipelinePerformanceTracker {
    /// Create new pipeline performance tracker
    pub fn new(execution_id: String, execution_prefix: String) -> Self {
        let mut system = System::new_all();
        system.refresh_all();
        
        let process_id = std::process::id();
        let initial_memory = MemorySnapshot::new(&system, process_id);
        let peak_memory_mb = initial_memory.memory_mb;

        Self {
            execution_id,
            execution_prefix,
            system,
            process_id,
            pipeline_start_time: Instant::now(),
            total_duration: None,
            initial_memory,
            final_memory: None,
            stage_metrics: HashMap::new(),
            peak_memory_mb,
            total_memory_delta_mb: None,
        }
    }

    /// Start tracking a new stage
    pub fn start_stage(&mut self, stage_name: String) -> &mut StagePerformanceMetrics {
        // Refresh system information
        self.system.refresh_all();
        
        let metrics = StagePerformanceMetrics::new(stage_name.clone(), &self.system, self.process_id);
        
        // Update peak memory if current usage is higher
        if metrics.memory_before.memory_mb > self.peak_memory_mb {
            self.peak_memory_mb = metrics.memory_before.memory_mb;
        }
        
        debug!(
            "Pipeline stage started: stage={} memory={}MB",
            stage_name, metrics.memory_before.memory_mb
        );
        
        self.stage_metrics.insert(stage_name.clone(), metrics);
        self.stage_metrics.get_mut(&stage_name).unwrap()
    }

    /// Complete tracking for a stage
    pub fn complete_stage(&mut self, stage_name: &str, artifacts_processed: usize) {
        // Refresh system information
        self.system.refresh_all();
        
        if let Some(metrics) = self.stage_metrics.get_mut(stage_name) {
            metrics.complete(&self.system, self.process_id, artifacts_processed);
            
            // Update peak memory if current usage is higher
            if let Some(ref memory_after) = metrics.memory_after {
                if memory_after.memory_mb > self.peak_memory_mb {
                    self.peak_memory_mb = memory_after.memory_mb;
                }
            }
            
            debug!(
                "Pipeline stage completed: stage={} duration={} memory_before={}MB memory_after={}MB memory_delta={}MB artifacts={}",
                stage_name,
                crate::utils::human_format::format_duration_precise(metrics.duration.unwrap_or(Duration::ZERO)),
                metrics.memory_before.memory_mb,
                metrics.memory_after.as_ref().map(|m| m.memory_mb).unwrap_or(0.0),
                metrics.memory_delta_mb.unwrap_or(0.0),
                artifacts_processed
            );
        }
    }

    /// Add custom metric to a stage
    pub fn add_stage_metric<T: Into<serde_json::Value>>(&mut self, stage_name: &str, key: String, value: T) {
        if let Some(metrics) = self.stage_metrics.get_mut(stage_name) {
            metrics.add_metric(key, value);
        }
    }

    /// Complete pipeline tracking
    pub fn complete_pipeline(&mut self) {
        // Refresh system information one final time
        self.system.refresh_all();
        
        self.total_duration = Some(self.pipeline_start_time.elapsed());
        self.final_memory = Some(MemorySnapshot::new(&self.system, self.process_id));

        // Calculate total memory delta
        if let Some(ref final_memory) = self.final_memory {
            self.total_memory_delta_mb = Some(final_memory.memory_mb - self.initial_memory.memory_mb);
        }
    }

    /// Generate comprehensive performance report
    pub fn generate_performance_report(&self) -> String {
        let mut report = String::new();
        
        // Pipeline overview
        report.push_str(&format!("\nPipeline Performance Report\n"));
        report.push_str(&format!("==================================\n"));
        report.push_str(&format!("Execution ID: {}\n", self.execution_id));
        report.push_str(&format!("Execution Prefix: {}\n", self.execution_prefix));
        
        if let Some(total_duration) = self.total_duration {
            report.push_str(&format!("Total Duration: {}\n", 
                crate::utils::human_format::format_duration_precise(total_duration)));
        }
        
        report.push_str(&format!("Initial Memory: {:.2}MB\n", self.initial_memory.memory_mb));
        
        if let Some(ref final_memory) = self.final_memory {
            report.push_str(&format!("Final Memory: {:.2}MB\n", final_memory.memory_mb));
        }
        
        if let Some(total_delta) = self.total_memory_delta_mb {
            let delta_sign = if total_delta >= 0.0 { "+" } else { "" };
            report.push_str(&format!("Total Memory Delta: {}{:.2}MB\n", delta_sign, total_delta));
        }
        
        report.push_str(&format!("Peak Memory Usage: {:.2}MB\n", self.peak_memory_mb));

        // Stage breakdown
        report.push_str(&format!("\nStage Performance Breakdown\n"));
        report.push_str(&format!("--------------------------------\n"));
        
        // Sort stages by start time for chronological order
        let mut sorted_stages: Vec<_> = self.stage_metrics.iter().collect();
        sorted_stages.sort_by_key(|(_, metrics)| metrics.start_time);
        
        for (stage_name, metrics) in &sorted_stages {
            report.push_str(&format!("\nStage: {}\n", stage_name));
            
            if let Some(duration) = metrics.duration {
                report.push_str(&format!("   Duration: {}\n", 
                    crate::utils::human_format::format_duration_precise(duration)));
            }
            
            report.push_str(&format!("   Memory Before: {:.2}MB\n", metrics.memory_before.memory_mb));
            
            if let Some(ref memory_after) = metrics.memory_after {
                report.push_str(&format!("   Memory After: {:.2}MB\n", memory_after.memory_mb));
            }
            
            if let Some(delta) = metrics.memory_delta_mb {
                let delta_sign = if delta >= 0.0 { "+" } else { "" };
                report.push_str(&format!("   Memory Delta: {}{:.2}MB\n", delta_sign, delta));
            }
            
            report.push_str(&format!("   Artifacts Processed: {}\n", metrics.artifacts_processed));
            
            // Include custom metrics
            if !metrics.custom_metrics.is_empty() {
                report.push_str(&format!("   Custom Metrics:\n"));
                for (key, value) in &metrics.custom_metrics {
                    report.push_str(&format!("     {}: {}\n", key, value));
                }
            }
        }

        // Memory usage timeline
        report.push_str(&format!("\nMemory Usage Timeline\n"));
        report.push_str(&format!("------------------------\n"));
        report.push_str(&format!("Initial: {:.2}MB\n", self.initial_memory.memory_mb));
        
        for (stage_name, metrics) in &sorted_stages {
            if let Some(ref memory_after) = metrics.memory_after {
                let delta = memory_after.memory_mb - self.initial_memory.memory_mb;
                let delta_sign = if delta >= 0.0 { "+" } else { "" };
                report.push_str(&format!("After {}: {:.2}MB ({}{}MB from start)\n", 
                    stage_name, memory_after.memory_mb, delta_sign, delta));
            }
        }
        
        report.push_str(&format!("\n"));
        
        report
    }

    /// Log performance report using tracing
    pub fn log_performance_report(&self) {
        let report = self.generate_performance_report();
        
        // Log each line separately for better formatting in logs
        for line in report.lines() {
            if line.trim().is_empty() {
                continue;
            }
            info!("{}", line);
        }
    }

    /// Get stage summaries for pipeline overview
    pub fn get_stage_summaries(&self) -> Vec<(String, &StagePerformanceMetrics)> {
        self.stage_metrics
            .iter()
            .map(|(name, metrics)| (name.clone(), metrics))
            .collect()
    }

    /// Get final memory snapshot
    pub fn get_final_memory_snapshot(&self) -> Option<&MemorySnapshot> {
        self.final_memory.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn test_memory_snapshot_creation() {
        let mut system = System::new_all();
        system.refresh_all();
        let process_id = std::process::id();
        
        let snapshot = MemorySnapshot::new(&system, process_id);
        
        assert!(snapshot.memory_bytes > 0);
        assert!(snapshot.memory_kb > 0);
        assert!(snapshot.memory_mb > 0.0);
        assert!(snapshot.system_total_bytes > 0);
    }

    #[test]
    fn test_stage_performance_metrics() {
        let mut system = System::new_all();
        system.refresh_all();
        let process_id = std::process::id();
        
        let mut metrics = StagePerformanceMetrics::new("test_stage".to_string(), &system, process_id);
        
        // Simulate some work
        thread::sleep(Duration::from_millis(10));
        
        metrics.complete(&system, process_id, 5);
        
        assert!(metrics.duration.unwrap().as_millis() >= 10);
        assert_eq!(metrics.artifacts_processed, 5);
        assert!(metrics.memory_after.is_some());
        assert!(metrics.memory_delta_bytes.is_some());
        assert!(metrics.memory_delta_mb.is_some());
    }

    #[test]
    fn test_pipeline_performance_tracker() {
        let mut tracker = PipelinePerformanceTracker::new(
            "test_execution".to_string(),
            "test_prefix".to_string(),
        );
        
        // Start and complete a stage
        tracker.start_stage("test_stage".to_string());
        thread::sleep(Duration::from_millis(10));
        tracker.complete_stage("test_stage", 3);
        
        // Add custom metric
        tracker.add_stage_metric("test_stage", "custom_metric".to_string(), 42);
        
        // Complete pipeline
        tracker.complete_pipeline();
        
        assert!(tracker.total_duration.unwrap().as_millis() >= 10);
        assert!(tracker.final_memory.is_some());
        assert!(tracker.total_memory_delta_mb.is_some());
        assert_eq!(tracker.stage_metrics.len(), 1);
        
        let report = tracker.generate_performance_report();
        assert!(report.contains("Pipeline Performance Report"));
        assert!(report.contains("test_stage"));
        assert!(report.contains("custom_metric"));
    }
}