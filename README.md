# M3U Proxy

A modern, high-performance M3U proxy service built in Rust for managing and filtering IPTV streams from multiple sources.

## Features

- **Multiple Source Support**: Ingest from M3U playlists and Xtream Codes APIs
- **EPG Integration**: Complete XMLTV EPG support with advanced processing
- **Advanced Filtering**: Pattern-based filtering with regex support for both streams and EPG data
- **Data Mapping & Transformation**: Sophisticated channel metadata transformation system
- **Timeshift Channel Support**: Automatic EPG time adjustment for +1h, +24h channels
- **Stream Proxies**: Create filtered channel lists with versioning
- **Logo Caching**: Automatic caching and serving of channel logos
- **Web Interface**: Modern web UI for management and configuration
- **Database Support**: SQLite (default), PostgreSQL, MySQL, MariaDB
- **Scheduled Updates**: Configurable cron-based source updates
- **RESTful API**: Complete API for programmatic management
- **Self-Contained Binary**: All assets and migrations embedded for zero-dependency deployment
- **Shared Components**: Modular web interface with reusable templates and components

## Quick Start

### Pre-built Binary

Download the latest release binary - it's completely self-contained with all web assets and database migrations embedded:

```bash
# Download and run (no additional files needed)
wget https://github.com/jmylchreest/m3u-proxy/releases/latest/download/m3u-proxy
chmod +x m3u-proxy
./m3u-proxy
```

### Building from Source

```bash
# Clone the repository
git clone https://github.com/jmylchreest/m3u-proxy.git
cd m3u-proxy

# Build the project
cargo build --release

# Run the application
./target/release/m3u-proxy
```

### Memory Tracking with jemalloc (Recommended)

For production deployments, enable jemalloc for accurate memory tracking and better performance:

```bash
# Build with jemalloc support
cargo build --release --features jemalloc

# Run with jemalloc
./target/release/m3u-proxy
```

#### jemalloc Benefits

- **Accurate Memory Tracking**: Real allocator statistics instead of estimates
- **Better Performance**: Optimized memory allocation for high-throughput workloads
- **Memory Limit Enforcement**: Precise memory usage monitoring and warnings
- **Cross-Platform**: Works on Linux, macOS, and Windows

#### Memory Configuration

Configure memory limits in `config.toml`:

```toml
[proxy_generation.memory]
max_memory_mb = 512              # Memory limit for proxy generation
batch_size = 1000               # Channels processed per batch
enable_parallel_processing = true
memory_check_interval = 100     # Memory check frequency
warning_threshold = 0.8         # Warn at 80% of limit
```

#### Without jemalloc

If jemalloc is not available, the system automatically falls back to simple memory tracking:

```bash
# Standard build (uses system allocator)
cargo build --release

# Still provides memory tracking, just less accurate
./target/release/m3u-proxy
```

### Command Line Options

The application supports several command-line arguments for configuration:

```bash
# Show help
./target/release/m3u-proxy --help

# Show version
./target/release/m3u-proxy --version

# Basic usage with custom IP and port
./target/release/m3u-proxy --host 127.0.0.1 --port 3000

# Use custom configuration file
./target/release/m3u-proxy --config /path/to/custom-config.toml

# Override database URL
./target/release/m3u-proxy --database-url "postgresql://user:pass@localhost/m3u_proxy"

# Set log level
./target/release/m3u-proxy --log-level debug

# Combine multiple options
./target/release/m3u-proxy \
  --host 0.0.0.0 \
  --port 8080 \
  --config production.toml \
  --database-url "sqlite://./prod.db" \
  --log-level info
```

#### Available Options

| Option | Short | Description | Default |
|--------|-------|-------------|---------|
| `--config` | `-c` | Configuration file path | `config.toml` |
| `--host` | `-H` | Listening IP address | From config file |
| `--port` | `-p` | Listening port | From config file |
| `--database-url` | `-d` | Database URL (overrides config) | From config file |
| `--log-level` | `-v` | Log level (trace, debug, info, warn, error) | `info` |
| `--help` | `-h` | Show help message | - |
| `--version` | `-V` | Show version information | - |

### Configuration

The application will create a default `config.toml` file on first run:

