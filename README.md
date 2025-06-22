# M3U Proxy

A modern, high-performance M3U proxy service built in Rust for managing and filtering IPTV streams from multiple sources.

## Features

- **Multiple Source Support**: Ingest from M3U playlists and Xtream Codes APIs
- **Advanced Filtering**: Pattern-based filtering with regex support
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
- **Filters**: Pattern-based rules for including/excluding channels
- **Stream Proxies**: Collections of sources with applied filters
- **Ingestor**: Scheduled service for updating source data
- **Proxy Generator**: Creates filtered M3U files with versioning

### Data Processing Flow

The M3U Proxy follows a multi-stage processing pipeline that preserves original data integrity while allowing flexible transformations for each proxy output:

```mermaid
graph TD
    A[Stream Sources] --> B[Ingestor Service]
    B --> C[Original Channels Storage]
    C --> D[Database]
    
    E[Stream Proxy Request] --> F[Proxy Generator]
    F --> G[Retrieve Original Channels]
    G --> H[Apply Data Mapping Rules]
    H --> I[Apply Filters]
    I --> J[Generate M3U Output]
    
    K[Data Mapping Rules] --> H
    L[Proxy Filters] --> I
    
    D --> G
    
    subgraph "Ingestion Pipeline"
        A
        B
        C
        D
    end
    
    subgraph "Proxy Generation Pipeline"
        E
        F
        G
        H
        I
        J
    end
    
    subgraph "Configuration"
        K
        L
    end
    
    style C fill:#e1f5fe
    style H fill:#f3e5f5
    style I fill:#e8f5e8
    style J fill:#fff3e0
```

#### Key Processing Stages

1. **Ingestion Stage**:
   - Stream sources (M3U/Xtream) are processed by the ingestor service
   - **Original channel data is preserved** in the database unchanged
   - No data mapping or filtering occurs during ingestion
   - Ensures data integrity and allows multiple proxy configurations

2. **Proxy Generation Stage**:
   - Each proxy request triggers an independent generation process
   - **Data mapping is applied dynamically** - transforming channel metadata (names, groups, logos) without affecting stored data
   - **Filters are applied** to include/exclude channels based on patterns
   - **Final M3U is generated** with transformed and filtered results

3. **Data Mapping Process**:
   - Data mapping rules are applied **only during proxy generation**
   - Original ingested data remains untouched in the database
   - Transformations include: channel name changes, group assignments, logo updates, custom labels
   - The "preview rules" functionality shows exactly what the final proxy output will contain

4. **Filter Application**:
   - Filters operate on the **mapped channel data**, not the original data
   - Supports regex patterns, field-specific matching, and logical operators
   - Filters are applied in order, allowing complex channel selection logic

#### Benefits of this Architecture

- **Data Integrity**: Original source data is never modified
- **Flexibility**: Multiple proxies can apply different transformations to the same source data
- **Consistency**: Data mapping preview exactly matches final proxy output
- **Performance**: Transformations are applied only when generating proxies, not during ingestion
- **Reliability**: Failed data mapping doesn't corrupt source data

### Database Schema

The application uses a relational database with the following main tables:

- `stream_sources`: Source configurations (M3U URLs, Xtream credentials)
- `stream_proxies`: Proxy definitions with unique ULIDs
- `filters`: Filter patterns and configurations
- `channels`: **Original ingested channel data** (unchanged by data mapping)
- `data_mapping_rules`: Rules for transforming channel metadata during proxy generation
- `proxy_generations`: Versioned M3U outputs with applied transformations

## API Endpoints

### Stream Sources
- `GET /api/sources` - List all sources
- `POST /api/sources` - Create new source
- `GET /api/sources/{id}` - Get source details
- `PUT /api/sources/{id}` - Update source
- `DELETE /api/sources/{id}` - Delete source

### Stream Proxies
- `GET /api/proxies` - List all proxies
- `POST /api/proxies` - Create new proxy
- `GET /api/proxies/{id}` - Get proxy details
- `PUT /api/proxies/{id}` - Update proxy
- `DELETE /api/proxies/{id}` - Delete proxy

### Filters
- `GET /api/filters` - List all filters
- `POST /api/filters` - Create new filter
- `GET /api/filters/{id}` - Get filter details
- `PUT /api/filters/{id}` - Update filter
- `DELETE /api/filters/{id}` - Delete filter

### Proxy Output
- `GET /proxy/{ulid}.m3u8` - Get generated M3U playlist
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

- `channel_name` - The display name of the channel
- `group_title` - The group/category the channel belongs to
- `tvg_id` - The TVG ID identifier
- `tvg_name` - The TVG name
- `stream_url` - The streaming URL

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

Data mapping allows you to transform channel metadata (names, groups, logos) during proxy generation without modifying the original source data.

### Data Mapping Rules

Rules consist of **conditions** (when to apply) and **actions** (what to change):

```json
{
  "name": "Clean Channel Names",
  "description": "Remove HD/FHD suffixes and standardize names",
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

### Available Conditions

| Field Name | Description |
|------------|-------------|
| `channel_name` | Channel display name |
| `tvg_id` | EPG identifier |
| `tvg_name` | EPG name |
| `tvg_logo` | Logo URL |
| `group_title` | Channel group |
| `stream_url` | Stream URL |

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

### Available Actions

| Action Type | Description | Fields |
|-------------|-------------|---------|
| `set_value` | Set field to specific value | `target_field`, `value` |
| `set_default_if_empty` | Set only if field is empty | `target_field`, `value` |
| `set_logo` | Set logo from uploaded assets | `target_field`, `logo_asset_id` |
| `set_label` | Add custom label | `label_key`, `label_value` |

### Testing and Preview

Use the **preview functionality** to test your data mapping rules:

- **Individual Rule Test**: Test a single rule against channels from a specific source
- **Source Preview**: See how all active rules affect channels from one source  
- **Full Preview**: See the complete transformation across all sources and rules

The preview shows exactly what will appear in your final M3U proxy output.

## Stream Proxy Setup

1. Create a new Stream Proxy
2. Attach one or more Stream Sources
3. Configure data mapping rules (optional) - transforms channel metadata
4. Add and order filters as needed - operates on mapped data
5. The system will generate a unique ULID for the proxy
6. Access the filtered playlist at `/proxy/{ulid}.m3u8`

### Processing Order

1. **Original Data**: Retrieved from database (unchanged)
2. **Data Mapping**: Applied during proxy generation (channel names, groups, logos)
3. **Filtering**: Applied to mapped data (include/exclude channels)
4. **M3U Generation**: Final output with transformations and filtering

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

- [ ] Complete logo caching implementation
- [ ] Advanced web interface with real-time updates
- [ ] Docker containerization
- [ ] Performance optimizations
- [ ] Extended filtering options
- [ ] Import/export functionality
- [ ] Monitoring and metrics
- [ ] Multiple output format support

## Support

For issues, questions, or contributions, please visit the [GitHub Issues](https://github.com/jmylchreest/m3u-proxy/issues) page.