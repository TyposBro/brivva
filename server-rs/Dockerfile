# ── Multi-stage cross-compile build for server-rs ────────────
# Build natively on BUILDPLATFORM (amd64 on GH runner),
# cross-compile to aarch64-unknown-linux-gnu via cargo-zigbuild,
# runtime on TARGETPLATFORM (arm64 for Fargate Graviton).
# ~6× faster than QEMU emulation; cargo-chef keeps warm deps cached.

FROM --platform=$BUILDPLATFORM rust:1.88-slim AS chef
RUN apt-get update && apt-get install -y --no-install-recommends \
        cmake build-essential libclang-dev python3 python3-pip \
    && rm -rf /var/lib/apt/lists/* \
    && pip3 install --break-system-packages ziglang \
    && cargo install cargo-chef cargo-zigbuild --locked \
    && rustup target add aarch64-unknown-linux-gnu
WORKDIR /app

# ── planner: extract dep recipe without building ─────────────
FROM chef AS planner
COPY Cargo.toml Cargo.lock ./
COPY server-rs/Cargo.toml server-rs/Cargo.toml
COPY server-rs/src server-rs/src
RUN cargo chef prepare --recipe-path recipe.json

# ── builder: cook deps (cached), cross-compile app ───────────
FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --zigbuild \
    --target aarch64-unknown-linux-gnu --recipe-path recipe.json

COPY Cargo.toml Cargo.lock ./
COPY server-rs/Cargo.toml server-rs/Cargo.toml
COPY server-rs/src server-rs/src
RUN cargo zigbuild --release --target aarch64-unknown-linux-gnu \
    --manifest-path server-rs/Cargo.toml --bin server-rs

# ── runtime: arm64 slim debian with ffmpeg + subtitle fonts ──
FROM --platform=$TARGETPLATFORM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates \
        ffmpeg \
        fontconfig \
        fonts-dejavu-core \
        fonts-noto-cjk \
    && fc-cache -f \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/aarch64-unknown-linux-gnu/release/server-rs /usr/local/bin/server-rs

EXPOSE 3000

CMD ["server-rs"]
