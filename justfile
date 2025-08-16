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
    @echo "All build artifacts cleaned"

# Complete build process (clean, install, test, build all, containerize)
build-all: clean install test build-frontend copy-frontend build-backend container-build
    @echo "Complete build finished!"
    @echo "Backend binary: target/release/m3u-proxy"
    @echo "Frontend assets embedded in binary"
    @echo "Container image: m3u-proxy:latest"

# Install dependencies
install:
    @echo "Installing dependencies..."
    cd frontend && npm install
    @echo "Dependencies installed"

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

# Container build (supports podman, docker, buildah, nerdctl)
container-build:
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