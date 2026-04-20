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

# ── ffmpeg-builder: compile ffmpeg with --enable-librtmp ─────
# Debian's apt ffmpeg does NOT include librtmp. Without librtmp, pushes
# to AWS IVS (Grip's backend) silently fail after ~3 frames — IVS
# accepts the RTMP handshake but never sends onStatus Publish.Start,
# then closes the socket server-side. ffmpeg's native RTMP implementation
# doesn't satisfy IVS's client fingerprinting; librtmp (same lib OBS
# uses) does. See `docs/runbook.md §2b` for the full write-up.
#
# Compile native on TARGETPLATFORM (arm64). Cross-compiling ffmpeg from
# amd64 is possible but the configure plumbing is fragile; QEMU-emulated
# native build is ~8 min on CI and fully cached in BuildKit, so unless
# the FFMPEG_VERSION pin changes this stage is a no-op on rebuilds.
FROM --platform=$TARGETPLATFORM debian:bookworm-slim AS ffmpeg-builder

ARG FFMPEG_VERSION=6.1.2

RUN apt-get update && apt-get install -y --no-install-recommends \
        build-essential yasm nasm pkg-config curl xz-utils ca-certificates \
        libx264-dev libx265-dev libmp3lame-dev libopus-dev \
        librtmp-dev libssl-dev \
        libfreetype-dev libfontconfig-dev libass-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /tmp
RUN curl -fsSL "https://ffmpeg.org/releases/ffmpeg-${FFMPEG_VERSION}.tar.xz" | tar -xJ
WORKDIR /tmp/ffmpeg-${FFMPEG_VERSION}

RUN ./configure \
        --prefix=/usr/local \
        --enable-gpl \
        --enable-nonfree \
        --enable-openssl \
        --enable-librtmp \
        --enable-libx264 \
        --enable-libx265 \
        --enable-libmp3lame \
        --enable-libopus \
        --enable-libfreetype \
        --enable-libfontconfig \
        --enable-libass \
        --disable-doc \
        --disable-debug \
    && make -j"$(nproc)" \
    && make install

# Build-time guard: fail the image if librtmp didn't make it into the
# binary. A silent regression here produces a green deploy that breaks
# every RTMP push to IVS on first real traffic — that's worse than a
# broken build, so stop it at the build.
RUN /usr/local/bin/ffmpeg -version | grep -q "enable-librtmp" \
    || (echo "CRITICAL: ffmpeg built without --enable-librtmp — IVS pushes will silently fail (docs/runbook.md §2b)"; exit 1)

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
    && fc-cache -f \
    && rm -rf /var/lib/apt/lists/*

COPY --from=ffmpeg-builder /usr/local/bin/ffmpeg /usr/local/bin/ffmpeg
COPY --from=ffmpeg-builder /usr/local/bin/ffprobe /usr/local/bin/ffprobe

# Runtime re-verify — catches any weird case where the binary was
# replaced/corrupted between stages. Logs at image start (cheap).
RUN /usr/local/bin/ffmpeg -version | grep -q "enable-librtmp" \
    || (echo "CRITICAL: runtime ffmpeg lacks librtmp"; exit 1)

COPY --from=builder /app/target/aarch64-unknown-linux-gnu/release/server-rs /usr/local/bin/server-rs

EXPOSE 3000

CMD ["server-rs"]
