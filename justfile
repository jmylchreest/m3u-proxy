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

# Get the version to use for builds (tag-based release or snapshot with date)
# - If on git tag: uses tag version (strips 'v' prefix)
# - If not on tag: calculates next patch version with '-snapshot-YYYYMMDD' suffix
get-version:
    #!/usr/bin/env bash
    set -euo pipefail

    # Check if we're on a git tag
    if git describe --exact-match --tags HEAD 2>/dev/null; then
        # We're on a tag, use it as version (strip 'v' prefix if present)
        TAG=$(git describe --exact-match --tags HEAD 2>/dev/null)
        VERSION=${TAG#v}
        echo "$VERSION"
    else
        # Not on a tag, calculate next version with snapshot suffix
        CURRENT_VERSION=$(just get-current-version)

        # Parse version parts (assuming semver X.Y.Z)
        IFS='.' read -r MAJOR MINOR PATCH <<< "$CURRENT_VERSION"

        # Increment patch version for snapshot
        NEXT_PATCH=$((PATCH + 1))

        # Get current date in YYYYMMDD format
        DATE=$(date +%Y%m%d)

        # Create snapshot version
        SNAPSHOT_VERSION="${MAJOR}.${MINOR}.${NEXT_PATCH}-snapshot-${DATE}"
        echo "$SNAPSHOT_VERSION"
    fi

# Set version in all relevant files (Cargo.toml, package.json, package-lock.json)
# Includes semver validation and prevents downgrades unless --force is used
# Usage: just set-version 0.1.3 [--force]
set-version version *force="":
    #!/usr/bin/env bash
    set -euo pipefail

    VERSION="{{version}}"
    CURRENT_VERSION=$(just get-current-version)
    FORCE="{{force}}"

    # Validate version format (basic semver check)
    if ! echo "$VERSION" | grep -qE '^[0-9]+\.[0-9]+\.[0-9]+(-.*)?$'; then
        echo "Error: Invalid version format. Expected semver (e.g., 1.0.0)"
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

    # Update frontend package-lock.json version (both root and packages section)
    if [ -f frontend/package-lock.json ]; then
        sed -i.bak "s/\"version\": \".*\"/\"version\": \"$VERSION\"/" frontend/package-lock.json
        rm -f frontend/package-lock.json.bak
    fi

    echo "Version set to: $VERSION"

# Run all tests (Rust and Next.js)
test:
    @echo "Running Rust tests..."
    cargo test
    @echo "Running Next.js tests..."
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
    rm -rf tests/playwright/node_modules
    rm -rf tests/playwright/test-results
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

# Install all dependencies including UI testing
install-all: install ui-setup
    @echo "All dependencies (frontend + UI testing) installed"

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
build-container:
    @echo "Building container using external script with runtime detection..."
    ./build-container.sh

# Push container image to registry (supports podman, docker, nerdctl)
push-container registry="":
    @echo "Pushing container to registry using external script with runtime detection..."
    #!/usr/bin/env bash
    set -euo pipefail
    ./push-container.sh "{{registry}}"

# Format all code (Rust and frontend)
fmt:
    @echo "Formatting Rust code..."
    cargo fmt
    @echo "Formatting frontend code..."
    cd frontend && npm run format || echo "No format script found, skipping..."

# Lint all code
lint:
    @echo "Linting Rust code..."
    cargo clippy -- -D warnings
    @echo "Linting frontend code..."
    cd frontend && npm run lint || echo "No lint script found, skipping..."

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

# Full quality check (format, lint, audit, test)
check: fmt lint audit test
    @echo "All quality checks passed!"

# Performance testing (benchmarks + tests)
perf: test bench
    @echo "Performance testing completed!"

# UI Testing with Playwright
# ===========================

# Setup Playwright for the first time
ui-setup:
    @echo "🎭 Setting up Playwright UI testing..."
    @if [ ! -d "tests/playwright" ]; then echo "❌ Playwright tests not found. Please ensure tests/playwright directory exists."; exit 1; fi
    cd tests/playwright && npm install
    cd tests/playwright && npx playwright install
    @echo "✅ Playwright setup complete!"

# Run UI tests (headless)
ui-test:
    @echo "🧪 Running UI tests..."
    @if [ ! -d "tests/playwright/node_modules" ]; then echo "📦 Installing Playwright dependencies first..."; just ui-setup; fi
    cd tests/playwright && npm test

# Run UI tests with visible browser (for debugging)
ui-test-headed:
    @echo "🧪 Running UI tests with visible browser..."
    @if [ ! -d "tests/playwright/node_modules" ]; then echo "📦 Installing Playwright dependencies first..."; just ui-setup; fi
    cd tests/playwright && npm run test:headed

# Run UI tests in debug mode (step-through)
ui-test-debug:
    @echo "🐛 Running UI tests in debug mode..."
    @if [ ! -d "tests/playwright/node_modules" ]; then echo "📦 Installing Playwright dependencies first..."; just ui-setup; fi
    cd tests/playwright && npm run test:debug

# Run UI tests with Playwright UI (interactive)
ui-test-ui:
    @echo "🎮 Running UI tests with interactive UI..."
    @if [ ! -d "tests/playwright/node_modules" ]; then echo "📦 Installing Playwright dependencies first..."; just ui-setup; fi
    cd tests/playwright && npm run test:ui

# Show UI test report
ui-report:
    @echo "📊 Opening UI test report..."
    @if [ ! -d "tests/playwright/test-results" ]; then echo "❌ No test results found. Run 'just ui-test' first."; exit 1; fi
    cd tests/playwright && npm run report

# Run UI tests on specific browser
ui-test-browser browser="chromium":
    @echo "🌐 Running UI tests on {{browser}}..."
    @if [ ! -d "tests/playwright/node_modules" ]; then echo "📦 Installing Playwright dependencies first..."; just ui-setup; fi
    cd tests/playwright && npx playwright test --project={{browser}}

# Run UI tests on mobile devices
ui-test-mobile:
    @echo "📱 Running UI tests on mobile devices..."
    @if [ ! -d "tests/playwright/node_modules" ]; then echo "📦 Installing Playwright dependencies first..."; just ui-setup; fi
    cd tests/playwright && npx playwright test --project=mobile-chrome --project=mobile-safari

# Clean UI test results
ui-clean:
    @echo "🧹 Cleaning UI test results..."
    rm -rf tests/playwright/test-results
    @echo "✅ UI test results cleaned!"

# Complete UI testing workflow (setup, test, report)
ui-full: ui-setup ui-test ui-report
    @echo "🎭 Complete UI testing workflow finished!"

# Quick UI smoke test (just chromium, essential tests)
ui-quick:
    @echo "⚡ Running quick UI smoke test..."
    @if [ ! -d "tests/playwright/node_modules" ]; then echo "📦 Installing Playwright dependencies first..."; just ui-setup; fi
    cd tests/playwright && npx playwright test ui-layout.spec.ts --project=chromium

# Extended test command that includes UI tests
test-all: test ui-test
    @echo "🧪 All tests (backend, frontend, UI) completed!"

# Quality check with UI tests
check-all: fmt lint audit test ui-test
    @echo "✅ All quality checks (including UI tests) passed!"

# Development workflow: build and test everything including UI
dev-check: build-dev ui-quick
    @echo "🚀 Development check complete!"

# Version-aware builds
# ====================

# Build with version management (updates versions before building)
build-versioned:
    #!/usr/bin/env bash
    set -euo pipefail
    VERSION=$(just get-version)
    just set-version "$VERSION"
    just build-all
    @echo "✅ Versioned build complete with version: $VERSION"

# Build container with proper version tagging
build-container-versioned:
    #!/usr/bin/env bash
    set -euo pipefail

    # Get the version to use
    VERSION=$(just get-version)
    echo "Building container with version: $VERSION"

    # Update versions first
    just set-version "$VERSION"

    # Run the container build with version as argument
    ./build-container.sh "$VERSION"

# Push container with proper version tagging
push-container-versioned registry="":
    #!/usr/bin/env bash
    set -euo pipefail

    # Get the version to use
    VERSION=$(just get-version)
    echo "Pushing container with version: $VERSION"

    # Run the container push with version and optional registry
    if [ -z "{{registry}}" ]; then
        ./push-container.sh --version "$VERSION"
    else
        ./push-container.sh --version "$VERSION" "{{registry}}"
    fi
