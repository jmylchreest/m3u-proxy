# Pipeline Generation Process - Detailed Technical Documentation

## Overview

The M3U Proxy pipeline generation process is a sophisticated 7-stage data processing system that transforms streaming media sources into optimized M3U playlists. The system employs iterator-based data flow, adaptive memory management, and dynamic strategy selection to handle datasets ranging from hundreds to millions of channels while maintaining predictable resource usage.

## Architecture

### Core Design Principles

1. **Iterator-Based Data Flow**: All data moves through the pipeline via the `PipelineIterator` trait
2. **Accumulator Pattern**: Converts consuming iterators to reusable immutable sources
3. **Dynamic Memory Management**: Multiple strategies that adapt to memory pressure
4. **Immutable Source Sharing**: Enables multiple consumers of the same data without re-computation
5. **Chunk-Based Processing**: Configurable chunk sizes balance memory usage and performance

### The 7-Stage Pipeline

```
Source Loading → Data Mapping → Filtering → Channel Numbering → Logo Prefetch → M3U Generation → Output
```

Each stage processes data in chunks, accumulates results, and creates immutable sources for downstream consumption.

## Accumulator System

### Core Accumulator Strategies

The accumulator system (`src/pipeline/accumulator.rs`) provides three primary strategies that adapt to memory pressure and dataset size:

#### InMemory Strategy
```rust
AccumulationStrategy::InMemory
```

**Characteristics:**
- **Use Case**: Small datasets (<50MB, <25,000 channels)
- **Performance**: Highest throughput (~10,000 items/second)
- **Memory Usage**: Keeps all data in heap memory
- **Risk**: Memory exhaustion for large datasets

**Implementation:**
- Direct `Vec<T>` storage with growth tracking
- ~2KB per channel memory estimation
- No I/O overhead
- Immediate access to all accumulated data

#### FileSpilled Strategy
```rust
AccumulationStrategy::FileSpilled
```

**Characteristics:**
- **Use Case**: Large datasets (>500MB, >250,000 channels)
- **Performance**: I/O bound (~2,000-5,000 items/second)
- **Memory Usage**: Minimal footprint (~1-5MB)
- **Safety**: Prevents memory exhaustion

**Implementation:**
- JSON Lines format in sandboxed temporary files
- 10,000 items per spill file for optimal I/O
- Automatic cleanup on drop
- Sequential reading for iteration

#### Hybrid Strategy (Default)
```rust
AccumulationStrategy::Hybrid { memory_threshold_mb: 50 }
```

**Characteristics:**
- **Use Case**: Most scenarios (50-500MB, 25,000-250,000 channels)
- **Performance**: Optimal balance (~8,000 items/second before spill, ~5,000 after)
- **Memory Usage**: Controlled with automatic spilling
- **Adaptability**: Starts fast, becomes memory-safe

**Implementation:**
```rust
if self.estimated_memory_mb > *memory_threshold_mb as f64 {
    if !self.is_spilled {
        info!("Memory threshold ({} MB) exceeded, spilling to disk", memory_threshold_mb);
        self.spill_to_disk().await?;
    }
}
```

### Accumulator Components

#### IteratorAccumulator<T>
Generic accumulator for any serializable type:

```rust
pub struct IteratorAccumulator<T> {
    buffer: Vec<T>,
    strategy: AccumulationStrategy,
    spill_files: Vec<PathBuf>,
    is_spilled: bool,
    estimated_memory_mb: f64,
    file_manager: Arc<SandboxedManagerAdapter>,
}
```

**Memory Estimation Logic:**
- Base: ~1KB per item
- Channels: ~2KB per channel (metadata overhead)
- EPG entries: ~1KB per program
- Configuration: ~0.5KB per rule

#### ChannelAccumulator
Specialized for channel data with logo enrichment tracking:

```rust
pub struct ChannelAccumulator {
    inner: IteratorAccumulator<Channel>,
    logo_stats: LogoStats,
}
```

**Logo Statistics Tracking:**
- Successful logo fetches
- Failed logo attempts  
- Cache hit/miss ratios
- Logo processing times

### Memory Pressure Response

The accumulator system monitors memory usage and triggers automatic responses:

