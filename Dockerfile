# syntax=docker/dockerfile:1

########################################
# Stage 1: builder
########################################
FROM rust:1-bookworm AS builder

RUN apt-get update && apt-get install -y --no-install-recommends \
    libssl-dev \
    pkg-config \
    cmake \
    libgit2-dev \
    build-essential \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /usr/src/genome

# Copy the whole workspace. Individual crates may still be under active
# development; this build only requires that `crates/server` and its
# workspace dependencies compile.
COPY Cargo.toml Cargo.lock* ./
COPY crates ./crates

RUN cargo build --release --bin server

########################################
# Stage 2: runtime
########################################
FROM debian:bookworm-slim AS runtime

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    git \
    libssl3 \
    openssh-client \
    curl \
    && rm -rf /var/lib/apt/lists/* \
    && update-ca-certificates

RUN groupadd --system genome && \
    useradd --system --gid genome --create-home --home-dir /home/genome genome

WORKDIR /app

COPY --from=builder /usr/src/genome/target/release/server /app/server

# Optional default configuration shipped with the image; the runtime
# config directory can be overridden with a bind mount at deploy time.
COPY config /app/config

RUN mkdir -p /data/repos && chown -R genome:genome /app /data

USER genome

EXPOSE 8000

HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD curl -fsS http://localhost:8000/health || exit 1

ENTRYPOINT ["/app/server"]
