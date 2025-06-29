# Data Mapping Analysis & Solution

## Issue Summary

The data mapping system in M3U Proxy was not applying logo rules to channels due to a **case sensitivity mismatch** in condition evaluation.

## Root Cause

### The Problem
- **Database channels** have `tvg_id = "SkySportsF1.uk"` (mixed case)
- **Data mapping rule** condition looks for `tvg_id = "skysportsf1.uk"` (lowercase)
- **Engine comparison** used exact string matching: `field_value == condition.value`
- **Result**: `"SkySportsF1.uk" == "skysportsf1.uk"` → `false` (no match)

### Evidence
```sql
-- Existing channels in database
SELECT tvg_id, tvg_logo, group_title FROM channels WHERE tvg_id = 'SkySportsF1.uk' LIMIT 1;
-- Result: SkySportsF1.uk | http://icon-tmdb.me/stalker_portal/misc/logos/320/12415.jpg?89982 | (empty)

-- Existing data mapping rule
SELECT c.value, a.action_type, a.target_field FROM data_mapping_conditions c 
JOIN data_mapping_actions a ON c.rule_id = a.rule_id 
WHERE c.field_name = 'tvg_id';
-- Results:
-- Condition: tvg_id equals "skysportsf1.uk"
-- Action 1: set_value -> group_title = "Sports"  
-- Action 2: set_logo -> tvg_logo = custom logo asset
```

### Impact
- Logo replacement rules were **never triggered**
- Channels kept their original logos and empty group titles
- Data mapping appeared broken despite being fully implemented

## How Data Mapping Works

### System Architecture
1. **Scheduler Service** (`src/ingestor/scheduler.rs`) triggers source ingestion on cron schedule
2. **Source ingestion** downloads and parses M3U/Xtream data
3. **Data Mapping Engine** (`src/data_mapping/engine.rs`) applies rules to all channels
4. **Processed channels** are saved to database with transformed data

### Processing Flow
```rust
// For each channel from ingested source:
for channel in channels {
    let mapped = apply_rules_to_channel(channel, rules, logo_assets);
    // Rules are evaluated in sort_order sequence
    // Only active rules are processed
    // Each rule can have multiple conditions (AND/OR logic)
    // Each rule can have multiple actions (set_value, set_logo, set_label)
}
```

### Logo Rule Example
For a channel with `tvg_id = "SkySportsF1.uk"`:

1. **Condition evaluation**: Check if `tvg_id equals "skysportsf1.uk"`
2. **Action execution** (if matched):
   - Set `group_title = "Sports"`
   - Set `tvg_logo = "/api/logos/c63d556e-7b3c-4a85-accd-214c32663482"`

## Solution Implemented

### Fix Applied
Modified `DataMappingEngine::evaluate_condition()` in `src/data_mapping/engine.rs` to use **case-insensitive string comparisons**:

```rust
// Before (case-sensitive)
FilterOperator::Equals => Ok(field_value == condition.value),

// After (case-insensitive) 
FilterOperator::Equals => Ok(field_value.to_lowercase() == condition.value.to_lowercase()),
```

### All String Operators Updated
- `Equals` / `NotEquals`
- `Contains` / `NotContains` 
- `StartsWith` / `EndsWith`

**Note**: Regex operators (`Matches`/`NotMatches`) already use case-insensitive matching via `RegexBuilder::case_insensitive(true)`.

## Expected Results After Fix

### Before Fix
```sql
SELECT tvg_id, tvg_logo, group_title FROM channels WHERE tvg_id = 'SkySportsF1.uk';
-- tvg_id: SkySportsF1.uk
-- tvg_logo: http://icon-tmdb.me/stalker_portal/misc/logos/320/12415.jpg?89982  
-- group_title: (empty)
```

### After Fix (Next Ingestion)
```sql
SELECT tvg_id, tvg_logo, group_title FROM channels WHERE tvg_id = 'SkySportsF1.uk';
-- tvg_id: SkySportsF1.uk
-- tvg_logo: /api/logos/c63d556e-7b3c-4a85-accd-214c32663482  -- CUSTOM LOGO!
-- group_title: Sports  -- UPDATED!
```

## Testing the Fix

### Immediate Verification
The fix will be applied on the next scheduled source ingestion (based on cron schedule).

### Manual Testing
1. **Trigger source refresh** via API: `POST /api/sources/{id}/refresh`
2. **Check rule test endpoint**: `POST /api/data-mapping/test` 
3. **Verify channel updates** in database after ingestion

### Monitoring
Check server logs for data mapping execution:
```bash
tail -f server.log | grep -i "data mapping\|rule.*applied"
```

## Logo Asset System Details

### Logo Storage
- **Uploaded logos**: `uploaded/{uuid}.{ext}`
- **Cached logos**: `cached/{uuid}.{ext}`
- **Access URL**: `/api/logos/{uuid}`

### Logo Rule Processing
When a `set_logo` action is triggered:
1. **Logo asset lookup** by UUID in `logo_assets` table
2. **URL generation**: `/api/logos/{logo_asset_id}`
3. **Field assignment**: `mapped_tvg_logo = logo_url`
4. **Database save**: Transformed channel data persisted

### Current Logo Asset
```sql
SELECT name, file_name FROM logo_assets WHERE id = 'c63d556e-7b3c-4a85-accd-214c32663482';
-- name: Sky_Sports_F1_logo_2020
-- file_name: c63d556e-7b3c-4a85-accd-214c32663482.svg
```

## Implementation Status

✅ **Data mapping engine** - Fully implemented and working  
✅ **Logo asset system** - Complete with storage and API endpoints  
✅ **Scheduler integration** - Data mapping applied during ingestion  
✅ **Case sensitivity fix** - Resolved condition matching issue  
✅ **Rule storage** - Rules properly stored in database  

## Key Technical Notes

- **Processing timing**: Data mapping only runs during source ingestion (scheduler-triggered)
- **Rule order**: Rules are applied in `sort_order` sequence
- **Fallback behavior**: If data mapping fails, original channels are saved
- **Performance**: Rules are cached and logo assets are preloaded per ingestion
- **Error handling**: Individual rule failures don't stop processing of other rules

## Future Enhancements

1. **Manual rule application** - Allow applying rules to existing channels without re-ingestion
2. **Rule preview** - Show exactly which channels a rule would affect before activation
3. **Batch operations** - Apply multiple rules simultaneously with conflict resolution
4. **Rule analytics** - Track which rules are most frequently triggered