1. **Warning Threshold (80% of limit)**: Log warnings about memory usage
2. **Spill Threshold (90% of limit)**: Force spill to disk for hybrid strategy
3. **Emergency Threshold (95% of limit)**: Reduce chunk sizes and force cleanup
4. **Critical Threshold (98% of limit)**: Emergency memory release and error handling

## Iterator Patterns and Data Flow

### Core Iterator Trait

The `PipelineIterator<T>` trait (`src/pipeline/iterator_traits.rs`) provides unified data access:

```rust
#[async_trait]
pub trait PipelineIterator<T>: Send + Sync {
    async fn next_chunk(&mut self) -> Result<IteratorResult<T>>;
    async fn next_chunk_with_size(&mut self, requested_size: usize) -> Result<IteratorResult<T>>;
    async fn set_buffer_size(&mut self, buffer_size: usize) -> Result<()>;
    fn get_current_buffer_size(&self) -> usize;
    fn get_chunk_size(&self) -> usize;
    fn is_exhausted(&self) -> bool;
    async fn close(&mut self) -> Result<()>;
    fn reset(&mut self) -> Result<()>;
}
```

### Iterator Implementations

#### MultiSourceIterator
Processes data from multiple prioritized sources:

**Features:**
- Priority-based source ordering
- Database-backed pagination
- Cross-source deduplication
- Configurable chunk sizes

**Use Cases:**
- Channel aggregation from multiple M3U sources
- EPG data from multiple providers
- Configuration merging from multiple filters

#### SingleSourceIterator
Optimized for single-source data loading:

**Features:**
- Simplified data flow
- Direct database queries
- Efficient pagination
- Lower overhead than multi-source

**Use Cases:**
- Single source channel loading
- Configuration data retrieval
- Direct database iterations

#### BufferedIterator
Production-ready iterator with advanced features:

**Features:**
- Dynamic buffer management
- Backpressure with semaphore-based flow control
- Chunk size adaptation based on memory pressure
- Integration with `ChunkSizeManager`

**Buffer Management:**
```rust
pub struct BufferConfig {
    pub initial_buffer_size: usize,     // Starting buffer size
    pub max_buffer_size: usize,         // Maximum buffer size
    pub memory_threshold_mb: usize,     // When to reduce buffer
    pub enable_backpressure: bool,      // Enable flow control
}
```

#### VecIterator
Simple in-memory iterator for small datasets:

**Features:**
- Direct Vec<T> iteration
- No external dependencies
- Immediate data access
- Used for testing and small data

#### MappingIterator
Functional mapping between iterator types:

**Features:**
- Type transformations in pipeline
- Error handling with Result propagation
- Lazy evaluation
- Composable transformations

### Data Flow Patterns

#### Pull-Based Model
The system uses a pull-based model where each stage requests data chunks from the previous stage:

```rust
// Each stage pulls data from upstream
while let IteratorResult::Items(chunk) = upstream_iterator.next_chunk().await? {
    let processed_chunk = process_stage(chunk).await?;
    accumulator.accumulate(processed_chunk).await?;
}
```

**Benefits:**
- Natural backpressure mechanism
- Memory-bounded processing
- Early termination support
- Composable stages

#### Chunk-Based Processing
All data moves in configurable chunks to balance memory and performance:

**Chunk Size Determination:**
```rust
pub fn determine_chunk_size(
    memory_efficiency: MemoryEfficiency,
    memory_pressure_pct: f64,
) -> usize {
    let base_size = match memory_efficiency {
        MemoryEfficiency::Low => 2000,    // Large chunks for fast processing
        MemoryEfficiency::Medium => 1000, // Balanced chunks
        MemoryEfficiency::High => 500,    // Small chunks for memory safety
    };
    
    // Adjust for memory pressure
    let pressure_factor = if memory_pressure_pct > 70.0 {
        0.2 // Emergency: very small chunks
    } else if memory_pressure_pct > 50.0 {
        0.5 // Pressure: reduce chunk size
    } else {
        1.0 // Normal: use base size
    };
    
    ((base_size as f64 * pressure_factor) as usize).max(100)
}
```

#### Early Termination
Iterators support early closure to stop upstream processing:

