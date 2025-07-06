# Orchestrator Data Loading Architecture

## Overview

The orchestrator implements a sophisticated buffer management system for loading and processing channel and EPG data from multiple sources in priority order. This system is designed to optimize memory usage while maintaining consistent data flow through the processing pipeline.

## Core Principles

### 1. Priority-Ordered Processing
- Sources are processed in the order they exist in the proxy configuration
- Lower `priority_order` values are processed first
- Each proxy maintains its own ordered list of stream sources and EPG sources

### 2. Active Source Filtering
- Only channels from **active** stream sources are selected and processed
- Sources with `is_active = false` are completely excluded from the pipeline
- This filtering happens at the database query level for efficiency

### 3. Rolling Buffer Management
- The system maintains a configurable buffer size (default: 1000 channels)
- As soon as buffer space becomes available, the next source begins loading
- This enables continuous data flow rather than sequential source exhaustion

### 4. Memory-Efficient Streaming
- Data is loaded in chunks rather than loading entire sources into memory
- Buffer size limits prevent memory exhaustion on large datasets
- Concurrent source loading optimizes processing time

## Data Flow Pattern

### Example Scenario
**Proxy SP1** has 2 stream sources:
- **S1**: 1500 channels (priority_order = 1, is_active = true)
- **S2**: 500 channels (priority_order = 2, is_active = true)
- **Buffer Size**: 1000 channels

### Processing Flow

1. **Initial Load**: Load first 1000 channels from S1 into buffer
2. **Processing Begins**: Pipeline starts consuming channels from buffer
3. **Buffer Trigger**: When 501 channels have been processed (499 remaining in buffer)
4. **Concurrent Loading**: Start loading channels from S2 while continuing to process S1
5. **Continuous Flow**: S2 channels flow into buffer as S1 channels are consumed
6. **Source Completion**: When S1 is exhausted, continue with remaining S2 channels

### Buffer State Transitions

```
Time T0: Buffer=[S1:1000] Processing=0
Time T1: Buffer=[S1:499] Processing=501 → Trigger S2 loading
Time T2: Buffer=[S1:400, S2:100] Processing=601
Time T3: Buffer=[S1:0, S2:500] Processing=1000
Time T4: Buffer=[S2:200] Processing=1300
Time T5: Buffer=[S2:0] Processing=1500 → All sources exhausted
```

## Implementation Components

### 1. Database Layer (`Database`)

**Active Source Filtering:**
```sql
SELECT * FROM channels 
WHERE source_id = ? AND EXISTS (
    SELECT 1 FROM stream_sources 
    WHERE id = ? AND is_active = true
)
ORDER BY channel_name 
LIMIT ? OFFSET ?
```

**Key Methods:**
- `get_channels_for_active_source_paginated()`
- `get_epg_data_for_active_source_paginated()`

### 2. Orchestrator Layer (`OrchestratorIteratorFactory`)

**Source Configuration:**
- Filters out inactive sources during iterator creation
- Pre-sorts sources by `priority_order`
- Validates source accessibility and permissions

**Iterator Types:**
- `OrderedChannelAggregateIterator`: Multi-source channel streaming
- `OrderedEpgAggregateIterator`: Multi-source EPG data streaming
- `OrderedDataMappingIterator`: Single-source mapping rules
- `OrderedFilterIterator`: Multi-source filter rules

### 3. Buffer Management (`RollingBufferIterator`)

**Buffer Logic:**
- Maintains configurable buffer size
- Triggers next source loading at configurable threshold (e.g., 50% remaining)
- Handles source exhaustion and buffer draining
- Provides backpressure for memory management

**Configuration:**
```rust
pub struct BufferConfig {
    pub buffer_size: usize,        // Default: 1000
    pub trigger_threshold: f32,    // Default: 0.5 (50%)
    pub chunk_size: usize,         // Default: 100
    pub max_concurrent_sources: usize, // Default: 2
}
```

### 4. Generic Iterator (`OrderedMultiSourceIterator`)

