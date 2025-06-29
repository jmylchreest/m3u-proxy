# New Data Mapping Features - Test Guide

## Overview

Two new powerful features have been added to the M3U Proxy data mapping system:

1. **Rule Preview** - See exactly which channels will be affected by your rules before applying them
2. **Remap Existing Channels** - Apply data mapping rules to channels already in the database without waiting for source re-ingestion

## Features Added

### 1. Rule Preview (`GET /api/data-mapping/preview`)

Shows a detailed preview of what your active data mapping rules will do:
- Which channels match each rule's conditions
- What changes will be made to each field
- Summary of total affected channels across all rules

### 2. Remap Existing Channels (`POST /api/data-mapping/remap`)

Applies all active data mapping rules to existing channels in the database:
- Processes all active sources
- Applies rules in sort_order sequence
- Updates channels with transformed data immediately
- Provides detailed statistics on processing results

## UI Changes

### New Buttons in Data Mapping Page

The data mapping interface now has three buttons in the header:

```
[➕ Add Rule] [👁️ Preview Rules] [🔄 Remap Existing Channels]
```

- **Add Rule**: Original functionality to create new rules
- **Preview Rules**: Generate a detailed preview of rule effects
- **Remap Existing Channels**: Apply rules to existing data immediately

## Usage Examples

### Example 1: Preview Rules Before Applying

1. **Navigate** to the Data Mapping page
2. **Click** "👁️ Preview Rules" button
3. **Review** the generated preview showing:
   - Each active rule and its conditions
   - Total number of channels that will be affected
   - Detailed breakdown of changes for each channel
   - Before/after values for each field modification

**Sample Preview Output:**
```
Summary: 2 active rules will affect 15 channels

Rule: "F1>Sports"
├─ Affects 10 channels
├─ Conditions: tvg_id equals "skysportsf1.uk"
├─ Actions: 
│  ├─ set_value → group_title = "Sports"
│  └─ set_logo → tvg_logo = Custom Logo
└─ Affected Channels:
   ├─ VIP: SKY SPORTS F1 ᴴᴰ (Source: strong8k)
   │  ├─ group_title: null → "Sports" ✓
   │  └─ tvg_logo: "http://icon-tmdb.me/..." → "/api/logos/c63d556e-7b3c-4a85-accd-214c32663482" ✓
   └─ [9 more channels...]
```

### Example 2: Apply Rules to Existing Channels

1. **Click** "🔄 Remap Existing Channels" 
2. **Confirm** the action (this cannot be undone)
3. **Wait** for processing to complete
4. **Review** the results summary:

**Sample Results:**
```
✅ Remapping completed! 
   • Processed 1 sources
   • 245 total channels processed  
   • 15 channels affected
```

## Testing the Case Sensitivity Fix

### Before the Fix
The rule condition `tvg_id equals "skysportsf1.uk"` would NOT match channels with `tvg_id = "SkySportsF1.uk"` due to case sensitivity.

### After the Fix
The same rule now matches correctly using case-insensitive comparison:
- Database: `SkySportsF1.uk`
- Rule condition: `skysportsf1.uk` 
- Result: ✅ Match (case-insensitive)

### Test Verification

1. **Check current state:**
```sql
SELECT tvg_id, tvg_logo, group_title 
FROM channels 
WHERE tvg_id = 'SkySportsF1.uk' 
LIMIT 1;

-- Expected BEFORE fix:
-- tvg_logo: http://icon-tmdb.me/stalker_portal/misc/logos/320/12415.jpg?89982
-- group_title: (empty)
```

2. **Use Preview to verify matching:**
   - Click "👁️ Preview Rules"
   - Verify that channels with `SkySportsF1.uk` appear in the "F1>Sports" rule preview

3. **Apply rules:**
   - Click "🔄 Remap Existing Channels"
   - Confirm the action

4. **Verify results:**
```sql
SELECT tvg_id, tvg_logo, group_title 
FROM channels 
WHERE tvg_id = 'SkySportsF1.uk' 
LIMIT 1;

-- Expected AFTER fix:
-- tvg_logo: /api/logos/c63d556e-7b3c-4a85-accd-214c32663482
-- group_title: Sports
```

