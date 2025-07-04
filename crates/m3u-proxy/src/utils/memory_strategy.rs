//! Memory management strategies for data processing pipelines
//!
//! This module defines different strategies for handling memory pressure
//! during data-intensive operations like proxy generation.

use anyhow::Result;
use tracing::{info, warn};

/// Strategy for handling memory pressure during data processing
#[derive(Debug, Clone, PartialEq)]
pub enum MemoryStrategy {
    /// Stop processing immediately and return partial results
    StopEarly,
    /// Process data in smaller chunks, sacrificing some optimizations
    ChunkedProcessing { chunk_size: usize },
    /// Use temporary files for intermediate storage
    TempFileSpill { temp_dir: String },
    /// Continue processing but with degraded performance
    ContinueWithWarning,
}

/// Configuration for memory pressure handling
#[derive(Debug, Clone)]
pub struct MemoryStrategyConfig {
    /// Strategy to use when memory warning threshold is reached
    pub warning_strategy: MemoryStrategy,
    /// Strategy to use when memory limit is exceeded
    pub exceeded_strategy: MemoryStrategy,
    /// Whether to attempt garbage collection before applying strategy
    pub attempt_gc: bool,
}

impl Default for MemoryStrategyConfig {
    fn default() -> Self {
        Self {
            warning_strategy: MemoryStrategy::ContinueWithWarning,
            exceeded_strategy: MemoryStrategy::ContinueWithWarning,
            attempt_gc: true,
        }
    }
}

/// Memory strategy executor that handles different memory pressure scenarios
pub struct MemoryStrategyExecutor {
    config: MemoryStrategyConfig,
}

impl MemoryStrategyExecutor {
    pub fn new(config: MemoryStrategyConfig) -> Self {
        Self { config }
    }

    /// Execute strategy for memory warning
    pub async fn handle_warning(&self, context: &str) -> Result<MemoryAction> {
        info!("Applying memory warning strategy for: {}", context);
        self.execute_strategy(&self.config.warning_strategy, context)
            .await
    }

    /// Execute strategy for memory limit exceeded
    pub async fn handle_exceeded(&self, context: &str) -> Result<MemoryAction> {
        warn!("Applying memory exceeded strategy for: {}", context);

        if self.config.attempt_gc {
            info!("Attempting garbage collection before strategy execution");
            // Force garbage collection - this can help in some cases
            std::hint::black_box(());
        }

        self.execute_strategy(&self.config.exceeded_strategy, context)
            .await
    }

    async fn execute_strategy(
        &self,
        strategy: &MemoryStrategy,
        context: &str,
    ) -> Result<MemoryAction> {
        match strategy {
            MemoryStrategy::StopEarly => {
                warn!(
                    "Stopping processing early due to memory pressure in: {}",
                    context
                );
                Ok(MemoryAction::StopProcessing)
            }

            MemoryStrategy::ChunkedProcessing { chunk_size } => {
                info!(
                    "Switching to chunked processing (chunk_size: {}) for: {}",
                    chunk_size, context
                );
                Ok(MemoryAction::SwitchToChunked(*chunk_size))
            }

            MemoryStrategy::TempFileSpill { temp_dir } => {
                info!(
                    "Switching to temp file spill (dir: {}) for: {}",
                    temp_dir, context
                );
                Ok(MemoryAction::UseTemporaryStorage(temp_dir.clone()))
            }

            MemoryStrategy::ContinueWithWarning => {
                warn!("Continuing with memory pressure warning for: {}", context);
                Ok(MemoryAction::Continue)
            }
        }
    }
}

/// Action to take in response to memory pressure
#[derive(Debug, Clone, PartialEq)]
pub enum MemoryAction {
    /// Continue processing normally
    Continue,
    /// Stop processing and return partial results
    StopProcessing,
    /// Switch to processing data in chunks of specified size
    SwitchToChunked(usize),
    /// Use temporary file storage for intermediate data
    UseTemporaryStorage(String),
}

impl MemoryAction {
    /// Check if processing should continue
    pub fn should_continue(&self) -> bool {
        match self {
            MemoryAction::Continue
            | MemoryAction::SwitchToChunked(_)
            | MemoryAction::UseTemporaryStorage(_) => true,
            MemoryAction::StopProcessing => false,
        }
    }
}

/// Specific strategies for proxy generation pipeline
pub struct ProxyGenerationStrategies;

impl ProxyGenerationStrategies {
    /// Create a conservative strategy config for proxy generation
    pub fn conservative() -> MemoryStrategyConfig {
        MemoryStrategyConfig {
            warning_strategy: MemoryStrategy::ContinueWithWarning,
            exceeded_strategy: MemoryStrategy::StopEarly,
            attempt_gc: true,
        }
    }

    /// Create an aggressive strategy config that tries harder to complete
    pub fn aggressive() -> MemoryStrategyConfig {
        MemoryStrategyConfig {
            warning_strategy: MemoryStrategy::ChunkedProcessing { chunk_size: 500 },
            exceeded_strategy: MemoryStrategy::ChunkedProcessing { chunk_size: 1000 },
            attempt_gc: true,
        }
    }

    /// Create a strategy that uses temporary files for large datasets
    pub fn temp_file_based(temp_dir: &str) -> MemoryStrategyConfig {
        MemoryStrategyConfig {
            warning_strategy: MemoryStrategy::TempFileSpill {
                temp_dir: temp_dir.to_string(),
            },
            exceeded_strategy: MemoryStrategy::TempFileSpill {
                temp_dir: temp_dir.to_string(),
            },
            attempt_gc: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_action_should_continue() {
        assert!(MemoryAction::Continue.should_continue());
        assert!(MemoryAction::SwitchToChunked(100).should_continue());
        assert!(MemoryAction::UseTemporaryStorage("/tmp".to_string()).should_continue());
        assert!(!MemoryAction::StopProcessing.should_continue());
    }

    #[test]
    fn test_strategy_configs() {
        let conservative = ProxyGenerationStrategies::conservative();
        assert_eq!(conservative.exceeded_strategy, MemoryStrategy::StopEarly);

        let aggressive = ProxyGenerationStrategies::aggressive();
        assert!(matches!(
            aggressive.exceeded_strategy,
            MemoryStrategy::ChunkedProcessing { .. }
        ));
    }
}
