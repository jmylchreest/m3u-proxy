# Extended Expressions for Data Mapping

This document provides comprehensive examples of the advanced expression syntax supported by the M3U Proxy data mapping system. These expressions allow complex conditional logic with regex capture groups, multiple actions, and nested conditions.

## Table of Contents

1. [Basic Syntax](#basic-syntax)
2. [Simple Examples](#simple-examples)
3. [Intermediate Examples](#intermediate-examples)
4. [Complex Examples](#complex-examples)
5. [Advanced Features](#advanced-features)
6. [Real-World Use Cases](#real-world-use-cases)
7. [Troubleshooting](#troubleshooting)

## Basic Syntax

The general syntax for extended expressions is:

```
condition SET action [, action, ...]
```

Or for conditional action groups:

```
(condition SET action) AND (condition SET action)
```

### Available Operators

**Condition Operators:**
- `equals` - Exact match
- `contains` - Contains substring
- `starts_with` - Starts with text
- `ends_with` - Ends with text
- `matches` - Regex pattern match
- `not_equals` - Not equal to
- `not_contains` - Does not contain
- `not_matches` - Does not match regex

**Logical Operators:**
- `AND` - Both conditions must be true
- `OR` - Either condition can be true

**Action Operators:**
- `=` - Set value (overwrites existing)
- `?=` - Set if empty (only if field is null/empty)

### Available Fields

**Stream Sources:**
- `channel_name` - Channel display name
- `tvg_id` - TVG identifier
- `tvg_name` - TVG name
- `tvg_logo` - Logo URL
- `tvg_shift` - Timeshift offset
- `group_title` - Channel group
- `stream_url` - Stream URL

**EPG Sources:**
- `channel_id` - Channel identifier
- `channel_name` - Channel name
- `channel_logo` - Channel logo
- `channel_group` - Channel group
- `language` - Channel language

## Simple Examples

### 1. Basic Group Assignment
```
channel_name contains "sport" SET group_title = "Sports"
```
*Sets all channels containing "sport" to the Sports group*

### 2. Default Logo Assignment
```
tvg_logo equals "" SET tvg_logo = "https://example.com/default.png"
```
*Assigns default logo to channels without logos*

### 3. Language Standardization
```
language equals "en" SET language = "English"
```
*Converts language code to full name*

### 4. Multiple Actions
```
channel_name contains "HD" SET group_title = "HD Channels", tvg_logo = "https://example.com/hd.png"
```
*Sets both group and logo for HD channels*

### 5. Conditional Assignment
```
group_title equals "" SET group_title ?= "Uncategorized"
```
*Sets group only if it's currently empty*

## Intermediate Examples

### 6. Country-Based Grouping
```
tvg_id starts_with "uk." SET group_title = "UK Channels", tvg_logo = "https://logos.com/uk.png"
```
*Groups UK channels and assigns country logo*

### 7. Multiple Condition Matching
```
channel_name contains "BBC" AND channel_name contains "HD" SET group_title = "BBC HD"
```
*Specific grouping for BBC HD channels*

### 8. Exclusion Logic
```
channel_name contains "sport" AND channel_name not_contains "news" SET group_title = "Sports"
```
*Sports channels excluding sports news*

### 9. Regex Pattern Matching
```
channel_name matches "^([A-Z]+) .*" SET tvg_id = "$1"
```
*Extracts network prefix as TVG ID*

### 10. Timeshift Extraction
```
channel_name matches "(.+) \\+([0-9]+)" SET channel_name = "$1", tvg_shift = "$2"
```
*Separates channel name from timeshift indicator*

## Complex Examples

### 11. Multi-Pattern Channel Cleanup
```
(channel_name matches "^\\[.*?\\] (.+)" SET channel_name = "$1") AND 
(channel_name matches "(.+) - \\d{4}$" SET channel_name = "$1") AND
(channel_name matches "(.+) \\(.*\\)$" SET channel_name = "$1")
```
*Removes brackets, years, and parenthetical info from channel names*

### 12. Advanced Regional Grouping
```
(tvg_id matches "^(uk|gb)\\." SET group_title = "United Kingdom") OR
(tvg_id matches "^us\\." SET group_title = "United States") OR
(tvg_id matches "^ca\\." SET group_title = "Canada") OR
(tvg_id matches "^au\\." SET group_title = "Australia")
```
*Groups channels by country code in TVG ID*

### 13. Complex News Channel Organization
```
(channel_name matches ".*(?i)(news|cnn|bbc|sky)" AND channel_name not_matches ".*(?i)(sport|weather)" SET group_title = "News") AND
(channel_name matches ".*(?i)(sport|espn|eurosport)" SET group_title = "Sports") AND
(channel_name matches ".*(?i)(weather|meteo)" SET group_title = "Weather")
```
*Categorizes channels with overlapping keywords*

### 14. Quality-Based Grouping with Logo Assignment
```
(channel_name matches ".*\\b(4K|UHD)\\b.*" SET group_title = "4K Ultra HD", tvg_logo = "@logo:4k-badge") AND
(channel_name matches ".*\\b(HD|1080p)\\b.*" AND channel_name not_matches ".*\\b(4K|UHD)\\b.*" SET group_title = "HD Channels", tvg_logo = "@logo:hd-badge") AND
(channel_name not_matches ".*\\b(HD|1080p|4K|UHD)\\b.*" SET group_title = "Standard Definition")
```
*Groups by video quality with appropriate logos*

### 15. Language and Region Matrix
```
(tvg_id matches "^uk\\." AND language equals "en" SET group_title = "UK English") OR
(tvg_id matches "^uk\\." AND language equals "cy" SET group_title = "UK Welsh") OR
(tvg_id matches "^fr\\." AND language equals "fr" SET group_title = "France") OR
(tvg_id matches "^de\\." AND language equals "de" SET group_title = "Germany") OR
(tvg_id matches "^es\\." AND language equals "es" SET group_title = "Spain")
```
*Matrix grouping based on country and language*

## Advanced Features

### 16. Nested Conditional Logic
```
(
  (channel_name matches "^(BBC|ITV|Channel [45])" AND tvg_id not_equals "") OR
  (channel_name matches "Sky (Sports|Movies|News)" AND group_title equals "")
) SET group_title = "Premium UK"
```
*Complex nested conditions with multiple criteria*

### 17. Multi-Stage Processing
```
(channel_name matches "^\\[([A-Z]{2,3})\\] (.+)" SET tvg_id = "$1", channel_name = "$2") AND
(tvg_id matches "^(BBC|ITV|C4|C5)$" SET group_title = "UK Terrestrial") AND
(tvg_id matches "^SKY" SET group_title = "Sky")
```
*Sequential processing: extract country code, then categorize*

### 18. Dynamic Logo Assignment
```
(channel_name matches "^(BBC One|BBC Two|BBC Three)" SET tvg_logo = "@logo:bbc-$1") AND
(channel_name matches "^(ITV|ITV2|ITV3|ITV4)" SET tvg_logo = "@logo:itv-$1") AND
(channel_name matches "^Sky (Sports|Movies|News)" SET tvg_logo = "@logo:sky-$1")
```
*Dynamic logo selection based on captured groups*

### 19. Comprehensive Channel Normalization
```
(
  channel_name matches "^(.+?) *(?:\\|| - |: ).*(?:HD|FHD|4K|UHD)" SET 
  channel_name = "$1",
  group_title ?= "High Definition"
) AND (
  channel_name matches "^(.+?) *\\+([0-9]+)h?$" SET
  channel_name = "$1",
  tvg_shift = "$2"
) AND (
  channel_name matches "^\\[.*?\\]\\s*(.+)" SET
  channel_name = "$1"
) AND (
  group_title equals "" SET group_title = "General"
)
```
*Complete channel name cleanup and categorization*

### 20. Advanced Sports Channel Matrix
```
(
  (channel_name matches ".*(?i)(premier|football|soccer)" AND channel_name not_matches ".*(?i)(news|talk)") SET
  group_title = "Football",
  tvg_logo = "@logo:football"
) OR (
  (channel_name matches ".*(?i)(espn|eurosport)" AND channel_name matches ".*(?i)(tennis|golf|motorsport)") SET
  group_title = "Premium Sports"
) OR (
  (channel_name matches ".*(?i)(sky sports)" AND channel_name matches ".*(?i)(f1|formula)") SET
  group_title = "Motorsports",
  tvg_shift ?= "0"
)
```
*Complex sports categorization with multiple criteria*

## Real-World Use Cases

### Provider Cleanup Examples

**Sky UK Channel Normalization:**
```
(channel_name matches "Sky (Sports|Movies|News) (.+)" SET channel_name = "Sky $1 $2", group_title = "Sky") AND
(channel_name starts_with "Sky Sports" SET group_title = "Sky Sports") AND
(channel_name starts_with "Sky Movies" SET group_title = "Sky Movies")
```

**US Cable Provider Cleanup:**
```
(channel_name matches "^([A-Z]+) East \\+([0-9]+)" SET channel_name = "$1 East", tvg_shift = "$2") AND
(channel_name matches "^([A-Z]+) West \\+([0-9]+)" SET channel_name = "$1 West", tvg_shift = "$2") AND
(channel_name contains "ESPN" SET group_title = "ESPN Family")
```

**European Multi-Language Setup:**
```
(tvg_id matches "^de\\." AND language equals "de" SET group_title = "Deutschland") AND
(tvg_id matches "^fr\\." AND language equals "fr" SET group_title = "France") AND
(tvg_id matches "^it\\." AND language equals "it" SET group_title = "Italia") AND
(language equals "" SET language ?= "en")
```

### IPTV Provider Integration

**Regional Sports Networks:**
```
(
  channel_name matches "^(Fox Sports|ESPN) (\\w+ \\w+)" SET
  group_title = "Regional Sports",
  tvg_logo = "@logo:$1-regional"
) AND (
  channel_name matches "^NBC Sports (\\w+)" SET
  group_title = "NBC Sports Regional",
  tvg_id = "nbc-sports-$1"
)
```

**International News Channels:**
```
(channel_name matches "^(BBC|CNN|Al Jazeera|RT|France 24)" SET group_title = "International News") AND
(channel_name matches "^(ABC|CBS|NBC|FOX) News" SET group_title = "US Network News") AND
(channel_name contains "local" OR channel_name contains "Local" SET group_title = "Local News")
```

## Troubleshooting

### Common Issues

1. **Regex Escaping**: Use double backslashes `\\` for literal backslashes
2. **Case Sensitivity**: Use `(?i)` for case-insensitive regex matching
3. **Capture Groups**: Reference with `$1`, `$2`, etc.
4. **Quotes**: Always use double quotes `"` for string values
5. **Empty Checks**: Use `equals ""` to check for empty fields

### Testing Tips

1. Start with simple expressions and build complexity gradually
2. Test regex patterns in a regex tester before using them
3. Use the preview function to verify results before saving
4. Check logs for detailed parsing information

### Performance Considerations

1. Simple `contains` and `equals` operations are faster than regex
2. Use specific patterns rather than broad wildcards
3. Order conditions from most specific to least specific
4. Consider using multiple simple rules instead of one complex rule

### Best Practices

1. **Document your expressions**: Add comments explaining complex logic
2. **Use consistent naming**: Standardize group names and TVG IDs
3. **Test thoroughly**: Always preview changes before applying
4. **Keep backups**: Export rules before making major changes
5. **Monitor performance**: Watch for rules that significantly slow processing

## Logo Reference Syntax

### Standard Logo Assignment
```
tvg_logo = "https://example.com/logo.png"
```

### Logo Asset Reference (Autocomplete Supported)
```
tvg_logo = "@logo:channel-name"
```

### Dynamic Logo Assignment
```
channel_name matches "^(BBC|ITV|Channel 4)" SET tvg_logo = "@logo:$1"
```

This reference system allows for efficient logo management and automatic completion in the UI.