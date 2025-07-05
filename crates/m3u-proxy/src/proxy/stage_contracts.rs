//! Clear stage contracts with well-defined input/output types
//!
//! Each stage has specific input and output types that make strategy swapping clean and predictable.

use anyhow::Result;
use async_trait::async_trait;
use std::collections::HashMap;
use uuid::Uuid;

use crate::models::*;

/// Statistics for source loading stage
#[derive(Debug, Clone, Default)]
pub struct SourceStats {
    pub channels_loaded: usize,
    pub load_duration_ms: u64,
    pub memory_used_mb: Option<f64>,
    pub errors: Vec<String>,
}

/// Statistics for data mapping stage  
#[derive(Debug, Clone, Default)]
pub struct MappingStats {
    pub channels_processed: usize,
    pub channels_transformed: usize,
    pub transformations_applied: usize,
    pub mapping_duration_ms: u64,
    pub memory_used_mb: Option<f64>,
}

/// Statistics for filtering stage
#[derive(Debug, Clone, Default)]
pub struct FilterStats {
    pub channels_input: usize,
    pub channels_output: usize,
    pub channels_filtered_out: usize,
    pub filters_applied: Vec<String>,
    pub filter_duration_ms: u64,
    pub memory_used_mb: Option<f64>,
}

/// Statistics for channel numbering stage
#[derive(Debug, Clone, Default)]
pub struct NumberingStats {
    pub channels_numbered: usize,
    pub starting_number: i32,
    pub numbering_strategy: String,
    pub numbering_duration_ms: u64,
}

/// Statistics for M3U generation stage
#[derive(Debug, Clone, Default)]
pub struct M3uStats {
    pub channels_processed: usize,
    pub m3u_size_bytes: usize,
    pub m3u_lines: usize,
    pub generation_duration_ms: u64,
    pub memory_used_mb: Option<f64>,
}

/// Stage 1: Source Loading Input
#[derive(Debug, Clone)]
pub struct SourceLoadingInput {
    pub source_ids: Vec<Uuid>,
    pub proxy_config: ResolvedProxyConfig,
}

/// Stage 1: Source Loading Output
#[derive(Debug, Clone)]
pub struct SourceLoadingOutput {
    pub channels: Vec<Channel>,
    pub source_stats: HashMap<Uuid, SourceStats>,
    pub total_stats: SourceStats,
}

/// Stage 2: Data Mapping Input
#[derive(Debug, Clone)]
pub struct DataMappingInput {
    pub channels: Vec<Channel>,
    pub source_configs: Vec<ProxySourceConfig>,
    pub engine_config: Option<crate::config::DataMappingEngineConfig>,
    pub base_url: String,
}

/// Stage 2: Data Mapping Output
#[derive(Debug, Clone)]
pub struct DataMappingOutput {
    pub mapped_channels: Vec<Channel>,
    pub mapping_stats: MappingStats,
}

/// Stage 3: Filtering Input
#[derive(Debug, Clone)]
pub struct FilteringInput {
    pub channels: Vec<Channel>,
    pub filters: Vec<ProxyFilterConfig>,
}

/// Stage 3: Filtering Output
#[derive(Debug, Clone)]
pub struct FilteringOutput {
    pub filtered_channels: Vec<Channel>,
    pub filter_stats: FilterStats,
}

/// Stage 4: Channel Numbering Input
#[derive(Debug, Clone)]
pub struct ChannelNumberingInput {
    pub channels: Vec<Channel>,
    pub starting_number: i32,
    pub numbering_strategy: ChannelNumberingStrategy,
}

/// Channel numbering strategies
#[derive(Debug, Clone)]
pub enum ChannelNumberingStrategy {
    Sequential,
    PreserveOriginal,
    GroupBased,
    Custom(String),
}

/// Stage 4: Channel Numbering Output
#[derive(Debug, Clone)]
pub struct ChannelNumberingOutput {
    pub numbered_channels: Vec<NumberedChannel>,
    pub numbering_stats: NumberingStats,
}

/// Stage 5: M3U Generation Input
#[derive(Debug, Clone)]
pub struct M3uGenerationInput {
    pub numbered_channels: Vec<NumberedChannel>,
    pub proxy_ulid: String,
    pub base_url: String,
}

/// Stage 5: M3U Generation Output
#[derive(Debug, Clone)]
pub struct M3uGenerationOutput {
    pub m3u_content: String,
    pub m3u_stats: M3uStats,
}

/// Clear stage interfaces - each stage has ONE job with clear types
#[async_trait]
pub trait SourceLoadingStage: Send + Sync {
    async fn execute(&self, input: SourceLoadingInput) -> Result<SourceLoadingOutput>;
    fn strategy_name(&self) -> &str;
    fn estimated_memory_usage(&self, input: &SourceLoadingInput) -> Option<usize>;
    fn supports_streaming(&self) -> bool { false }
}

