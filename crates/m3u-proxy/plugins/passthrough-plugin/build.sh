#!/bin/bash
# Build script for pass-through WASM plugin

set -e

echo "🚀 Building Pass-through WASM Plugin..."

# Create dist directory
mkdir -p dist

# Build for WASM target
echo "📦 Building WASM binary..."
cargo build --target wasm32-wasip1 --release

# Check if wasm-opt is available for optimization
if command -v wasm-opt &> /dev/null; then
    echo "⚡ Optimizing WASM binary..."
    wasm-opt target/wasm32-wasip1/release/passthrough_plugin.wasm \
        -O3 --enable-bulk-memory --enable-sign-ext \
        -o dist/passthrough_plugin.wasm
else
    echo "⚠️  wasm-opt not found, copying unoptimized binary..."
    cp target/wasm32-wasip1/release/passthrough_plugin.wasm dist/
fi

# Generate plugin manifest
echo "📝 Generating plugin manifest..."
cat > dist/plugin.toml << EOF
[plugin]
name = "passthrough-plugin"
version = "0.1.0"
author = "m3u-proxy developers"
description = "Simple pass-through plugin for testing WASM integration"
license = "MIT"

[capabilities]
stages = ["source_loading", "data_mapping", "filtering", "channel_numbering", "m3u_generation"]
supports_streaming = true
requires_all_data = false
can_produce_early_output = true
memory_efficient = true
preferred_chunk_size = 1000

[host_interface]
version = "1.0"
required_functions = [
    "host_get_memory_usage",
    "host_get_memory_pressure",
    "host_log",
    "host_report_progress"
]

[config]
# No configuration needed for pass-through

[performance]
estimated_memory_per_item = 512  # bytes
max_items_in_memory = 10000
EOF

# Generate example configuration
echo "⚙️  Generating example configuration..."
cat > dist/example_config.json << EOF
{
  "enabled": true,
  "stages": ["source_loading", "data_mapping", "filtering", "channel_numbering", "m3u_generation"],
  "priority": 5
}
EOF

# Generate test data
echo "🧪 Generating test data..."
cat > dist/test_source_ids.json << EOF
[
  "01234567-89ab-cdef-0123-456789abcdef",
  "11234567-89ab-cdef-0123-456789abcdef",
  "21234567-89ab-cdef-0123-456789abcdef"
]
EOF

cat > dist/test_channels.json << EOF
[
  {
    "id": "01234567-89ab-cdef-0123-456789abcdef",
    "channel_name": "Test Channel 1",
    "source_id": "01234567-89ab-cdef-0123-456789abcdef",
    "stream_url": "http://test.example/1",
    "tvg_id": "test1",
    "tvg_name": "Test Channel 1",
    "tvg_logo": null,
    "group_title": "Test Group"
  }
]
EOF

# Generate deployment instructions
echo "📋 Generating deployment instructions..."
cat > dist/DEPLOYMENT.md << EOF
# Pass-through Plugin Deployment

## Overview
This is a simple pass-through WASM plugin that demonstrates the basic plugin interface.
It can be used for testing the WASM plugin system and as a template for other plugins.