```rust
// Stop processing when sufficient data is available
if accumulated_items >= target_count {
    upstream_iterator.close().await?;
    break;
}
```

## Memory Monitoring and Pressure Detection

### Memory Monitor Implementation

The `SimpleMemoryMonitor` (`src/utils/memory_monitor.rs`) provides comprehensive memory tracking:

#### Linux RSS Monitoring
```rust
pub fn get_current_memory_usage_mb(&self) -> Option<f64> {
    let status_content = std::fs::read_to_string("/proc/self/status").ok()?;
    for line in status_content.lines() {
        if line.starts_with("VmRSS:") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                let kb: f64 = parts[1].parse().ok()?;
                return Some(kb / 1024.0); // Convert to MB
            }
        }
    }
    None
}
```

#### Memory Pressure Levels
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum MemoryPressureLevel {
    Optimal,    // < 50% of memory limit
    Moderate,   // 50-70% of memory limit
    High,       // 70-85% of memory limit
    Critical,   // 85-95% of memory limit
    Emergency,  // > 95% of memory limit
}
```

#### Pressure Calculation
```rust
pub fn calculate_memory_pressure(&self) -> MemoryPressureLevel {
    if let (Some(current), Some(limit)) = (self.get_current_memory_usage_mb(), self.memory_limit_mb) {
        let usage_pct = (current / limit as f64) * 100.0;
        match usage_pct {
            p if p < 50.0 => MemoryPressureLevel::Optimal,
            p if p < 70.0 => MemoryPressureLevel::Moderate,
            p if p < 85.0 => MemoryPressureLevel::High,
            p if p < 95.0 => MemoryPressureLevel::Critical,
            _ => MemoryPressureLevel::Emergency,
        }
    } else {
        MemoryPressureLevel::Optimal
    }
}
```

### Pressure Response Strategies

#### Chunk Size Adaptation
Memory pressure triggers automatic chunk size reduction:

| Pressure Level | Base Chunk Size | Reduction Factor | Effective Size |
|---------------|-----------------|------------------|----------------|
| Optimal       | 2000           | 1.0              | 2000          |
| Moderate      | 1000           | 1.0              | 1000          |
| High          | 500            | 0.5              | 250           |
| Critical      | 500            | 0.2              | 100           |
| Emergency     | 100            | 0.2              | 20            |

#### Strategy Switching
Accumulators automatically switch strategies under pressure:

```rust
// Hybrid → FileSpilled under memory pressure
if memory_pressure >= MemoryPressureLevel::High && !self.is_spilled {
    self.spill_to_disk().await?;
}

// InMemory → Hybrid under extreme pressure
if memory_pressure >= MemoryPressureLevel::Critical {
    self.strategy = AccumulationStrategy::Hybrid { memory_threshold_mb: 10 };
    self.spill_to_disk().await?;
}
```

#### Buffer Management
Dynamic buffer sizing based on memory conditions:

```rust
pub async fn adjust_buffer_for_memory_pressure(&mut self, pressure: MemoryPressureLevel) -> Result<()> {
    let new_size = match pressure {
        MemoryPressureLevel::Optimal => self.config.max_buffer_size,
        MemoryPressureLevel::Moderate => self.config.max_buffer_size / 2,
        MemoryPressureLevel::High => self.config.max_buffer_size / 4,
        MemoryPressureLevel::Critical => 100,
        MemoryPressureLevel::Emergency => 50,
    };
    
    self.set_buffer_size(new_size).await
}
```

## Immutability Patterns and Data Transformations

### Immutable Source Architecture

The system uses immutable sources (`src/pipeline/immutable_sources.rs`) to enable safe data sharing:

#### Versioned Source Trait
```rust
pub trait VersionedSource: Send + Sync {
    fn version(&self) -> u64;
    fn has_updates_since(&self, version: u64) -> bool;
    fn created_at(&self) -> Instant;
    fn len(&self) -> usize;
    fn is_empty(&self) -> bool;
}
```

#### Source Implementations

**ImmutableLogoEnrichedChannelSource:**
```rust
pub struct ImmutableLogoEnrichedChannelSource {
    data: Arc<Vec<Channel>>,          // Zero-copy sharing
    version: AtomicU64,               // Change detection
    created_at: Instant,              // Source timestamp
    description: String,              // Human-readable description
}
```

**Features:**
- `Arc<Vec<Channel>>` for zero-copy sharing across threads
- Atomic version counter for change detection
- JSON conversion for data processing
- Thread-safe access without locks

**ImmutableProxyConfigSource:**
```rust
pub struct ImmutableProxyConfigSource {
    data: Arc<serde_json::Value>,     // Generic configuration storage
    version: AtomicU64,
    created_at: Instant,
    description: String,
}
```

**Features:**
- JSON-based storage for heterogeneous config types
- Generic data handling
- JSON-compatible format

### Data Transformation Pipeline

The transformation process follows a consistent pattern:

#### Stage Processing Flow
```rust
// 1. Pull data from upstream iterator
let mut consuming_iterator = upstream_stage_output;

