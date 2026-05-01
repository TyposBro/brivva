# Runtime base for server-rs on ECS-on-EC2 GPU.
# Copies FFmpeg built with NVENC support, then installs runtime libs/fonts.
# NVIDIA libcuda/libnvidia-encode come from the ECS GPU host driver.

ARG FFMPEG_GPU_BASE_IMAGE=brivva-ffmpeg-gpu-base-image-not-set-pass-build-arg:latest

FROM ${FFMPEG_GPU_BASE_IMAGE} AS ffmpeg-gpu-base

FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates \
        fontconfig \
        fonts-dejavu-core \
        fonts-noto-cjk \
        libx264-164 \
        libx265-199 \
        libmp3lame0 \
        libopus0 \
        libssl3 \
        libfreetype6 \
        libfontconfig1 \
        libass9 \
        libharfbuzz0b \
    && fc-cache -f \
    && rm -rf /var/lib/apt/lists/*

COPY --from=ffmpeg-gpu-base /usr/local/bin/ffmpeg /usr/local/bin/ffmpeg
COPY --from=ffmpeg-gpu-base /usr/local/bin/ffprobe /usr/local/bin/ffprobe

RUN /usr/local/bin/ffmpeg -version | grep -q "enable-openssl" \
    || (echo "CRITICAL: runtime ffmpeg lacks OpenSSL support for RTMPS"; exit 1)
RUN /usr/local/bin/ffmpeg -hide_banner -protocols 2>&1 | grep -q "rtmps" \
    || (echo "CRITICAL: runtime ffmpeg lacks native rtmps protocol"; exit 1)
RUN /usr/local/bin/ffmpeg -hide_banner -filters 2>&1 | grep -q drawtext \
    || (echo "CRITICAL: runtime ffmpeg lacks drawtext filter"; exit 1)
RUN /usr/local/bin/ffmpeg -hide_banner -encoders 2>&1 | grep -q h264_nvenc \
    || (echo "CRITICAL: runtime ffmpeg lacks h264_nvenc"; exit 1)
