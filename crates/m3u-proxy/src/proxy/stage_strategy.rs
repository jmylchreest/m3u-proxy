//! Stage-level strategy interface for dynamic memory management
//!
//! This module defines the interface for stage-specific processing strategies
//! that can adapt to memory pressure dynamically.

use anyhow::Result;
use async_trait::async_trait;
use std::collections::HashMap;
use uuid::Uuid;

use crate::models::*;
use crate::utils::memory_strategy::MemoryAction;

/// Memory pressure levels for fine-grained strategy selection
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum MemoryPressureLevel {
    /// < 50% memory usage - optimal performance strategies
    Optimal,
    /// 50-70% memory usage - slight optimizations  
    Moderate,
    /// 70-85% memory usage - more aggressive memory saving
    High,
    /// 85-95% memory usage - emergency memory conservation
    Critical,
    /// > 95% memory usage - last resort strategies
    Emergency,
}

/// Context information passed to stage strategies
#[derive(Debug, Clone)]
pub struct StageContext {
    pub proxy_config: ResolvedProxyConfig,
    pub output: GenerationOutput,
    pub base_url: String,
    pub engine_config: Option<crate::config::DataMappingEngineConfig>,
    pub memory_pressure: MemoryPressureLevel,
    pub available_memory_mb: Option<usize>,
    pub current_stage: String,
    pub stats: GenerationStats,
}

/// Input data for a stage
#[derive(Debug, Clone)]
pub enum StageInput {
    SourceIds(Vec<Uuid>),
    Channels(Vec<Channel>),
    MappedChannels(Vec<Channel>),
    FilteredChannels(Vec<Channel>),
    NumberedChannels(Vec<NumberedChannel>),
}

/// Output data from a stage
#[derive(Debug, Clone)]
pub enum StageOutput {
    Channels(Vec<Channel>),
    MappedChannels(Vec<Channel>),
    FilteredChannels(Vec<Channel>),
    NumberedChannels(Vec<NumberedChannel>),
    M3uContent(String),
    Generation(ProxyGeneration),
}

/// Strategy for executing a specific stage
#[async_trait]
pub trait StageStrategy: Send + Sync {
    /// Execute source loading stage
    async fn execute_source_loading(
        &self,
        context: &StageContext,
        source_ids: Vec<Uuid>,
    ) -> Result<Vec<Channel>>;

    /// Execute data mapping stage  
    async fn execute_data_mapping(
        &self,
        context: &StageContext,
        channels: Vec<Channel>,
    ) -> Result<Vec<Channel>>;

    /// Execute filtering stage
    async fn execute_filtering(
        &self,
        context: &StageContext,
        channels: Vec<Channel>,
    ) -> Result<Vec<Channel>>;

    /// Execute channel numbering stage
    async fn execute_channel_numbering(
        &self,
        context: &StageContext,
        channels: Vec<Channel>,
    ) -> Result<Vec<NumberedChannel>>;

    /// Execute M3U generation stage
    async fn execute_m3u_generation(
        &self,
        context: &StageContext,
        numbered_channels: Vec<NumberedChannel>,
    ) -> Result<String>;

    /// Check if this strategy can handle current memory pressure
    fn can_handle_memory_pressure(&self, level: MemoryPressureLevel) -> bool;

    /// Check if strategy supports switching mid-execution
    fn supports_mid_stage_switching(&self) -> bool;

    /// Get strategy name for logging/metrics
    fn strategy_name(&self) -> &str;

    /// Get estimated memory usage for this strategy
    fn estimated_memory_usage(&self, input_size: usize) -> Option<usize>;
}

/// Registry of available strategies for each stage
pub struct StageStrategyRegistry {
    source_loading_strategies: HashMap<String, Box<dyn StageStrategy>>,
    data_mapping_strategies: HashMap<String, Box<dyn StageStrategy>>,
    filtering_strategies: HashMap<String, Box<dyn StageStrategy>>,
    channel_numbering_strategies: HashMap<String, Box<dyn StageStrategy>>,
    m3u_generation_strategies: HashMap<String, Box<dyn StageStrategy>>,
}

impl StageStrategyRegistry {
    pub fn new() -> Self {
        Self {
            source_loading_strategies: HashMap::new(),
            data_mapping_strategies: HashMap::new(),
            filtering_strategies: HashMap::new(),
            channel_numbering_strategies: HashMap::new(),
            m3u_generation_strategies: HashMap::new(),
        }
    }

    pub fn register_source_loading_strategy(&mut self, name: String, strategy: Box<dyn StageStrategy>) {
        self.source_loading_strategies.insert(name, strategy);
    }

    pub fn register_data_mapping_strategy(&mut self, name: String, strategy: Box<dyn StageStrategy>) {
        self.data_mapping_strategies.insert(name, strategy);
    }

    pub fn register_filtering_strategy(&mut self, name: String, strategy: Box<dyn StageStrategy>) {
        self.filtering_strategies.insert(name, strategy);
    }

    pub fn register_channel_numbering_strategy(&mut self, name: String, strategy: Box<dyn StageStrategy>) {
        self.channel_numbering_strategies.insert(name, strategy);
    }

