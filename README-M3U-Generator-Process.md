# M3U Generator Process - Detailed Technical Documentation

## Overview

The M3U generation process is the final stage of the 7-stage proxy pipeline, responsible for converting numbered channels into the standard M3U playlist format. This stage takes numbered channels as input and produces a complete M3U playlist file that clients can use to access the proxied streams.

## Architecture

### Core Components

1. **M3U Generation Stage** (`src/proxy/strategies/m3u_generation.rs`)
   - Transforms numbered channels into M3U format
   - Supports multiple generation strategies for different memory conditions
   - Generates proxy URLs for each stream

2. **Format Generation Logic**
   - Creates EXTINF entries with channel metadata
   - Constructs proxy stream URLs using the proxy ULID
   - Handles optional fields (tvg-id, tvg-name, tvg-logo, etc.)

3. **Strategy Implementations**
   - **InMemoryM3uStrategy**: Fast in-memory generation for optimal conditions
   - **StreamingM3uStrategy**: Memory-efficient streaming approach for high memory pressure

## M3U Generation Process Flow

### Stage 6: M3U Generation

```
Numbered Channels → M3U Generation → M3U Playlist String
```

#### Input Processing
The M3U generation stage receives `NumberedChannel` objects containing:
- Channel metadata (name, group, logos, etc.)
- Assigned channel numbers
- Stream URLs and identifiers
- TVG (TV Guide) information

#### Format Generation Logic

1. **M3U Header Creation**
   ```
   #EXTM3U
   ```

2. **Channel Entry Generation**
   For each numbered channel:
   ```
   #EXTINF:-1 tvg-id="channel_id" tvg-name="Channel Name" tvg-logo="logo_url" tvg-chno="123" group-title="Group",Channel Display Name
   http://proxy-base-url/stream/proxy_ulid/channel_id
   ```

3. **Proxy URL Construction**
   - Base URL from configuration
   - Proxy ULID for identification
   - Channel ID for stream routing
   - Format: `{base_url}/stream/{proxy_ulid}/{channel_id}`

#### Field Mapping

The M3U generation maps channel data to M3U attributes:

- **tvg-id**: Uses `channel.tvg_id` or empty string
- **tvg-name**: Uses `channel.tvg_name` or empty string  
- **tvg-logo**: Uses `channel.tvg_logo` or empty string
- **tvg-chno**: Uses `assigned_number` from numbering stage
- **group-title**: Uses `channel.group_title` or empty string
- **Channel Name**: Uses `channel.channel_name`

## Generation Strategies

### InMemoryM3uStrategy

**Characteristics:**
- Processes all channels in memory simultaneously
- Fastest generation approach
- Suitable for optimal and moderate memory conditions
- Estimated memory usage: `input_size * 2048 bytes`

**Process:**
1. Creates M3U header
2. Iterates through all numbered channels
3. Generates EXTINF and URL lines for each channel
4. Returns complete M3U string

**Memory Pressure Handling:**
- Supports: Optimal, Moderate, High
- Does not support: Critical
- No mid-stage switching capability

### StreamingM3uStrategy

**Characteristics:**
- Memory-efficient streaming approach
- Processes channels in smaller chunks
- Yields control periodically to prevent blocking
- Suitable for all memory pressure levels
- Estimated memory usage: ~100KB buffer

**Process:**
1. Creates M3U header
2. Processes channels one by one
3. Yields control every 100 channels (`tokio::task::yield_now()`)
4. Builds M3U string incrementally

**Memory Pressure Handling:**
- Supports: All memory pressure levels
- Supports mid-stage switching
- Optimized for memory-constrained environments

## Performance Characteristics

### Memory Usage

| Strategy | Memory Usage | Scalability | Best Use Case |
|----------|-------------|-------------|---------------|
| InMemory | ~2KB per channel | Up to ~50,000 channels | Fast generation, sufficient memory |
| Streaming | ~100KB fixed | Unlimited | Memory-constrained, large playlists |

### Generation Speed

1. **InMemory Strategy**: Fastest - single pass through channels
2. **Streaming Strategy**: Moderate - includes yield points for responsiveness

### Scalability Limits

- **Channel Count**: Streaming strategy has no practical limit
- **Memory Constraints**: Streaming strategy adapts to available memory
- **Generation Time**: Linear with channel count for both strategies

## Error Handling

### Channel Processing Errors

The M3U generation stage handles various error conditions:

1. **Missing Channel Data**
   - Uses empty strings for missing TVG fields
   - Continues processing remaining channels
   - Logs warnings for incomplete data

2. **URL Construction Errors**
   - Validates base URL format
   - Handles missing proxy ULID
   - Falls back to basic URL structure

3. **Memory Pressure Responses**
   - InMemory strategy switches to Streaming under high pressure
   - Streaming strategy maintains operation under all conditions
   - Automatic strategy selection based on memory availability

