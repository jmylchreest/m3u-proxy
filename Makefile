# Makefile for m3u-proxy development

.PHONY: help build run test clean fmt lint check prepare docker docker-run dev setup release

# Default target
help: ## Show this help message
	@echo "Available targets:"
	@awk 'BEGIN {FS = ":.*?## "} /^[a-zA-Z_-]+:.*?## / {printf "  %-15s %s\n", $$1, $$2}' $(MAKEFILE_LIST)

# Development setup
setup: ## Set up development environment
	@echo "Setting up development environment..."
	cargo install sqlx-cli --no-default-features --features sqlite,rustls
	cargo install cargo-watch
	cargo install cargo-audit
	cargo install cargo-deny
	mkdir -p data/logos data/m3u
	touch data/m3u-proxy.db
	sqlx migrate run --database-url sqlite:data/m3u-proxy.db

# Database operations
db-create: ## Create database
	@echo "Creating database..."
	mkdir -p data
	touch data/m3u-proxy.db

db-migrate: ## Run database migrations
	@echo "Running migrations..."
	sqlx migrate run --database-url sqlite:data/m3u-proxy.db

db-reset: ## Reset database (drop and recreate)
	@echo "Resetting database..."
	rm -f data/m3u-proxy.db
	$(MAKE) db-create
	$(MAKE) db-migrate

prepare: ## Prepare SQLx queries
	@echo "Preparing SQLx queries..."
	sqlx prepare --database-url sqlite:data/m3u-proxy.db

# Build targets
build: ## Build the project in debug mode
	@echo "Building project..."
	cargo build

build-release: ## Build the project in release mode
	@echo "Building project (release)..."
	cargo build --release

# Run targets
run: ## Run the project
	@echo "Running project..."
	cargo run

run-release: ## Run the release build
	@echo "Running project (release)..."
	cargo run --release

dev: ## Run with auto-reload on file changes
	@echo "Starting development server with auto-reload..."
	cargo watch -x run

# Testing
test: ## Run all tests
	@echo "Running tests..."
	cargo test

test-verbose: ## Run tests with verbose output
	@echo "Running tests (verbose)..."
	cargo test --verbose

test-watch: ## Run tests continuously on file changes
	@echo "Running tests with watch mode..."
	cargo watch -x test

# Code quality
fmt: ## Format code
	@echo "Formatting code..."
	cargo fmt --all

fmt-check: ## Check code formatting
	@echo "Checking code formatting..."
	cargo fmt --all -- --check

lint: ## Run clippy linter
	@echo "Running clippy..."
	cargo clippy --all-targets --all-features -- -D warnings

check: ## Run all checks (format, lint, test)
	@echo "Running all checks..."
	$(MAKE) fmt-check
	$(MAKE) lint
	$(MAKE) test

# Security and audit
audit: ## Run security audit
	@echo "Running security audit..."
	cargo audit

deny: ## Run cargo-deny checks
	@echo "Running cargo-deny checks..."
	cargo deny check

security: ## Run all security checks
	@echo "Running security checks..."
	$(MAKE) audit
	$(MAKE) deny

# Docker operations
docker-build: ## Build Docker image
	@echo "Building Docker image..."
	docker build -t m3u-proxy:latest .

docker-run: ## Run Docker container
	@echo "Running Docker container..."
	docker run -p 8080:8080 -v $(PWD)/data:/app/data m3u-proxy:latest

docker-compose-up: ## Start services with docker-compose
	@echo "Starting services with docker-compose..."
	docker-compose up -d

docker-compose-down: ## Stop services with docker-compose
	@echo "Stopping services with docker-compose..."
	docker-compose down

docker-compose-logs: ## View docker-compose logs
	@echo "Viewing docker-compose logs..."
	docker-compose logs -f

# Cross-compilation
build-linux-amd64: ## Build for Linux AMD64
	@echo "Building for Linux AMD64..."
	cargo build --release --target x86_64-unknown-linux-gnu

build-linux-arm64: ## Build for Linux ARM64 (requires cross)
	@echo "Building for Linux ARM64..."
	cross build --release --target aarch64-unknown-linux-gnu

build-macos-amd64: ## Build for macOS AMD64
	@echo "Building for macOS AMD64..."
	cargo build --release --target x86_64-apple-darwin

build-macos-arm64: ## Build for macOS ARM64
	@echo "Building for macOS ARM64..."
	cargo build --release --target aarch64-apple-darwin

build-all: ## Build for all supported platforms
	@echo "Building for all platforms..."
	$(MAKE) build-linux-amd64
	$(MAKE) build-linux-arm64
	$(MAKE) build-macos-amd64
	$(MAKE) build-macos-arm64

# Release preparation
release-prepare: ## Prepare for release (check, test, build)
	@echo "Preparing for release..."
	$(MAKE) check
	$(MAKE) security
	$(MAKE) build-release
	@echo "Release preparation complete!"

release-archives: ## Create release archives
	@echo "Creating release archives..."
	mkdir -p dist
	tar -czf dist/m3u-proxy-linux-x86_64.tar.gz -C target/x86_64-unknown-linux-gnu/release m3u-proxy
	tar -czf dist/m3u-proxy-linux-arm64.tar.gz -C target/aarch64-unknown-linux-gnu/release m3u-proxy
	tar -czf dist/m3u-proxy-macos-x86_64.tar.gz -C target/x86_64-apple-darwin/release m3u-proxy
	tar -czf dist/m3u-proxy-macos-arm64.tar.gz -C target/aarch64-apple-darwin/release m3u-proxy
	ls -la dist/

# Cleanup
clean: ## Clean build artifacts
	@echo "Cleaning build artifacts..."
	cargo clean
	rm -rf dist/

clean-all: ## Clean everything including data
	@echo "Cleaning everything..."
	$(MAKE) clean
	rm -rf data/
	docker system prune -f

# Documentation
docs: ## Generate documentation
	@echo "Generating documentation..."
	cargo doc --no-deps --open

docs-serve: ## Serve documentation locally
	@echo "Serving documentation..."
	cargo doc --no-deps
	python3 -m http.server 8080 -d target/doc

# Benchmarking
bench: ## Run benchmarks
	@echo "Running benchmarks..."
	cargo bench

# Environment setup for CI
ci-setup: ## Setup CI environment
	@echo "Setting up CI environment..."
	rustup component add rustfmt clippy
	cargo install sqlx-cli --no-default-features --features sqlite,rustls
	cargo install cargo-audit cargo-deny

# Quick development cycle
quick: ## Quick development cycle (format, check, test, run)
	@echo "Running quick development cycle..."
	$(MAKE) fmt
	$(MAKE) check
	$(MAKE) run

# Production deployment helpers
deploy-check: ## Check if ready for deployment
	@echo "Checking deployment readiness..."
	$(MAKE) check
	$(MAKE) security
	$(MAKE) build-release
	@echo "✅ Ready for deployment!"

# Variables
RUST_LOG ?= info
DATABASE_URL ?= sqlite:data/m3u-proxy.db
HOST ?= 127.0.0.1
PORT ?= 8080

# Export environment variables
export RUST_LOG
export DATABASE_URL
