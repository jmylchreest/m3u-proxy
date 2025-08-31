# Source-Level Connection Limiting Implementation Plan

## Overview

This document outlines the implementation plan for adding source-level connection limiting with load balancing to the M3U Proxy system. The goal is to enforce `max_concurrent_streams` limits per stream source URL while providing intelligent load balancing across multiple sources serving the same channel.

## Requirements

The system needs to:
1. Track active connections per **stream source URL** (not per channel)
2. Load balance across multiple sources that serve the same channel
3. Respect `max_concurrent_streams` limits per source
4. Count relay connections as 1 client regardless of downstream viewers
5. Deny connections when all available sources are at capacity

## Current State Analysis

### Existing Infrastructure
- **Connection Limiter Service**: Exists at `src/services/connection_limiter.rs` but not used in streaming
- **Max Concurrent Streams**: Configured in both stream sources and proxies
- **Session Tracking**: Active via `SessionTracker` for metrics only
- **No Active Enforcement**: Stream handler doesn't check limits before allowing streams

### What's Missing
1. Integration of connection limiter with streaming endpoint
2. Source-level (URL-based) connection tracking
3. Load balancing logic for source selection
4. Relay-aware connection counting

## Proposed Architecture

### 1. Enhanced Connection Tracking

**File**: `src/services/connection_limiter.rs`

```rust
// Enhanced connection limiter to track by source URL
pub struct SourceConnectionLimiter {
    // Map: source_url -> current active connections
    active_connections: Arc<RwLock<HashMap<String, u32>>>,
    // Map: source_url -> max_concurrent_streams limit
    source_limits: Arc<RwLock<HashMap<String, u32>>>,
}

pub struct SourceConnectionInfo {
    pub source_url: String,
    pub max_concurrent: u32,
    pub current_connections: u32,
    pub available_capacity: u32,
}

#[derive(Debug, Clone)]
pub enum LimitExceededError {
    // ... existing variants ...
    SourceAtCapacity { 
        source_url: String, 
        current: u32, 
        max: u32 
    },
}
```

### 2. Source Resolution and Load Balancing

**File**: `src/services/stream_source_resolver.rs`

```rust
pub struct StreamSourceResolver {
    proxy_repo: StreamProxyRepository,
    connection_limiter: SourceConnectionLimiter,
}

impl StreamSourceResolver {
    // Find all available sources for a channel and select best one
    pub async fn resolve_best_available_source(
        &self, 
        proxy_id: Uuid, 
        channel_id: Uuid
    ) -> Result<SelectedSource, SourceResolutionError> {
        // 1. Get all sources that contain this channel
        let available_sources = self.get_channel_sources(proxy_id, channel_id).await?;
        
        // 2. Check capacity for each source
        let mut viable_sources = Vec::new();
        for source in available_sources {
            let capacity = self.connection_limiter
                .get_source_capacity(&source.stream_url).await?;
            if capacity.available_capacity > 0 {
                viable_sources.push((source, capacity));
            }
        }
        
        // 3. Load balance - pick source with most available capacity
        viable_sources.sort_by(|a, b| b.1.available_capacity.cmp(&a.1.available_capacity));
        
        viable_sources.into_iter().next()
            .map(|(source, _)| SelectedSource { source, reason: "capacity_available" })
            .ok_or(SourceResolutionError::AllSourcesAtCapacity)
    }
}
```

### 3. Modified Stream Handler Flow

**File**: `src/web/handlers/proxies.rs`

The `proxy_stream` handler would be enhanced:

