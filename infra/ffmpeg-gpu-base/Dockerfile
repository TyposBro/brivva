# Prebuilt FFmpeg GPU base — native RTMPS + drawtext + NVIDIA NVENC.
#
# Build + push from repo root:
#   docker build --platform linux/amd64 \
#     -t <ECR_REGISTRY>/brivva/ffmpeg-gpu-base:7.1.1-nvenc \
#     -f infra/ffmpeg-gpu-base/Dockerfile infra/ffmpeg-gpu-base
#
# Runtime requires NVIDIA host driver/container runtime. ECS GPU-optimized AMI
# provides that on ECS-on-EC2 GPU instances.

FROM debian:bookworm-slim AS build

ARG FFMPEG_VERSION=7.1.1
ARG NV_CODEC_HEADERS_REF=n12.2.72.0

RUN apt-get update && apt-get install -y --no-install-recommends \
        build-essential yasm nasm pkg-config curl xz-utils ca-certificates git \
        libx264-dev libx265-dev libmp3lame-dev libopus-dev \
        libssl-dev \
        libfreetype-dev libfontconfig-dev libass-dev libharfbuzz-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /tmp
RUN git clone --depth 1 --branch "${NV_CODEC_HEADERS_REF}" https://git.videolan.org/git/ffmpeg/nv-codec-headers.git \
    && make -C nv-codec-headers install

RUN curl -fsSL "https://ffmpeg.org/releases/ffmpeg-${FFMPEG_VERSION}.tar.xz" | tar -xJ
WORKDIR /tmp/ffmpeg-${FFMPEG_VERSION}

RUN ./configure \
        --prefix=/usr/local \
        --enable-gpl \
        --enable-nonfree \
        --enable-openssl \
        --enable-ffnvcodec \
        --enable-nvenc \
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
RUN /usr/local/bin/ffmpeg -hide_banner -filters 2>&1 | grep -q drawtext \
    || (echo "CRITICAL: drawtext filter missing"; exit 1)
RUN /usr/local/bin/ffmpeg -hide_banner -encoders 2>&1 | grep -q h264_nvenc \
    || (echo "CRITICAL: h264_nvenc encoder missing"; exit 1)

FROM debian:bookworm-slim
COPY --from=build /usr/local/bin/ffmpeg /usr/local/bin/ffmpeg
COPY --from=build /usr/local/bin/ffprobe /usr/local/bin/ffprobe
