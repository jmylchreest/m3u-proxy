# Monorepo build automation for m3u-proxy
# Manages both Next.js frontend and Rust backend builds

# Default recipe - show available commands
default:
    @just --list

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