## Files
- \`passthrough_plugin.wasm\` - The compiled WASM plugin
- \`plugin.toml\` - Plugin metadata
- \`example_config.json\` - Example configuration
- \`test_*.json\` - Test data for manual testing

## Installation

### 1. Copy Plugin Files
\`\`\`bash
# Copy to m3u-proxy plugins directory
sudo cp passthrough_plugin.wasm /opt/m3u-proxy/plugins/
sudo cp plugin.toml /opt/m3u-proxy/plugins/passthrough_plugin.toml
\`\`\`

### 2. Configure Plugin
Edit your m3u-proxy configuration to enable WASM plugins:

\`\`\`toml
[plugins]
enabled = true
plugin_directory = "/opt/m3u-proxy/plugins"

[plugins.passthrough_plugin]
enabled = true
priority = 5
stages = ["source_loading", "data_mapping", "filtering", "channel_numbering", "m3u_generation"]
\`\`\`

### 3. Restart Service
\`\`\`bash
sudo systemctl restart m3u-proxy
\`\`\`

### 4. Verify Plugin
Check logs for plugin initialization:
\`\`\`bash
sudo journalctl -u m3u-proxy -f | grep "Pass-through"
\`\`\`

## Testing

### Manual Testing
You can test individual plugin functions using a WASM runtime:

\`\`\`bash
# Test plugin info
wasmtime --invoke plugin_get_info passthrough_plugin.wasm

# Test capabilities
wasmtime --invoke plugin_get_capabilities passthrough_plugin.wasm
\`\`\`

### Integration Testing
The plugin will be automatically tested when m3u-proxy processes requests.
Look for log messages starting with "Pass-through plugin" in the service logs.

## Stage Behavior

- **Source Loading**: Creates mock channels for each source ID
- **Data Mapping**: Passes channels through unchanged
- **Filtering**: Passes channels through unchanged
- **Channel Numbering**: Assigns sequential numbers to channels
- **M3U Generation**: Converts numbered channels to M3U format

## Performance
This plugin is designed for testing and has minimal performance impact:
- Memory efficient (passes data through)
- Low CPU usage
- Supports streaming processing
- Provides progress reporting

## Troubleshooting

### Plugin Not Loading
- Check that WASM target is available: \`rustup target list --installed\`
- Verify plugin file permissions: \`ls -la /opt/m3u-proxy/plugins/\`
- Check service logs: \`journalctl -u m3u-proxy -n 50\`

### Plugin Errors
- Enable debug logging in m3u-proxy configuration
- Check for memory pressure issues
- Verify input data format matches expected JSON schema

## Development
To modify this plugin:
1. Edit \`src/lib.rs\`
2. Run \`./build.sh\`
3. Test with new binary
4. Deploy following installation steps above
EOF

# Create a simple test script
echo "🧪 Creating test script..."
cat > dist/test.sh << EOF
#!/bin/bash
# Simple test script for pass-through plugin

echo "Testing pass-through plugin..."

if [ ! -f "passthrough_plugin.wasm" ]; then
    echo "❌ Plugin binary not found. Run build.sh first."
    exit 1
fi

# Check if wasmtime is available
if ! command -v wasmtime &> /dev/null; then
    echo "⚠️  wasmtime not found. Install it to run tests:"
    echo "   curl https://wasmtime.dev/install.sh -sSf | bash"
    exit 1
fi

echo "📋 Testing plugin info..."
if wasmtime --invoke plugin_get_info passthrough_plugin.wasm > /dev/null 2>&1; then
    echo "✅ Plugin info test passed"
else
    echo "❌ Plugin info test failed"
fi

echo "🔧 Testing plugin capabilities..."
if wasmtime --invoke plugin_get_capabilities passthrough_plugin.wasm > /dev/null 2>&1; then
    echo "✅ Plugin capabilities test passed"
else
    echo "❌ Plugin capabilities test failed"
fi

echo "📊 Plugin file size:"
ls -lh passthrough_plugin.wasm

echo ""
echo "🎯 Plugin ready for deployment!"
echo "   See DEPLOYMENT.md for installation instructions"
EOF

chmod +x dist/test.sh

# Display build summary
echo ""
echo "✅ Build completed successfully!"
echo ""
echo "📊 Build Summary:"
echo "  📁 Binary: dist/passthrough_plugin.wasm"
echo "  📋 Manifest: dist/plugin.toml"
echo "  ⚙️  Example Config: dist/example_config.json"
echo "  🧪 Test Data: dist/test_*.json"
echo "  🧪 Test Script: dist/test.sh"
echo "  📋 Deployment Guide: dist/DEPLOYMENT.md"
echo ""

# Show file sizes
echo "📏 File Sizes:"
ls -lh dist/

echo ""
echo "🎯 Next Steps:"
echo "  1. Test the plugin: cd dist && ./test.sh"
echo "  2. Deploy: Follow instructions in dist/DEPLOYMENT.md"
echo "  3. Monitor: Check logs for plugin activity"
echo ""