```toml
[database]
url = "sqlite://./m3u-proxy.db"
max_connections = 10

[web]
host = "0.0.0.0"
port = 8080

[storage]
m3u_path = "./data/m3u"
logo_path = "./data/logos"
proxy_versions_to_keep = 3
```

### Access the Web Interface

Once running, access the web interface at: `http://localhost:8080`

## Architecture

### Core Components

- **Stream Sources**: M3U or Xtream Codes endpoints that provide channel data
- **EPG Sources**: XMLTV or Xtream Codes EPG endpoints that provide program guide data
- **Data Mapping**: Transform channel and EPG metadata during proxy generation
- **Filters**: Pattern-based rules for including/excluding streams and EPG data
- **Stream Proxies**: Collections of sources with applied transformations and filters
- **Ingestor**: Scheduled service for updating source and EPG data
- **Proxy Generator**: Creates filtered M3U files and XMLTV EPG with versioning

### Data Processing Flow

The M3U Proxy features a comprehensive dual-pipeline architecture for both stream and EPG data processing, preserving original data integrity while enabling sophisticated transformations:

```mermaid
graph TD
    A[Stream Sources] --> B[Stream Ingestor]
    A2[EPG Sources] --> B2[EPG Ingestor]
    B --> C[Original Stream Data]
    B2 --> C2[Original EPG Data]
    C --> D[Database]
    C2 --> D
    
    E[Proxy Request] --> F[Proxy Generator]
    F --> G[Retrieve Original Data]
    G --> H[Apply Stream Data Mapping]
    G --> H2[Apply EPG Data Mapping]
    H --> I[Apply Stream Filters]
    H2 --> I2[Apply EPG Filters]
    I --> J[Generate M3U Output]
    I2 --> J2[Generate XMLTV Output]
    
    K[Stream Mapping Rules] --> H
    K2[EPG Mapping Rules] --> H2
    L[Stream Filters] --> I
    L2[EPG Filters] --> I2
    
    D --> G
    
    subgraph "Ingestion Pipeline"
        A
        A2
        B
        B2
        C
        C2
        D
    end
    
    subgraph "Proxy Generation Pipeline"
        E
        F
        G
        H
        H2
        I
        I2
        J
        J2
    end
    
    subgraph "Stream Configuration"
        K
        L
    end
    
    subgraph "EPG Configuration"
        K2
        L2
    end
    
    style C fill:#e1f5fe
    style C2 fill:#e1f5fe
    style H fill:#f3e5f5
    style H2 fill:#f3e5f5
    style I fill:#e8f5e8
    style I2 fill:#e8f5e8
    style J fill:#fff3e0
    style J2 fill:#fff3e0
```

#### Enhanced Processing Stages

1. **Dual Ingestion Stage**:
   - **Stream sources** (M3U/Xtream) provide channel and stream data
   - **EPG sources** (XMLTV/Xtream) provide program guide data
   - **Original data is preserved** unchanged in the database
   - No data mapping or filtering occurs during ingestion
   - Supports multiple overlapping sources with conflict resolution

2. **Advanced Data Mapping Stage**:
   - **Stream Data Mapping**: Transform channel metadata (names, groups, logos)
   - **EPG Data Mapping**: Advanced EPG processing including:
     - **Timeshift Support**: Detect and handle +1h, +24h timeshift channels with automatic program time adjustment
     - **Field Transformation**: Normalize EPG channel names, groups, and metadata
   - **Transient Processing**: Mapped data exists only during proxy generation, not stored

3. **Dual Filter Application**:
   - **Stream Filters**: Include/exclude channels based on stream metadata
   - **EPG Filters**: Filter EPG data based on channel info, program titles, categories, language
   - **Source Type Awareness**: Different filter fields available for stream vs EPG data
   - **Cascading Logic**: Filters applied to mapped data, supporting complex selection rules

4. **Unified Output Generation**:
   - **M3U Generation**: Filtered and transformed channel list with proper numbering
   - **XMLTV Generation**: Corresponding EPG data with deduplicated channels and adjusted timeshift programs
   - **Coordinated Output**: Stream and EPG outputs are synchronized for optimal compatibility

#### EPG-Specific Features

