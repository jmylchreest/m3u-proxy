# M3U Proxy

A high-performance M3U proxy service built in Rust for managing and filtering IPTV streams with advanced data mapping and EPG support.

## Quick Start

### Docker Compose

```yaml
services:
  m3u-proxy:
    image: ghcr.io/jmylchreest/m3u-proxy:latest
    ports:
      - "${M3U_PROXY_PORT:-8080}:8080"
    volumes:
      - ./data:/app/data
    env_file:
      - .env
    restart: unless-stopped
```

```bash
# Start the service
docker-compose up -d

# Access web interface
open http://localhost:8080
```

### Binary Installation

```bash
# Download and run (self-contained binary)
wget https://github.com/jmylchreest/m3u-proxy/releases/latest/download/m3u-proxy
chmod +x m3u-proxy
./m3u-proxy
```

## Configuration

### Environment Variables

| Variable | Description | Default | Required |
|----------|-------------|---------|----------|
| **MANDATORY** |
| `M3U_PROXY_DATABASE__URL` | Database connection URL | `sqlite://./m3u-proxy.db` | ✓ |
| **WEB SERVER** |
| `M3U_PROXY_WEB__HOST` | Listening IP address | `0.0.0.0` | |
| `M3U_PROXY_WEB__PORT` | Listening port | `8080` | |
| `M3U_PROXY_WEB__ENABLE_REQUEST_LOGGING` | Enable HTTP request/response logging | `false` | |
| **DATABASE** |
| `M3U_PROXY_DATABASE__MAX_CONNECTIONS` | Database connection pool size | `10` | |
| `M3U_PROXY_DATABASE__BATCH_SIZE` | Batch processing size | `1000` | |
| **STORAGE** |
| `M3U_PROXY_STORAGE__M3U_PATH` | M3U file storage directory | `./data/m3u` | |
| `M3U_PROXY_STORAGE__LOGO_PATH` | Logo cache directory | `./data/logos` | |
| `M3U_PROXY_STORAGE__PROXY_VERSIONS_TO_KEEP` | Number of proxy versions to retain | `3` | |
| **INGESTION** |
| `M3U_PROXY_INGESTION__PARALLEL_SOURCES` | Parallel source processing | `3` | |
| `M3U_PROXY_INGESTION__REQUEST_TIMEOUT_SECONDS` | HTTP request timeout | `30` | |
| `M3U_PROXY_INGESTION__MAX_RETRIES` | Maximum retry attempts | `3` | |

### Example .env file

```bash
# Database (SQLite default - no setup required)
M3U_PROXY_DATABASE__URL=sqlite://./m3u-proxy.db

# For PostgreSQL
# M3U_PROXY_DATABASE__URL=postgresql://user:pass@localhost/m3u_proxy

# Web server
M3U_PROXY_WEB__HOST=0.0.0.0
M3U_PROXY_WEB__PORT=8080
M3U_PROXY_WEB__ENABLE_REQUEST_LOGGING=false

# Storage paths
M3U_PROXY_STORAGE__M3U_PATH=./data/m3u
M3U_PROXY_STORAGE__LOGO_PATH=./data/logos
```

## Expression Syntax

The system uses natural language expressions for filtering and data mapping:

### Filter Expressions

Control which channels are included/excluded from proxy outputs:

```
# Basic patterns
channel_name contains "news"
group_title equals "Sports"
channel_name not contains "adult"

# Logical combinations
channel_name contains "sport" AND group_title not contains "adult"
(channel_name contains "HD" OR channel_name contains "4K") AND group_title equals "Movies"

# Advanced matching
channel_name matches "^(StreamCast|ViewMedia).*HD$"
channel_name starts_with "StreamCast" AND channel_name ends_with "HD"
```

### Data Mapping Expressions

Transform channel metadata during proxy generation:

```
# Set values
channel_name = "StreamCast News HD"
group_title = "News Channels"

# Conditional assignment (only if empty)
group_title ?= "General"

# Regex transformations
channel_name matches "^(.+)\\s+HD$" SET channel_name = "$1 High Definition"

# Remove channels
channel_name contains "test" REMOVE
```

### Available Fields

