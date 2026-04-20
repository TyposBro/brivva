# Prebuilt ffmpeg arm64 base image — with --enable-librtmp + --enable-openssl.
#
# Why this image exists:
#   server-rs/Dockerfile used to build ffmpeg from source in the deploy
#   pipeline. Under QEMU emulation on an amd64 runner that took 2+ hours
#   per push (observed 2026-04-20). This image pre-bakes the ffmpeg binary
#   once per FFMPEG_VERSION bump so every server-rs build is a fast COPY.
#
# librtmp is non-negotiable:
#   AWS IVS (Grip's ingest) silently rejects ffmpeg's native RTMP
#   implementation — accepts handshake, never sends Publish.Start, closes
#   the socket at ~frame 3. librtmp (same library OBS uses) is the only
#   path that pushes cleanly. See docs/runbook.md §2b.
#
# Build + push (from the repo root):
#   docker build --platform linux/arm64 \
#     -t <ECR_REGISTRY>/brivva/ffmpeg-base:6.1.2-librtmp \
#     -f infra/ffmpeg-base/Dockerfile infra/ffmpeg-base
#   docker push <ECR_REGISTRY>/brivva/ffmpeg-base:6.1.2-librtmp
#
# Tag convention:
#   <ffmpeg-version>-librtmp   — primary, immutable pin
#   latest                     — convenience pointer, not used in prod
#
# The CI workflow in .github/workflows/ffmpeg-base.yml automates the above
# on pushes that touch this directory.

FROM debian:bookworm-slim AS build

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

RUN /usr/local/bin/ffmpeg -version | grep -q "enable-librtmp" \
    || (echo "CRITICAL: ffmpeg built without --enable-librtmp — IVS pushes will silently fail (docs/runbook.md §2b)"; exit 1)

# Final image is minimal: just the ffmpeg + ffprobe binaries. The server-rs
# runtime stage copies these out; the runtime-side .so deps (librtmp1,
# libssl3, libx264/5, libass, fonts) are apt-installed there, not here.
#
# No post-COPY `ffmpeg -version` check here — this stage lacks the
# runtime .so libs (libass.so.9 etc.), so executing ffmpeg would fail
# even for a healthy binary. The build-stage verify above catches any
# librtmp regression at compile time, and `server-rs/Dockerfile` re-runs
# the same check after its own COPY in an image that has the runtime
# deps installed.
FROM debian:bookworm-slim

COPY --from=build /usr/local/bin/ffmpeg /usr/local/bin/ffmpeg
COPY --from=build /usr/local/bin/ffprobe /usr/local/bin/ffprobe