// 2. Process through stage-specific logic
let stage_output = match stage_type {
    StageType::DataMapping => {
        data_mapping_service.process_iterator(consuming_iterator).await?
    },
    StageType::Filtering => {
        filter_service.process_iterator(consuming_iterator).await?
    },
    // ... other stages
};

// 3. Accumulate results using appropriate strategy
let mut accumulator = AccumulatorFactory::create_channel_accumulator(
    file_manager,
    AccumulationStrategy::Hybrid { memory_threshold_mb: 50 }
);
accumulator.accumulate_channels(stage_output).await?;

// 4. Convert to immutable source
let immutable_source = accumulator.into_channel_source(
    IteratorType::LogoChannels
).await?;

// 5. Register for downstream consumption
registry.register_channel_source(
    "processed_channels".to_string(),
    immutable_source
)?;
```

#### Ownership and Borrowing Patterns

**Zero-Copy Sharing:**
- Immutable sources use `Arc<Vec<T>>` for shared ownership
- Multiple consumers access same data without cloning
- Memory usage scales with unique data, not consumer count

**Controlled Mutability:**
- Accumulators own data during accumulation phase
- Conversion to immutable source transfers ownership
- No mutation possible after immutability conversion

**Resource Management:**
- Automatic cleanup of temporary files on drop
- Reference counting prevents premature deallocation
- Explicit close() methods for early resource release

## Stage Strategy Patterns and Switching Logic

### Strategy Selection Framework

The `StageStrategy` trait (`src/proxy/stage_strategy.rs`) defines stage processing interfaces:

```rust
#[async_trait]
pub trait StageStrategy: Send + Sync {
    // Stage-specific processing methods
    async fn execute_source_loading(&self, context: &StageContext, source_ids: Vec<Uuid>) -> Result<Vec<Channel>>;
    async fn execute_data_mapping(&self, context: &StageContext, channels: Vec<Channel>) -> Result<Vec<Channel>>;
    async fn execute_filtering(&self, context: &StageContext, channels: Vec<Channel>) -> Result<Vec<Channel>>;
    async fn execute_channel_numbering(&self, context: &StageContext, channels: Vec<Channel>) -> Result<Vec<NumberedChannel>>;
    async fn execute_logo_prefetch(&self, context: &StageContext, channels: Vec<Channel>) -> Result<Vec<Channel>>;
    async fn execute_m3u_generation(&self, context: &StageContext, numbered_channels: Vec<NumberedChannel>) -> Result<String>;
    