**Stream Fields**: `channel_name`, `group_title`, `tvg_id`, `tvg_name`, `tvg_logo`, `stream_url`  
**EPG Fields**: `channel_id`, `channel_name`, `channel_logo`, `channel_group`, `language`, `program_title`, `program_category`

### Operators

| Operator | Description |
|----------|-------------|
| `contains` | Text contains substring (case insensitive) |
| `equals` | Exact match (case insensitive) |
| `matches` | Regular expression match |
| `starts_with` | Text starts with substring |
| `ends_with` | Text ends with substring |
| `not` | Negates any operator |
| `case_sensitive` | Makes match case sensitive |

## Core Workflow

```mermaid
graph TD
    A[Stream Sources<br/>M3U/Xtream] --> C[Original Data<br/>Database]
    B[EPG Sources<br/>XMLTV/Xtream] --> C
    
    D[Proxy Request] --> E[Retrieve Original Data]
    E --> F[Apply Data Mapping<br/>Transform metadata]
    F --> G[Apply Filters<br/>Include/exclude channels]
    G --> H[Generate M3U Output<br/>/proxy/ulid.m3u8]
    G --> I[Generate XMLTV Output<br/>/proxy/ulid.xmltv]
    
    C --> E
    
    subgraph "Ingestion (Scheduled)"
        A
        B
        C
    end
    
    subgraph "Proxy Generation (On-Demand)"
        D
        E
        F
        G
        H
        I
    end
    
    style C fill:#e1f5fe
    style F fill:#f3e5f5
    style G fill:#e8f5e8
    style H fill:#fff3e0
    style I fill:#fff3e0
```

1. **Setup Sources**: Add M3U/Xtream stream sources and XMLTV/Xtream EPG sources
2. **Create Proxies**: Define stream proxies that combine multiple sources
3. **Configure Mapping**: Set up data mapping rules to transform channel metadata
4. **Add Filters**: Configure filters to include/exclude specific channels
5. **Generate Output**: Access filtered playlists at `/proxy/{ulid}.m3u8` and EPG at `/proxy/{ulid}.xmltv`

## API Documentation

Complete OpenAPI documentation available at: `/openapi.json`

Interactive Swagger UI at: `/docs` (when running)

## Key Features

- **Multi-Source Support**: M3U playlists and Xtream Codes APIs
- **Advanced EPG Processing**: XMLTV support with automatic timeshift detection
- **Data Transformation**: Sophisticated channel metadata mapping system  
- **Natural Language Filtering**: Intuitive expression syntax for complex rules
- **Logo Caching**: Automatic channel logo management
- **Database Flexibility**: SQLite, PostgreSQL, MySQL, MariaDB support
- **Zero Dependencies**: Self-contained binary with embedded assets

## Roadmap

### Current Development
- [ ] Add support for manipulation of EPG data in data-mapping and filters
- [ ] Add support for manual stream sources (custom local streams, literal manual list)
- [ ] Add OpenTelemetry integration with automatic request/response tracing
- [ ] Implement Prometheus metrics endpoint (`/metrics`) for monitoring stream counts, response times, errors
- [ ] Add request correlation IDs to logs for easier debugging across components
- [ ] Export performance metrics (channel fetch times, proxy generation duration, database query times)

## Provider Links

Useful resources for finding IPTV streams and EPG data:

### IPTV Sources
- **[IPTV-Org](https://github.com/iptv-org/iptv)** - Collection of publicly available IPTV channels from all over the world
- **[Free-TV IPTV](https://github.com/Free-TV/IPTV)** - Community-maintained collection of free IPTV channels

### EPG Sources  
- **[IPTV-Org EPG Sites](https://github.com/iptv-org/epg/tree/master/sites)** - Electronic Program Guide sources for various regions
- **[Free-TV EPG List](https://github.com/Free-TV/IPTV/blob/master/epglist.txt)** - Curated list of EPG sources

*Note: Always respect content licensing and terms of service when using IPTV sources.*

## Support

- **Documentation**: Full API docs at `/openapi.json`
- **Web Interface**: Management UI at `http://localhost:8080`
- **Issues**: [GitHub Issues](https://github.com/jmylchreest/m3u-proxy/issues)