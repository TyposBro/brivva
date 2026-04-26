# Runtime base for server-rs. This is rebuilt only when the FFmpeg pin,
# runtime shared libraries, or subtitle font set changes.
ARG FFMPEG_BASE_IMAGE=brivva-ffmpeg-base-image-not-set-pass-build-arg:latest

FROM ${FFMPEG_BASE_IMAGE} AS ffmpeg-base

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
        librtmp1 \
        libssl3 \
        libfreetype6 \
        libfontconfig1 \
        libass9 \
        libharfbuzz0b \
    && fc-cache -f \
    && rm -rf /var/lib/apt/lists/*

COPY --from=ffmpeg-base /usr/local/bin/ffmpeg /usr/local/bin/ffmpeg
COPY --from=ffmpeg-base /usr/local/bin/ffprobe /usr/local/bin/ffprobe

RUN /usr/local/bin/ffmpeg -version | grep -q "enable-librtmp" \
    || (echo "CRITICAL: runtime ffmpeg lacks librtmp"; exit 1)

RUN /usr/local/bin/ffmpeg -hide_banner -filters 2>&1 | grep -q drawtext \
    || { \
         echo "CRITICAL: runtime ffmpeg lacks drawtext filter - subtitle burn-in will crash every stream"; \
         /usr/local/bin/ffmpeg -hide_banner -filters 2>&1 | grep -i draw || echo "(no 'draw' matches at all)"; \
         exit 1; \
       }