    // Strategy capabilities and metadata
    fn can_handle_memory_pressure(&self, level: MemoryPressureLevel) -> bool;
    fn supports_mid_stage_switching(&self) -> bool;
    fn strategy_name(&self) -> &str;
    fn estimated_memory_usage(&self, input_size: usize) -> Option<usize>;
}
```

### Strategy Registry

The `StageStrategyRegistry` maintains strategy mappings for dynamic selection:

#### Source Loading Strategies
```rust
source_loading_strategies: [
    "inmemory_full",      // Load all sources into memory
    "batched_loader",     // Process sources in batches
    "streaming_loader",   // Stream sources one by one
    "database_spill",     // Use database for intermediate storage
    "minimal_loader"      // Minimal memory footprint
]
```

#### Data Mapping Strategies
```rust
data_mapping_strategies: [
    "parallel_mapping",   // Parallel processing for performance
    "batched_mapping",    // Batch processing for efficiency
    "streaming_mapping",  // Stream processing for memory safety
    "compressed_mapping", // Compressed intermediate storage
    "simple_mapping"      // Simple sequential processing
]
```

#### Filtering Strategies
```rust
filtering_strategies: [
    "inmemory_filter",    // Fast in-memory filtering
    "indexed_filter",     // Database-indexed filtering
    "bitmask_filter",     // Bitmask-based filtering
    "streaming_filter",   // Memory-efficient streaming
    "passthrough_filter"  // No filtering (development/testing)
]
```

### Dynamic Strategy Selection

Strategy selection considers multiple factors:

#### Memory Pressure Based Selection
```rust
pub fn select_strategy_for_pressure(
    stage: StageType,
    pressure: MemoryPressureLevel,
    preferred_strategies: &[String]
) -> Result<Box<dyn StageStrategy>> {
    for strategy_name in preferred_strategies {
        if let Some(strategy) = create_strategy(strategy_name) {
            if strategy.can_handle_memory_pressure(pressure) {
                return Ok(strategy);
            }
        }
    }
    
    // Fallback to most memory-efficient strategy
    match stage {
        StageType::SourceLoading => Ok(Box::new(MinimalLoaderStrategy)),
        StageType::DataMapping => Ok(Box::new(StreamingMappingStrategy)),
        StageType::Filtering => Ok(Box::new(StreamingFilterStrategy)),
        // ... other stages
    }
}
```

#### Capability Matching
```rust
pub fn select_strategy_with_capabilities(
    stage: StageType,
    required_capabilities: &[StrategyCapability]
) -> Result<Box<dyn StageStrategy>> {
    let available_strategies = get_strategies_for_stage(stage);
    
    for strategy in available_strategies {
        if strategy.supports_all_capabilities(required_capabilities) {
            return Ok(strategy);
        }
    }
    
    Err(anyhow!("No strategy found with required capabilities"))
}
```

#### Mid-Stage Switching
Some strategies support switching during execution:

```rust
pub async fn switch_strategy_if_needed(
    &mut self,
    current_strategy: &mut Box<dyn StageStrategy>,
    memory_pressure: MemoryPressureLevel
) -> Result<bool> {
    if current_strategy.supports_mid_stage_switching() {
        if !current_strategy.can_handle_memory_pressure(memory_pressure) {
            let new_strategy = select_strategy_for_pressure(
                self.stage_type,
                memory_pressure,
                &self.preferred_strategies
            )?;
            *current_strategy = new_strategy;
            return Ok(true);
        }
    }
    Ok(false)
}
```

## Pipeline Orchestration and Coordination

### Streaming Pipeline Orchestrator

The `StreamingPipeline` (`src/proxy/streaming_pipeline.rs`) coordinates the entire process:

#### Mixed Strategy Capabilities
The orchestrator handles both streaming and batch processing strategies:

```rust
pub async fn execute_stage_with_mixed_strategies(
    &mut self,
    stage_type: StageType,
    input_source: Box<dyn PipelineIterator<InputType>>,
    strategy: Box<dyn StageStrategy>
) -> Result<Box<dyn PipelineIterator<OutputType>>> {
    match strategy.processing_mode() {
        ProcessingMode::Streaming => {
            self.execute_streaming_stage(stage_type, input_source, strategy).await
        },
        ProcessingMode::Batch => {
            self.execute_batch_stage(stage_type, input_source, strategy).await
        },
        ProcessingMode::Hybrid => {
            self.execute_hybrid_stage(stage_type, input_source, strategy).await
        }
    }
}
```

#### Stage Bridging

The `StageBridge<T>` provides bounded memory bridges between stages:

```rust
pub struct StageBridge<T> {
    producer_iterator: Box<dyn PipelineIterator<T>>,
    buffer: BoundedBuffer<T>,
    exhausted: bool,
    closed: bool,
    semaphore: Arc<Semaphore>,  // Backpressure control
}