**Enhanced Features:**
- Rolling buffer support with size-based triggers
- Concurrent source loading within buffer constraints
- Memory pressure awareness and backpressure
- Detailed progress tracking and logging

## Memory Management

### Buffer Size Calculation
- **Default Buffer Size**: 1000 items
- **Memory Estimation**: ~500KB for 1000 channels (estimated 500 bytes per channel)
- **Configurable Limits**: Can be adjusted based on available system memory

### Backpressure Handling
- Monitor system memory usage during processing
- Reduce buffer size if memory pressure detected
- Pause source loading if buffer cannot be consumed fast enough
- Graceful degradation to sequential processing if needed

### Cleanup Strategy
- Automatic buffer clearing when sources are exhausted
- Early cleanup on iterator close/cancellation
- Memory usage logging for monitoring and debugging

## Configuration

### Proxy Source Configuration
```rust
pub struct ProxySourceConfig {
    pub source: StreamSource,      // Must have is_active = true
    pub priority_order: i32,       // Lower values processed first
}
```

### EPG Source Configuration
```rust
pub struct ProxyEpgSourceConfig {
    pub epg_source: EpgSource,     // Must have is_active = true  
    pub priority_order: i32,       // Lower values processed first
}
```

### Buffer Configuration
```rust
pub struct OrchestratorConfig {
    pub buffer_size: usize,                    // Default: 1000
    pub trigger_threshold: f32,                // Default: 0.5
    pub chunk_size: usize,                     // Default: 100
    pub max_concurrent_sources: usize,         // Default: 2
    pub memory_limit_mb: Option<usize>,        // Optional memory limit
    pub enable_backpressure: bool,             // Default: true
}
```

## Error Handling

### Source Failures
- Individual source failures don't stop the pipeline
- Failed sources are logged and skipped
- Continue processing remaining active sources
- Partial results are still valid and usable

### Memory Pressure
- Reduce buffer size dynamically
- Switch to sequential processing if needed
- Log memory pressure events for monitoring
- Graceful degradation rather than hard failures

### Database Connectivity
- Retry logic for transient database errors
- Circuit breaker pattern for persistent failures
- Fallback to cached data when available
- Clear error reporting to calling services

## Monitoring and Observability

### Key Metrics
- **Buffer Utilization**: Current buffer size vs. maximum
- **Source Processing Rate**: Channels/second per source
- **Memory Usage**: Current memory consumption by buffers
- **Processing Latency**: Time from load to consumption
- **Error Rates**: Failed database queries, source errors

### Debug Logging
- Source loading start/completion events
- Buffer state transitions and trigger points
- Memory allocation and cleanup events
- Performance metrics and timing information

### Health Checks
- Verify active sources are accessible
- Check database connectivity and performance
- Monitor memory usage trends
- Validate buffer management efficiency

## Performance Characteristics

### Expected Throughput
- **Small Sources** (< 1000 channels): ~5000 channels/second
- **Medium Sources** (1000-10000 channels): ~3000 channels/second  
- **Large Sources** (> 10000 channels): ~2000 channels/second

### Memory Usage
- **Base Memory**: ~50MB for orchestrator components
- **Buffer Memory**: ~500KB per 1000 channels in buffer
- **Peak Memory**: ~2-3x buffer size during concurrent loading

### Scalability Limits
- **Maximum Sources**: Limited by system memory and database connections
- **Maximum Channels**: No hard limit, constrained by memory and processing time
- **Concurrent Operations**: Limited by database connection pool size

## Future Enhancements

### Planned Improvements
- **Adaptive Buffer Sizing**: Dynamic buffer size based on source characteristics
- **Parallel Source Processing**: Process multiple sources simultaneously
- **Intelligent Prefetching**: Predict next sources and preload data
- **Memory Pool Management**: Reuse allocated memory across iterations

### Performance Optimizations
- **Database Query Optimization**: Optimized indexes and query patterns
- **Compression**: Compress channel data in buffers to reduce memory usage
- **Caching**: Cache frequently accessed source metadata
- **Batching**: Optimize database query batching for better throughput