#[async_trait]
pub trait DataMappingStage: Send + Sync {
    async fn execute(&self, input: DataMappingInput) -> Result<DataMappingOutput>;
    fn strategy_name(&self) -> &str;
    fn estimated_memory_usage(&self, input: &DataMappingInput) -> Option<usize>;
    fn supports_streaming(&self) -> bool { false }
}

#[async_trait]
pub trait FilteringStage: Send + Sync {
    async fn execute(&self, input: FilteringInput) -> Result<FilteringOutput>;
    fn strategy_name(&self) -> &str;
    fn estimated_memory_usage(&self, input: &FilteringInput) -> Option<usize>;
    fn supports_streaming(&self) -> bool { false }
}

#[async_trait]
pub trait ChannelNumberingStage: Send + Sync {
    async fn execute(&self, input: ChannelNumberingInput) -> Result<ChannelNumberingOutput>;
    fn strategy_name(&self) -> &str;
    fn estimated_memory_usage(&self, input: &ChannelNumberingInput) -> Option<usize>;
}

#[async_trait]
pub trait M3uGenerationStage: Send + Sync {
    async fn execute(&self, input: M3uGenerationInput) -> Result<M3uGenerationOutput>;
    fn strategy_name(&self) -> &str;
    fn estimated_memory_usage(&self, input: &M3uGenerationInput) -> Option<usize>;
    fn supports_streaming(&self) -> bool { false }
}

/// Registry for stage strategies with clear separation
pub struct StageRegistry {
    source_loading: HashMap<String, Box<dyn SourceLoadingStage>>,
    data_mapping: HashMap<String, Box<dyn DataMappingStage>>,
    filtering: HashMap<String, Box<dyn FilteringStage>>,
    channel_numbering: HashMap<String, Box<dyn ChannelNumberingStage>>,
    m3u_generation: HashMap<String, Box<dyn M3uGenerationStage>>,
}

impl StageRegistry {
    pub fn new() -> Self {
        Self {
            source_loading: HashMap::new(),
            data_mapping: HashMap::new(),
            filtering: HashMap::new(),
            channel_numbering: HashMap::new(),
            m3u_generation: HashMap::new(),
        }
    }

    // Registration methods
    pub fn register_source_loading(&mut self, name: String, strategy: Box<dyn SourceLoadingStage>) {
        self.source_loading.insert(name, strategy);
    }

    pub fn register_data_mapping(&mut self, name: String, strategy: Box<dyn DataMappingStage>) {
        self.data_mapping.insert(name, strategy);
    }

    pub fn register_filtering(&mut self, name: String, strategy: Box<dyn FilteringStage>) {
        self.filtering.insert(name, strategy);
    }

    pub fn register_channel_numbering(&mut self, name: String, strategy: Box<dyn ChannelNumberingStage>) {
        self.channel_numbering.insert(name, strategy);
    }

    pub fn register_m3u_generation(&mut self, name: String, strategy: Box<dyn M3uGenerationStage>) {
        self.m3u_generation.insert(name, strategy);
    }

    // Selection methods
    pub fn get_source_loading(&self, name: &str) -> Option<&dyn SourceLoadingStage> {
        self.source_loading.get(name).map(|s| s.as_ref())
    }

    pub fn get_data_mapping(&self, name: &str) -> Option<&dyn DataMappingStage> {
        self.data_mapping.get(name).map(|s| s.as_ref())
    }

    pub fn get_filtering(&self, name: &str) -> Option<&dyn FilteringStage> {
        self.filtering.get(name).map(|s| s.as_ref())
    }

    pub fn get_channel_numbering(&self, name: &str) -> Option<&dyn ChannelNumberingStage> {
        self.channel_numbering.get(name).map(|s| s.as_ref())
    }

    pub fn get_m3u_generation(&self, name: &str) -> Option<&dyn M3uGenerationStage> {
        self.m3u_generation.get(name).map(|s| s.as_ref())
    }

    // List available strategies
    pub fn list_source_loading_strategies(&self) -> Vec<&str> {
        self.source_loading.keys().map(|s| s.as_str()).collect()
    }

    pub fn list_data_mapping_strategies(&self) -> Vec<&str> {
        self.data_mapping.keys().map(|s| s.as_str()).collect()
    }

    pub fn list_filtering_strategies(&self) -> Vec<&str> {
        self.filtering.keys().map(|s| s.as_str()).collect()
    }

    pub fn list_channel_numbering_strategies(&self) -> Vec<&str> {
        self.channel_numbering.keys().map(|s| s.as_str()).collect()
    }

    pub fn list_m3u_generation_strategies(&self) -> Vec<&str> {
        self.m3u_generation.keys().map(|s| s.as_str()).collect()
    }
}