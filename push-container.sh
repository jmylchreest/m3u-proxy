#!/bin/bash
set -euo pipefail

# Container push script for m3u-proxy with registry tagging support
# Supports both Docker and Podman with auto-detection
# Assumes you're already logged in to the container registry

# Configuration
DEFAULT_REGISTRY="ghcr.io/jmylchreest/m3u-proxy"  # Default to GitHub Container Registry
IMAGE_NAME="m3u-proxy"

# Help function
show_help() {
    cat << EOF
Usage: $0 [OPTIONS] [REGISTRY]

Push m3u-proxy container images to a registry with proper tagging.

ARGUMENTS:
    REGISTRY            Container registry URL (default: ${DEFAULT_REGISTRY})

OPTIONS:
    -h, --help         Show this help message
    -n, --dry-run      Show what would be pushed without actually pushing
    -v, --version VER  Specify version to push (default: auto-detect from get-version)
    --runtime RUNTIME  Force specific container runtime (podman|docker)

EXAMPLES:
    # Push to default registry (GitHub)
    $0

    # Push to custom registry
    $0 my-registry.com/user/m3u-proxy

    # Dry run to see what would be pushed
    $0 --dry-run

    # Push specific version
    $0 --version 0.1.5

    # Force use of docker instead of auto-detection
    $0 --runtime docker

NOTES:
    - You must be logged in to the registry before running this script
    - Images are expected to already be built locally (run build-container.sh first)
    - For release versions: pushes VERSION and 'latest' tags
    - For snapshot versions: pushes VERSION and 'snapshot' tags
EOF
}

# Parse command line arguments
REGISTRY=""
DRY_RUN=false
VERSION=""
CONTAINER_RUNTIME=""

while [[ $# -gt 0 ]]; do
    case $1 in
        -h|--help)
            show_help
            exit 0
            ;;
        -n|--dry-run)
            DRY_RUN=true
            shift
            ;;
        -v|--version)
            VERSION="$2"
            shift 2
            ;;
        --runtime)
            CONTAINER_RUNTIME="$2"
            shift 2
            ;;
        -*)
            echo "Unknown option: $1" >&2
            echo "Run '$0 --help' for usage information." >&2
            exit 1
            ;;
        *)
            if [[ -z "$REGISTRY" ]]; then
                REGISTRY="$1"
            else
                echo "Error: Multiple registries specified: '$REGISTRY' and '$1'" >&2
                exit 1
            fi
            shift
            ;;
    esac
done

# Set default registry if not provided
if [[ -z "$REGISTRY" ]]; then
    REGISTRY="$DEFAULT_REGISTRY"
fi

# Container runtime detection with preference order
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
if [[ -n "$CONTAINER_RUNTIME" ]]; then
    echo "Using explicitly set container runtime: $CONTAINER_RUNTIME"
    if ! command -v "$CONTAINER_RUNTIME" &> /dev/null; then
        echo "Error: Specified container runtime '$CONTAINER_RUNTIME' not found!"
        exit 1
    fi
else
    detect_container_runtime
fi

# Get version to push
if [[ -z "$VERSION" ]]; then
    if command -v just >/dev/null 2>&1; then
        VERSION=$(just get-version)
        echo "Using version from just get-version: $VERSION"
    else
        # Fallback to Cargo.toml if just is not available
        VERSION=$(grep '^version' crates/m3u-proxy/Cargo.toml | head -1 | cut -d'"' -f2)
        echo "Using fallback version from Cargo.toml: $VERSION"
    fi
else
    echo "Using specified version: $VERSION"
fi

# Determine tag strategy based on version type
# Check for both localhost/ prefixed and non-prefixed images (Podman vs Docker)
SOURCE_IMAGE_TAG="${IMAGE_NAME}:${VERSION}"
LOCALHOST_SOURCE_IMAGE_TAG="localhost/${IMAGE_NAME}:${VERSION}"
if echo "$VERSION" | grep -q "snapshot"; then
    ADDITIONAL_TAG="snapshot"
    echo "Detected snapshot version - will push as 'snapshot' tag"
else
    ADDITIONAL_TAG="latest"
    echo "Detected release version - will push as 'latest' tag"
fi

# Define target tags
VERSIONED_TARGET="${REGISTRY}:${VERSION}"
ADDITIONAL_TARGET="${REGISTRY}:${ADDITIONAL_TAG}"

# Verify source images exist locally
echo "Verifying local images..."
if "${CONTAINER_RUNTIME}" images --format "{{.Repository}}:{{.Tag}}" | grep -q "^${SOURCE_IMAGE_TAG}$"; then
    ACTUAL_SOURCE_IMAGE_TAG="${SOURCE_IMAGE_TAG}"
    echo "Found source image: ${ACTUAL_SOURCE_IMAGE_TAG}"
elif "${CONTAINER_RUNTIME}" images --format "{{.Repository}}:{{.Tag}}" | grep -q "^${LOCALHOST_SOURCE_IMAGE_TAG}$"; then
    ACTUAL_SOURCE_IMAGE_TAG="${LOCALHOST_SOURCE_IMAGE_TAG}"
    echo "Found source image: ${ACTUAL_SOURCE_IMAGE_TAG}"
else
    echo "Error: Local image '${SOURCE_IMAGE_TAG}' or '${LOCALHOST_SOURCE_IMAGE_TAG}' not found!"
    echo "Please run 'build-container.sh' or 'just build-container' first."
    echo ""
    echo "Available m3u-proxy images:"
    "${CONTAINER_RUNTIME}" images "${IMAGE_NAME}" --format "table {{.Repository}}\t{{.Tag}}\t{{.Size}}" || echo "No m3u-proxy images found"
    exit 1
