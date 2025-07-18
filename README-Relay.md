# Relay System Architecture

The M3U Proxy relay system implements a sophisticated multi-client streaming architecture that provides high-performance video streaming with automatic failover capabilities using a cyclic buffer as a universal content multiplexer.

## Table of Contents

1. [Overview](#overview)
2. [Architecture](#architecture)
3. [Installation & Setup](#installation--setup)
4. [Configuration](#configuration)
5. [API Reference](#api-reference)
6. [Web Interface](#web-interface)
7. [Profiles](#profiles)
8. [Management](#management)
9. [Monitoring](#monitoring)
10. [Troubleshooting](#troubleshooting)
11. [Performance Tuning](#performance-tuning)

## Overview

### What is the Relay System?

The Relay System uses FFmpeg processes to create optimized relay streams from original source transport stream (TS) URLs. Each relay creates a direct connection to the source and maintains an in-memory buffer to efficiently serve multiple internal m3u-proxy clients.

### Key Benefits

- **Reduced Bandwidth**: One connection to source serves multiple clients
- **Hardware Acceleration**: Optional GPU-accelerated transcoding
- **Buffer Management**: In-memory buffering for smooth playback
- **Format Optimization**: Transcode streams for better compatibility
- **Connection Pooling**: Efficient resource utilization
- **On-Demand Processing**: Processes only spawn when clients connect

### How it Works

1. **Relay Profiles** define FFmpeg parameters and transcoding settings
2. **Stream Proxies** are assigned relay profiles for enhanced delivery
3. **FFmpeg Process** connects to the original TS URL and maintains the stream
4. **Multiple Clients** connect to the relay instead of the original source
5. **Real-time Statistics** track performance and connection health

## Architecture Overview

The relay system uses a **cyclic buffer** as a universal content multiplexer, allowing multiple HTTP clients to consume the same stream efficiently while supporting dynamic source switching and automatic failover.

```
Input Sources          Cyclic Buffer         Output Clients
┌─────────────────┐    ┌─────────────┐      ┌─────────────┐
│ FFmpeg Process  │──▶ │             │ ──▶  │ HTTP Client │
│ Image Loop      │──▶ │   Cyclic    │ ──▶  │ HTTP Client │
│ Test Pattern    │──▶ │   Buffer    │ ──▶  │ HTTP Client │
│ Static Message  │──▶ │             │ ──▶  │ HTTP Client │
│ Error Fallback  │──▶ │             │ ──▶  │ HTTP Client │
└─────────────────┘    └─────────────┘      └─────────────┘
```

### Key Components

1. **Cyclic Buffer**: Multi-client streaming buffer with automatic memory management
2. **FFmpeg Process Wrapper**: Hardware-accelerated transcoding with process monitoring
3. **Source Abstraction**: Support for multiple input types (streams, images, test patterns)
4. **Relay Manager**: Process lifecycle management and health monitoring
5. **Error Fallback System**: Automatic generation of error images with embedded failure information
6. **Sandboxed File Manager**: Secure temporary file handling

### How It Works

#### Data Flow
1. **Input Source**: FFmpeg process or virtual generator produces Transport Stream data
2. **Single Reader**: One tokio task reads from the source (FFmpeg stdout or generator)
3. **Buffer Storage**: Data is stored in the cyclic buffer with sequence numbers and timestamps
4. **Multiple Clients**: Each HTTP client reads from the buffer independently, maintaining their own position

#### Multi-Client Streaming
The system solves the fundamental limitation that **multiple clients cannot read from the same pipe/stdout** by using a buffer-based approach:

```rust
// Single reader from FFmpeg stdout
tokio::spawn(async move {
    let mut reader = tokio::io::BufReader::new(stdout);
    loop {
        match reader.read(&mut buffer_bytes).await {
            Ok(n) => {
                let chunk = bytes::Bytes::copy_from_slice(&buffer_bytes[..n]);
                cyclic_buffer.write_chunk(chunk).await?;
            }
        }
    }
});

// Multiple clients read from buffer
async fn serve_client(client_info: &ClientInfo) -> Result<RelayContent, RelayError> {
    let client = cyclic_buffer.add_client(
        client_info.user_agent.clone(),
        Some(client_info.ip.clone())
    ).await;
    
    let chunks = cyclic_buffer.read_chunks_for_client(&client).await;
    Ok(RelayContent::Segment(combined_data))
}
```

### Client Independence
Each client maintains:
- **Last sequence number**: Tracks their position in the stream
- **Read timestamps**: For stale client detection
- **Bytes read counter**: For analytics and monitoring
- **Connection metadata**: User agent, IP address, connection time

## Automatic Failover System

### Upstream Failure Detection
The system continuously monitors upstream connection health and automatically switches to error fallback content when failures occur.

### Error Fallback Generation
When upstream failures occur, the system automatically generates error images with embedded failure information:

1. **Error Detection**: FFmpeg process exits or stream becomes unavailable
2. **Image Generation**: Creates a Transport Stream compatible error image
3. **Seamless Transition**: Clients continue receiving data without interruption
4. **Error Information**: Displays specific error details and timestamps

### Fallback Content Types
- **Connection Error**: "Unable to connect to upstream source"
- **Stream Timeout**: "Stream timeout after 30 seconds"
- **Authentication Error**: "Authentication failed for upstream source"
- **Format Error**: "Unsupported stream format"
- **Generic Error**: Custom error messages with technical details

## Use Cases and Ideas

### Live Streaming
- **Multiple viewers**: Single source stream to unlimited clients
- **Dynamic joining**: Clients can connect/disconnect without affecting others
- **Quality consistency**: All clients receive the same quality stream
- **Bandwidth efficiency**: One upstream connection serves many clients

### Failover Broadcasting
- **Primary/backup**: Automatic switchover to backup sources
- **Error notification**: Visual error messages when primary source fails
- **Maintenance mode**: Scheduled content during maintenance windows
- **Service continuity**: Seamless transitions between sources

### Virtual Channels
- **Channel branding**: Show logo/branding when primary stream is offline
- **Image loops**: Static images converted to video streams for placeholder content
- **Test patterns**: Generate color bars, test signals, or calibration patterns
- **Emergency broadcasting**: Override with emergency messages or alerts
- **Maintenance notifications**: Display "Service Under Maintenance" messages

### Content Management
- **Dynamic switching**: Change content sources without client disconnection
- **Scheduled content**: Time-based content switching (e.g., show logos during breaks)
- **Multi-source fallback**: Chain multiple backup sources for reliability
- **Content injection**: Insert announcements or advertisements

### Technical Applications
- **Stream transcoding**: Real-time format conversion and optimization
- **Quality adaptation**: Multiple quality levels from single source
- **Protocol bridging**: Convert between different streaming protocols
- **Latency optimization**: Buffer management for low-latency streaming

### Monitoring and Analytics
- **Real-time metrics**: Track viewer count, bandwidth usage, connection patterns
- **Health monitoring**: Process status, error rates, resource utilization
- **Performance analytics**: Latency measurements, throughput statistics
- **Client tracking**: Connection duration, geographic distribution

### Development and Testing
- **Mock streams**: Generate test content for development environments
- **Load testing**: Simulate high client loads for performance testing
- **Protocol testing**: Test different streaming formats and configurations
- **Error simulation**: Test error handling and recovery mechanisms

## Installation & Setup

### Prerequisites

- FFmpeg installed on the system
- Sufficient disk space for temp files
- Optional: GPU drivers for hardware acceleration

### Database Schema

The relay system uses the following database tables:

```sql
-- Relay profiles (reusable FFmpeg configurations)
CREATE TABLE relay_profiles (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    description TEXT,
    ffmpeg_args TEXT NOT NULL,
    output_format TEXT NOT NULL DEFAULT 'transport_stream',
    segment_duration INTEGER,
    max_segments INTEGER,
    input_timeout INTEGER NOT NULL DEFAULT 30,
    hardware_acceleration TEXT,
    is_system_default BOOLEAN NOT NULL DEFAULT FALSE,
    is_active BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

-- Channel relay configurations
CREATE TABLE channel_relay_configs (
    id TEXT PRIMARY KEY,
    proxy_id TEXT NOT NULL,
    channel_id TEXT NOT NULL,
    profile_id TEXT NOT NULL,
    name TEXT NOT NULL,
    description TEXT,
    custom_args TEXT,
    is_active BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE(proxy_id, channel_id)
);

-- Runtime status tracking
CREATE TABLE relay_runtime_status (
    channel_relay_config_id TEXT PRIMARY KEY,
    process_id TEXT,
    sandbox_path TEXT NOT NULL,
    is_running BOOLEAN NOT NULL DEFAULT FALSE,
    started_at TEXT,
    client_count INTEGER NOT NULL DEFAULT 0,
    bytes_served INTEGER NOT NULL DEFAULT 0,
    error_message TEXT,
    last_heartbeat TEXT,
    updated_at TEXT NOT NULL
);

-- Event logging
CREATE TABLE relay_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    config_id TEXT NOT NULL,
    event_type TEXT NOT NULL,
    details TEXT,
    timestamp TEXT NOT NULL,
    created_at TEXT NOT NULL
);
```

### Configuration

Add the following to your `config.toml`:

```toml
[relay]
enabled = true
max_concurrent_processes = 10
cleanup_interval_seconds = 30
default_timeout_seconds = 300
temp_directory = "temp"
```

## Configuration

### Relay Profiles

Relay profiles define how FFmpeg processes streams. Each profile contains:

- **Name**: Human-readable identifier
- **Description**: Optional description
- **FFmpeg Args**: Array of FFmpeg command arguments
- **Output Format**: `transport_stream`, `hls`, `dash`, or `copy`
- **Segment Duration**: For HLS/DASH (seconds)
- **Max Segments**: Buffer size for circular buffer
- **Input Timeout**: Connection timeout (seconds)
- **Hardware Acceleration**: GPU acceleration settings

### Template Variables

FFmpeg arguments support template variables:

- `{input_url}`: Original stream URL
- `{output_path}`: Sandbox output directory
- `{segment_duration}`: From profile configuration
- `{max_segments}`: From profile configuration

### Example Profiles

#### Transport Stream Passthrough
```json
{
  "name": "Transport Stream Passthrough",
  "description": "Direct copy to transport stream",
  "ffmpeg_args": ["-i", "{input_url}", "-c", "copy", "-f", "mpegts", "-y", "{output_path}/stream.ts"],
  "output_format": "transport_stream",
  "max_segments": 1,
  "input_timeout": 30
}
```

#### HLS Conversion
```json
{
  "name": "HLS Conversion",
  "description": "Convert to HLS format with 6-second segments",
  "ffmpeg_args": [
    "-i", "{input_url}",
    "-c", "copy",
    "-f", "hls",
    "-hls_time", "{segment_duration}",
    "-hls_list_size", "{max_segments}",
    "-hls_flags", "delete_segments",
    "-hls_segment_filename", "{output_path}/segment_%03d.ts",
    "-y", "{output_path}/playlist.m3u8"
  ],
  "output_format": "hls",
  "segment_duration": 6,
  "max_segments": 10,
  "input_timeout": 30
}
```

#### Hardware Acceleration (NVIDIA)
```json
{
  "name": "NVIDIA Hardware Acceleration",
  "description": "GPU-accelerated encoding using NVENC",
  "ffmpeg_args": [
    "-hwaccel", "cuda",
    "-i", "{input_url}",
    "-c:v", "h264_nvenc",
    "-c:a", "copy",
    "-preset", "fast",
    "-b:v", "2M",
    "-f", "mpegts",
    "-y", "{output_path}/stream.ts"
  ],
  "output_format": "transport_stream",
  "hardware_acceleration": "cuda",
  "input_timeout": 30
}
```

## API Reference

### Relay Profiles

#### List Profiles
```http
GET /api/v1/relay/profiles
```

#### Create Profile
```http
POST /api/v1/relay/profiles
Content-Type: application/json

{
  "name": "Custom Profile",
  "description": "Custom FFmpeg profile",
  "ffmpeg_args": ["-i", "{input_url}", "-c", "copy", "-f", "mpegts", "-y", "{output_path}/stream.ts"],
  "output_format": "transport_stream",
  "segment_duration": 6,
  "max_segments": 10,
  "input_timeout": 30,
  "hardware_acceleration": null,
  "is_system_default": false,
  "is_active": true
}
```

#### Get Profile
```http
GET /api/v1/relay/profiles/{profile_id}
```

#### Update Profile
```http
PUT /api/v1/relay/profiles/{profile_id}
Content-Type: application/json

{
  "name": "Updated Profile Name",
  "description": "Updated description"
}
```

#### Delete Profile
```http
DELETE /api/v1/relay/profiles/{profile_id}
```

### Channel Relay Configuration

#### Get Channel Relay Config
```http
GET /api/v1/proxies/{proxy_id}/channels/{channel_id}/relay
```

#### Create Channel Relay Config
```http
POST /api/v1/proxies/{proxy_id}/channels/{channel_id}/relay
Content-Type: application/json

{
  "profile_id": "profile-uuid",
  "name": "Channel Relay Config",
  "description": "Relay config for specific channel",
  "custom_args": null,
  "is_active": true
}
```

#### Delete Channel Relay Config
```http
DELETE /api/v1/proxies/{proxy_id}/channels/{channel_id}/relay
```

### Relay Control

#### Get Relay Status
```http
GET /api/v1/relay/{config_id}/status
```

#### Start Relay
```http
POST /api/v1/relay/{config_id}/start
```

#### Stop Relay
```http
POST /api/v1/relay/{config_id}/stop
```

### Monitoring

#### Get Relay Metrics
```http
GET /api/v1/relay/metrics
```

#### Get Relay Health
```http
GET /api/v1/relay/health
```

#### Get Health for Specific Config
```http
GET /api/v1/relay/health/{config_id}
```

### Content Serving

#### HLS Playlist
```http
GET /api/v1/relay/{config_id}/playlist.m3u8
```

#### HLS Segment
```http
GET /api/v1/relay/{config_id}/segments/{segment_name}
```

#### Generic HLS Content
```http
GET /api/v1/relay/{config_id}/hls/{path}
```

## Web Interface

### Accessing the Interface

Navigate to `/relay` in your m3u-proxy web interface to access the relay management dashboard.

### Features

- **Active Relays**: View currently running relay processes
- **Relay Profiles**: Create, edit, and manage relay profiles
- **Proxy Assignments**: Assign relay profiles to stream proxies
- **System Resources**: Monitor CPU, memory, and bandwidth usage
- **Real-time Updates**: Dashboard updates every 5 seconds

### Creating Profiles

1. Click "Create Profile" in the Relay Profiles section
2. Fill in the profile name and description
3. Select output format (Transport Stream or HLS)
4. Enter FFmpeg parameters using template variables
5. Configure segment duration and max segments
6. Enable hardware acceleration if desired
7. Click "Create Profile"

### Managing Assignments

1. Click "Assign Relay to Proxy" in the Proxy Relay Assignments section
2. Select a stream proxy from the dropdown
3. Select a relay profile to assign
4. Provide an assignment name and description
5. Click "Assign Relay"

## Profiles

### System Default Profiles

The system includes several default profiles:

1. **Transport Stream Passthrough**: Direct copy without transcoding
2. **HLS Conversion**: Convert to HLS with configurable segments
3. **NVIDIA Hardware Acceleration**: GPU-accelerated encoding
4. **Intel/AMD Hardware Acceleration**: VAAPI-based encoding
5. **H.264 Transcode**: Software transcoding to H.264
6. **Low Latency HLS**: HLS with 2-second segments

### Custom Profiles

You can create custom profiles for specific use cases:

- **Quality Transcoding**: Adjust bitrate, resolution, codecs
- **Hardware-Specific**: Optimize for specific GPU models
- **Format Conversion**: Convert between different container formats
- **Streaming Optimization**: Optimize for different streaming scenarios

### Profile Validation

All profiles undergo validation:

- **FFmpeg Arguments**: Checked for security and validity
- **Template Variables**: Verified for proper usage
- **Output Format**: Matched with FFmpeg arguments
- **Security**: Checked for potential command injection

## Management

### Process Lifecycle

1. **On-Demand Spawning**: Processes start when clients connect
2. **Automatic Cleanup**: Processes stop after 5 minutes of inactivity
3. **Health Monitoring**: Processes are monitored for health
4. **Resource Tracking**: CPU, memory, and bandwidth usage tracked

### Manual Control

You can manually control relay processes:

```bash
# Start a relay process
curl -X POST http://localhost:8080/api/v1/relay/{config_id}/start

# Stop a relay process
curl -X POST http://localhost:8080/api/v1/relay/{config_id}/stop

# Check relay status
curl http://localhost:8080/api/v1/relay/{config_id}/status
```

### Bulk Operations

For managing multiple relays:

```bash
# Get all relay metrics
curl http://localhost:8080/api/v1/relay/metrics

# Get system health
curl http://localhost:8080/api/v1/relay/health
```

## Monitoring

### Health Checks

The system provides comprehensive health monitoring:

- **Process Health**: CPU, memory, and status monitoring
- **Connection Health**: Client count and connection quality
- **System Health**: Overall system load and resource usage
- **Error Tracking**: Automatic error detection and reporting

### Metrics Collection

Metrics are collected for:

- **Throughput**: Bytes served per second
- **Client Connections**: Active client count
- **Process Uptime**: How long processes have been running
- **Error Rates**: Failed connections and process errors
- **Resource Usage**: CPU and memory consumption

### Alerting

The system can alert on:

- **Process Failures**: When FFmpeg processes crash
- **High Resource Usage**: CPU or memory threshold exceeded
- **Connection Issues**: When clients can't connect
- **Disk Space**: When temp directory fills up

## Troubleshooting

### Common Issues

#### FFmpeg Not Found
```
Error: FFmpeg process failed: Failed to spawn FFmpeg: No such file or directory
```

**Solution**: Install FFmpeg and ensure it's in your PATH:
```bash
# Ubuntu/Debian
sudo apt-get install ffmpeg

# CentOS/RHEL
sudo yum install ffmpeg

# macOS
brew install ffmpeg
```

#### Permission Denied
```
Error: Failed to create sandbox directory: Permission denied
```

**Solution**: Ensure the temp directory is writable:
```bash
chmod 755 /path/to/temp/directory
chown user:group /path/to/temp/directory
```

#### Hardware Acceleration Issues
```
Error: [h264_nvenc @ 0x...] Cannot load libcuda.so.1
```

**Solution**: Install proper GPU drivers:
```bash
# NVIDIA
sudo apt-get install nvidia-driver-470

# AMD
sudo apt-get install mesa-vdpau-drivers
```

### Debug Mode

Enable debug logging:

```toml
[logging]
level = "debug"
modules = ["relay", "ffmpeg"]
```

### Performance Issues

#### High CPU Usage
- Use hardware acceleration when available
- Reduce transcoding quality settings
- Limit concurrent processes

#### High Memory Usage
- Reduce segment buffer sizes
- Implement more aggressive cleanup
- Monitor for memory leaks

#### Network Issues
- Check upstream connection stability
- Implement retry logic
- Monitor bandwidth usage

## Performance Tuning

### Hardware Acceleration

For optimal performance, use hardware acceleration:

```json
{
  "name": "Optimized NVENC",
  "ffmpeg_args": [
    "-hwaccel", "cuda",
    "-hwaccel_output_format", "cuda",
    "-i", "{input_url}",
    "-c:v", "h264_nvenc",
    "-preset", "p1",
    "-profile:v", "main",
    "-b:v", "2M",
    "-bufsize", "4M",
    "-maxrate", "2M",
    "-c:a", "copy",
    "-f", "mpegts",
    "-y", "{output_path}/stream.ts"
  ],
  "hardware_acceleration": "cuda"
}
```

### Buffer Management

Optimize buffer sizes:

- **HLS**: 6-10 segments for live streaming
- **Transport Stream**: 1-2 segments for minimal latency
- **High Bitrate**: Increase buffer for stability

### System Configuration

#### Kernel Parameters
```bash
# Increase network buffer sizes
echo 'net.core.rmem_max = 16777216' >> /etc/sysctl.conf
echo 'net.core.wmem_max = 16777216' >> /etc/sysctl.conf

# Increase file descriptor limits
echo 'fs.file-max = 1048576' >> /etc/sysctl.conf
```

#### Process Limits
```bash
# Increase process limits
ulimit -n 65536
ulimit -u 32768
```

### Resource Monitoring

Monitor system resources:

```bash
# CPU usage
top -p $(pgrep ffmpeg)

# Memory usage
ps aux | grep ffmpeg

# Network usage
iftop -i eth0

# Disk I/O
iotop -o
```

---

## Support

For issues and questions:

1. Check the [troubleshooting section](#troubleshooting)
2. Review logs in debug mode
3. Test with simple configurations first
4. Verify FFmpeg installation and capabilities

## License

This relay system is part of the m3u-proxy project and follows the same licensing terms.