# ── Multi-stage cross-compile build for server-rs ────────────
# Build natively on BUILDPLATFORM (amd64 on GH runner),
# cross-compile to aarch64-unknown-linux-gnu via cargo-zigbuild,
# runtime on TARGETPLATFORM (arm64 for Fargate Graviton).
# ~6× faster than QEMU emulation; cargo-chef keeps warm deps cached.

# Global ARG — must be declared BEFORE the first FROM so it expands
# inside the `FROM ${FFMPEG_BASE_IMAGE}` below. Any ARG defined after a
# FROM is scoped to that stage only and leaves subsequent `FROM <ARG>`
# lines empty, which is exactly how the first prod attempt failed (run
# 24657046453: `ERROR: base name (${FFMPEG_BASE_IMAGE}) should not be
# blank`).
#
# CI sets this in `.github/workflows/deploy.yml` as
# `<ECR_REGISTRY>/brivva/ffmpeg-base:<FFMPEG_VERSION>-librtmp`. Local
# builds must pass `--build-arg FFMPEG_BASE_IMAGE=...`; the default
# string is intentionally invalid so an omitted arg fails fast at
# `docker pull` with a clear image-not-found.
ARG FFMPEG_BASE_IMAGE=brivva-ffmpeg-base-image-not-set-pass-build-arg:latest

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

# ── ffmpeg-builder: pull prebuilt arm64 ffmpeg (with --enable-librtmp) ─
# Previously this stage compiled ffmpeg from source. Under QEMU emulation
# on an amd64 runner that took 2+ hours per push (2026-04-20 incident).
# The build now lives in `infra/ffmpeg-base/Dockerfile` + is published by
# `.github/workflows/ffmpeg-base.yml` as an ECR image; this stage just
# COPYs the binary out, taking the full ffmpeg work out of the critical
# deploy path.
#
# librtmp is still non-negotiable (AWS IVS rejects ffmpeg's native RTMP
# impl after ~3 frames). The runtime `ffmpeg -version | grep enable-librtmp`
# check below still fires so a mistagged base image can't ship silently.
#
# FFMPEG_BASE_IMAGE is declared as a global ARG at the top of this file
# — see the comment block next to that declaration for the why.
FROM ${FFMPEG_BASE_IMAGE} AS ffmpeg-builder

# ── runtime: arm64 slim debian with ffmpeg + subtitle fonts ──
FROM --platform=$TARGETPLATFORM debian:bookworm-slim

# Runtime shared libs ffmpeg links against, minus the dev/build toolchain.
RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates \
        fontconfig \
        fonts-dejavu-core \
        fonts-noto-cjk \
        libx264-164 \
        libx265-199 \
        libmp3lame0 \
        libopus0 \
        librtmp1 \
        libssl3 \
        libfreetype6 \
        libfontconfig1 \
        libass9 \
        libharfbuzz0b \
    && fc-cache -f \
    && rm -rf /var/lib/apt/lists/*

COPY --from=ffmpeg-builder /usr/local/bin/ffmpeg /usr/local/bin/ffmpeg
COPY --from=ffmpeg-builder /usr/local/bin/ffprobe /usr/local/bin/ffprobe

# Runtime re-verify — catches any weird case where the binary was
# replaced/corrupted between stages. Logs at image start (cheap).
RUN /usr/local/bin/ffmpeg -version | grep -q "enable-librtmp" \
    || (echo "CRITICAL: runtime ffmpeg lacks librtmp"; exit 1)

# drawtext filter is required for subtitle burn-in on every
# translation target. FFmpeg 6.1+ silently omits drawtext unless
# libharfbuzz is linked. This check catches a broken ffmpeg-base
# rebuild (2026-04-20 incident) before it reaches a live session.
RUN /usr/local/bin/ffmpeg -hide_banner -filters 2>&1 | grep -q drawtext \
    || { \
         echo "CRITICAL: runtime ffmpeg lacks drawtext filter — subtitle burn-in will crash every stream"; \
         /usr/local/bin/ffmpeg -hide_banner -filters 2>&1 | grep -i draw || echo "(no 'draw' matches at all)"; \
         exit 1; \
       }

COPY --from=builder /app/target/aarch64-unknown-linux-gnu/release/server-rs /usr/local/bin/server-rs

EXPOSE 3000

CMD ["server-rs"]
