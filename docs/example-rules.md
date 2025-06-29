# Example Data Mapping Rules

This document provides examples of data mapping rules that users can manually create through the web interface or add directly to the database.

## Channel 1 Logo Assignment Rule

This rule assigns a specific logo to channels containing "Channel 1" in their name, but only if they don't already have a logo assigned.

### Rule Configuration

**Basic Settings:**
- **Name**: Channel 1 Logo
- **Description**: Assigns logo to Channel 1 channels that don't already have a logo
- **Source Type**: Stream Sources
- **Scope**: Individual Items (per channel)
- **Active**: Yes

### Expression Syntax

```
(tvg_logo equals "") AND (channel_name contains "Channel 1") SET tvg_logo = "@logo:258c96ee-230c-4f31-8d6d-161b52e2426d"
```

### Manual Database Insert (Alternative)

If you prefer to add this rule directly to the database:

```sql
-- Insert the rule
INSERT INTO data_mapping_rules (id, name, description, source_type, scope, sort_order, is_active, created_at, updated_at)
VALUES (
    '550e8400-e29b-41d4-a716-446655440004',
    'Channel 1 Logo',
    'Assigns logo to Channel 1 channels that don''t already have a logo',
    'stream',
    'individual',
    2,
    1,
    datetime('now'),
    datetime('now')
);

-- Add condition to check tvg_logo is empty
INSERT INTO data_mapping_conditions (id, rule_id, field_name, operator, value, logical_operator, sort_order, created_at)
VALUES (
    '550e8400-e29b-41d4-a716-446655440006',
    '550e8400-e29b-41d4-a716-446655440004',
    'tvg_logo',
    'equals',
    '',
    'and',
    0,
    datetime('now')
);

-- Add condition to match Channel 1 channels
INSERT INTO data_mapping_conditions (id, rule_id, field_name, operator, value, logical_operator, sort_order, created_at)
VALUES (
    '550e8400-e29b-41d4-a716-446655440007',
    '550e8400-e29b-41d4-a716-446655440004',
    'channel_name',
    'contains',
    'Channel 1',
    'and',
    1,
    datetime('now')
);

-- Add action to set Channel 1 logo
INSERT INTO data_mapping_actions (id, rule_id, action_type, target_field, value, sort_order, created_at)
VALUES (
    '550e8400-e29b-41d4-a716-446655440008',
    '550e8400-e29b-41d4-a716-446655440004',
    'set_value',
    'tvg_logo',
    '@logo:258c96ee-230c-4f31-8d6d-161b52e2426d',
    0,
    datetime('now')
);
```

### How It Works

1. **Empty Logo Check**: First condition ensures `tvg_logo` is empty (`""`)
2. **Channel Matching**: Matches channels containing "Channel 1" in the name
3. **Logo Assignment**: Sets the logo to the specified UUID using `@logo:` format
4. **Logic**: Uses AND logic: `(empty logo) AND (Channel 1)`

### Expected Results

Channels like:
- "Channel 1 HD" ✅ (gets logo if no existing logo)
- "BBC Channel 1" ✅ (gets logo if no existing logo) 
- "News Channel 1" ✅ (gets logo if no existing logo)
- "Channel 1 Sports" ❌ (if already has a logo, keeps existing)

### Notes

- Replace `258c96ee-230c-4f31-8d6d-161b52e2426d` with your actual logo UUID
- You can find logo UUIDs in the Logos management section of the web interface
- The `@logo:` prefix tells the system to use an uploaded logo asset
- The rule only applies to channels that don't already have a logo assigned

## Other Example Rules

### Sports Channel Grouping

```
channel_name contains "sport" SET group_title = "Sports"
```

### HD Quality Indicator

```
channel_name contains "HD" SET tvg_name = channel_name + " [HD]"
```

### Remove Low Quality Channels

```
(channel_name contains "SD") OR (channel_name contains "LOW") SET remove_channel = true
```

### Timeshift Detection with Regex

```
channel_name matches ".*\s\+([0-9]+)h?\s.*" SET tvg_shift = "$1"
```

For more advanced rule creation, see the Data Mapping documentation in the web interface.