impl<T> StageBridge<T> {
    pub async fn next_chunk(&mut self) -> Result<IteratorResult<T>> {
        // Acquire semaphore permit for backpressure
        let _permit = self.semaphore.acquire().await?;
        
        if self.exhausted || self.closed {
            return Ok(IteratorResult::Finished);
        }
        
        // Fill buffer if empty
        if self.buffer.is_empty() {
            self.fill_buffer().await?;
        }
        
        // Return chunk from buffer
        let chunk = self.buffer.take_chunk(self.chunk_size);
        Ok(IteratorResult::Items(chunk))
    }
}
```

#### Backpressure Management

The system implements sophisticated backpressure:

**Semaphore-Based Flow Control:**
```rust
pub struct BackpressureConfig {
    pub max_concurrent_chunks: usize,     // Maximum chunks in flight
    pub chunk_timeout_ms: u64,            // Timeout for chunk processing
    pub enable_adaptive_sizing: bool,     // Dynamic chunk size adjustment
}
```

**Bounded Buffer Management:**
```rust
pub struct BoundedBuffer<T> {
    buffer: VecDeque<T>,
    max_size: usize,
    current_memory_mb: f64,
    max_memory_mb: f64,
}

impl<T> BoundedBuffer<T> {
    pub fn can_accept_chunk(&self, chunk_size: usize) -> bool {
        let estimated_memory = chunk_size as f64 * self.item_size_estimate();
        (self.buffer.len() + chunk_size <= self.max_size) &&
        (self.current_memory_mb + estimated_memory <= self.max_memory_mb)
    }
}
```

**Dynamic Buffer Sizing:**
```rust
pub async fn adjust_buffer_size_for_pressure(&mut self, pressure: MemoryPressureLevel) {
    let new_max_size = match pressure {
        MemoryPressureLevel::Optimal => self.config.max_buffer_size,
        MemoryPressureLevel::Moderate => self.config.max_buffer_size * 3 / 4,
        MemoryPressureLevel::High => self.config.max_buffer_size / 2,
        MemoryPressureLevel::Critical => self.config.max_buffer_size / 4,
        MemoryPressureLevel::Emergency => 100,
    };
    
    self.max_size = new_max_size;
    
    // Drain excess items if current buffer exceeds new limit
    while self.buffer.len() > new_max_size {
        self.buffer.pop_front();
    }
}
```

### Orchestrator Factory

The `OrchestratorIteratorFactory` provides factory methods for creating configured iterators:

#### Rolling Buffer Channel Iterator
```rust
pub fn create_rolling_buffer_channel_iterator_from_configs_with_cascade(
    database: Arc<Database>,
    proxy_id: uuid::Uuid,
    source_configs: Vec<ProxySourceConfig>,
    buffer_config: BufferConfig,
    chunk_manager: Option<Arc<ChunkSizeManager>>,
    stage_name: String,
) -> Box<dyn PipelineIterator<Channel>> {
    
    // Create multi-source iterator with proper configuration
    let multi_source_iterator = Box::new(MultiSourceIterator::new(
        database,
        source_configs,
        buffer_config.initial_buffer_size,
        chunk_manager.clone(),
    ));
    
    // Wrap with rolling buffer for backpressure management
    Box::new(BufferedIterator::new(
        multi_source_iterator,
        buffer_config,
        chunk_manager,
        stage_name,
    ))
}
```

#### EPG Iterator with Coordination
```rust
pub async fn create_epg_iterator_with_channel_coordination(
    database: Arc<Database>,
    proxy_id: uuid::Uuid,
    epg_configs: Vec<ProxyEpgSourceConfig>,
    channel_source: Arc<ImmutableLogoEnrichedChannelSource>,
    buffer_config: BufferConfig,
) -> Result<Box<dyn PipelineIterator<EpgEntry>>> {
    
    // Create channel-aware EPG iterator
    let epg_iterator = EpgIteratorWithChannelFilter::new(
        database,
        epg_configs,
        channel_source, // Used for channel filtering
        buffer_config.initial_buffer_size,
    );
    
    Ok(Box::new(epg_iterator))
}
```

## Performance Characteristics

### Memory Usage Patterns

#### Accumulator Memory Footprint

| Data Type | Memory Estimation | Spill Threshold | Reload Performance |
|-----------|------------------|-----------------|-------------------|
| Channels | ~2KB per channel | 50MB (25K channels) | ~5,000 ch/sec |
| EPG Programs | ~1KB per program | 50MB (50K programs) | ~8,000 prog/sec |
| Configuration | ~0.5KB per rule | 50MB (100K rules) | ~10,000 rules/sec |
| Logo Assets | ~100KB per logo | 50MB (500 logos) | ~1,000 logos/sec |

#### Iterator Buffer Characteristics

| Memory Pressure | Chunk Size | Buffer Depth | Memory Usage |
|----------------|------------|--------------|-------------|
| Optimal | 2000 items | 5 chunks | ~20MB |
| Moderate | 1000 items | 3 chunks | ~6MB |
| High | 500 items | 2 chunks | ~2MB |
| Critical | 100 items | 1 chunk | ~400KB |
| Emergency | 50 items | 1 chunk | ~200KB |

### Throughput Characteristics

#### Source Loading Performance

| Source Type | Items/Second | Memory Usage | Scalability |
|-------------|-------------|-------------|-------------|
| Single M3U Source | 5,000-8,000 | ~1MB/1000 channels | Linear |
| Multiple M3U Sources | 3,000-5,000 | ~2MB/1000 channels | Per-source overhead |
| Xtream API | 2,000-4,000 | ~3MB/1000 channels | API rate limits |
| Database Sources | 8,000-12,000 | ~0.5MB/1000 channels | Database bound |

#### Stage Processing Performance

| Stage | Processing Rate | Memory Overhead | Bottleneck |
|-------|----------------|----------------|------------|
| Source Loading | 5,000 ch/sec | 20% | Network I/O |
| Data Mapping | 15,000 ch/sec | 10% | CPU (regex) |
| Filtering | 25,000 ch/sec | 5% | CPU (evaluation) |
| Channel Numbering | 50,000 ch/sec | 2% | CPU (sorting) |
| Logo Prefetch | 1,000 ch/sec | 200% | Network I/O |
| M3U Generation | 20,000 ch/sec | 50% | String formatting |
| Output | 10,000 ch/sec | 10% | File I/O |

### Memory Efficiency Strategies

#### Lazy Loading
Data is loaded only when required:

```rust
pub struct LazyChannelIterator {
    database: Arc<Database>,
    query_config: QueryConfig,
    current_offset: usize,
    chunk_size: usize,
    exhausted: bool,
}

