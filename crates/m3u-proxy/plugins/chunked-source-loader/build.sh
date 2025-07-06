#!/bin/bash
# Build script for chunked-source-loader WASM plugin

set -e

echo "🚀 Building Chunked Source Loader WASM Plugin..."

# Create dist directory
mkdir -p dist

# Build for WASM target
echo "📦 Building WASM binary..."
cargo build --target wasm32-wasip1 --release

# Check if wasm-opt is available for optimization
if command -v wasm-opt &> /dev/null; then
    echo "⚡ Optimizing WASM binary..."
    wasm-opt target/wasm32-wasip1/release/chunked_source_loader.wasm \
        -O3 --enable-bulk-memory --enable-sign-ext \
        -o dist/chunked_source_loader.wasm
else
    echo "⚠️  wasm-opt not found, copying unoptimized binary..."
    cp target/wasm32-wasip1/release/chunked_source_loader.wasm dist/
fi

# Generate plugin manifest
echo "📝 Generating plugin manifest..."
cat > dist/plugin.toml << EOF
[plugin]
name = "chunked-source-loader"
version = "0.0.1"
author = "m3u-proxy developers"
description = "Memory-efficient chunked source loading with automatic spilling"
license = "MIT"

[capabilities]
stages = ["source_loading"]
supports_streaming = true
requires_all_data = false
can_produce_early_output = false
memory_efficient = true
preferred_chunk_size = 1000

[host_interface]
version = "1.0"
required_functions = [
    "host_get_memory_usage",
    "host_get_memory_pressure",
    "host_write_temp_file",
    "host_read_temp_file",
    "host_delete_temp_file",
    "host_database_query_source",
    "host_log"
]

[config]
memory_threshold_mb = 256
chunk_size = 1000
compression_enabled = true
max_spill_files = 100

[performance]
estimated_memory_per_channel = 2048  # bytes
max_channels_in_memory = 131072      # ~256MB / 2KB
spill_cleanup_on_finalize = true
EOF

# Generate example configuration
echo "⚙️  Generating example configuration..."
cat > dist/example_config.json << EOF
{
  "memory_threshold_mb": 256,
  "chunk_size": 1000,
  "compression_enabled": true,
  "max_spill_files": 100
}
EOF

# Generate deployment instructions
echo "📋 Generating deployment instructions..."
cat > dist/DEPLOYMENT.md << EOF
# Deployment Instructions

## 1. Copy Plugin Files

\`\`\`bash
# Copy to m3u-proxy plugins directory
sudo cp chunked_source_loader.wasm /opt/m3u-proxy/plugins/
sudo cp plugin.toml /opt/m3u-proxy/plugins/chunked_source_loader.toml
\`\`\`

## 2. Configure Plugin

Edit \`/etc/m3u-proxy/plugins.toml\`:

\`\`\`toml
[plugins.chunked_source_loader]
enabled = true
priority = 10
memory_threshold_mb = 256
chunk_size = 1000

[plugins.chunked_source_loader.config]
compression_enabled = true
max_spill_files = 100
temp_file_retention_hours = 24
\`\`\`

## 3. Restart Service

\`\`\`bash
sudo systemctl restart m3u-proxy
\`\`\`

## 4. Verify Plugin

Check logs for plugin initialization:

\`\`\`bash
sudo journalctl -u m3u-proxy -f | grep "Chunked Source Loader"
\`\`\`
EOF

# Display build summary
echo ""
echo "✅ Build completed successfully!"
echo ""
echo "📊 Build Summary:"
echo "  📁 Binary: dist/chunked_source_loader.wasm"
echo "  📋 Manifest: dist/plugin.toml"
echo "  ⚙️  Example Config: dist/example_config.json"
echo "  📋 Deployment Guide: dist/DEPLOYMENT.md"
echo ""

# Show file sizes
echo "📏 File Sizes:"
ls -lh dist/

echo ""
echo "🎯 Next Steps:"
echo "  1. Test the plugin: ./test.sh"
echo "  2. Deploy: Follow instructions in dist/DEPLOYMENT.md"
echo "  3. Monitor: Check logs for plugin activity"
echo ""