- **Timeshift Channel Support**: Automatic EPG time adjustment for timeshift channels:
  - Timeshift variants: +1, +24, +1h, +24h
  - Custom patterns: Configurable regex patterns for timeshift detection
- **Timeshift Processing**: Automatically shifts program times for timeshift channels
- **Language Support**: EPG language detection and filtering capabilities
- **Conflict Resolution**: Intelligent handling of overlapping EPG data from multiple sources

#### Benefits of Enhanced Architecture

- **Unified Processing**: Single system handles both stream and EPG data consistently
- **Advanced EPG Support**: Sophisticated EPG processing eliminates manual configuration
- **Timeshift Automation**: No manual EPG time adjustment needed for timeshift channels
- **Source Independence**: Multiple EPG sources can be combined without conflicts
- **Performance Optimized**: Transient data mapping reduces database overhead
- **Data Integrity**: Original source data remains untouched for reliability

### Database Schema

The application uses a relational database with the following main tables:

#### Stream Management
- `stream_sources`: Source configurations (M3U URLs, Xtream credentials)
- `channels`: **Original ingested channel data** (unchanged by data mapping)
- `stream_proxies`: Proxy definitions with unique ULIDs
- `proxy_generations`: Versioned M3U outputs with applied transformations

#### EPG Management
- `epg_sources`: EPG source configurations (XMLTV URLs, Xtream EPG endpoints)
- `epg_channels`: **Original EPG channel data** (preserved unchanged)
- `epg_programs`: **Original EPG program data** (preserved unchanged)
- `channel_epg_mapping`: Links between stream channels and EPG channels

#### Data Processing
- `data_mapping_rules`: Rules for transforming stream and EPG metadata during proxy generation
- `data_mapping_conditions`: Conditions that trigger data mapping rules
- `data_mapping_actions`: Actions to perform when conditions are met
- `filters`: Filter patterns and configurations for stream and EPG data
- `filter_conditions`: Conditions for filter matching

#### Assets and Configuration
- `logo_assets`: Cached channel logos and custom logo uploads
- `proxy_filters`: Links between proxies and their associated filters

## API Endpoints

### Sources (Unified API)

- `GET /api/v1/sources` - List all sources (both stream and EPG)
- `GET /api/v1/sources/stream` - List all stream sources
- `POST /api/v1/sources/stream` - Create new stream source
- `GET /api/v1/sources/stream/{id}` - Get stream source details
- `PUT /api/v1/sources/stream/{id}` - Update stream source
- `DELETE /api/v1/sources/stream/{id}` - Delete stream source
- `POST /api/v1/sources/stream/{id}/refresh` - Trigger manual stream source refresh
- `POST /api/v1/sources/stream/{id}/cancel` - Cancel stream source ingestion
- `GET /api/v1/sources/stream/{id}/progress` - Get stream source progress
- `GET /api/v1/sources/stream/{id}/processing` - Get stream source processing info
- `GET /api/v1/sources/stream/{id}/channels` - Get stream source channels

### EPG Sources

- `GET /api/v1/sources/epg` - List all EPG sources
- `POST /api/v1/sources/epg` - Create new EPG source
- `GET /api/v1/sources/epg/{id}` - Get EPG source details
- `PUT /api/v1/sources/epg/{id}` - Update EPG source
- `DELETE /api/v1/sources/epg/{id}` - Delete EPG source
- `POST /api/v1/sources/epg/{id}/refresh` - Trigger manual EPG refresh
- `GET /api/v1/sources/epg/{id}/channels` - Get EPG source channels

### Stream Proxies
- `GET /api/v1/proxies` - List all proxies
- `POST /api/v1/proxies` - Create new proxy
- `GET /api/v1/proxies/{id}` - Get proxy details
- `PUT /api/v1/proxies/{id}` - Update proxy
- `DELETE /api/v1/proxies/{id}` - Delete proxy

### Data Mapping
- `GET /api/v1/data-mapping/rules` - List all data mapping rules
- `POST /api/v1/data-mapping/rules` - Create new data mapping rule
- `GET /api/v1/data-mapping/rules/{id}` - Get rule details
- `PUT /api/v1/data-mapping/rules/{id}` - Update data mapping rule
- `DELETE /api/v1/data-mapping/rules/{id}` - Delete data mapping rule
- `POST /api/v1/data-mapping/test` - Test data mapping rule against source data
- `GET /api/v1/data-mapping/preview/{source_type}` - Preview all rules for source type
- `GET /api/v1/data-mapping/fields/{source_type}` - Get available fields for source type