    pub fn register_m3u_generation_strategy(&mut self, name: String, strategy: Box<dyn StageStrategy>) {
        self.m3u_generation_strategies.insert(name, strategy);
    }

    /// Select best strategy for a stage given memory pressure
    pub fn select_strategy_for_stage(
        &self,
        stage: &str,
        memory_pressure: MemoryPressureLevel,
        preferred_strategies: &[String],
    ) -> Option<&dyn StageStrategy> {
        let strategies = match stage {
            "source_loading" => &self.source_loading_strategies,
            "data_mapping" => &self.data_mapping_strategies,
            "filtering" => &self.filtering_strategies,
            "channel_numbering" => &self.channel_numbering_strategies,
            "m3u_generation" => &self.m3u_generation_strategies,
            _ => return None,
        };

        // Try preferred strategies first
        for strategy_name in preferred_strategies {
            if let Some(strategy) = strategies.get(strategy_name) {
                if strategy.can_handle_memory_pressure(memory_pressure) {
                    return Some(strategy.as_ref());
                }
            }
        }

        // Fallback: find any strategy that can handle the pressure
        for strategy in strategies.values() {
            if strategy.can_handle_memory_pressure(memory_pressure) {
                return Some(strategy.as_ref());
            }
        }

        None
    }
}

/// Dynamic strategy selector that adapts to memory conditions
pub struct DynamicStrategySelector {
    registry: StageStrategyRegistry,
    stage_preferences: HashMap<String, Vec<String>>,
    memory_thresholds: MemoryThresholds,
}

#[derive(Debug, Clone)]
pub struct MemoryThresholds {
    pub optimal_mb: usize,
    pub moderate_mb: usize, 
    pub high_mb: usize,
    pub critical_mb: usize,
}

impl Default for MemoryThresholds {
    fn default() -> Self {
        Self {
            optimal_mb: 256,   // < 256MB = optimal
            moderate_mb: 384,  // < 384MB = moderate
            high_mb: 448,      // < 448MB = high  
            critical_mb: 486,  // < 486MB = critical, >486MB = emergency
        }
    }
}

impl DynamicStrategySelector {
    pub fn new(registry: StageStrategyRegistry) -> Self {
        let mut stage_preferences = HashMap::new();
        
        // Default strategy preferences (best to worst for each stage)
        stage_preferences.insert("source_loading".to_string(), vec![
            "inmemory_full".to_string(),
            "batched_loader".to_string(),
            "streaming_loader".to_string(),
            "database_spill".to_string(),
            "minimal_loader".to_string(),
        ]);

        stage_preferences.insert("data_mapping".to_string(), vec![
            "parallel_mapping".to_string(),
            "batched_mapping".to_string(),
            "streaming_mapping".to_string(),
            "compressed_mapping".to_string(),
            "simple_mapping".to_string(),
        ]);

        stage_preferences.insert("filtering".to_string(), vec![
            "inmemory_filter".to_string(),
            "indexed_filter".to_string(),
            "bitmask_filter".to_string(),
            "streaming_filter".to_string(),
            "passthrough_filter".to_string(),
        ]);

        stage_preferences.insert("channel_numbering".to_string(), vec![
            "inmemory_numbering".to_string(),
            "streaming_numbering".to_string(),
        ]);

        stage_preferences.insert("m3u_generation".to_string(), vec![
            "inmemory_m3u".to_string(),
            "streaming_m3u".to_string(),
            "chunked_m3u".to_string(),
        ]);

        Self {
            registry,
            stage_preferences,
            memory_thresholds: MemoryThresholds::default(),
        }
    }

    /// Determine memory pressure level from current memory usage
    pub fn assess_memory_pressure(&self, current_memory_mb: usize) -> MemoryPressureLevel {
        if current_memory_mb < self.memory_thresholds.optimal_mb {
            MemoryPressureLevel::Optimal
        } else if current_memory_mb < self.memory_thresholds.moderate_mb {
            MemoryPressureLevel::Moderate
        } else if current_memory_mb < self.memory_thresholds.high_mb {
            MemoryPressureLevel::High
        } else if current_memory_mb < self.memory_thresholds.critical_mb {
            MemoryPressureLevel::Critical
        } else {
            MemoryPressureLevel::Emergency
        }
    }

    /// Select optimal strategy for a stage
    pub fn select_strategy(&self, stage: &str, memory_pressure: MemoryPressureLevel) -> Option<&dyn StageStrategy> {
        let preferences = self.stage_preferences.get(stage)?;
        self.registry.select_strategy_for_stage(stage, memory_pressure, preferences)
    }

    /// Update strategy preferences for a stage
    pub fn set_stage_preferences(&mut self, stage: String, preferences: Vec<String>) {
        self.stage_preferences.insert(stage, preferences);
    }
}

/// Convert memory action to memory pressure level for strategy selection
impl From<MemoryAction> for MemoryPressureLevel {
    fn from(action: MemoryAction) -> Self {
        match action {
            MemoryAction::Continue => MemoryPressureLevel::Optimal,
            MemoryAction::SwitchToChunked(_) => MemoryPressureLevel::High,
            MemoryAction::UseTemporaryStorage(_) => MemoryPressureLevel::Critical,
            MemoryAction::StopProcessing => MemoryPressureLevel::Emergency,
        }
    }
}