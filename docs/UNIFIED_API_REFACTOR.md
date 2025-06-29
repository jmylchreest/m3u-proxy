# Unified Sources API Refactoring Summary

## Overview

This document summarizes the major refactoring of the sources API to provide a unified structure for both stream and EPG sources, reducing endpoint complexity and improving consistency.

## Changes Made

### 1. API Endpoint Consolidation

#### Before (Legacy)
- **Stream Sources**: `/api/sources/*`
- **EPG Sources**: `/api/epg-sources/*`
- Separate endpoints with different response structures

#### After (Unified)
- **All Sources**: `/api/sources` (returns both stream and EPG)
- **Stream Sources**: `/api/sources/stream/*`
- **EPG Sources**: `/api/sources/epg/*`
- Consistent unified response structure with type discrimination

### 2. New Unified Response Structure

All source endpoints now return a unified structure with a `source_kind` discriminator:

```json
{
  "source_kind": "stream" | "epg",
  "source": {
    "id": "uuid",
    "name": "string",
    "url": "string",
    "source_type": "m3u" | "xtream" | "xmltv",
    "update_cron": "string",
    "username": "string?",
    "password": "string?",
    "created_at": "datetime",
    "updated_at": "datetime",
    "last_ingested_at": "datetime?",
    "is_active": "boolean"
  },
  "channel_count": "number",
  "next_scheduled_update": "datetime?",
  
  // Stream-specific fields (when source_kind = "stream")
  "max_concurrent_streams": "number",
  "field_map": "string?",
  
  // EPG-specific fields (when source_kind = "epg")
  "timezone": "string",
  "timezone_detected": "boolean",
  "time_offset": "string",
  "program_count": "number"
}
```

### 3. Backend Changes

#### Models (`src/models/mod.rs`)
- Added `UnifiedSourceWithStats` enum with `Stream` and `Epg` variants
- Added `EpgSourceBase` struct to match stream source structure
- Added conversion methods `from_stream()` and `from_epg()`
- Added utility methods `get_id()`, `get_name()`, `is_stream()`, `is_epg()`

#### API Routes (`src/web/mod.rs`)
- **Removed**: Legacy separate endpoints
- **Added**: Unified endpoints under `/api/sources`
  - `GET /api/sources` - List all sources
  - `GET /api/sources/stream` - List stream sources
  - `GET /api/sources/epg` - List EPG sources
  - Full CRUD operations for both types under their respective paths

#### API Implementations (`src/web/api.rs`)
- Added `list_all_sources()` - Combines both source types
- Added `list_stream_sources()` - Unified format for stream sources
- Added `list_epg_sources_unified()` - Unified format for EPG sources
- Added unified CRUD operations for both source types
- All operations now return unified response format

### 4. Frontend Changes

#### Data Mapping (`static/js/data-mapping.js`)
- Updated `loadSourcesForTesting()` to use:
  - `/api/sources/stream` for stream-type rules
  - `/api/sources/epg` for EPG-type rules
- Updated source selection to handle unified response structure
- Added support for extracting source data from `source` property

#### Stream Sources Management (`static/js/sources.js`)
- Updated all API calls from `/api/sources/*` to `/api/sources/stream/*`
- Modified source rendering to handle unified structure
- Updated CRUD operations to use new endpoints
- Added handling for nested source data structure

#### EPG Sources Management (`static/js/epg-sources.js`)
- Updated all API calls from `/api/epg-sources/*` to `/api/sources/epg/*`
- Modified source rendering to handle unified structure
- Updated CRUD operations to use new endpoints
- Added handling for nested source data structure

#### Channel Viewer (`static/js/channels-viewer.js`)
- Updated to use `/api/sources/stream/{id}/channels`

### 5. Documentation Updates

#### README.md
- Updated API documentation to reflect new unified structure
- Organized endpoints by unified categories
- Added examples of new endpoint usage

## New API Endpoints

### Core Endpoints
| Method | Endpoint | Description |
|--------|----------|-------------|
| GET | `/api/sources` | List all sources (stream + EPG) |
| GET | `/api/sources/stream` | List stream sources only |
| GET | `/api/sources/epg` | List EPG sources only |

