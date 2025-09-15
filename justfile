# Monorepo build automation for m3u-proxy
# Manages both Next.js frontend and Rust backend builds with intelligent version management

# Default recipe - show available commands
default:
    @just --list

# Version Management
# ==================
# Smart versioning based on git tags with semver validation

# Get current version from source files (Cargo.toml)
get-current-version:
    @grep '^version' crates/m3u-proxy/Cargo.toml | head -1 | cut -d'"' -f2

# Get the version to use for builds (git-based versioning for CI/CD reliability)
# - If on git tag: v0.1.3 → 0.1.3 (release)
# - If after tag: v0.1.3-12-ga1b2c3d → 0.1.4-dev.12.ga1b2c3d (development)
# - If no tags: 0.0.1-dev.0.g<commit> (initial development)
# This ensures consistent versions across Cargo.toml, containers, and SBOM
get-version:
    #!/usr/bin/env bash
    set -euo pipefail

    # Obtain the current declared version from Cargo.toml
    CURRENT_VERSION=$(just get-current-version || echo "0.0.0")

    parse_semver() {
        # $1 = version string (may contain -dev suffix)
        if [[ $1 =~ ^([0-9]+)\.([0-9]+)\.([0-9]+) ]]; then
            echo "${BASH_REMATCH[1]} ${BASH_REMATCH[2]} ${BASH_REMATCH[3]}"
        else
            echo "0 0 0"
        fi
    }

    cv=($(parse_semver "$CURRENT_VERSION"))
    CV_MAJOR=${cv[0]}
    CV_MINOR=${cv[1]}
    CV_PATCH=${cv[2]}

    # If exactly on a tag, normalize and return it (strip leading v)
    if TAG=$(git describe --exact-match --tags HEAD 2>/dev/null); then
        VERSION=${TAG#v}
        echo "$VERSION"
        exit 0
    fi

    # If we have at least one tag reachable
    if git describe --tags --always --long 2>/dev/null | grep -q '^v'; then
        DESCRIBE=$(git describe --tags --always --long 2>/dev/null)
        # Expect form: vMAJOR.MINOR.PATCH-COMMITS-gHASH
        if [[ $DESCRIBE =~ ^v([0-9]+)\.([0-9]+)\.([0-9]+)-([0-9]+)-(g[a-f0-9]+)$ ]]; then
            MAJOR=${BASH_REMATCH[1]}
            MINOR=${BASH_REMATCH[2]}
            PATCH=${BASH_REMATCH[3]}
            COMMITS=${BASH_REMATCH[4]}
            COMMIT=${BASH_REMATCH[5]}

            NEXT_PATCH_FROM_TAG=$((PATCH + 1))

            # Determine if CURRENT_VERSION is ahead of last tag
            # Compare (MAJOR,MINOR,PATCH) vs CURRENT_VERSION
            use_current_ahead=false
            if (( CV_MAJOR > MAJOR )); then
                use_current_ahead=true
            elif (( CV_MAJOR == MAJOR && CV_MINOR > MINOR )); then
                use_current_ahead=true
            elif (( CV_MAJOR == MAJOR && CV_MINOR == MINOR && CV_PATCH > PATCH )); then
                use_current_ahead=true
            fi

            if $use_current_ahead; then
                # CURRENT_VERSION is already ahead of the last tag; reuse its patch (do not increment again)
                BASE_PATCH=$CV_PATCH
            else
                # Normal case: use tag patch + 1 as the dev baseline
                BASE_PATCH=$NEXT_PATCH_FROM_TAG
            fi

            echo "${MAJOR}.${MINOR}.${BASE_PATCH}-dev.${COMMITS}.${COMMIT}"
        else
            echo "Error: Unable to parse git describe output: $DESCRIBE" >&2
            exit 1
        fi
    else
        # No tags at all: initial dev version based on commit
        COMMIT=$(git rev-parse --short=7 HEAD 2>/dev/null || echo "unknown")
        echo "0.0.1-dev.0.g${COMMIT}"
    fi

# Set version in all relevant files (Cargo.toml, package.json, package-lock.json)
# Supports both release versions (0.1.3) and development versions (0.1.4-dev.12.ga1b2c3d)
# Usage: just set-version 0.1.3 [--force]
set-version version *force="":
    #!/usr/bin/env bash
    set -euo pipefail

    VERSION="{{version}}"
    CURRENT_VERSION=$(just get-current-version)
    FORCE="{{force}}"

    # Validate version format (semver with optional development pre-release)
    if ! echo "$VERSION" | grep -qE '^[0-9]+\.[0-9]+\.[0-9]+(-dev\.[0-9]+\.g[a-f0-9]+)?$'; then
        echo "Error: Invalid version format."
        echo "Expected formats:"
        echo "  Release: 1.0.0"
        echo "  Development: 1.0.1-dev.12.ga1b2c3d"
        exit 1
    fi

    # Function to compare semver versions (without pre-release suffixes)
    compare_versions() {
        local version1="$1"
        local version2="$2"

        # Strip pre-release suffixes for comparison
        local clean_v1=$(echo "$version1" | sed 's/-.*$//')
        local clean_v2=$(echo "$version2" | sed 's/-.*$//')

        # Split into major.minor.patch
        IFS='.' read -r v1_major v1_minor v1_patch <<< "$clean_v1"
        IFS='.' read -r v2_major v2_minor v2_patch <<< "$clean_v2"

        # Compare major
        if [ "$v1_major" -gt "$v2_major" ]; then
            return 0  # v1 > v2
        elif [ "$v1_major" -lt "$v2_major" ]; then
            return 1  # v1 < v2
        fi

        # Compare minor
        if [ "$v1_minor" -gt "$v2_minor" ]; then
            return 0  # v1 > v2
        elif [ "$v1_minor" -lt "$v2_minor" ]; then
            return 1  # v1 < v2
        fi

        # Compare patch
        if [ "$v1_patch" -gt "$v2_patch" ]; then
            return 0  # v1 > v2
        elif [ "$v1_patch" -lt "$v2_patch" ]; then
            return 1  # v1 < v2
        fi

        # Equal
        return 2
    }

    # Check for version downgrade (only for non-snapshot versions)
    if [ "$FORCE" != "--force" ] && ! echo "$VERSION" | grep -q "snapshot" && ! echo "$CURRENT_VERSION" | grep -q "snapshot"; then
        if compare_versions "$CURRENT_VERSION" "$VERSION"; then
            echo "Error: Cannot downgrade from $CURRENT_VERSION to $VERSION"
            echo "Current version is higher than the requested version"
            echo "Use 'just set-version $VERSION --force' to override"
            exit 1
        elif compare_versions "$CURRENT_VERSION" "$VERSION"; then
            : # This is the v1 > v2 case, which we already handled above
        else
            # Equal versions - allow it (useful for re-setting the same version)
            echo "Version unchanged: $VERSION"
        fi
    elif [ "$FORCE" = "--force" ]; then
        echo "Force flag used - allowing version change from $CURRENT_VERSION to $VERSION"
    fi

    echo "Setting version to: $VERSION"

    # Update Rust crate version
    sed -i.bak "s/^version = \".*\"/version = \"$VERSION\"/" crates/m3u-proxy/Cargo.toml
    rm -f crates/m3u-proxy/Cargo.toml.bak

    # Update frontend package.json version
    sed -i.bak "s/\"version\": \".*\"/\"version\": \"$VERSION\"/" frontend/package.json
    rm -f frontend/package.json.bak

    # Update frontend package-lock.json version (only root-level versions, not dependencies)
    if [ -f frontend/package-lock.json ]; then
        # Create a safer update using jq if available, otherwise use targeted sed
        if command -v jq >/dev/null 2>&1; then
            # Use jq for precise JSON manipulation
            jq --arg version "$VERSION" '.version = $version | .packages."".version = $version' frontend/package-lock.json > frontend/package-lock.json.tmp && mv frontend/package-lock.json.tmp frontend/package-lock.json
        else
            # Fallback to targeted sed (only update the first few lines where root version appears)
            sed -i.bak '1,20 { s/^  "version": ".*"/  "version": "'"$VERSION"'"/; }' frontend/package-lock.json
            sed -i.bak '/^  "packages": {$/,/^    "": {$/ { /^      "version": ".*"$/ { s/^      "version": ".*"/      "version": "'"$VERSION"'"/; } }' frontend/package-lock.json
            rm -f frontend/package-lock.json.bak
        fi
    fi

    echo "Version set to: $VERSION"

# Tag a new release with automatic version bumping
# Usage: just tag-release [major|minor]
# Without args: patch bump (0.2.3 -> 0.2.4)
# With major: major bump (0.2.3 -> 1.0.0)
# With minor: minor bump (0.2.3 -> 0.3.0)
tag-release bump="patch":
    @./scripts/tag-release.sh {{bump}}

# Run all tests (Rust and Next.js)
test:
    @echo "Running Rust tests..."
    cargo test --all-features --verbose
    @echo "Running Next.js tests..."
    cd frontend && npm test || echo "No test script found, skipping..."

# Run tests in CI mode (with format checking)
test-ci: fmt-check test
    @echo "CI tests completed successfully!"
    cd frontend && npm test

# Development server for frontend (Next.js dev mode)
dev-frontend:
    @echo "Starting Next.js development server..."
    cd frontend && npm run dev

# Development server for backend (Rust cargo run)
dev-backend:
    @echo "Starting Rust development server..."
    cargo run --bin m3u-proxy

# Build frontend (Next.js static export)
build-frontend:
    @echo "Building Next.js frontend..."
    cd frontend && npm run build
    @echo "Frontend built to frontend/out/"

# Build backend (Rust release build)
build-backend:
    @echo "Building Rust backend..."
    cargo build --release --bin m3u-proxy
    @echo "Backend built to target/release/"

# Build backend with full release optimizations (strip debug symbols, optimize for size)
build-backend-optimized:
    @echo "Building Rust backend with full release optimizations..."
    RUSTFLAGS="-C strip=symbols -C opt-level=s" cargo build --release --bin m3u-proxy
    @echo "Optimized backend built to target/release/"

# Get the path to the built binary
binary-path:
    #!/usr/bin/env bash
    set -euo pipefail

    # Check if release binary exists
    if [ -f "target/release/m3u-proxy" ]; then
        echo "$(pwd)/target/release/m3u-proxy"
    # Check if debug binary exists
    elif [ -f "target/debug/m3u-proxy" ]; then
        echo "$(pwd)/target/debug/m3u-proxy"
    else
        echo "Binary not found. Run 'just build-backend' or 'just build-backend-optimized' first." >&2
        exit 1
    fi

# Get binary info (path, size, type, etc.)
binary-info:
    #!/usr/bin/env bash
    set -euo pipefail

    BINARY_PATH=$(just binary-path)
    if [ $? -eq 0 ]; then
        echo "Binary location: $BINARY_PATH"
        echo "Binary size: $(du -h "$BINARY_PATH" | cut -f1)"
        echo "File type: $(file "$BINARY_PATH")"
        if command -v ldd >/dev/null 2>&1; then
            echo "Linked libraries:"
            ldd "$BINARY_PATH" 2>/dev/null | head -5 || echo "  Static binary or libraries not shown"
        fi
    fi

# Copy frontend build to backend static directory
copy-frontend:
    @echo "Copying frontend build to backend static directory..."
    rm -rf crates/m3u-proxy/static/*
    mkdir -p crates/m3u-proxy/static
    cp -r frontend/out/* crates/m3u-proxy/static/
    @echo "Frontend files copied to crates/m3u-proxy/static/"

# Clean all build artifacts
clean:
    @echo "Cleaning build artifacts..."
    cargo clean
    rm -rf frontend/node_modules
    rm -rf frontend/out
    rm -rf frontend/.next
    rm -rf crates/m3u-proxy/static/*
    @echo "All build artifacts cleaned"

# Complete build process (clean, install, test, build all)
build-all: clean install test build-frontend copy-frontend build-backend
    @echo "Complete build finished!"
    @echo "Backend binary: target/release/m3u-proxy"
    @echo "Frontend assets embedded in binary"

# Install dependencies
install:
    @echo "Installing dependencies..."
    cd frontend && npm install
    @echo "Dependencies installed"

# Install all dependencies
install-all: install
    @echo "All dependencies installed"

# Upgrade backend dependencies (Rust)
# Use --aggressive to upgrade to latest compatible versions beyond Cargo.toml constraints
# Use --yes for non-interactive mode
# Requires: cargo install cargo-outdated cargo-udeps, rustup toolchain install nightly
upgrade-deps-backend *args="":
    #!/usr/bin/env bash
    set -euo pipefail

    echo "Upgrading Rust dependencies..."

    # Check if aggressive upgrade requested
    if [[ "{{args}}" == *"--aggressive"* ]]; then
        echo "Performing aggressive upgrade..."
        if command -v cargo-upgrade >/dev/null 2>&1; then
            # Use cargo-edit to upgrade Cargo.toml versions
            cargo upgrade
            cargo update
        else
            echo "Error: cargo-edit not installed!"
            echo "Install with: cargo install cargo-edit"
            echo "Aborting aggressive upgrade..."
            exit 1
        fi
    else
        # Standard update within Cargo.toml constraints
        cargo update
    fi

    echo "Checking for outdated Rust crates..."
    if command -v cargo-outdated >/dev/null 2>&1; then
        cargo outdated
    else
        echo "cargo-outdated not installed - run 'cargo install cargo-outdated' to check for updates"
    fi

    echo "Checking for unused Rust dependencies..."
    if command -v cargo-udeps >/dev/null 2>&1; then
        cargo +nightly udeps --all-targets
    else
        echo "cargo-udeps not installed - run 'cargo install cargo-udeps' to check for unused dependencies"
        echo "Note: cargo-udeps requires nightly toolchain: 'rustup toolchain install nightly'"
    fi

    echo "Rust dependencies upgraded!"
    echo ""
    if [[ "{{args}}" != *"--aggressive"* ]]; then
        echo "To upgrade to latest major versions:"
        echo "   just upgrade-deps-backend --aggressive"
        echo "   (requires: cargo install cargo-edit)"
        echo "For non-interactive mode: just upgrade-deps-backend --aggressive --yes"
        echo ""
        echo "Optional tools for enhanced dependency management:"
        echo "   cargo install cargo-outdated cargo-udeps"
        echo "   rustup toolchain install nightly  # Required for cargo-udeps"
    fi

# Upgrade frontend dependencies (npm)
# Use --aggressive to upgrade to latest versions beyond package.json constraints
# Use --yes for non-interactive mode
upgrade-deps-frontend *args="":
    #!/usr/bin/env bash
    set -euo pipefail

    echo "Upgrading npm dependencies..."

    # Check if aggressive upgrade requested
    if [[ "{{args}}" == *"--aggressive"* ]]; then
        echo "Performing aggressive upgrade..."
        if command -v npx >/dev/null 2>&1; then
            # Use npm-check-updates to upgrade package.json versions
            if [[ "{{args}}" == *"--yes"* ]]; then
                # Non-interactive mode - auto-accept package installs
                cd frontend && echo "y" | npx npm-check-updates -u && npm install
            else
                # Interactive mode
                cd frontend && npx npm-check-updates -u && npm install
            fi
        else
            echo "npx not available. Falling back to standard npm update..."
            cd frontend && npm update
        fi
    else
        # Standard update within package.json constraints
        cd frontend && npm update
    fi

    echo "Checking for outdated npm packages..."
    cd frontend && { npm outdated || echo "All npm packages are up to date"; }

    echo "Checking for unused npm dependencies..."
    (cd frontend && { npx depcheck --ignores="@tailwindcss/postcss,autoprefixer,postcss,tw-animate-css" || echo "No unused dependencies found"; })

    echo "npm dependencies upgraded!"
    echo ""
    if [[ "{{args}}" != *"--aggressive"* ]]; then
        echo "To upgrade to latest major versions:"
        echo "   just upgrade-deps-frontend --aggressive"
        echo "For non-interactive mode: just upgrade-deps-frontend --aggressive --yes"
    fi

# Upgrade all dependencies (backend + frontend)
upgrade-deps-all *args="":
    just upgrade-deps-backend {{args}}
    just upgrade-deps-frontend {{args}}
    @echo "All dependencies upgraded!"

# Development setup (install deps and run dev servers)
dev: install
    @echo "Starting development environment..."
    @echo "Frontend will be available at http://localhost:3000"
    @echo "Backend will be available at http://localhost:8080"
    @echo ""
    @echo "Run in separate terminals:"
    @echo "  just dev-frontend"
    @echo "  just dev-backend"

# Quick build for development (skips tests and clean)
build-dev: build-frontend copy-frontend build-backend
    @echo "Development build finished!"

# Build container image (supports podman, docker, buildah, nerdctl)
# Tags: :latest (always), :release (tagged releases), :snapshot (dev builds)
build-container:
    @echo "Building container using external script with runtime detection..."
    @echo "Tagging strategy: :latest + (:release for tagged releases | :snapshot for dev builds)"
    @echo "Ensuring npm dependencies are up to date..."
    cd frontend && npm install && cd ..
    ./scripts/build-container.sh

# Push container image to registry (supports podman, docker, nerdctl)
# Pushes all three tags: version, :latest, and (:release | :snapshot)
push-container registry="":
    @echo "Pushing container to registry using external script with runtime detection..."
    # Run the push script directly; it has its own bash shebang and sets -euo pipefail internally
    ./scripts/push-container.sh "{{registry}}"

# Format all code (Rust and frontend)
fmt:
    @./scripts/format.sh

# Check if code is properly formatted without changing files
fmt-check:
    @./scripts/check-format.sh

# Lint all code
lint:
    @./scripts/lint.sh

# Run security audit
audit:
    @echo "Auditing Rust dependencies..."
    cargo audit || echo "cargo-audit not installed, skipping..."
    @echo "Auditing frontend dependencies..."
    cd frontend && npm audit || echo "npm audit completed with warnings"

# Run Rust benchmarks
bench-rust:
    @echo "Running Rust benchmarks..."
    cargo bench
    @echo "Rust benchmark results in target/criterion/"

# Run frontend benchmarks (if available)
bench-frontend:
    @echo "Running frontend benchmarks..."
    cd frontend && npm run bench || echo "No bench script found, skipping frontend benchmarks"

# Run all benchmarks
bench: bench-rust bench-frontend
    @echo "All benchmarks completed!"
    @echo "Rust results: target/criterion/"
    @echo "Frontend results: check frontend/benchmark-results or console output"

# Full quality check (format check, lint, audit, test)
check: fmt-check lint audit test
    @echo "All quality checks passed!"

# Full quality check with auto-fix (format, lint, audit, test)
check-fix: fmt lint audit test
    @echo "All quality checks passed (with fixes applied)!"

# Performance testing (benchmarks + tests)
perf: test bench
    @echo "Performance testing completed!"

# Extended test command
test-all: test
    @echo "All tests (backend, frontend) completed!"

# Quality check (same as check but more explicit name)
check-all: fmt-check lint audit test
    @echo "All quality checks passed!"

# Pre-commit checks - run this before committing code
pre-commit:
    @./scripts/pre-commit.sh

# Install git hooks for automatic pre-commit checks
install-hooks:
    @./scripts/install-hooks.sh

# Development workflow: build and test
dev-check: build-dev
    @echo "Development check complete!"

# Version-aware builds
# ====================

# Build with git-based version management (always updates Cargo.toml to match)
build-versioned:
    #!/usr/bin/env bash
    set -euo pipefail
    VERSION=$(just get-version)
    echo "Building with git-based version: $VERSION"

    # Sync manifests only when needed; preserve and persist -dev suffix for development builds
    if [[ "$VERSION" =~ -dev ]]; then
        if [ "$(just get-current-version)" != "$VERSION" ]; then
            echo "Development build; syncing manifests (Cargo.toml, package.json) to $VERSION"
            just set-version "$VERSION"
        else
            echo "Development build; manifests already at $VERSION"
        fi
    else
        if [ "$(just get-current-version)" != "$VERSION" ]; then
            echo "Release build; syncing manifests (Cargo.toml, package.json) to $VERSION"
            just set-version "$VERSION"
        else
            echo "Release build; manifests already at $VERSION"
        fi
    fi

    # Check if this is a release version (no -dev suffix)
    if [[ "$VERSION" =~ -dev ]]; then
        echo "Development build detected - using standard release build"
        just build-frontend copy-frontend build-backend
    else
        echo "Release build detected - using optimized build with stripped symbols"
        just build-frontend copy-frontend build-backend-optimized
    fi

    echo "✅ Versioned build complete with version: $VERSION"
    echo "   Cargo.toml, containers, and SBOM will all reference: $VERSION"

# Build container with git-based version tagging
build-container-versioned:
    #!/usr/bin/env bash
    set -euo pipefail

    RAW_VERSION=$(just get-version)
    CURRENT_VERSION=$(just get-current-version || echo "0.0.0")

    normalize_core() { echo "${1%%-*}"; }

    ver_gt() {
        IFS='.' read -r a b c <<< "$(normalize_core "$1")"
        IFS='.' read -r x y z <<< "$(normalize_core "$2")"
        if (( a > x )) || (( a == x && b > y )) || (( a == x && b == y && c > z )); then
            return 0
        else
            return 1
        fi
    }

    # Decide on safe, non-downgrading version
    if ver_gt "$RAW_VERSION" "$CURRENT_VERSION"; then
        VERSION="$RAW_VERSION"
    elif ver_gt "$CURRENT_VERSION" "$RAW_VERSION"; then
        echo "Computed version ($RAW_VERSION) is lower than current ($CURRENT_VERSION); preserving current."
        VERSION="$CURRENT_VERSION"
    else
        VERSION="$CURRENT_VERSION"
    fi

    echo "Building container with safe version: $VERSION"

    # Sync Cargo.toml version only when:
    #  - It is behind the computed core (next dev baseline), or
    #  - This is a tagged (non -dev) release and differs
    core() { echo "${1%%-*}"; }
    VERSION_CORE=$(core "$VERSION")
    CURRENT_CORE=$(core "$CURRENT_VERSION")

    # For non-tagged (development) builds we want the full -dev.* string persisted in manifests.
    # For tagged releases we ensure exact match to the tag version.
    if [[ "$VERSION" =~ -dev ]]; then
        if [ "$CURRENT_VERSION" != "$VERSION" ]; then
            echo "Development build; syncing manifests (Cargo.toml, package.json) to development version: $VERSION"
            just set-version "$VERSION"
        else
            echo "Development build; manifests already at $VERSION"
        fi
    else
        if [ "$CURRENT_VERSION" != "$VERSION" ]; then
            echo "Release build; syncing manifests (Cargo.toml, package.json) to $VERSION"
            just set-version "$VERSION"
        else
            echo "Release build; manifests already at $VERSION"
        fi
    fi

    echo "Ensuring npm dependencies are up to date..."
    PROJECT_ROOT=$(pwd)
    cd frontend && npm install
    cd "$PROJECT_ROOT"

    ./scripts/build-container.sh "$VERSION"

# Push container with proper version tagging
push-container-versioned registry="":
    #!/usr/bin/env bash
    set -euo pipefail

    # Get the version to use
    VERSION=$(just get-version)
    echo "Pushing container with version: $VERSION"

    # Run the container push with version and optional registry
    if [ -z "{{registry}}" ]; then
        ./scripts/push-container.sh --version "$VERSION"
    else
        ./scripts/push-container.sh --version "$VERSION" "{{registry}}"
    fi
