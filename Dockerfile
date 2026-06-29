# ─── Stage 1: Builder ─────────────────────────────────────────────────────────
FROM rust:1.96-slim AS builder

WORKDIR /build

# musl toolchain + OpenSSL vendored build deps (no system libssl needed)
RUN apt-get update && apt-get install -y \
    musl-tools \
    perl \
    make \
    && rm -rf /var/lib/apt/lists/*

RUN rustup target add x86_64-unknown-linux-musl

# Copy source
COPY . .

# Remove local path patch (not available in CI/Docker)
RUN sed -i '/^\[patch\.crates-io\]/,/^hf-hub.*\.tmp.*/d' Cargo.toml

# Build fully static musl binary — no GLIBC dependency at runtime
ENV OPENSSL_VENDORED=1
ENV CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER=x86_64-linux-musl-gcc
RUN cargo build --release --bin raios --bin aiosd --no-default-features \
    --target x86_64-unknown-linux-musl

# ─── Stage 2: Runtime ─────────────────────────────────────────────────────────
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y \
    ca-certificates \
    git \
    && rm -rf /var/lib/apt/lists/*

# Copy static binaries
COPY --from=builder /build/target/x86_64-unknown-linux-musl/release/raios /usr/local/bin/raios
COPY --from=builder /build/target/x86_64-unknown-linux-musl/release/aiosd /usr/local/bin/aiosd

# Config and workspace directories
RUN mkdir -p /data/config /data/dev

ENV HOME=/data
ENV XDG_CONFIG_HOME=/data/.config

EXPOSE 42069 42070 42071

CMD ["aiosd"]