```rust
pub async fn proxy_stream(
    axum::extract::Path((proxy_id, channel_id_str)): axum::extract::Path<(String, String)>,
    headers: axum::http::HeaderMap,
    State(state): State<AppState>,
) -> impl IntoResponse {
    // ... existing validation code ...

    // NEW: Resolve best available source for this channel
    let selected_source = match state.source_resolver
        .resolve_best_available_source(resolved_proxy_uuid, channel_id)
        .await 
    {
        Ok(source) => source,
        Err(SourceResolutionError::AllSourcesAtCapacity) => {
            return generate_capacity_exceeded_error_video(
                &state, 
                &proxy, 
                &channel_id.to_string()
            ).await;
        }
        Err(e) => {
            error!("Failed to resolve source: {}", e);
            return (StatusCode::INTERNAL_SERVER_ERROR, "Source resolution failed").into_response();
        }
    };

    // NEW: Register connection to the selected source
    let connection_handle = match state.source_connection_limiter
        .register_source_connection(
            &selected_source.source.stream_url,
            &format!("{}_{}", proxy_id, channel_id)
        )
        .await 
    {
        Ok(handle) => handle,
        Err(LimitExceededError::SourceAtCapacity { source_url, current, max }) => {
            warn!("Source {} at capacity: {}/{}", source_url, current, max);
            return generate_capacity_exceeded_error_video(
                &state, 
                &proxy, 
                &channel_id.to_string()
            ).await;
        }
        Err(e) => {
            error!("Failed to register connection: {}", e);
            return (StatusCode::INTERNAL_SERVER_ERROR, "Connection registration failed").into_response();
        }
    };

    // ... rest of existing streaming logic, using selected_source.source.stream_url ...
    // The connection_handle will auto-decrement on drop
}
```

## Implementation Phases

### Phase 1: Enhance ConnectionLimiter for Source-Level Tracking

**File**: `src/services/connection_limiter.rs`

**Changes**:
- Add `SourceConnectionLimiter` struct
- Add source-specific error types
- Implement source URL-based connection tracking
- Add `SourceConnectionHandle` with auto-cleanup on drop

**Key Features**:
```rust
impl SourceConnectionLimiter {
    pub async fn register_source_connection(
        &self,
        source_url: &str,
        connection_id: &str,
    ) -> Result<SourceConnectionHandle, LimitExceededError> {
        // Check current capacity
        // Register if available
        // Return handle that auto-decrements on drop
    }

    pub async fn update_source_limits(&self, source_url: &str, max_concurrent: u32) {
        // Update max_concurrent_streams for a source
    }

    pub async fn get_source_capacity(&self, source_url: &str) -> SourceCapacityInfo {
        // Return current/max/available capacity for source
    }
}
```

### Phase 2: Create StreamSourceResolver Service

**File**: `src/services/stream_source_resolver.rs`

**Purpose**: Find and select the best available source for a given channel

**Key Components**:
```rust
pub struct StreamSourceResolver {
    proxy_repo: StreamProxyRepository,
    connection_limiter: Arc<SourceConnectionLimiter>,
}

#[derive(Debug)]
pub struct SelectedSource {
    pub channel: Channel,
    pub source: StreamSource,
    pub source_url: String, // The actual stream URL to use
    pub selection_reason: String,
}

#[derive(Debug)]
pub enum SourceResolutionError {
    ChannelNotFound,
    AllSourcesAtCapacity,
    NoSourcesAvailable,
    DatabaseError(String),
}
```

**Algorithm**:
1. Find all stream sources containing the requested channel (by matching stream URL)
2. Check capacity for each source
3. Sort by available capacity (most available first)
4. Return the best option or error if all at capacity

### Phase 3: Implement Source Resolution in proxy_stream Handler

**File**: `src/web/handlers/proxies.rs`

**Changes**:
- Replace direct channel lookup with source resolution
- Add connection registration before streaming
- Handle capacity exceeded scenarios
- Use selected source URL for actual streaming

**Error Handling**:
- Generate error video when all sources at capacity
- Proper logging of source selection decisions
- Graceful fallback for resolution failures

### Phase 4: Handle Relay Mode Connection Counting

