# EPG Implementation Summary and Fixes

This document summarizes the EPG (Electronic Program Guide) implementation fixes and improvements made to ensure proper functionality and consistency with stream sources.

## Issues Identified and Fixed

### 1. **Default Cron Schedule Inconsistency** ✅ FIXED
**Problem**: EPG sources defaulted to refreshing every 12 hours while stream sources defaulted to every 6 hours.

**Solution**:
- Updated frontend JavaScript (`static/js/epg-sources.js`) to use `"0 0 */6 * * * *"` (every 6 hours) instead of `"0 0 */12 * * * *"`
- Created database migration (`migrations/003_update_epg_cron_default.sql`) to update the schema default from 12 hours to 6 hours
- This ensures consistency between EPG and stream source refresh schedules

### 2. **Missing Initial Refresh After Creation** ✅ FIXED
**Problem**: When creating new EPG sources, there was no automatic initial refresh to populate data immediately.

**Solution**:
- Modified `create_epg_source` API endpoint in `src/web/api.rs` to trigger immediate background refresh after creation
- Added similar functionality to `create_source` endpoint for stream sources to maintain consistency
- Uses `tokio::spawn` for non-blocking background refresh with proper error handling and logging

### 3. **Compilation Errors in Scheduler** ✅ FIXED
**Problem**: Multiple compilation errors in `src/ingestor/scheduler.rs`:
- References to non-existent `linked_xtream_source_id` field on models
- Method calls being treated as field access
- Incorrect argument types for `update_epg_source_data`

**Solution**:
- Fixed method calls to use correct syntax (`.name()` instead of `.name`)
- Removed direct field access to `linked_xtream_source_id` (field doesn't exist on models)
- Fixed `update_epg_source_data` call to pass `Vec` by value instead of reference
- Fixed `EpgIngestor::new` call to use correct single-parameter signature

### 4. **Missing DateTime Trait Imports** ✅ FIXED
**Problem**: Compilation errors in `src/ingestor/epg_ingestor.rs` due to missing `Datelike` and `Timelike` trait imports.

**Solution**:
- Added `Datelike` and `Timelike` imports from chrono crate
- These traits are needed for the test functions that access `.year()`, `.month()`, `.day()`, `.hour()`, `.minute()`, `.second()` methods

### 5. **EPG URL Building Error** ✅ FIXED
**Problem**: EPG ingestor failed with "builder error: relative URL without a base" when trying to fetch Xtream EPG data.

**Solution**:
- Added import for `normalize_url_scheme` utility function in `src/ingestor/epg_ingestor.rs`
- Updated `ingest_xtream_source()` method to use `normalize_url_scheme()` to ensure proper http:// or https:// scheme
- This ensures EPG URLs are properly formatted for HTTP requests

### 6. **Linked Source Database Constraint Error** ✅ FIXED
**Problem**: Linked source creation failed with "NOT NULL constraint failed: linked_xtream_sources.id".

**Solution**:
- Fixed INSERT queries in both `src/database/stream_sources.rs` and `src/database/epg_sources.rs`
- Added missing `id` field (primary key) to linked source INSERT statements
- Added all required fields: `id`, `name`, `url`, `username`, `password` to match schema
- This allows proper creation of linked Xtream source relationships

### 7. **Linked Source Refresh Functionality** ✅ IMPLEMENTED
**Problem**: User wanted linked sources (Xtream Codes with both stream and EPG data) to refresh both components together.

**Solution**:
- Added new database methods in `src/database/linked_xtream.rs`:
  - `find_linked_epg_by_stream_id()`: Find EPG source linked to a stream source
  - `find_linked_stream_by_epg_id()`: Find stream source linked to an EPG source
- Updated scheduler in `src/ingestor/scheduler.rs` to:
  - When refreshing a stream source, also refresh its linked EPG source (if any)
  - When refreshing an EPG source, also refresh its linked stream source (if any)
- Added proper logging for linked source refreshes

### 8. **Schedule Logging for EPG Sources** ✅ VERIFIED
**Problem**: User mentioned not seeing schedule logs for EPG sources like they do for stream sources.

**Solution**:
- Verified that EPG sources are properly included in the scheduler cache refresh
- The `log_startup_schedule()` method processes both stream and EPG sources
- EPG sources will appear in logs with format: `"Source 'name' (ID: uuid) - Next scheduled update: datetime (cron: expression)"`

## Key Files Modified

### Backend Changes
- `src/ingestor/scheduler.rs` - Fixed compilation errors, implemented linked source refresh
- `src/ingestor/epg_ingestor.rs` - Fixed missing trait imports, fixed URL building with normalize_url_scheme
- `src/database/linked_xtream.rs` - Added methods to find linked sources
- `src/database/stream_sources.rs` - Fixed linked source creation with proper INSERT query
- `src/database/epg_sources.rs` - Fixed linked source creation with proper INSERT query  
- `src/web/api.rs` - Added immediate refresh after EPG/stream source creation
- `migrations/003_update_epg_cron_default.sql` - Database schema update for default cron

### Frontend Changes
- `static/js/epg-sources.js` - Updated default cron from 12 hours to 6 hours

## Expected Behavior After Fixes

### 1. **EPG Source Creation**
- New EPG sources default to refreshing every 6 hours (same as stream sources)
- Immediate background refresh is triggered after creation to populate data
- Success/error logging provides clear feedback

### 2. **Scheduling and Logging**
- EPG sources appear in startup schedule logs alongside stream sources
- Schedule format: `"Source 'EPG Name' (ID: uuid) - Next scheduled update: 2025-01-XX XX:XX:XX UTC (cron: 0 0 */6 * * * *)"`
- Missed runs are detected and executed immediately if configured

### 3. **Linked Source Refresh**
- When a stream source refreshes, its linked EPG source (if any) also refreshes automatically
- When an EPG source refreshes, its linked stream source (if any) also refreshes automatically
- Clear logging indicates when linked refreshes occur

### 4. **Database Consistency**
- New EPG sources created via database default to 6-hour refresh schedule
- Existing sources maintain their configured schedules
- Migration safely updates schema without data loss

## Testing Recommendations

1. **Create New EPG Source**: Verify default cron is `"0 0 */6 * * * *"`
2. **Check Immediate Refresh**: New sources should populate data within seconds
3. **Monitor Logs**: Look for EPG sources in scheduler startup logs
4. **Test Linked Refresh**: Create linked Xtream source and verify both components refresh together
5. **Verify Schedule Execution**: EPG sources should refresh according to their cron schedules
6. **Test EPG URL Building**: Verify Xtream EPG sources work with URLs that lack http:// scheme
7. **Verify Linked Source Creation**: Ensure linked sources are created without database constraint errors

## Migration Notes

- The database migration (`003_update_epg_cron_default.sql`) safely recreates the `epg_sources` table with the new default
- Existing data is preserved during migration
- Existing sources keep their current cron schedules
- Only new sources will use the 6-hour default

## Compilation Status

✅ **All compilation errors fixed**
- Project builds successfully with `cargo build`
- Only warnings remain (unused methods/imports - normal for development)
- No breaking changes to existing functionality

The EPG implementation now provides feature parity with stream sources and includes the requested linked source refresh functionality.