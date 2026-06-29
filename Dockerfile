# ─── Stage 1: Builder ─────────────────────────────────────────────────────────
FROM rust:1.96-slim AS builder

WORKDIR /build

# System deps for rusqlite (bundled) and OpenSSL
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Copy source
COPY . .

# Remove local path patch (not available in CI/Docker)
RUN sed -i '/^\[patch\.crates-io\]/,/^hf-hub.*\.tmp.*/d' Cargo.toml

# Build release binaries — cortex (fastembed/ONNX) disabled, not needed for Hub
RUN cargo build --release --bin raios --bin aiosd --no-default-features

# ─── Stage 2: Runtime ─────────────────────────────────────────────────────────
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y \
    ca-certificates \
    git \
    && rm -rf /var/lib/apt/lists/*

# Copy binaries
COPY --from=builder /build/target/release/raios /usr/local/bin/raios
COPY --from=builder /build/target/release/aiosd /usr/local/bin/aiosd

# Config and workspace directories
RUN mkdir -p /data/config /data/dev

ENV HOME=/data
ENV XDG_CONFIG_HOME=/data/.config

EXPOSE 42069 42070 42071

CMD ["aiosd"]
