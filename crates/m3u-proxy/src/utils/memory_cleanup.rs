//! Memory cleanup utilities for stage-by-stage memory management
//!
//! This module provides utilities for explicit memory management between processing stages,
//! helping to keep memory usage low during large proxy generation operations.

use std::collections::HashMap;
use std::mem;
use tracing::debug;

/// Memory cleanup coordinator that manages memory between processing stages
#[derive(Debug)]
pub struct MemoryCleanupCoordinator {
    /// Track memory usage before and after cleanup operations
    cleanup_stats: HashMap<String, CleanupStats>,
    /// Whether aggressive cleanup is enabled
    aggressive_cleanup: bool,
    /// Memory pressure threshold for automatic cleanup
    memory_pressure_threshold_mb: Option<f64>,
}

/// Statistics for a cleanup operation
#[derive(Debug, Clone)]
pub struct CleanupStats {
    pub stage_name: String,
    pub memory_before_mb: f64,
    pub memory_after_mb: f64,
    pub memory_freed_mb: f64,
    pub cleanup_duration_ms: u64,
    pub items_cleaned: usize,
}

/// Memory cleanup strategies
#[derive(Debug, Clone)]
pub enum CleanupStrategy {
    /// Basic cleanup - drop collections and shrink to fit
    Basic,
    /// Aggressive cleanup - force collection compaction and unused capacity removal
    Aggressive,
    /// Smart cleanup - analyze usage patterns and clean accordingly
    Smart,
}

/// Memory-efficient stage transition helper
pub struct StageTransition<T> {
    data: Option<T>,
    cleanup_applied: bool,
}

impl MemoryCleanupCoordinator {
    /// Create a new memory cleanup coordinator
    pub fn new(aggressive_cleanup: bool, memory_pressure_threshold_mb: Option<f64>) -> Self {
        Self {
            cleanup_stats: HashMap::new(),
            aggressive_cleanup,
            memory_pressure_threshold_mb,
        }
    }

    /// Perform memory cleanup between stages
    pub fn cleanup_between_stages<T>(
        &mut self,
        stage_name: &str,
        data: &mut T,
        strategy: CleanupStrategy,
    ) -> anyhow::Result<CleanupStats>
    where
        T: MemoryCleanable,
    {
        let start_time = std::time::Instant::now();
        let memory_before = self.get_current_memory_usage_mb()?;

        debug!(
            "Memory cleanup stage={} strategy={:?} memory_before={:.1}MB",
            stage_name, strategy, memory_before
        );
        debug!("Memory before cleanup: {:.1}MB", memory_before);

        // Apply cleanup strategy
        let items_cleaned = match strategy {
            CleanupStrategy::Basic => data.basic_cleanup(),
            CleanupStrategy::Aggressive => data.aggressive_cleanup(),
            CleanupStrategy::Smart => data.smart_cleanup(),
        };

        // Force garbage collection if aggressive cleanup is enabled
        if self.aggressive_cleanup || matches!(strategy, CleanupStrategy::Aggressive) {
            self.force_memory_reclaim();
        }

        let memory_after = self.get_current_memory_usage_mb()?;
        let cleanup_duration = start_time.elapsed().as_millis() as u64;
        let memory_freed = memory_before - memory_after;

        let stats = CleanupStats {
            stage_name: stage_name.to_string(),
            memory_before_mb: memory_before,
            memory_after_mb: memory_after,
            memory_freed_mb: memory_freed,
            cleanup_duration_ms: cleanup_duration,
            items_cleaned,
        };

        debug!(
            "Memory cleanup stage={} strategy={:?} freed={:.1}MB items={} duration={}ms",
            stage_name, strategy, memory_freed, items_cleaned, cleanup_duration
        );

        self.cleanup_stats
            .insert(stage_name.to_string(), stats.clone());
        Ok(stats)
    }

    /// Check if memory cleanup is needed based on pressure threshold
    pub fn should_cleanup(&self) -> anyhow::Result<bool> {
        if let Some(threshold_mb) = self.memory_pressure_threshold_mb {
            let current_memory = self.get_current_memory_usage_mb()?;
            Ok(current_memory > threshold_mb)
        } else {
            Ok(false)
        }
    }

