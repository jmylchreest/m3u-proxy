#!/bin/bash
set -euo pipefail

# Container build script for m3u-proxy with LinuxServer FFmpeg base + GPU acceleration
# Supports multiple container runtimes with auto-detection and fallback
# Priority: podman > docker > buildah

# Extract app version from Cargo.toml
APP_VERSION=$(grep '^version' crates/m3u-proxy/Cargo.toml | head -1 | cut -d'"' -f2)

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
IMAGE_TAG_LATEST="${BASE_TAG}:latest"

echo "  Target: $TARGET_STAGE"
echo "  Tags: $IMAGE_TAG, $IMAGE_TAG_LATEST"

# Build command arguments
BUILD_ARGS=(
    --target "$TARGET_STAGE"
    --build-arg "RUST_VERSION=${RUST_VERSION}"
    --build-arg "NODE_VERSION=${NODE_VERSION}"
    --build-arg "LINUXSERVER_FFMPEG_VERSION=${LINUXSERVER_FFMPEG_VERSION}"
    --tag "$IMAGE_TAG"
    --tag "$IMAGE_TAG_LATEST"
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
echo "Built images: $BUILT_IMAGES"

# Show run commands
echo ""
echo "Basic usage:"
echo "  ${CONTAINER_RUNTIME} run -p 8080:8080 -v \$(pwd)/data:/app/data -v \$(pwd)/config:/app/config ${IMAGE_TAG}"
echo ""
echo "With GPU hardware acceleration:"
echo "  ${CONTAINER_RUNTIME} run -p 8080:8080 --device=/dev/dri:/dev/dri --group-add \$(getent group render | cut -d: -f3) -v \$(pwd)/data:/app/data -v \$(pwd)/config:/app/config ${IMAGE_TAG}"
echo ""
echo "Note: If localhost doesn't work in browser (resolves to IPv6), use:"
echo "  Access via: http://127.0.0.1:8080"
echo "  Or bind to both: -p 127.0.0.1:8080:8080 -p '[::1]:8080:8080'"
echo ""
echo "Test GPU capabilities:"
echo "  ${CONTAINER_RUNTIME} run --rm --device=/dev/dri:/dev/dri --group-add \$(getent group render | cut -d: -f3) --entrypoint=/usr/local/bin/ffmpeg ${IMAGE_TAG} -hwaccels"

echo ""
echo "Container image size:"
"${CONTAINER_RUNTIME}" images "m3u-proxy" --format "table {{.Repository}}\t{{.Tag}}\t{{.Size}}" | head -3 || echo "Could not display image size"