### Fallback Mechanisms

1. **Strategy Fallback**
   ```rust
   // Automatic fallback from InMemory to Streaming
   if memory_pressure >= MemoryPressureLevel::Critical {
       // Switch to StreamingM3uStrategy
   }
   ```

2. **Field Fallbacks**
   - Missing `tvg_id` → empty string
   - Missing `tvg_name` → empty string
   - Missing `tvg_logo` → empty string
   - Missing `group_title` → empty string

## Integration with Pipeline

### Stage Dependencies

The M3U generation stage depends on:

1. **Channel Numbering Stage** (Stage 5)
   - Provides numbered channels with assigned channel numbers
   - Ensures channels are properly ordered and numbered

2. **Logo Prefetch Stage** (Stage 4)
   - Provides resolved logo URLs in `tvg_logo` field
   - Ensures logos are cached and accessible

3. **Filtering Stage** (Stage 3)
   - Provides filtered channel set
   - Ensures only desired channels are included

### Output Integration

The M3U generation stage produces:

1. **M3U Playlist String**
   - Complete M3U formatted content
   - Ready for client consumption
   - Includes all channel metadata and proxy URLs

2. **Performance Metrics**
   - Generation time statistics
   - Memory usage tracking
   - Channel count and processing rate

## Configuration

### Strategy Selection

M3U generation strategy is selected automatically based on:

1. **Memory Pressure Level**
   - Optimal/Moderate → InMemoryM3uStrategy
   - High/Critical → StreamingM3uStrategy

2. **Channel Count**
   - Large playlists automatically use streaming approach
   - Small playlists prefer in-memory generation

### Tuning Parameters

Key configuration options:

```toml
[proxy_generation]
# Enable memory tracking for strategy selection
enable_memory_tracking = true

# Memory limit affects strategy choice
[proxy_generation.memory]
max_memory_mb = 512
strategy_preset = "conservative"

# Base URL for proxy stream generation
[web]
base_url = "http://localhost:8080"
```

## Debugging and Troubleshooting

### Common Issues

1. **Missing Proxy URLs**
   - Check base URL configuration
   - Verify proxy ULID is set correctly
   - Ensure channel IDs are valid UUIDs

2. **Incomplete Channel Metadata**
   - Review data mapping stage output
   - Check TVG field population
   - Verify logo prefetch completion

3. **Memory Issues**
   - Monitor memory usage during generation
   - Consider using streaming strategy
   - Reduce channel count or batch size

### Debug Logging

Enable detailed M3U generation logging:

```rust
tracing::debug!(
    "M3U Generation - Channel #{}: id={}, channel_name='{}', tvg_name='{:?}', stream_url='{}'",
    nc.assigned_number, nc.channel.id, nc.channel.channel_name, nc.channel.tvg_name, nc.channel.stream_url
);

tracing::debug!("M3U Generation - Generated EXTINF: '{}'", extinf);
```

### Performance Monitoring

Key metrics to monitor:

1. **Generation Time**: Time to process all channels
2. **Memory Usage**: Peak memory during generation
3. **Channel Rate**: Channels processed per second
4. **Strategy Switches**: Frequency of strategy changes

## Example M3U Output

```m3u
#EXTM3U
#EXTINF:-1 tvg-id="example-news-hd" tvg-name="Example News HD" tvg-logo="http://localhost:8080/logos/example-news.png" tvg-chno="1" group-title="News",Example News HD
http://localhost:8080/stream/01ARZ3NDEKTSV4RRFFQ69G5FAV/550e8400-e29b-41d4-a716-446655440001
#EXTINF:-1 tvg-id="sports-channel" tvg-name="Sports Channel" tvg-logo="http://localhost:8080/logos/sports.jpg" tvg-chno="2" group-title="Sports",Sports Channel
http://localhost:8080/stream/01ARZ3NDEKTSV4RRFFQ69G5FAV/550e8400-e29b-41d4-a716-446655440002
```

## Advanced Features

### Batch Processing

For large playlists, the system supports:

1. **Chunked Generation**: Process channels in configurable batches
2. **Progress Reporting**: Track generation progress for large playlists
3. **Incremental Updates**: Update only changed channels when possible

### Custom Metadata

The M3U generation supports additional metadata:

1. **Extended EXTINF Attributes**: Custom TVG fields and attributes
2. **Playlist Comments**: Additional information in M3U comments
3. **Group Organization**: Hierarchical channel grouping

### Performance Optimizations

1. **String Allocation**: Pre-allocate strings based on channel count
2. **Template Caching**: Cache EXTINF templates for repeated patterns
3. **Parallel Processing**: Generate entries in parallel where safe

This comprehensive M3U generation process ensures reliable, efficient, and scalable playlist creation that integrates seamlessly with the overall proxy pipeline architecture.