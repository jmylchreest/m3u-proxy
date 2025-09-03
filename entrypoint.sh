#!/bin/bash
set -e

# Enable proper signal handling
trap 'exit 143' TERM
trap 'exit 130' INT

# Add /app to PATH so binaries can be found
export PATH="/app:$PATH"

# Check for GPU device access and warn if not available
if [ -e "/dev/dri" ]; then
    if ! ls /dev/dri/render* >/dev/null 2>&1 || ! ls /dev/dri/card* >/dev/null 2>&1; then
        echo "Warning: GPU devices found but may not be accessible due to permissions"
        echo "For hardware acceleration, run with: --device=/dev/dri:/dev/dri --group-add \$(getent group render | cut -d: -f3)"
    fi
fi

# Function to check if argument is already provided
has_arg() {
    local long_arg="$1"
    local short_arg="$2"
    shift 2
    
    for provided_arg in "$@"; do
        case "$provided_arg" in
            "$long_arg"|"$long_arg"=*|"$short_arg") return 0 ;;
        esac
    done
    return 1
}

# Default values from environment variables
HOST="${M3U_PROXY_HOST:-0.0.0.0}"
PORT="${M3U_PROXY_PORT:-8080}"
CONFIG="${M3U_PROXY_CONFIG:-/app/config/config.toml}"
LOG_LEVEL="${M3U_PROXY_LOG_LEVEL:-info}"
DATABASE_URL="${M3U_PROXY_DATABASE__URL:-sqlite:///app/data/m3u-proxy.db}"

# Build command line arguments only if not already provided
ARGS=()

# Add arguments only if not already specified by user
if ! has_arg "--host" "-H" "$@"; then
    ARGS+=("--host" "$HOST")
fi

if ! has_arg "--port" "-p" "$@"; then
    ARGS+=("--port" "$PORT")
fi

if ! has_arg "--config" "-c" "$@"; then
    ARGS+=("--config" "$CONFIG")
fi

if ! has_arg "--log-level" "-l" "$@"; then
    ARGS+=("--log-level" "$LOG_LEVEL")
fi

# Add database URL only if not already provided (always recommended in containers)
if ! has_arg "--database-url" "-d" "$@"; then
    ARGS+=("--database-url" "$DATABASE_URL")
fi

# Execute m3u-proxy with the constructed arguments and user arguments
# Use exec to replace the shell process so signals are handled properly
exec m3u-proxy "${ARGS[@]}" "$@"