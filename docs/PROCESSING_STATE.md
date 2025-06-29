# Processing State Implementation

## Overview

This document describes the implementation of the "Processing" state for channel list refresh operations in the M3U Proxy application.

## Problem Statement

Previously, when refreshing channel lists from stream sources, the system would transition directly from the download/parsing phase to "Completed" status. However, there was actually an intermediate step where channels were being inserted into the database. During this database insertion phase:

1. The UI showed no progress indication
2. Users couldn't see how many channels had been processed
3. Large channel lists (10,000+ channels) could take significant time to insert
4. The system appeared to be "stuck" during database operations

## Solution

### Backend Changes

#### 1. New IngestionState: Processing

Added a new `Processing` state to the `IngestionState` enum in `src/models/mod.rs`:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IngestionState {
    Idle,
    Connecting,
    Downloading,
    Parsing,
    Saving,       // Used for initial preparation
    Processing,   // NEW: Database insertion phase
    Completed,
    Error,
}
```

#### 2. Enhanced Database Method

Modified `update_source_channels()` in `src/database/stream_sources.rs` to:

- Accept an optional `IngestionStateManager` parameter
- Report progress during database insertion
- Update progress at configurable intervals (default: 1000 channels)
- Provide percentage completion and channel counts

Key features:
- Progress updates at configurable intervals (default: 1000 channels)
- Progress updates after each chunk (5000 channels) for large sets
- Real-time tracking of `channels_saved` count
- Percentage completion calculation
- Configurable via `ingestion.progress_update_interval` in config file

#### 3. API Handler Updates

Updated the `refresh_source` API handler in `src/web/api.rs` to:

- Pass the state manager to the database method
- Properly manage state transitions from ingestion → processing → completed

### Frontend Changes

#### JavaScript State Handling

Updated `static/js/sources.js` to handle the new "processing" state:

1. **State Colors**: Added "processing" to the `stateColors` object with "primary" color
2. **Progress Polling**: Included "processing" in active state checks
3. **Completion Detection**: Added "processing" to states that trigger source reload when completed

## State Flow

The updated state flow for channel refresh operations:

```
Idle → Connecting → Downloading → Parsing → Processing → Completed
  ↓                                                         ↑
Error ←←←←←←←←←←←←←←←←←←←←←←←←←←←←←←←←←←←←←←←←←←←←←←←←←←←←←←←←←←
```

### State Descriptions

- **Connecting**: Establishing connection to source
- **Downloading**: Fetching M3U/Xtream data
- **Parsing**: Parsing channel data from source format
- **Processing**: Inserting channels into local database
- **Completed**: All operations finished successfully

## Progress Information

During the Processing state, the following information is tracked and reported:

- `current_step`: Descriptive text showing current operation
- `channels_parsed`: Total number of channels from source
- `channels_saved`: Number of channels inserted so far
- `percentage`: Completion percentage (0-100%)

Example progress update:
```json
{
  "source_id": "uuid-here",
  "state": "processing",
  "progress": {
    "current_step": "Processing channels into database (1500/5000)",
    "channels_parsed": 5000,
    "channels_saved": 1500,
    "percentage": 30.0
  }
}
```

## Performance Considerations

### Progress Update Frequency

The progress update frequency is configurable via the `ingestion.progress_update_interval` setting in the configuration file (default: 1000 channels).

- **Small channel sets** (≤5000): Updates every N channels (where N = `progress_update_interval`)
- **Large channel sets** (>5000): Updates every N channels within chunks, plus after each 5000-channel chunk

To configure the update frequency, add to your `config.toml`:
```toml
[ingestion]
progress_update_interval = 1000  # Update every 1000 channels
```

Lower values provide more frequent updates but increase overhead. Higher values reduce overhead but provide less frequent feedback.

### Database Optimization

The implementation maintains existing optimizations:
- Chunked transactions for large datasets (5000 channels per chunk)
- Exclusive locking to prevent concurrent modifications
- WAL checkpoint forcing for very large operations (>10,000 channels)

## Testing

To test the new Processing state:

1. Add a stream source with a large channel count (5000+ recommended)
2. Trigger a manual refresh via the UI
3. Observe the progress bar and state transitions
4. Verify that:
   - State transitions from "parsing" to "processing"
   - Progress updates incrementally during database insertion
   - Channel count increases in real-time
   - State transitions to "completed" when finished

## Backward Compatibility

This implementation is fully backward compatible:
- Existing API endpoints unchanged
- Optional state manager parameter allows gradual adoption
- Frontend gracefully handles new state alongside existing ones
- No database schema changes required

## Configuration

### Progress Update Interval

The frequency of progress updates can be configured in `config.toml`:

```toml
[ingestion]
# How often to update progress during channel processing (in number of channels)
# Lower values = more frequent updates but higher overhead
# Higher values = less frequent updates but better performance
progress_update_interval = 1000
```

**Recommended values:**
- **High-frequency updates**: 100-500 channels (more responsive UI, higher overhead)
- **Balanced**: 1000 channels (default, good balance of responsiveness and performance)
- **Performance-focused**: 2000-5000 channels (less frequent updates, better performance)

### Database Configuration

The Database struct now accepts ingestion configuration during initialization:

```rust
let database = Database::new(&config.database, &config.ingestion).await?;
```

## Future Enhancements

Potential improvements for future versions:
- Estimated time remaining calculations
- Detailed error reporting during processing phase
- Cancellation support for long-running operations
- Adaptive progress update intervals based on channel count