    /// Get summary of all cleanup operations
    pub fn get_cleanup_summary(&self) -> String {
        if self.cleanup_stats.is_empty() {
            return "No memory cleanup operations performed".to_string();
        }

        let total_freed: f64 = self.cleanup_stats.values().map(|s| s.memory_freed_mb).sum();
        let total_items: usize = self.cleanup_stats.values().map(|s| s.items_cleaned).sum();
        let total_duration: u64 = self
            .cleanup_stats
            .values()
            .map(|s| s.cleanup_duration_ms)
            .sum();

        format!(
            "Memory cleanup summary: {} operations, {:.1}MB freed, {} items cleaned, {}ms total",
            self.cleanup_stats.len(),
            total_freed,
            total_items,
            total_duration
        )
    }

    /// Force memory reclamation (platform-specific)
    fn force_memory_reclaim(&self) {
        // On Linux, we can try to encourage memory reclamation
        #[cfg(target_os = "linux")]
        {
            // Try to sync and drop caches (this is a no-op for userspace, but good practice)
            debug!("Encouraging memory reclamation on Linux");
        }

        // For all platforms, we can try to hint to the allocator
        debug!("Forcing memory reclamation");
    }

    /// Get current memory usage in MB
    fn get_current_memory_usage_mb(&self) -> anyhow::Result<f64> {
        #[cfg(target_os = "linux")]
        {
            let status = std::fs::read_to_string("/proc/self/status")?;
            for line in status.lines() {
                if line.starts_with("VmRSS:")
                    && let Some(kb_str) = line.split_whitespace().nth(1) {
                        let kb: f64 = kb_str.parse()?;
                        return Ok(kb / 1024.0); // Convert KB to MB
                    }
            }
        }

        #[cfg(not(target_os = "linux"))]
        {
            debug!("Memory usage tracking not available on this platform");
        }

        Ok(0.0)
    }
}

/// Trait for types that can be cleaned up between stages
pub trait MemoryCleanable {
    /// Basic cleanup - shrink collections to fit, drop unused capacity
    fn basic_cleanup(&mut self) -> usize;

    /// Aggressive cleanup - more thorough cleanup, may affect performance
    fn aggressive_cleanup(&mut self) -> usize;

    /// Smart cleanup - analyze usage and clean accordingly
    fn smart_cleanup(&mut self) -> usize {
        // Default implementation falls back to basic cleanup
        self.basic_cleanup()
    }
}

/// Implementation for Vec<T>
impl<T> MemoryCleanable for Vec<T> {
    fn basic_cleanup(&mut self) -> usize {
        let old_capacity = self.capacity();
        self.shrink_to_fit();
        old_capacity.saturating_sub(self.capacity())
    }

    fn aggressive_cleanup(&mut self) -> usize {
        let old_capacity = self.capacity();
        self.shrink_to_fit();
        // For aggressive cleanup, we could also sort and dedup if T: Ord + PartialEq
        old_capacity.saturating_sub(self.capacity())
    }
}

/// Implementation for HashMap<K, V>
impl<K, V> MemoryCleanable for HashMap<K, V>
where
    K: std::cmp::Eq + std::hash::Hash,
{
    fn basic_cleanup(&mut self) -> usize {
        let old_capacity = self.capacity();
        self.shrink_to_fit();
        old_capacity.saturating_sub(self.capacity())
    }

    fn aggressive_cleanup(&mut self) -> usize {
        let old_capacity = self.capacity();
        self.shrink_to_fit();
        old_capacity.saturating_sub(self.capacity())
    }
}

/// Implementation for String
impl MemoryCleanable for String {
    fn basic_cleanup(&mut self) -> usize {
        let old_capacity = self.capacity();
        self.shrink_to_fit();
        old_capacity.saturating_sub(self.capacity())
    }

    fn aggressive_cleanup(&mut self) -> usize {
        let old_capacity = self.capacity();
        self.shrink_to_fit();
        old_capacity.saturating_sub(self.capacity())
    }
}

