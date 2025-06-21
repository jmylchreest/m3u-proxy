# Multi-stage build for m3u-proxy
FROM rust:slim as builder

# Install system dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Set the working directory
WORKDIR /app

# Copy dependency files
COPY Cargo.toml Cargo.lock ./

# Create a dummy main.rs to build dependencies
RUN mkdir src && echo "fn main() {}" > src/main.rs

# Build dependencies (this will be cached)
RUN cargo build --release && rm -rf src

# Copy source code
COPY src ./src
COPY migrations ./migrations
COPY static ./static

# Build the application
RUN cargo build --release

# Runtime stage
FROM debian:bookworm-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Create app user
RUN useradd -m -u 1000 appuser

# Create directories
RUN mkdir -p /app/data/logos /app/data/m3u /app/static \
    && chown -R appuser:appuser /app

# Copy binary from builder stage
COPY --from=builder /app/target/release/m3u-proxy /app/m3u-proxy

# Copy static files
COPY --from=builder /app/static /app/static

# Copy migrations
COPY --from=builder /app/migrations /app/migrations

# Copy example config
COPY config.example.toml /app/config.toml

# Set permissions
RUN chmod +x /app/m3u-proxy && chown -R appuser:appuser /app

# Switch to app user
USER appuser

# Set working directory
WORKDIR /app

# Expose port
EXPOSE 8080

# Health check
HEALTHCHECK --interval=30s --timeout=10s --start-period=5s --retries=3 \
    CMD curl -f http://localhost:8080/health || exit 1

# Run the application
CMD ["./m3u-proxy"]