### Stream Source Operations
| Method | Endpoint | Description |
|--------|----------|-------------|
| POST | `/api/sources/stream` | Create stream source |
| GET | `/api/sources/stream/{id}` | Get stream source |
| PUT | `/api/sources/stream/{id}` | Update stream source |
| DELETE | `/api/sources/stream/{id}` | Delete stream source |
| POST | `/api/sources/stream/{id}/refresh` | Refresh stream source |
| POST | `/api/sources/stream/{id}/cancel` | Cancel ingestion |
| GET | `/api/sources/stream/{id}/progress` | Get progress |
| GET | `/api/sources/stream/{id}/processing` | Get processing info |
| GET | `/api/sources/stream/{id}/channels` | Get channels |

### EPG Source Operations
| Method | Endpoint | Description |
|--------|----------|-------------|
| POST | `/api/sources/epg` | Create EPG source |
| GET | `/api/sources/epg/{id}` | Get EPG source |
| PUT | `/api/sources/epg/{id}` | Update EPG source |
| DELETE | `/api/sources/epg/{id}` | Delete EPG source |
| POST | `/api/sources/epg/{id}/refresh` | Refresh EPG source |
| GET | `/api/sources/epg/{id}/channels` | Get channels |

## Breaking Changes

⚠️ **Important**: This refactoring introduces breaking changes.

### Removed Endpoints
- `/api/sources` (legacy stream sources) → `/api/sources/stream`
- `/api/sources/{id}` → `/api/sources/stream/{id}`
- `/api/epg-sources` → `/api/sources/epg`
- `/api/epg-sources/{id}` → `/api/sources/epg/{id}`

### Response Format Changes
- All responses now use the unified structure with `source_kind` discriminator
- Source data is nested under a `source` property
- Statistics (`channel_count`, `program_count`) are at the top level

## Benefits

### 1. Consistency
- Unified response format across all source types
- Consistent endpoint structure (`/api/sources/{type}`)
- Standardized operation patterns

### 2. Discoverability
- Single `/api/sources` endpoint shows all sources
- Clear type-based filtering with `/api/sources/{type}`
- Logical grouping of related operations

### 3. Maintainability
- Reduced code duplication in frontend
- Easier to add new source types in the future
- Centralized source handling logic

### 4. Type Safety
- Clear discrimination between source types
- Compile-time validation of source-specific fields
- Better error handling and validation

## Migration Guide

### For API Consumers

1. **Update endpoint URLs**:
   ```diff
   - GET /api/sources
   + GET /api/sources/stream
   
   - GET /api/epg-sources
   + GET /api/sources/epg
   ```

2. **Handle new response format**:
   ```javascript
   // Before
   const source = response.data;
   console.log(source.name);
   
   // After
   const sourceWithStats = response.data;
   const source = sourceWithStats.source || sourceWithStats;
   console.log(source.name);
   ```

3. **Check source type**:
   ```javascript
   // Use source_kind to determine type
   if (sourceWithStats.source_kind === 'stream') {
     // Handle stream-specific fields
     console.log(sourceWithStats.max_concurrent_streams);
   } else if (sourceWithStats.source_kind === 'epg') {
     // Handle EPG-specific fields
     console.log(sourceWithStats.timezone);
   }
   ```

### For Frontend Development

1. Update all API calls to use new endpoints
2. Modify source rendering to handle unified structure
3. Add type checking based on `source_kind`
4. Update form handling for type-specific fields

## Testing

- All existing functionality has been preserved
- New unified endpoints return data in consistent format
- Frontend pages updated to work with new API structure
- No data loss or functionality regression

## Future Enhancements

This unified structure makes it easier to:
- Add new source types (e.g., RSS feeds, custom APIs)
- Implement cross-source operations
- Create unified source management interfaces
- Add source-type-agnostic features

## Files Modified

### Backend
- `src/models/mod.rs` - Added unified source structures
- `src/web/mod.rs` - Updated route definitions
- `src/web/api.rs` - Added unified API implementations

### Frontend
- `static/js/data-mapping.js` - Updated for unified API
- `static/js/sources.js` - Updated for unified API
- `static/js/epg-sources.js` - Updated for unified API
- `static/js/channels-viewer.js` - Updated channel endpoint

### Documentation
- `README.md` - Updated API documentation
- `UNIFIED_API_REFACTOR.md` - This summary document
- `test_unified_api.md` - Test cases for new API