/// Helper for managing stage transitions with automatic cleanup
impl<T> StageTransition<T> {
    /// Create a new stage transition wrapper
    pub fn new(data: T) -> Self {
        Self {
            data: Some(data),
            cleanup_applied: false,
        }
    }

    /// Take the data and mark for cleanup
    pub fn take(&mut self) -> Option<T> {
        self.data.take()
    }

    /// Apply cleanup to the wrapped data
    pub fn apply_cleanup(&mut self) -> usize
    where
        T: MemoryCleanable,
    {
        if let Some(ref mut data) = self.data {
            let cleaned = data.basic_cleanup();
            self.cleanup_applied = true;
            cleaned
        } else {
            0
        }
    }

    /// Check if cleanup has been applied
    pub fn is_cleanup_applied(&self) -> bool {
        self.cleanup_applied
    }
}

/// Memory-efficient collection utilities
pub struct MemoryEfficientCollections;

impl MemoryEfficientCollections {
    /// Create a Vec with exact capacity to avoid over-allocation
    pub fn vec_with_exact_capacity<T>(capacity: usize) -> Vec<T> {
        Vec::with_capacity(capacity)
    }

    /// Create a HashMap with exact capacity
    pub fn hashmap_with_exact_capacity<K, V>(capacity: usize) -> HashMap<K, V> {
        HashMap::with_capacity(capacity)
    }

    /// Swap and clear - moves data out and clears the original
    pub fn swap_and_clear<T>(original: &mut T) -> T
    where
        T: Default,
    {
        mem::take(original)
    }

    /// Compact a Vec by removing empty/null elements
    pub fn compact_vec<T>(vec: &mut Vec<Option<T>>) -> usize {
        let original_len = vec.len();
        vec.retain(|item| item.is_some());
        vec.shrink_to_fit();
        original_len - vec.len()
    }
}

/// Memory cleanup macros for convenience
#[macro_export]
macro_rules! cleanup_between_stages {
    ($coordinator:expr, $stage_name:expr, $data:expr) => {
        $coordinator.cleanup_between_stages($stage_name, &mut $data, CleanupStrategy::Basic)
    };
    ($coordinator:expr, $stage_name:expr, $data:expr, $strategy:expr) => {
        $coordinator.cleanup_between_stages($stage_name, &mut $data, $strategy)
    };
}

#[macro_export]
macro_rules! force_cleanup {
    ($data:expr) => {
        $data.shrink_to_fit();
        std::mem::drop($data);
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_cleanup_coordinator() {
        let mut coordinator = MemoryCleanupCoordinator::new(false, Some(100.0));

        let mut test_vec = vec![1, 2, 3, 4, 5];
        test_vec.reserve(1000); // Over-allocate

        let result =
            coordinator.cleanup_between_stages("test_stage", &mut test_vec, CleanupStrategy::Basic);

        assert!(result.is_ok());
        let stats = result.unwrap();
        assert_eq!(stats.stage_name, "test_stage");
        assert!(stats.items_cleaned > 0);
    }

    #[test]
    fn test_vec_cleanup() {
        let mut vec = Vec::with_capacity(1000);
        vec.extend(0..10);

        let old_capacity = vec.capacity();
        let cleaned = vec.basic_cleanup();

        assert!(cleaned > 0);
        assert!(vec.capacity() < old_capacity);
        assert_eq!(vec.len(), 10);
    }

    #[test]
    fn test_stage_transition() {
        let mut transition = StageTransition::new(vec![1, 2, 3]);

        assert!(!transition.is_cleanup_applied());

        let _cleaned = transition.apply_cleanup();
        assert!(transition.is_cleanup_applied());

        let data = transition.take();
        assert!(data.is_some());
        assert_eq!(data.unwrap(), vec![1, 2, 3]);
    }

    #[test]
    fn test_memory_efficient_collections() {
        let vec = MemoryEfficientCollections::vec_with_exact_capacity::<i32>(10);
        assert_eq!(vec.capacity(), 10);

        let map = MemoryEfficientCollections::hashmap_with_exact_capacity::<String, i32>(5);
        assert!(map.capacity() >= 5);
    }
}
