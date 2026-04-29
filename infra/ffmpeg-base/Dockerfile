# Prebuilt ffmpeg amd64 base image — native RTMP/RTMPS with --enable-openssl.
#
# Why this image exists:
#   server-rs/Dockerfile used to build ffmpeg from source in the deploy
#   pipeline. Under QEMU emulation on an amd64 runner that took 2+ hours
#   per push (observed 2026-04-20). This image pre-bakes the ffmpeg binary
#   once per FFMPEG_VERSION bump so every server-rs build is a fast COPY.
#
# Native RTMP/RTMPS is the supported path now:
#   Debian bookworm's librtmp 2.4 has caused AWS IVS/RTMPS handshake
#   failures. FFmpeg's native TLS-backed RTMP implementation is kept by
#   default; we explicitly enable OpenSSL and verify RTMPS protocol support.
#
# Build + push (from the repo root):
#   docker build --platform linux/amd64 \
#     -t <ECR_REGISTRY>/brivva/ffmpeg-base:7.1.1-native-rtmp \
#     -f infra/ffmpeg-base/Dockerfile infra/ffmpeg-base
#   docker push <ECR_REGISTRY>/brivva/ffmpeg-base:7.1.1-native-rtmp
#
# Tag convention:
#   <ffmpeg-version>-native-rtmp — primary, immutable pin
#   latest                     — convenience pointer, not used in prod
#
# The CI workflow in .github/workflows/ffmpeg-base.yml automates the above
# on pushes that touch this directory.

FROM debian:bookworm-slim AS build

ARG FFMPEG_VERSION=7.1.1

RUN apt-get update && apt-get install -y --no-install-recommends \
        build-essential yasm nasm pkg-config curl xz-utils ca-certificates \
        libx264-dev libx265-dev libmp3lame-dev libopus-dev \
        libssl-dev \
        libfreetype-dev libfontconfig-dev libass-dev libharfbuzz-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /tmp
RUN curl -fsSL "https://ffmpeg.org/releases/ffmpeg-${FFMPEG_VERSION}.tar.xz" | tar -xJ
WORKDIR /tmp/ffmpeg-${FFMPEG_VERSION}

RUN ./configure \
        --prefix=/usr/local \
        --enable-gpl \
        --enable-nonfree \
        --enable-openssl \
        --enable-libx264 \
        --enable-libx265 \
        --enable-libmp3lame \
        --enable-libopus \
        --enable-libfreetype \
        --enable-libfontconfig \
        --enable-libass \
        --enable-libharfbuzz \
        --disable-doc \
        --disable-debug \
    && make -j"$(nproc)" \
    && make install

RUN /usr/local/bin/ffmpeg -version | grep -q "enable-openssl" \
    || (echo "CRITICAL: ffmpeg built without --enable-openssl — RTMPS publishing will fail"; exit 1)
RUN /usr/local/bin/ffmpeg -hide_banner -protocols 2>&1 | grep -q "rtmps" \
    || (echo "CRITICAL: ffmpeg lacks native rtmps protocol"; exit 1)

# FFmpeg 6.1+ silently omits the `drawtext` filter when libharfbuzz is
# not detected at configure time, even if --enable-libfreetype succeeds.
# The first prod ffmpeg-base image shipped without drawtext (2026-04-20
# incident) because harfbuzz wasn't in the build deps. drawtext is
# non-negotiable for Brivva's subtitle burn-in path, so assert the
# filter is actually present in the binary. On failure, print the
# drawtext-related filter list so the real shape is grep-able in the
# CI log instead of just reporting "check failed".
RUN /usr/local/bin/ffmpeg -hide_banner -filters 2>&1 | grep -q drawtext \
    || { \
         echo "CRITICAL: drawtext filter missing — filter list below"; \
         /usr/local/bin/ffmpeg -hide_banner -filters 2>&1 | grep -i draw || echo "(no 'draw' matches at all)"; \
         exit 1; \
       }

# Final image is minimal: just the ffmpeg + ffprobe binaries. The server-rs
# runtime stage copies these out; runtime-side .so deps (libssl3, libx264/5,
# libass, fonts) are apt-installed there, not here.
#
# No post-COPY `ffmpeg -version` check here — this stage lacks the runtime
# .so libs (libass.so.9 etc.), so executing ffmpeg would fail even for a
# healthy binary. The build-stage verify above catches RTMPS/OpenSSL support.
FROM debian:bookworm-slim

COPY --from=build /usr/local/bin/ffmpeg /usr/local/bin/ffmpeg
COPY --from=build /usr/local/bin/ffprobe /usr/local/bin/ffprobe
