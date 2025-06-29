# Extended Filter Syntax Documentation

## Table of Contents
1. [Basic Syntax](#basic-syntax)
2. [Action Syntax](#action-syntax)
3. [Assignment Operators](#assignment-operators)
4. [Field Reference](#field-reference)
5. [Practical Examples](#practical-examples)
6. [Common Patterns](#common-patterns)
7. [Troubleshooting](#troubleshooting)

## Basic Syntax

The extended filter syntax builds upon the existing natural language filter system by adding action capabilities. The basic structure is:

```
condition_expression [SET action_list]
```

### Condition Expressions (Existing)

These remain unchanged and fully compatible:

```
field operator "value"
field operator "value" AND field operator "value"
field operator "value" OR field operator "value"
(field operator "value" AND field operator "value") OR field operator "value"
not field operator "value"
case_sensitive field operator "value"
```

#### Available Operators
- `contains` - Field contains the substring
- `equals` - Field exactly matches the value
- `matches` - Field matches the regex pattern
- `starts_with` - Field starts with the value
- `ends_with` - Field ends with the value
- `not_contains` - Field does not contain the substring
- `not_equals` - Field does not exactly match the value
- `not_matches` - Field does not match the regex pattern

#### Available Fields (Stream Sources)
- `channel_name` - Channel display name
- `group_title` - Channel group/category
- `tvg_id` - Electronic Program Guide ID
- `tvg_name` - Alternative channel name
- `tvg_logo` - Logo URL
- `tvg_shift` - Time shift offset
- `stream_url` - Stream URL
- `language` - Channel language

## Action Syntax

Actions are introduced with the `SET` keyword and allow you to modify field values when conditions match:

```
condition_expression SET field = "value"
condition_expression SET field1 = "value1", field2 = "value2"
```

### Basic Action Examples

**Set a default group:**
```
group_title equals "" SET group_title = "General"
```

**Categorize sports channels:**
```
channel_name contains "sport" SET group_title = "Sports"
```

**Add logo for BBC channels:**
```
tvg_id starts_with "bbc" SET tvg_logo = "https://logos.example.com/bbc.png"
```

## Assignment Operators

### `=` (Set Value)
Overwrites the field with the new value.

```
group_title equals "" SET group_title = "Uncategorized"
channel_name contains "bbc" SET tvg_logo = "bbc-logo.png"
```

### `+=` (Append Value)
Adds content to the existing field value (with space separator).

```
channel_name contains "HD" SET channel_name += "[High Definition]"
group_title contains "sport" SET group_title += "- Premium"
```

**Before:** `BBC One HD`  
**After:** `BBC One HD [High Definition]`

### `?=` (Set If Empty)
Sets the value only if the field is currently empty or null.

```
group_title equals "" SET group_title ?= "General"
tvg_logo equals "" SET tvg_logo ?= "default-logo.png"
```

### `-=` (Remove Value)
Removes all occurrences of the substring from the field.

```
channel_name contains "[AD]" SET channel_name -= "[AD]"
channel_name contains "  " SET channel_name -= "  "
```

**Before:** `BBC One [AD] Drama`  
**After:** `BBC One Drama`

## Field Reference

### Stream Source Fields

| Field | Type | Description | Example Values |
|-------|------|-------------|----------------|
| `channel_name` | String | Channel display name | `"BBC One HD"`, `"CNN International"` |
| `group_title` | String | Channel group/category | `"News"`, `"Sports"`, `"Entertainment"` |
| `tvg_id` | String | EPG identifier | `"bbc1.uk"`, `"cnn.us"` |
| `tvg_name` | String | Alternative name | `"BBC1"`, `"CNN"` |
| `tvg_logo` | String | Logo URL | `"https://example.com/logo.png"` |
| `tvg_shift` | String | Time offset | `"+1"`, `"-2"`, `"0"` |
| `stream_url` | String | Stream URL | `"http://stream.example.com/live"` |
| `language` | String | Channel language | `"en"`, `"es"`, `"fr"` |

### EPG Source Fields

| Field | Type | Description | Example Values |
|-------|------|-------------|----------------|
| `program_title` | String | Program name | `"BBC News"`, `"The Office"` |
| `program_description` | String | Program description | `"Latest news and weather"` |
| `category` | String | Program category | `"News"`, `"Comedy"`, `"Drama"` |
| `language` | String | Program language | `"en"`, `"es"`, `"fr"` |
| `channel_id` | String | Channel identifier | `"bbc1"`, `"cnn"` |
| `channel_name` | String | Channel name | `"BBC One"`, `"CNN"` |
| `channel_logo` | String | Channel logo | `"logo.png"` |
| `channel_group` | String | Channel group | `"UK"`, `"News"` |

## Practical Examples

### Default Value Assignment
```
group_title equals "" SET group_title = "Uncategorized"
language equals "" SET language = "en"
tvg_logo equals "" SET tvg_logo = "default-logo.png"
```

### Sports Channel Organization
```
(channel_name contains "sport" OR channel_name contains "football" OR channel_name contains "soccer") 
SET group_title = "Sports", tvg_logo ?= "sports-default.png"
```

### News Channel Categorization
```
(channel_name contains "news" OR channel_name contains "cnn" OR channel_name contains "bbc news") 
SET group_title = "News", category = "information"
```

### Logo Assignment by Provider
```
tvg_id starts_with "bbc" SET tvg_logo = "https://logos.example.com/bbc.png"
tvg_id starts_with "itv" SET tvg_logo = "https://logos.example.com/itv.png"
tvg_id starts_with "sky" SET tvg_logo = "https://logos.example.com/sky.png"
```

### Channel Name Cleanup
```
channel_name contains "[AD]" SET 
    channel_name -= "[AD]",
    channel_name -= "  ",
    group_title += " (Audio Description)"
```

### HD Channel Enhancement
```
channel_name contains "HD" AND not channel_name contains "[HD]" 
SET channel_name += " [HD]", group_title += " - High Definition"
```

### Language Detection and Assignment
```
channel_name matches ".*\\b(UK|GB)\\b.*" SET language ?= "en"
channel_name matches ".*\\b(ES|Spain)\\b.*" SET language ?= "es"
channel_name matches ".*\\b(FR|France)\\b.*" SET language ?= "fr"
```

### Time Shift Channel Handling
```
channel_name matches ".*\\+([0-9]+).*" SET 
    tvg_shift = "$1",
    group_title += " - Timeshift"
```

### Multi-Condition Categorization
```
(tvg_id contains "premium" OR channel_name contains "premium" OR group_title contains "premium") 
AND not group_title contains "Premium" 
SET group_title = "Premium " + group_title
```

## Common Patterns

### 1. Default Value Pattern
Set default values for empty fields:
```
field equals "" SET field ?= "default_value"
```

### 2. Categorization Pattern
Categorize content based on keywords:
```
channel_name contains "keyword" SET group_title = "Category"
```

### 3. Cleanup Pattern
Remove unwanted text from fields:
```
channel_name contains "unwanted" SET channel_name -= "unwanted"
```

### 4. Enhancement Pattern
Add additional information to existing content:
```
condition SET field += " [additional_info]"
```

### 5. Provider-Based Pattern
Apply settings based on content provider:
```
tvg_id starts_with "provider" SET multiple_fields = "provider_specific_values"
```

### 6. Conditional Assignment Pattern
Set values only when certain conditions are met:
```
condition1 AND condition2 SET field = "value"
```

### 7. Multi-Field Update Pattern
Update multiple related fields together:
```
condition SET field1 = "value1", field2 = "value2", field3 = "value3"
```

## Troubleshooting

### Common Syntax Errors

**Missing quotes around values:**
```
❌ channel_name contains sport SET group_title = Sports
✅ channel_name contains "sport" SET group_title = "Sports"
```

**Invalid assignment operator:**
```
❌ channel_name contains "sport" SET group_title == "Sports"
✅ channel_name contains "sport" SET group_title = "Sports"
```

**Missing comma between actions:**
```
❌ condition SET field1 = "value1" field2 = "value2"
✅ condition SET field1 = "value1", field2 = "value2"
```

**Unmatched parentheses:**
```
❌ (channel_name contains "sport" AND group_title equals "TV" SET group_title = "Sports"
✅ (channel_name contains "sport" AND group_title equals "TV") SET group_title = "Sports"
```

### Common Logic Errors

**Using wrong operator for field type:**
```
❌ tvg_shift += "extra_text"    # tvg_shift should be numeric
✅ tvg_shift = "1"              # Set proper numeric value
```

**Circular or conflicting actions:**
```
❌ channel_name contains "BBC" SET channel_name = "ITV"  # Confusing transformation
✅ channel_name contains "BBC" SET group_title = "BBC Channels"  # Clear intent
```

**Overly complex conditions:**
```
❌ Very long condition with many nested parentheses...
✅ Break into multiple simpler rules
```

### Validation Messages

The system provides helpful error messages:

- **Syntax errors**: Point to specific character positions
- **Field errors**: List available fields for the source type
- **Operator errors**: Suggest correct operators
- **Value errors**: Indicate format requirements

### Performance Tips

1. **Simple conditions first**: Put the most selective conditions early
2. **Avoid complex regex**: Use simple operators when possible
3. **Minimal actions**: Only update fields that actually need changes
4. **Test incrementally**: Start with simple rules and add complexity

### Best Practices

1. **Use descriptive names**: Make rules self-documenting
2. **Test with sample data**: Verify rules work as expected
3. **Document complex logic**: Add comments for complex patterns
4. **Version control**: Track rule changes over time
5. **Monitor performance**: Watch for slow-running rules

## Migration from Form-Based Rules

### Before (Form-Based)
- Field: `group_title`
- Operator: `equals`
- Value: `""`
- Action: Set to `"General"`

### After (Syntax-Based)
```
group_title equals "" SET group_title = "General"
```

### Benefits of Syntax-Based Rules
- **More expressive**: Complex conditions and multiple actions
- **Readable**: Natural language format
- **Versionable**: Text-based rules work with version control
- **Shareable**: Easy to copy, paste, and share rules
- **Powerful**: Supports advanced patterns and logic

This documentation provides a comprehensive guide to using the extended filter syntax effectively for both filtering and data transformation tasks.