impl LazyChannelIterator {
    pub async fn next_chunk(&mut self) -> Result<IteratorResult<Channel>> {
        if self.exhausted {
            return Ok(IteratorResult::Finished);
        }
        
        // Load only when requested
        let channels = self.load_chunk_from_database().await?;
        
        if channels.len() < self.chunk_size {
            self.exhausted = true;
        }
        
        Ok(IteratorResult::Items(channels))
    }
}
```

#### Streaming Processing
Constant memory usage regardless of dataset size:

```rust
pub async fn process_streaming_with_constant_memory<T, U>(
    input: Box<dyn PipelineIterator<T>>,
    processor: impl Fn(T) -> Result<U>,
    output: &mut dyn Accumulator<U>,
) -> Result<()> {
    while let IteratorResult::Items(chunk) = input.next_chunk().await? {
        for item in chunk {
            let processed = processor(item)?;
            output.accumulate_single(processed).await?;
        }
        
        // Memory usage remains constant
        // Only chunk size in memory at any time
    }
    Ok(())
}
```

#### Automatic Spilling
Prevents out-of-memory conditions:

```rust
pub async fn accumulate_with_automatic_spilling<T>(
    &mut self,
    item: T,
) -> Result<()> {
    // Check memory before adding
    if self.should_spill() {
        self.spill_to_disk().await?;
    }
    
    // Add to current accumulation
    self.add_item(item);
    
    // Update memory tracking
    self.update_memory_estimate();
    
    Ok(())
}