### Filters
- `GET /api/v1/filters` - List all filters
- `POST /api/v1/filters` - Create new filter
- `GET /api/v1/filters/{id}` - Get filter details
- `PUT /api/v1/filters/{id}` - Update filter
- `DELETE /api/v1/filters/{id}` - Delete filter
- `POST /api/v1/filters/test` - Test filter against source data
- `GET /api/v1/filters/fields/{source_type}` - Get available filter fields for source type

### EPG Data
- `GET /api/v1/epg/channels` - List EPG channels with filtering
- `GET /api/v1/epg/programs` - Get EPG program data
- `GET /api/v1/epg/viewer` - Get formatted EPG data for viewer interface

### Proxy Output
- `GET /proxy/{ulid}.m3u8` - Get generated M3U playlist
- `GET /proxy/{ulid}.xmltv` - Get generated XMLTV EPG data
- `GET /logos/{logo_id}` - Get cached logo

## Stream Source Configuration

### M3U Sources

```json
{
  "name": "Provider 1",
  "source_type": "m3u",
  "url": "http://example.com/playlist.m3u",
  "max_concurrent_streams": 2,
  "update_cron": "0 */6 * * *",
  "field_map": {
    "group_field": "group-title",
    "logo_field": "tvg-logo",
    "tvg_id_field": "tvg-id",
    "tvg_name_field": "tvg-name"
  }
}
```

### Xtream Sources

```json
{
  "name": "Provider 2",
  "source_type": "xtream",
  "url": "http://example.com:8080",
  "username": "user123",
  "password": "pass123",
  "max_concurrent_streams": 1,
  "update_cron": "0 */4 * * *"
}
```

## EPG Source Configuration

### XMLTV Sources

```json
{
  "name": "Provider 1 EPG",
  "source_type": "xmltv",
  "url": "http://example.com/epg.xml.gz",
  "update_cron": "0 2 * * *",
  "timezone": "UTC",
  "time_offset": "+0h"
}
```

### Xtream EPG Sources

```json
{
  "name": "Provider 2 EPG",
  "source_type": "xtream",
  "url": "http://example.com:8080",
  "username": "user123",
  "password": "pass123",
  "update_cron": "0 3 * * *",
  "timezone": "Europe/London",
  "time_offset": "+1h"
}
```

### EPG Configuration Options

| Field | Description | Example |
|-------|-------------|---------|
| `name` | Display name for the EPG source | `"UK EPG Data"` |
| `source_type` | Type of EPG source | `"xmltv"` or `"xtream"` |
| `url` | EPG source URL | `"http://example.com/epg.xml"` |
| `update_cron` | Cron schedule for EPG updates | `"0 2 * * *"` (daily at 2 AM) |
| `timezone` | Source timezone (auto-detected for XMLTV) | `"Europe/London"` |
| `time_offset` | Manual time adjustment | `"+1h"`, `"-30m"`, `"+0h"` |
| `username` | Username for Xtream sources | `"user123"` |
| `password` | Password for Xtream sources | `"pass123"` |

## Filter Configuration

Filters support natural language patterns with logical operators and field-specific matching:

### Natural Language Filter Patterns

Filters use an intuitive syntax that allows you to specify conditions using natural language operators:

```
channel_name contains "sport" AND group_title not contains "adult"
```

### Supported Operators

