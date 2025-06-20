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

### Database Schema

The application uses a relational database with the following main tables:

- `stream_sources`: Source configurations (M3U URLs, Xtream credentials)
- `stream_proxies`: Proxy definitions with unique ULIDs
- `filters`: Filter patterns and configurations
- `channels`: Ingested channel data from sources
- `proxy_generations`: Versioned M3U outputs

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

Filters support regex patterns and can operate in normal or inverse mode:

```json
{
  "name": "UK Channels Only",
  "pattern": "UK\\|.*",
  "starting_channel_number": 1,
  "is_inverse": false
}
```

```json
{
  "name": "Exclude Adult Content",
  "pattern": "XXX|Adult|18\\+",
  "starting_channel_number": 1,
  "is_inverse": true
}
```

## Stream Proxy Setup

1. Create a new Stream Proxy
2. Attach one or more Stream Sources
3. Add and order filters as needed
4. The system will generate a unique ULID for the proxy
5. Access the filtered playlist at `/proxy/{ulid}.m3u8`

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
static/                  # Static web assets (future)
```

### Running Tests

```bash
cargo test
```

### Contributing

1. Fork the repository
2. Create a feature branch
3. Make your changes
4. Add tests as appropriate
5. Submit a pull request

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