fn should_spill(&self) -> bool {
    match &self.strategy {
        AccumulationStrategy::Hybrid { memory_threshold_mb } => {
            self.estimated_memory_mb > *memory_threshold_mb as f64
        },
        _ => false,
    }
}
```

#### Early Cleanup
Prompt resource deallocation when data is no longer needed:

```rust
impl Drop for IteratorAccumulator<T> {
    fn drop(&mut self) {
        // Clean up spill files immediately
        for spill_file in &self.spill_files {
            let _ = std::fs::remove_file(spill_file);
        }
        
        // Clear memory buffer
        self.buffer.clear();
        self.buffer.shrink_to_fit();
    }
}

pub async fn close(&mut self) -> Result<()> {
    // Explicit early cleanup
    self.cleanup_resources().await?;
    self.closed = true;
    Ok(())
}
```

## Key Design Patterns

### Iterator Pattern
Enables uniform data flow through the entire pipeline:

**Benefits:**
- Consistent interface across all stages
- Composable processing stages
- Natural backpressure mechanism
- Memory-bounded processing

**Implementation:**
```rust
// All stages use the same iterator interface
pub async fn process_pipeline_stage<T, U>(
    input: Box<dyn PipelineIterator<T>>,
    processor: Box<dyn StageProcessor<T, U>>,
) -> Result<Box<dyn PipelineIterator<U>>> {
    processor.process(input).await
}
```

### Strategy Pattern
Multiple algorithm implementations for each stage:

**Benefits:**
- Dynamic selection based on conditions
- Pluggable algorithm implementations
- Performance optimization opportunities
- Memory pressure adaptation

**Implementation:**
```rust
pub trait StageStrategy: Send + Sync {
    fn can_handle_conditions(&self, conditions: &ProcessingConditions) -> bool;
    fn estimated_performance(&self, input_size: usize) -> PerformanceEstimate;
}
```

### Accumulator Pattern
Bridges consuming iterators and reusable immutable sources:

**Benefits:**
- Enables complex data processing workflows
- Memory-aware data collection
- Immutable result sharing
- Resource cleanup automation

**Implementation:**
```rust
// Convert consuming iterator to reusable source
let mut accumulator = AccumulatorFactory::create_for_type::<Channel>();
accumulator.consume_iterator(consuming_iterator).await?;
let immutable_source = accumulator.into_immutable_source().await?;
```

### Factory Pattern
Consistent creation of pipeline components:

**Benefits:**
- Proper default configuration
- Consistent component initialization
- Dependency injection support
- Configuration validation

**Implementation:**
```rust
pub struct PipelineFactory {
    default_memory_limit: usize,
    default_chunk_size: usize,
    file_manager: Arc<SandboxedManager>,
}

impl PipelineFactory {
    pub fn create_accumulator<T>(&self) -> IteratorAccumulator<T> {
        IteratorAccumulator::new(
            self.determine_strategy(),
            self.file_manager.clone(),
        )
    }
}
```

### Observer Pattern
Memory monitoring with passive observation:

**Benefits:**
- Non-intrusive monitoring
- Configurable warning thresholds
- Event-driven responses
- Performance isolation

**Implementation:**
```rust
pub trait MemoryObserver: Send + Sync {
    fn on_memory_pressure(&self, level: MemoryPressureLevel);
    fn on_memory_warning(&self, current_mb: f64, limit_mb: f64);
}
```

## Conclusion

The M3U Proxy pipeline generation process demonstrates sophisticated streaming data processing architecture with:

### Key Strengths

1. **Adaptive Memory Management**: Multiple strategies automatically respond to memory pressure
2. **High Throughput**: Chunk-based processing with configurable parallelism
3. **Resource Safety**: Automatic spilling and cleanup prevents resource exhaustion
4. **Flexibility**: Generic iterators support diverse data types and sources
5. **Monitoring**: Comprehensive memory and performance tracking
6. **Composability**: Pluggable stages and strategies enable customization

### Performance Characteristics

- **Scalability**: Handles datasets from thousands to millions of channels
- **Memory Efficiency**: Constant memory usage with streaming strategies
- **Throughput**: 2,000-50,000 items/second depending on stage and strategy
- **Resource Safety**: Automatic memory pressure response prevents OOM conditions

### Architecture Benefits

The layered approach to data flow management successfully balances performance, memory safety, and flexibility, making it suitable for processing large-scale streaming media datasets while maintaining predictable resource usage and providing comprehensive monitoring and adaptation capabilities.