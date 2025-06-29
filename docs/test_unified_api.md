# Unified Sources API Test Guide

This document provides test cases to verify the new unified sources API structure.

## API Structure Overview

The unified API consolidates stream and EPG sources under a single `/api/sources` endpoint with type-specific sub-routes:

- `GET /api/sources` - Returns all sources (both stream and EPG)
- `GET /api/sources/stream` - Returns only stream sources
- `GET /api/sources/epg` - Returns only EPG sources

## Response Format

All unified API endpoints return sources in the following format:

```json
{
  "source_kind": "stream" | "epg",
  "source": {
    "id": "uuid",
    "name": "string",
    "url": "string",
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

## Test Cases

### 1. Test All Sources Endpoint
```bash
curl -X GET http://localhost:8080/api/sources
```
Expected: Array of unified sources (both stream and EPG)

### 2. Test Stream Sources Only
```bash
curl -X GET http://localhost:8080/api/sources/stream
```
Expected: Array of unified sources where `source_kind = "stream"`

### 3. Test EPG Sources Only
```bash
curl -X GET http://localhost:8080/api/sources/epg
```
Expected: Array of unified sources where `source_kind = "epg"`

### 4. Create Stream Source
```bash
curl -X POST http://localhost:8080/api/sources/stream \
  -H "Content-Type: application/json" \
  -d '{
    "name": "Test Stream Source",
    "source_type": "m3u",
    "url": "http://example.com/playlist.m3u",
    "max_concurrent_streams": 10,
    "update_cron": "0 */6 * * *"
  }'
```
Expected: Created stream source in unified format

### 5. Create EPG Source
```bash
curl -X POST http://localhost:8080/api/sources/epg \
  -H "Content-Type: application/json" \
  -d '{
    "name": "Test EPG Source",
    "source_type": "xmltv",
    "url": "http://example.com/guide.xml",
    "update_cron": "0 */12 * * *",
    "timezone": "UTC",
    "time_offset": "0"
  }'
```
Expected: Created EPG source in unified format

### 6. Get Specific Stream Source
```bash
curl -X GET http://localhost:8080/api/sources/stream/{source_id}
```
Expected: Single stream source in unified format

### 7. Get Specific EPG Source
```bash
curl -X GET http://localhost:8080/api/sources/epg/{source_id}
```
Expected: Single EPG source in unified format

### 8. Update Stream Source
```bash
curl -X PUT http://localhost:8080/api/sources/stream/{source_id} \
  -H "Content-Type: application/json" \
  -d '{
    "name": "Updated Stream Source",
    "source_type": "m3u",
    "url": "http://example.com/new-playlist.m3u",
    "max_concurrent_streams": 15,
    "update_cron": "0 */4 * * *",
    "is_active": true
  }'
```
Expected: Updated stream source in unified format

### 9. Update EPG Source
```bash
curl -X PUT http://localhost:8080/api/sources/epg/{source_id} \
  -H "Content-Type: application/json" \
  -d '{
    "name": "Updated EPG Source",
    "source_type": "xmltv",
    "url": "http://example.com/new-guide.xml",
    "update_cron": "0 */8 * * *",
    "timezone": "America/New_York",
    "time_offset": "-5",
    "is_active": true
  }'
```
Expected: Updated EPG source in unified format

### 10. Stream Source Operations
```bash
# Refresh
curl -X POST http://localhost:8080/api/sources/stream/{source_id}/refresh

# Get channels
curl -X GET http://localhost:8080/api/sources/stream/{source_id}/channels

# Get progress
curl -X GET http://localhost:8080/api/sources/stream/{source_id}/progress

# Cancel ingestion
curl -X POST http://localhost:8080/api/sources/stream/{source_id}/cancel
```

### 11. EPG Source Operations
```bash
# Refresh
curl -X POST http://localhost:8080/api/sources/epg/{source_id}/refresh

# Get channels
curl -X GET http://localhost:8080/api/sources/epg/{source_id}/channels
```

### 12. Delete Sources
```bash
# Delete stream source
curl -X DELETE http://localhost:8080/api/sources/stream/{source_id}

# Delete EPG source
curl -X DELETE http://localhost:8080/api/sources/epg/{source_id}
```

## Frontend Integration Tests

### 1. Data Mapping Rule Testing
The data mapping interface should now use:
- `/api/sources/stream` for stream-type rules
- `/api/sources/epg` for EPG-type rules

### 2. Source Management Pages
- Stream sources page: Should load from `/api/sources/stream`
- EPG sources page: Should load from `/api/sources/epg`
- Both should handle the unified response format

### 3. Channel Viewer
- Should use `/api/sources/stream/{id}/channels` for stream sources
- Should use `/api/sources/epg/{id}/channels` for EPG sources

## Backward Compatibility

**Note:** This refactoring removes backward compatibility with the old API endpoints:
- `/api/sources` (old stream sources endpoint) → `/api/sources/stream`
- `/api/epg-sources` → `/api/sources/epg`

All frontend code and external integrations must be updated to use the new unified API structure.

## Error Handling

All endpoints should return appropriate HTTP status codes:
- `200 OK` - Successful operation
- `201 Created` - Resource created successfully
- `204 No Content` - Resource deleted successfully
- `400 Bad Request` - Invalid request data
- `404 Not Found` - Resource not found
- `500 Internal Server Error` - Server error

## Validation

Ensure that:
1. All sources have the correct `source_kind` field
2. Stream sources include stream-specific fields (`max_concurrent_streams`, `field_map`)
3. EPG sources include EPG-specific fields (`timezone`, `timezone_detected`, `time_offset`, `program_count`)
4. Channel counts and other statistics are properly populated
5. The `/api/sources` endpoint returns sources sorted by name consistently