| Operator | Description | Example |
|----------|-------------|---------|
| `contains` | Text contains substring (case insensitive) | `channel_name contains "news"` |
| `equals` | Exact text match (case insensitive) | `group_title equals "Sports"` |
| `matches` | Regular expression match (see [Rust regex syntax](https://docs.rs/regex/latest/regex/#syntax)) | `channel_name matches "(hd|4k|uhd)"` |
| `starts_with` | Text starts with substring | `channel_name starts_with "BBC"` |
| `ends_with` | Text ends with substring | `channel_name ends_with "HD"` |

### Modifiers

| Modifier | Description | Example |
|----------|-------------|---------|
| `not` | Negates the operator | `channel_name not contains "adult"` or `not channel_name contains "adult"` |
| `case_sensitive` | Makes the match case sensitive | `channel_name case_sensitive equals "BBC One"` |

### Logical Operators

| Operator | Description | Example |
|----------|-------------|---------|
| `AND` | All conditions must match | `channel_name contains "sport" AND group_title equals "HD"` |
| `OR` | Any condition can match | `channel_name contains "sport" OR channel_name contains "news"` |

### Available Fields

#### Stream Filter Fields
- `channel_name` - The display name of the channel
- `group_title` - The group/category the channel belongs to
- `tvg_id` - The TVG ID identifier
- `tvg_name` - The TVG name
- `tvg_logo` - The channel logo URL
- `stream_url` - The streaming URL

#### EPG Filter Fields
- `channel_id` - EPG channel identifier
- `channel_name` - EPG channel name
- `channel_logo` - EPG channel logo URL
- `channel_group` - EPG channel group/category
- `language` - EPG channel language
- `program_title` - Program/show title
- `program_category` - Program category/genre
- `program_description` - Program description/synopsis

### Complex Pattern Examples

```
# Include sports channels but exclude adult content
channel_name contains "sport" AND group_title not contains "adult"

# Multiple conditions with grouping
(channel_name contains "BBC" OR channel_name contains "ITV") AND channel_name ends_with "HD"

# Case sensitive matching
channel_name case_sensitive equals "BBC One" OR channel_name case_sensitive equals "BBC Two"

# Negation examples (two valid syntaxes)
channel_name not contains "adult" AND not group_title contains "xxx"

# Combined modifiers
not case_sensitive channel_name equals "TEST CHANNEL"

# Regular expression matching (case-insensitive by default)
# See https://docs.rs/regex/latest/regex/#syntax for supported patterns
channel_name matches "(hd|4k|uhd)" AND group_title not matches ".*adult.*"
```

### Legacy JSON Format (Deprecated)

The old JSON format with regex patterns is still supported but deprecated:

```json
{
  "name": "UK Channels Only",
  "pattern": "UK\\|.*",
  "starting_channel_number": 1,
  "is_inverse": false
}
```

## Data Mapping Configuration

Data mapping allows you to transform both stream and EPG metadata during proxy generation without modifying the original source data. The system supports separate rules for Stream and EPG sources with specialized actions for each.

### Data Mapping Rules

Rules consist of **source type**, **conditions** (when to apply), and **actions** (what to change):

#### Stream Data Mapping Example

```json
{
  "name": "Clean Stream Channel Names",
  "description": "Remove HD/FHD suffixes and standardize names",
  "source_type": "stream",
  "is_active": true,
  "conditions": [
    {
      "field_name": "channel_name",
      "operator": "contains",
      "value": " HD",
      "logical_operator": "and"
    }
  ],
  "actions": [
    {
      "action_type": "set_value",
      "target_field": "channel_name",
      "value": "Clean Channel Name"
    },
    {
      "action_type": "set_value", 
      "target_field": "group_title",
      "value": "Entertainment"
    }
  ]
}
```

#### EPG Data Mapping Example

```json
{
  "name": "Remove Unwanted Channels",
  "description": "Remove channels matching specific patterns",
  "source_type": "stream",
  "is_active": true,
  "conditions": [
    {
      "field_name": "channel_name",
      "operator": "matches",
      "value": ".*(Test|Demo|Sample).*",
      "logical_operator": "and"
    }
  ],
  "actions": [
    {
      "action_type": "remove_channel",
      "target_field": "channel_name"
    }
  ]
}
```

#### Timeshift Channel Example

```json
{
  "name": "Handle Timeshift Channels",
  "description": "Automatically adjust EPG times for +1 channels",
  "source_type": "epg",
  "is_active": true,
  "conditions": [
    {
      "field_name": "channel_name",
      "operator": "matches",
      "value": ".*\\+1.*",
      "logical_operator": "and"
    }
  ],
  "actions": [
    {
      "action_type": "timeshift_epg",
      "target_field": "channel_id",
      "timeshift_minutes": 60
    }
  ]
}
```

### Data Mapping Rule Scopes

Data mapping rules are processed in **sequential order** based on their `sort_order`. The scope determines how rules are applied:

```mermaid
graph TD
    A[Channel/Program Data] --> B{Check Next Rule}
    B --> C{Rule Scope Type}
    
    C -->|Individual| D[Apply to Single Item]
    C -->|Stream-wide| E[Apply to All Stream Channels]
    C -->|EPG-wide| F[Apply to All EPG Channels/Programs]
    
    D --> G{Conditions Met?}
    E --> H{Conditions Met?}
    F --> I{Conditions Met?}
    
    G -->|Yes| J[Execute Actions on Item]
    G -->|No| K[Skip Rule for Item]
    H -->|Yes| L[Execute Actions on All Stream Items]
    H -->|No| K
    I -->|Yes| M[Execute Actions on All EPG Items]
    I -->|No| K
    
    J --> N{More Rules?}
    L --> N
    K --> N
    M --> N
    
    N -->|Yes| B
    N -->|No| O[Processing Complete]
    
    style D fill:#e1f5fe
    style E fill:#f3e5f5
    style F fill:#e8f5e8
```

**Scope Types:**
- **Individual**: Rule applies to each channel/program independently
- **Stream-wide**: Rule applies globally to all channels from stream sources  
- **EPG-wide**: Rule applies globally to all channels/programs from EPG sources

**Processing Order:**
1. Rules are sorted by `sort_order` (ascending)
2. Each rule is evaluated against all applicable items
3. Actions are executed immediately when conditions match
4. Modified data is passed to subsequent rules

### Available Fields by Source Type

#### Stream Source Fields
| Field Name | Description |
|------------|-------------|
| `channel_name` | Channel display name |
| `tvg_id` | EPG identifier |
| `tvg_name` | EPG name |
| `tvg_logo` | Logo URL |
| `group_title` | Channel group |
| `stream_url` | Stream URL |

#### EPG Source Fields
| Field Name | Description |
|------------|-------------|
| `channel_id` | EPG channel identifier |
| `channel_name` | EPG channel name |
| `channel_logo` | EPG channel logo URL |
| `channel_group` | EPG channel group |
| `language` | EPG channel language |

### Available Operators

| Operator | Description |
|----------|-------------|
| `equals` | Exact match |
| `not_equals` | Not equal |
| `contains` | Contains substring |
| `not_contains` | Does not contain |
| `starts_with` | Starts with |
| `ends_with` | Ends with |
| `matches` | Regex pattern match |
| `not_matches` | Does not match regex |

### Available Actions by Source Type

#### Shared Actions (Stream and EPG)
| Action Type | Description | Fields |
|-------------|-------------|---------|
| `set_value` | Set field to specific value | `target_field`, `value` |
| `set_default_if_empty` | Set only if field is empty | `target_field`, `value` |
| `set_logo` | Set logo from uploaded assets | `target_field`, `logo_asset_id` |

#### Stream-Only Actions
| Action Type | Description | Fields |
|-------------|-------------|---------|
| `timeshift_epg` | Set timeshift offset for channels | `timeshift_minutes` |
| `deduplicate_stream_urls` | Deduplicate channels with same stream URL | None |
| `remove_channel` | Remove channel from output entirely | `target_field` |

#### EPG-Only Actions
| Action Type | Description | Fields |
|-------------|-------------|---------|
| `timeshift_epg` | Adjust EPG times for timeshift channels | `timeshift_minutes` |

### EPG-Specific Features

#### Channel Deduplication
- **Timeshift Detection**: Automatically detects timeshift channels (+1, +24 hour variants)
- **Unified Channel IDs**: Merges duplicates under a single channel ID
- **Preserves Data**: Keeps all EPG program data from all sources

#### Timeshift Support
- **Automatic Detection**: Recognizes +1, +24, +1h, +24h patterns
- **Program Adjustment**: Automatically shifts all program times
- **Flexible Timing**: Supports any offset in minutes
- **Maintains Relationships**: Preserves channel associations

### Testing and Preview

Use the **preview functionality** to test your data mapping rules:

- **Stream Preview**: See how rules transform stream channel metadata
- **EPG Preview**: See how rules affect EPG channels and clone groups
- **Individual Rule Test**: Test a single rule against data from a specific source
- **Clone Group Visualization**: Preview shows detected clone groups and their relationships
- **Timeshift Verification**: Preview displays time-adjusted program schedules

The preview shows exactly what will appear in your final M3U and XMLTV output.

### Advanced Expression Features

The data mapping system includes several advanced features for sophisticated metadata transformation:

#### Conditional Assignment Operator (?=)

The `?=` operator sets a value only if the target field is currently empty or null:

```json
{
  "name": "Set default group for ungrouped channels",
  "expression": "group_title ?= \"General\""
}
```

This is particularly useful for:
- Setting fallback values for missing metadata
- Providing defaults without overwriting existing data
- Creating conditional transformations based on field presence

#### Enhanced Regex Capture Groups

When using regex patterns with capture groups, the system provides detailed information about substitutions:

```json
{
  "name": "Extract and reformat channel numbers",
  "expression": "channel_name matches \"^(\\d+)\\s*-\\s*(.+)\" SET tvg_shift = \"$1$2\""
}
```

**Capture Group Display**: The preview shows both the template and resolved values:
- Template: `$1$2`
- Resolved value: `HD24`
- Individual captures: `$1='HD', $2='24'`

This enhanced display helps debug regex patterns and understand exactly how capture groups are being processed.

#### Hierarchical Expression Trees

The rule preview displays logical expressions as hierarchical trees with operators as parent nodes:

```
AND
├── channel_name contains "HD"
└── OR
    ├── group_title equals "Movies"
    └── group_title equals "Sports"
```

This visualization makes complex conditional logic easier to understand and debug.

#### Performance Metrics

The system tracks execution time for each rule, helping identify performance bottlenecks:
- **Execution Time**: Microsecond-precision timing per rule
- **Match Statistics**: Number of channels affected by each rule
- **Detailed Metrics**: Available in the rule preview modal

These metrics are especially valuable when optimizing large rule sets or working with high-volume channel data.

## Stream Proxy Setup

1. Create a new Stream Proxy
2. Attach one or more Stream Sources
3. Attach one or more EPG Sources (optional) - provides program guide data
4. Configure stream data mapping rules (optional) - transforms channel metadata
5. Configure EPG data mapping rules (optional) - handles channel deduplication and timeshift
6. Add and order stream filters as needed - operates on mapped stream data
7. Add and order EPG filters as needed - operates on mapped EPG data
8. The system will generate a unique ULID for the proxy
9. Access outputs:
   - Filtered playlist: `/proxy/{ulid}.m3u8`
   - EPG data: `/proxy/{ulid}.xmltv`

### Enhanced Processing Order

#### Stream Processing
1. **Original Stream Data**: Retrieved from database (unchanged)
2. **Stream Data Mapping**: Applied during proxy generation (channel names, groups, logos)
3. **Stream Filtering**: Applied to mapped data (include/exclude channels)
4. **M3U Generation**: Final output with transformations and filtering

#### EPG Processing
1. **Original EPG Data**: Retrieved from database (channels and programs unchanged)
2. **EPG Data Mapping**: Applied during proxy generation including:
   - **Timeshift processing**: Adjust program times for +1h, +24h channels
   - **Field transformation**: Clean up channel names and metadata
3. **EPG Filtering**: Applied to mapped data (include/exclude EPG data)
4. **XMLTV Generation**: Final EPG output with deduplicated channels and adjusted times

#### Coordinated Output
- **Channel Alignment**: Stream M3U and EPG XMLTV use consistent channel identifiers
- **Unified Deduplication**: Clone groups apply to both stream and EPG data
- **Synchronized Filtering**: Stream and EPG filters work together for consistent results

## Development

### Prerequisites

- Rust 1.75 or later
- SQLite (for default database)

### Project Structure

```
src/
├── main.rs              # Application entry point
├── config/              # Configuration management
├── database/            # Database connection and migrations
├── ingestor/            # M3U and Xtream parsing
├── models/              # Data models and schema
├── proxy/               # Filtering and generation logic
└── web/                 # Web server and API handlers

migrations/              # Database migrations
static/                  # Static web assets
.github/workflows/       # CI/CD pipelines
```

### Running Tests

```bash
cargo test
```

### CI/CD Pipeline

The project uses GitHub Actions for continuous integration and deployment:

#### Main CI Workflow (`.github/workflows/ci.yml`)

- **Check and Prepare**: 
  - Validates Rust formatting and clippy lints
  - Automatically prepares SQLx queries if needed
  - Commits updated `.sqlx/` files back to repository
- **Test Suite**: 
  - Runs unit and integration tests
  - Sets up test database with migrations
- **Cross-Platform Build**: 
  - Builds for Linux (x86_64, ARM64) and macOS (x86_64, ARM64)
  - Uses `cross` for ARM64 Linux builds
  - Caches build artifacts for efficiency
- **Docker Images**: 
  - Multi-platform Docker builds (AMD64, ARM64)
  - Publishes to GitHub Container Registry
- **Security Audit**: 
  - Runs `cargo-audit` for vulnerability scanning
  - Uses `cargo-deny` for license and dependency checks

#### Release Workflow (`.github/workflows/release.yml`)

Triggered on GitHub releases:
- Builds release binaries for all supported platforms
- Creates compressed archives with checksums
- Uploads assets to GitHub releases
- Builds and pushes Docker images with version tags
- Auto-generates release notes with changelog

#### Supported Platforms

| Platform | Target | Architecture |
|----------|--------|--------------|
| Linux | `x86_64-unknown-linux-gnu` | AMD64 |
| Linux | `aarch64-unknown-linux-gnu` | ARM64 |
| macOS | `x86_64-apple-darwin` | Intel |
| macOS | `aarch64-apple-darwin` | Apple Silicon |

#### Docker Images

- **Registry**: `ghcr.io/jmylchreest/m3u-proxy`
- **Tags**: `latest`, version tags (e.g., `v1.0.0`)
- **Platforms**: `linux/amd64`, `linux/arm64`

```bash
# Pull and run latest image
docker pull ghcr.io/jmylchreest/m3u-proxy:latest
docker run -p 3000:3000 -v ./data:/app/data ghcr.io/jmylchreest/m3u-proxy:latest
```

### Development Workflow

1. **Fork and Clone**: Fork the repository and clone locally
2. **SQLx Prepare**: Run `cargo sqlx prepare` if you modify database queries
3. **Testing**: Ensure all tests pass with `cargo test`
4. **Formatting**: Format code with `cargo fmt`
5. **Linting**: Check with `cargo clippy`
6. **Build**: Verify build with `cargo build --release`

### Contributing

1. Fork the repository
2. Create a feature branch
3. Make your changes
4. Add tests as appropriate
5. Ensure CI checks pass locally
6. Submit a pull request

#### Automated Checks

All pull requests automatically run:
- Code formatting validation
- Clippy linting
- SQLx query verification
- Test suite execution
- Cross-platform build verification
- Security vulnerability scanning

## License

This project is licensed under the MIT License - see the LICENSE file for details.

## Acknowledgments

- Inspired by [StreamMaster](https://github.com/carlreid/StreamMaster)
- Similar functionality to [Threadfin](https://github.com/Threadfin/Threadfin)
- Built with modern Rust ecosystem libraries

## Roadmap

### Completed ✅
- [x] **Complete EPG Integration**: Full XMLTV support with advanced processing
- [x] **Timeshift Support**: Automatic EPG time adjustment for timeshift channels
- [x] **Advanced Data Mapping**: Sophisticated transformation system for streams and EPG
- [x] **Dual Filter System**: Separate filtering for stream and EPG data
- [x] **Preview System**: Real-time preview of data mapping and filtering results

### In Progress 🚧
- [ ] Complete logo caching implementation
- [ ] Advanced web interface with real-time updates
- [ ] Docker containerization

### Planned 📋
- [ ] Performance optimizations for large EPG datasets
- [ ] Extended filtering options (time-based EPG filtering)
- [ ] Import/export functionality for rules and configurations
- [ ] Monitoring and metrics dashboard
- [ ] Multiple output format support (JSON, CSV exports)
- [ ] Advanced EPG features:
  - [ ] Series/episode detection and linking
  - [ ] Content rating and parental control filtering
  - [ ] Multi-language EPG support
  - [ ] EPG data analytics and insights

## Support

For issues, questions, or contributions, please visit the [GitHub Issues](https://github.com/jmylchreest/m3u-proxy/issues) page.