# Embedded Assets

This document explains how M3U Proxy uses embedded assets to create a self-contained binary with all required files built into the executable.

## Overview

M3U Proxy uses the `rust-embed` crate to embed all static assets and database migrations directly into the compiled binary. This approach provides several benefits:

- **Self-contained deployment**: Single binary with no external file dependencies
- **Simplified distribution**: No need to package or deploy separate asset files
- **Improved security**: Assets cannot be modified or tampered with after compilation
- **Better performance**: Assets are loaded from memory rather than disk
- **Simplified containerization**: Smaller container images with fewer layers

## Architecture

### Asset Types

The embedded asset system handles two main categories of assets:

1. **Static Web Assets** (`StaticAssets`)
   - HTML templates and pages
   - CSS stylesheets
   - JavaScript files
   - Images and fonts (if any)

2. **Database Migrations** (`MigrationAssets`)
   - SQL migration files
   - Schema updates
   - Data seeding scripts

### Implementation

#### Static Assets

```rust
#[derive(RustEmbed)]
#[folder = "static/"]
#[prefix = "static/"]
pub struct StaticAssets;
```

Static assets are embedded from the `static/` directory and served with the `static/` prefix. The system includes:

- **Content-Type Detection**: Automatic MIME type detection based on file extensions
- **Caching Headers**: Proper HTTP caching headers for optimal performance
- **Memory Serving**: Direct serving from embedded memory without disk I/O

#### Database Migrations

```rust
#[derive(RustEmbed)]
#[folder = "migrations/"]
#[prefix = "migrations/"]
pub struct MigrationAssets;
```

Database migrations are embedded from the `migrations/` directory and include:

- **Sequential Execution**: Migrations are sorted and executed in order
- **Migration Tracking**: Custom migration table tracks applied migrations
- **Rollback Safety**: Transactional migration execution with rollback on failure
- **Checksum Validation**: Content checksums prevent migration tampering

## File Structure

### Static Assets

```
static/
├── css/
│   └── main.css                 # Main stylesheet
├── html/
│   ├── shared/                  # Shared template components
│   │   ├── header.html         # Common header
│   │   ├── nav.html            # Navigation with active detection
│   │   ├── channels-modal.html # Shared channel browser modal
│   │   ├── footer-with-channels.html # Footer with modal loading
│   │   └── footer-basic.html   # Basic footer
│   ├── index.html              # Home page
│   ├── sources.html            # Source management page
│   ├── filters.html            # Filter management page
│   └── proxies.html            # Proxy management page
└── js/
    ├── shared-loader.js        # Template loading system
    ├── channels-viewer.js      # Shared channel browser
    ├── sources.js              # Source management logic
    ├── filters.js              # Filter management logic
    └── proxies.js              # Proxy management logic
```

### Database Migrations

```
migrations/
├── 001_initial_schema.sql      # Initial database schema
├── 002_default_filters.sql     # Default filter setup
└── [future migrations...]      # Additional schema updates
```

## Usage

### Static Asset Serving

The web server automatically serves embedded assets through the `/static/*` route:

```rust
// In web/handlers.rs
pub async fn serve_static_asset(Path(path): Path<String>) -> impl IntoResponse {
    let asset_path = format!("static/{}", path);
    serve_embedded_asset(&asset_path).await
}
```

### Database Migration Execution

Migrations are automatically applied during database initialization:

```rust
// In database/mod.rs
pub async fn migrate(&self) -> Result<()> {
    self.run_embedded_migrations().await?;
    Ok(())
}
```

## Development Workflow

### Adding New Static Assets

1. Place files in the appropriate `static/` subdirectory
2. Assets are automatically embedded during compilation
3. Access via `/static/path/to/asset` URLs
4. No additional configuration required

### Adding New Migrations

1. Create new `.sql` file in `migrations/` directory
2. Use sequential numbering: `003_description.sql`
3. Migrations are automatically embedded and executed
4. Test with `cargo test assets` to verify embedding

### Testing Embedded Assets

Run the embedded asset tests to verify all assets are properly embedded:

```bash
# Test all asset functionality
cargo test assets

# Test specific asset types
cargo test test_static_assets_exist
cargo test test_migration_assets_exist
```

## Performance Considerations

### Memory Usage

- Assets are loaded into memory at startup
- Memory usage scales with total asset size
- Consider asset optimization for production builds

### Build Time

- Embedding occurs at compile time
- Build time increases with asset count/size
- Use `cargo build --release` for optimized builds

### Caching

- Assets are served with aggressive caching headers
- Browser caching reduces server load
- Content never changes for a given binary version

## Security Benefits

### Tamper Resistance

- Assets cannot be modified after compilation
- No risk of file system tampering
- Consistent behavior across deployments

### Migration Integrity

- Migrations are checksummed and validated
- Prevents unauthorized schema changes
- Ensures consistent database state

## Deployment Advantages

### Single Binary Deployment

```bash
# Copy just the binary - no additional files needed
cp target/release/m3u-proxy /usr/local/bin/
```

### Container Optimization

```dockerfile
# Minimal container with just the binary
FROM scratch
COPY m3u-proxy /
ENTRYPOINT ["/m3u-proxy"]
```

### Configuration

The embedded asset system requires no configuration. All assets are automatically discovered and embedded during compilation.

## Troubleshooting

### Asset Not Found

If assets are not found:

1. Verify files exist in `static/` directory
2. Check file paths use forward slashes
3. Ensure `cargo build` completed successfully
4. Run `cargo test assets` to verify embedding

### Migration Failures

If migrations fail:

1. Check SQL syntax in migration files
2. Verify sequential numbering
3. Review migration logs for specific errors
4. Ensure database permissions are correct

### Build Issues

If compilation fails:

1. Verify `rust-embed` dependency is included
2. Check `static/` and `migrations/` directories exist
3. Ensure no file permission issues
4. Try `cargo clean` and rebuild

## Future Enhancements

Potential improvements to the embedded asset system:

- **Compression**: Gzip compression for embedded assets
- **Versioning**: Asset versioning for cache busting
- **Hot Reload**: Development mode with file system serving
- **Asset Optimization**: Automatic minification and optimization
- **Selective Embedding**: Conditional asset embedding based on features