# ==============================================================================
# OpenUnpack Multi-Stage Dockerfile (Headless Server & CLI)
# ==============================================================================
# Stage 1: Build binary using official Rust bookworm
FROM rust:1.85-bookworm AS builder

WORKDIR /build

# Install dependencies needed for compilation (including pkg-config, openssl, webkit/tauri build requirements)
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    libssl-dev \
    libglib2.0-dev \
    libgtk-3-dev \
    libwebkit2gtk-4.1-dev \
    libayatana-appindicator3-dev \
    librsvg2-dev \
    patchelf \
    && rm -rf /var/lib/apt/lists/*

# Copy workspace manifests
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY apps ./apps

# Build desktop host in headless mode and CLI
RUN cargo build --release -p openunpack-desktop -p openunpack-cli

# ==============================================================================
# Stage 2: Minimal Runtime Image
# ==============================================================================
FROM debian:bookworm-slim AS runtime

LABEL org.opencontainers.image.title="OpenUnpack Daemon" \
      org.opencontainers.image.description="High-performance batch unarchiving daemon with native Rust engine, Local API & CLI" \
      org.opencontainers.image.authors="OpenUnpack Authors" \
      org.opencontainers.image.licenses="MIT"

# Install runtime utilities & 7-Zip CLI engine fallback
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    curl \
    p7zip-full \
    unrar-free \
    libssl3 \
    libglib2.0-0 \
    libgtk-3-0 \
    libwebkit2gtk-4.1-0 \
    libayatana-appindicator3-1 \
    librsvg2-2 \
    && rm -rf /var/lib/apt/lists/*

# Create dedicated non-root user and directories
RUN groupadd -g 1000 openunpack && \
    useradd -u 1000 -g openunpack -m -s /bin/bash openunpack && \
    mkdir -p /data /downloads /extracted /watch && \
    chown -R openunpack:openunpack /data /downloads /extracted /watch

# Copy binaries from builder
COPY --from=builder --chown=openunpack:openunpack /build/target/release/openunpack-desktop /usr/local/bin/openunpack-desktop
COPY --from=builder --chown=openunpack:openunpack /build/target/release/openunpack /usr/local/bin/openunpack

USER openunpack
WORKDIR /home/openunpack

# Default configuration environment variables
ENV OPENUNPACK_DATA_DIR=/data \
    OPENUNPACK_ENGINE=native \
    OPENUNPACK_PORT=18420 \
    OPENUNPACK_TRUSTED_ALL_PATHS=1 \
    HOME=/home/openunpack

# Persistent and mountable volumes
VOLUME ["/data", "/downloads", "/extracted", "/watch"]

# Expose Local REST & WebSocket API port
EXPOSE 18420

# Health check to ensure Local API is responding
HEALTHCHECK --interval=30s --timeout=5s --start-period=5s --retries=3 \
    CMD curl -f http://127.0.0.1:18420/api/v1/status || exit 1

# Launch OpenUnpack in headless daemon mode
ENTRYPOINT ["/usr/local/bin/openunpack-desktop", "--headless", "--data-dir", "/data", "--port", "18420", "--trust-all"]
