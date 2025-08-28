# Multi-stage Dockerfile for m3u-proxy using LinuxServer FFmpeg as runtime base
ARG RUST_VERSION=1.89
ARG NODE_VERSION=22
ARG LINUXSERVER_FFMPEG_VERSION=latest

# Frontend build stage
FROM node:${NODE_VERSION}-alpine AS frontend-builder
WORKDIR /app/frontend
COPY frontend/package*.json ./
# Install ALL dependencies (including devDependencies) for building
RUN npm ci --silent
COPY frontend/ ./
# Build the frontend
RUN npm run build && ls -la out/

# Backend build stage
FROM rust:${RUST_VERSION} AS backend-builder

# Install build dependencies
RUN apt-get update && apt-get install -y \
    pkg-config libssl-dev curl ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Build Rust application
WORKDIR /usr/src/m3u-proxy
COPY Cargo.toml Cargo.lock justfile ./
COPY crates/ ./crates/
# Copy frontend assets
RUN mkdir -p crates/m3u-proxy/static
COPY --from=frontend-builder /app/frontend/out/ ./crates/m3u-proxy/static/

# Build binary
RUN cargo build --release --bin m3u-proxy

# Prepare entrypoint script
COPY entrypoint.sh /tmp/entrypoint.sh
RUN chmod +x /tmp/entrypoint.sh

# Runtime stage: LinuxServer FFmpeg with GPU acceleration + m3u-proxy
FROM lscr.io/linuxserver/ffmpeg:${LINUXSERVER_FFMPEG_VERSION} AS runtime

# Build args for labels
ARG RUST_VERSION
ARG NODE_VERSION
ARG LINUXSERVER_FFMPEG_VERSION

# Set container labels
LABEL maintainer="John Mylchreest <jmylchreest@gmail.com>" \
    description="M3U Proxy service with LinuxServer FFmpeg + GPU acceleration" \
    rust_version="${RUST_VERSION}" \
    node_version="${NODE_VERSION}" \
    ffmpeg_base="lscr.io/linuxserver/ffmpeg:${LINUXSERVER_FFMPEG_VERSION}" \
    gpu_support="Intel/AMD/NVIDIA"

# Create application user and directories in one layer
RUN groupadd -g 65532 m3u-proxy && \
    useradd -u 65532 -g 65532 -r -s /bin/false m3u-proxy && \
    mkdir -p /app/data /app/config && \
    chown -R 65532:65532 /app

# Copy application files and set final configuration
COPY --from=backend-builder /usr/src/m3u-proxy/target/release/m3u-proxy /app/m3u-proxy
COPY --from=backend-builder /usr/src/m3u-proxy/crates/m3u-proxy/config.example.toml /app/config.example.toml
COPY --from=backend-builder /tmp/entrypoint.sh /app/entrypoint.sh

WORKDIR /app
VOLUME ["/app/data", "/app/config"]
ENV PATH="/app:${PATH}"
USER 65532:65532
EXPOSE 8080
ENTRYPOINT ["/app/entrypoint.sh"]
