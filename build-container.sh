#!/bin/bash
set -euo pipefail

# Container build script for m3u-proxy with LinuxServer FFmpeg base + GPU acceleration
# Supports multiple container runtimes with auto-detection and fallback
# Priority: podman > docker > buildah

# Get app version from argument, environment, or calculate it
if [ $# -gt 0 ]; then
    APP_VERSION="$1"
    echo "Using version from argument: $APP_VERSION"
elif [ -n "${APP_VERSION:-}" ]; then
    echo "Using APP_VERSION from environment: $APP_VERSION"
elif command -v just >/dev/null 2>&1; then
    APP_VERSION=$(just get-version)
    echo "Using version from just get-version: $APP_VERSION"
else
    # Fallback to Cargo.toml if just is not available
    APP_VERSION=$(grep '^version' crates/m3u-proxy/Cargo.toml | head -1 | cut -d'"' -f2)
    echo "Using fallback version from Cargo.toml: $APP_VERSION"
fi

# Default versions (can be overridden with environment variables)
RUST_VERSION=${RUST_VERSION:-1.89}
NODE_VERSION=${NODE_VERSION:-22}
LINUXSERVER_FFMPEG_VERSION=${LINUXSERVER_FFMPEG_VERSION:-latest}

# Container runtime detection with preference order
CONTAINER_RUNTIME=""

detect_container_runtime() {
    echo "Detecting container runtime..."

    # Check for podman first (preferred)
    if command -v podman &> /dev/null; then
        echo "Found podman"
        CONTAINER_RUNTIME="podman"
        return 0
    fi

    # Check for docker
    if command -v docker &> /dev/null; then
        echo "Found docker"
        CONTAINER_RUNTIME="docker"
        return 0
    fi


    # Check for nerdctl (containerd)
    if command -v nerdctl &> /dev/null; then
        echo "Found nerdctl"
        CONTAINER_RUNTIME="nerdctl"
        return 0
    fi

    echo "Error: No supported container runtime found!"
    echo "Please install one of: podman, docker, nerdctl"
    exit 1
}

# Override detection if CONTAINER_RUNTIME is explicitly set
if [ -n "${CONTAINER_RUNTIME:-}" ]; then
    echo "Using explicitly set container runtime: $CONTAINER_RUNTIME"
    if ! command -v "$CONTAINER_RUNTIME" &> /dev/null; then
        echo "Error: Specified container runtime '$CONTAINER_RUNTIME' not found!"
        exit 1
    fi
else
    detect_container_runtime
fi

echo "Building m3u-proxy with LinuxServer FFmpeg base + GPU acceleration:"
echo "  Container Runtime: ${CONTAINER_RUNTIME}"
echo "  App Version: ${APP_VERSION}"
echo "  Rust: ${RUST_VERSION}"
echo "  Node.js: ${NODE_VERSION}"
echo "  LinuxServer FFmpeg: ${LINUXSERVER_FFMPEG_VERSION}"
echo ""

# Build with LinuxServer FFmpeg base
echo "Building with LinuxServer FFmpeg base + production-ready GPU acceleration"
echo "  Features: Proven LinuxServer FFmpeg with comprehensive Intel/AMD/NVIDIA GPU support"
echo ""

TARGET_STAGE="runtime"
BASE_TAG="m3u-proxy"
IMAGE_TAG="${BASE_TAG}:${APP_VERSION}"

# Determine additional tags based on version type
# Always tag with :latest, plus :release for tagged releases or :snapshot for dev builds
IMAGE_TAG_LATEST="${BASE_TAG}:latest"
if echo "$APP_VERSION" | grep -q -E "dev\.|snapshot"; then
    # Development/snapshot build: :latest + :snapshot + version
    IMAGE_TAG_ADDITIONAL="${BASE_TAG}:snapshot"
    echo "  Target: $TARGET_STAGE"
    echo "  Tags: $IMAGE_TAG, $IMAGE_TAG_LATEST, $IMAGE_TAG_ADDITIONAL (snapshot build)"
else
    # Tagged release build: :latest + :release + version
    IMAGE_TAG_ADDITIONAL="${BASE_TAG}:release"
    echo "  Target: $TARGET_STAGE"
    echo "  Tags: $IMAGE_TAG, $IMAGE_TAG_LATEST, $IMAGE_TAG_ADDITIONAL (release build)"
fi

# Build command arguments
BUILD_ARGS=(
    --target "$TARGET_STAGE"
    --build-arg "RUST_VERSION=${RUST_VERSION}"
    --build-arg "NODE_VERSION=${NODE_VERSION}"
    --build-arg "LINUXSERVER_FFMPEG_VERSION=${LINUXSERVER_FFMPEG_VERSION}"
    --build-arg "APP_VERSION=${APP_VERSION}"
    --tag "$IMAGE_TAG"
    --tag "$IMAGE_TAG_LATEST"
    --tag "$IMAGE_TAG_ADDITIONAL"
    --label "version=${APP_VERSION}"
    --label "build-date=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
    --label "vcs-ref=$(git rev-parse HEAD 2>/dev/null || echo 'unknown')"
    .
)

# Execute the build command
echo "  Running: ${CONTAINER_RUNTIME} build ${BUILD_ARGS[*]}"
if "${CONTAINER_RUNTIME}" build "${BUILD_ARGS[@]}"; then
    echo "Successfully built: $IMAGE_TAG"
    BUILT_IMAGES="$IMAGE_TAG"
else
    echo "Failed to build container"
    exit 1
fi
echo ""

echo "LinuxServer FFmpeg container build completed successfully!"
echo "Built images: $IMAGE_TAG, $IMAGE_TAG_LATEST, $IMAGE_TAG_ADDITIONAL"

# Show run commands
echo ""
echo "Basic usage:"
echo "  ${CONTAINER_RUNTIME} run -p 8080:8080 -v \$(pwd)/data:/app/data -v \$(pwd)/config:/app/config ${IMAGE_TAG_LATEST}"
echo ""
echo "With GPU hardware acceleration:"
echo "  ${CONTAINER_RUNTIME} run -p 8080:8080 --device=/dev/dri:/dev/dri --group-add \$(getent group render | cut -d: -f3) -v \$(pwd)/data:/app/data -v \$(pwd)/config:/app/config ${IMAGE_TAG_LATEST}"
echo ""
echo "Note: If localhost doesn't work in browser (resolves to IPv6), use:"
echo "  Access via: http://127.0.0.1:8080"
echo "  Or bind to both: -p 127.0.0.1:8080:8080 -p '[::1]:8080:8080'"
echo ""
echo "Test GPU capabilities:"
echo "  ${CONTAINER_RUNTIME} run --rm --device=/dev/dri:/dev/dri --group-add \$(getent group render | cut -d: -f3) --entrypoint=/usr/local/bin/ffmpeg ${IMAGE_TAG_LATEST} -hwaccels"

echo ""
echo "Container image size:"
"${CONTAINER_RUNTIME}" images "m3u-proxy" --format "table {{.Repository}}\t{{.Tag}}\t{{.Size}}" | head -3 || echo "Could not display image size"