**Special Logic for Relay Mode**:
```rust
StreamProxyMode::Relay => {
    // Check if relay is already running for this source
    let relay_already_active = state.relay_manager
        .is_relay_active_for_source(&selected_source.source_url)
        .await;

    let connection_id = if relay_already_active {
        // Relay exists - this is a viewer connection, not a source connection
        format!("relay_viewer_{}_{}", proxy_id, channel_id)
    } else {
        // New relay - this will count as 1 source connection
        format!("relay_source_{}_{}", proxy_id, channel_id)
    };

    let _connection_handle = if !relay_already_active {
        // Only register source connection if relay isn't already running
        Some(state.source_connection_limiter
            .register_source_connection(&selected_source.source_url, &connection_id)
            .await?)
    } else {
        None // No source connection registration needed for additional viewers
    };
}
```

### Phase 5: Add Error Video Generation for Capacity Exceeded

**Integration with Existing System**:
```rust
async fn generate_capacity_exceeded_response(
    state: &AppState,
    proxy: &StreamProxy,
    channel_id: Uuid,
    client_ip: &str,
) -> axum::response::Response {
    // Use the existing error video generation system
    let error = LimitExceededError::UpstreamSourceLimit {
        source_url: "all_sources".to_string(),
        error: "All available sources for this channel are at capacity".to_string(),
    };

    match state.error_fallback_service
        .generate_error_video(error, Some(&format!("{}_{}", proxy.id, channel_id)))
        .await 
    {
        Ok(video_response) => video_response,
        Err(e) => {
            error!("Failed to generate capacity error video: {}", e);
            (StatusCode::SERVICE_UNAVAILABLE, "All sources at capacity").into_response()
        }
    }
}
```

### Phase 6: Update Session Tracking for Source-Level Metrics

**Enhancements**:
- Add source URL to session tracking data
- Include source selection reasoning in logs
- Track source-level statistics
- Add metrics for load balancing decisions

### Phase 7: Add Tests and Integration

**Test Coverage**:
- Unit tests for `SourceConnectionLimiter`
- Integration tests for `StreamSourceResolver`
- End-to-end tests for capacity scenarios
- Load balancing verification tests
- Relay mode connection counting tests

## Integration Points

### AppState Changes

**File**: `src/web/mod.rs`

```rust
pub struct AppState {
    // ... existing fields ...
    pub source_connection_limiter: Arc<SourceConnectionLimiter>,
    pub stream_source_resolver: Arc<StreamSourceResolver>,
}
```

### Service Initialization

Services need to be initialized in the main application startup:

```rust
// Initialize source connection limiter
let source_connection_limiter = Arc::new(SourceConnectionLimiter::new());

// Initialize stream source resolver
let stream_source_resolver = Arc::new(StreamSourceResolver::new(
    stream_proxy_repo.clone(),
    source_connection_limiter.clone(),
));
```

### API Endpoints (Optional)

Consider adding API endpoints for monitoring:
- `GET /api/v1/sources/capacity` - View current source capacities
- `GET /api/v1/sources/{source_id}/connections` - View active connections
- `POST /api/v1/sources/{source_id}/limits` - Update connection limits

## Benefits

This implementation provides:

✅ **Per-source connection limiting**: Each unique stream source URL gets its own connection limit based on `max_concurrent_streams`

✅ **Load balancing across sources**: When multiple sources have the same channel, the system picks the one with the most available capacity

✅ **Relay counting**: FFmpeg relays count as 1 source connection, but multiple viewers can connect to the relay buffer without additional source connections

✅ **Capacity-based routing**: New clients get routed to available sources, and are denied when all sources are at capacity

✅ **Error video fallback**: When capacity is exceeded, the existing error video generation system provides a proper response

✅ **Real-time capacity tracking**: Dynamic source selection based on current load

✅ **Integration with existing infrastructure**: Leverages current session tracking, error video generation, and relay management systems

## Migration Strategy

1. **Phase 1-2**: Implement core services without integration (no breaking changes)
2. **Phase 3**: Add feature flag for new connection limiting behavior
3. **Phase 4-6**: Gradually enable features and add monitoring
4. **Phase 7**: Full deployment with comprehensive testing

This approach allows for incremental deployment and easy rollback if issues arise.