## API Reference

### Preview Rules
```http
GET /api/data-mapping/preview
```

**Response Format:**
```json
{
  "success": true,
  "message": "Data mapping rules preview generated",
  "rules": [
    {
      "rule_id": "uuid",
      "rule_name": "F1>Sports", 
      "rule_description": null,
      "affected_channels_count": 10,
      "affected_channels": [
        {
          "channel_id": "uuid",
          "channel_name": "VIP: SKY SPORTS F1 ᴴᴰ",
          "tvg_id": "SkySportsF1.uk",
          "source_name": "strong8k",
          "actions_preview": [
            {
              "action_type": "set_value",
              "target_field": "group_title", 
              "current_value": null,
              "new_value": "Sports",
              "will_change": true
            }
          ]
        }
      ],
      "conditions": [...],
      "actions": [...]
    }
  ],
  "total_rules": 2,
  "total_affected_channels": 15
}
```

### Remap Existing Channels
```http
POST /api/data-mapping/remap
```

**Response Format:**
```json
{
  "success": true,
  "message": "Channel remapping completed",
  "sources_processed": 1,
  "total_channels_processed": 245,
  "total_channels_affected": 15
}
```

## Error Handling

### Common Scenarios

1. **No Active Rules:**
   - Preview returns empty results with message
   - Remap processes but affects 0 channels

2. **Database Errors:**
   - API returns 500 status with error message
   - UI shows error alert with details

3. **Rule Processing Failures:**
   - Individual rule failures don't stop overall processing
   - Errors are logged but don't prevent other rules from running

## Performance Considerations

### Preview Rules
- Processes all channels across all sources
- For large datasets (>10k channels), may take 5-10 seconds
- Results are not cached - each preview regenerates data

### Remap Existing Channels  
- Processes channels in batches by source
- Database updates are transactional per source
- Progress is logged to server logs
- For large datasets, expect 30-60 seconds processing time

## Monitoring and Logging

### Server Logs
Both features generate detailed logs:

```
INFO Starting data mapping rule preview
INFO Data mapping preview completed: 2 rules, 15 total affected channels

INFO Starting remap of existing channels  
INFO Remapped 15 channels for source 'strong8k' (ID: uuid)
INFO Channel remapping completed: 1 sources, 245 channels processed, 15 channels affected
```

### UI Feedback
- Buttons show progress indicators during processing
- Success/error alerts display results
- Detailed results shown in preview modal

## Best Practices

1. **Always Preview First:**
   - Use preview to understand rule impact before applying
   - Verify conditions match expected channels
   - Check that actions produce desired results

2. **Test with Subset:**
   - Create rules with specific conditions first
   - Use preview to verify narrow targeting
   - Expand rules gradually after testing

3. **Backup Considerations:**
   - Remapping modifies database directly
   - Consider database backup before major rule changes
   - Document rule changes for audit trail

4. **Performance Planning:**
   - Schedule remapping during low-usage periods
   - Monitor server resources during large remapping operations
   - Consider breaking large rule sets into smaller batches

## Troubleshooting

### Preview Shows No Results
- Check if rules are marked as active (`is_active = true`)
- Verify condition syntax and values
- Test individual conditions using rule test feature

### Remap Doesn't Change Channels
- Verify preview shows expected matches first
- Check server logs for processing errors
- Confirm database write permissions

### Case Sensitivity Issues
- The fix handles all string operators (equals, contains, starts_with, etc.)
- Regex operators were already case-insensitive
- Mixed-case channel data should now match lowercase rule conditions

## Future Enhancements

Planned improvements based on these foundations:

1. **Selective Remapping:** Apply rules to specific sources only
2. **Dry Run Mode:** Preview with actual database write simulation
3. **Rule Conflict Detection:** Identify rules that might override each other
4. **Performance Optimization:** Batch processing and progress tracking
5. **Rule Analytics:** Track rule effectiveness over time