fi

# Check if additional source tag exists (e.g., m3u-proxy:latest or m3u-proxy:snapshot)
SOURCE_ADDITIONAL_TAG="${IMAGE_NAME}:${ADDITIONAL_TAG}"
LOCALHOST_SOURCE_ADDITIONAL_TAG="localhost/${IMAGE_NAME}:${ADDITIONAL_TAG}"
HAS_ADDITIONAL_SOURCE=false
ACTUAL_SOURCE_ADDITIONAL_TAG=""

if "${CONTAINER_RUNTIME}" images --format "{{.Repository}}:{{.Tag}}" | grep -q "^${SOURCE_ADDITIONAL_TAG}$"; then
    ACTUAL_SOURCE_ADDITIONAL_TAG="${SOURCE_ADDITIONAL_TAG}"
    HAS_ADDITIONAL_SOURCE=true
elif "${CONTAINER_RUNTIME}" images --format "{{.Repository}}:{{.Tag}}" | grep -q "^${LOCALHOST_SOURCE_ADDITIONAL_TAG}$"; then
    ACTUAL_SOURCE_ADDITIONAL_TAG="${LOCALHOST_SOURCE_ADDITIONAL_TAG}"
    HAS_ADDITIONAL_SOURCE=true
fi

echo "Container push summary:"
echo "  Container Runtime: ${CONTAINER_RUNTIME}"
echo "  Registry: ${REGISTRY}"
echo "  Version: ${VERSION}"
echo "  Source Images:"
echo "    - ${ACTUAL_SOURCE_IMAGE_TAG}"
if [[ "$HAS_ADDITIONAL_SOURCE" == true ]]; then
    echo "    - ${ACTUAL_SOURCE_ADDITIONAL_TAG}"
fi
echo "  Target Images:"
echo "    - ${VERSIONED_TARGET}"
echo "    - ${ADDITIONAL_TARGET}"
echo ""

if [[ "$DRY_RUN" == true ]]; then
    echo "DRY RUN - Commands that would be executed:"
    echo ""
    echo "# Tag images for registry"
    echo "${CONTAINER_RUNTIME} tag ${ACTUAL_SOURCE_IMAGE_TAG} ${VERSIONED_TARGET}"
    if [[ "$HAS_ADDITIONAL_SOURCE" == true ]]; then
        echo "${CONTAINER_RUNTIME} tag ${ACTUAL_SOURCE_ADDITIONAL_TAG} ${ADDITIONAL_TARGET}"
    else
        echo "${CONTAINER_RUNTIME} tag ${ACTUAL_SOURCE_IMAGE_TAG} ${ADDITIONAL_TARGET}"
    fi
    echo ""
    echo "# Push images to registry"
    echo "${CONTAINER_RUNTIME} push ${VERSIONED_TARGET}"
    echo "${CONTAINER_RUNTIME} push ${ADDITIONAL_TARGET}"
    echo ""
    echo "DRY RUN - No actual changes made"
    exit 0
fi

# Tag images for the registry
echo "Tagging images for registry..."
echo "  ${ACTUAL_SOURCE_IMAGE_TAG} → ${VERSIONED_TARGET}"
if ! "${CONTAINER_RUNTIME}" tag "${ACTUAL_SOURCE_IMAGE_TAG}" "${VERSIONED_TARGET}"; then
    echo "Error: Failed to tag image for registry"
    exit 1
fi

if [[ "$HAS_ADDITIONAL_SOURCE" == true ]]; then
    echo "  ${ACTUAL_SOURCE_ADDITIONAL_TAG} → ${ADDITIONAL_TARGET}"
    if ! "${CONTAINER_RUNTIME}" tag "${ACTUAL_SOURCE_ADDITIONAL_TAG}" "${ADDITIONAL_TARGET}"; then
        echo "Error: Failed to tag additional image for registry"
        exit 1
    fi
else
    echo "  ${ACTUAL_SOURCE_IMAGE_TAG} → ${ADDITIONAL_TARGET}"
    if ! "${CONTAINER_RUNTIME}" tag "${ACTUAL_SOURCE_IMAGE_TAG}" "${ADDITIONAL_TARGET}"; then
        echo "Error: Failed to tag additional image for registry"
        exit 1
    fi
fi

# Push images to registry
echo ""
echo "Pushing images to registry..."
echo "  Pushing ${VERSIONED_TARGET}..."
if ! "${CONTAINER_RUNTIME}" push "${VERSIONED_TARGET}"; then
    echo "Error: Failed to push versioned image"
    exit 1
fi

echo "  Pushing ${ADDITIONAL_TARGET}..."
if ! "${CONTAINER_RUNTIME}" push "${ADDITIONAL_TARGET}"; then
    echo "Error: Failed to push additional tag image"
    exit 1
fi

echo ""
echo "✅ Container push completed successfully!"
echo ""
echo "Images pushed:"
echo "  📦 ${VERSIONED_TARGET}"
echo "  📦 ${ADDITIONAL_TARGET}"
echo ""
echo "Usage examples:"
echo "  # Pull the specific version"
echo "  ${CONTAINER_RUNTIME} pull ${VERSIONED_TARGET}"
echo ""
echo "  # Pull the ${ADDITIONAL_TAG} version"
echo "  ${CONTAINER_RUNTIME} pull ${ADDITIONAL_TARGET}"
echo ""
echo "  # Run the container"
echo "  ${CONTAINER_RUNTIME} run -p 8080:8080 -v \$(pwd)/data:/app/data ${ADDITIONAL_TARGET}"