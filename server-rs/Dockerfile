# ── Multi-stage build for server-rs with cargo-chef ──────────
# Layer model:
#   1. chef    — base image + cargo-chef install (rarely rebuilt)
#   2. planner — reads Cargo.toml/Cargo.lock/sources, emits recipe.json
#   3. builder — cooks deps (big, cached), then compiles app src (small, fast)
#   4. runtime — slim debian with ffmpeg + fonts for drawtext subtitle overlay
#
# Result: source-only edits rebuild in ~30s vs ~8min. Deps layer invalidates
# only when Cargo.lock or Cargo.toml change.

FROM --platform=$BUILDPLATFORM rust:1.85-slim AS chef
RUN cargo install cargo-chef --locked --version 0.1.68
WORKDIR /app

# ── planner: extract dep recipe without building ─────────────
FROM chef AS planner
COPY Cargo.toml Cargo.lock ./
COPY server-rs/Cargo.toml server-rs/Cargo.toml
COPY server-rs/src server-rs/src
RUN cargo chef prepare --recipe-path recipe.json

# ── builder: cook deps (cached), then build app src ──────────
FROM chef AS builder
RUN apt-get update && apt-get install -y --no-install-recommends \
    libsqlite3-dev pkg-config \
    && rm -rf /var/lib/apt/lists/*

COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json

COPY Cargo.toml Cargo.lock ./
COPY server-rs/Cargo.toml server-rs/Cargo.toml
COPY server-rs/src server-rs/src
RUN cargo build --release --manifest-path server-rs/Cargo.toml --bin server-rs

# ── runtime: slim image with ffmpeg + subtitle fonts ─────────
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    libsqlite3-0 \
    ffmpeg \
    fontconfig \
    fonts-dejavu-core \
    fonts-noto-cjk \
    && fc-cache -f \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/server-rs /usr/local/bin/server-rs

EXPOSE 3000

CMD ["server-rs"]
