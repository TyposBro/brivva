#!/usr/bin/env bash
# Brivva RTMP Testing
#
# Chinese (zh) -> YouTube (user provides RTMP URL in Brivva UI)
# Japanese (ja) -> local RTMP server + player
#
# Usage:
#   ./test-rtmp.sh              # start server + player
#   ./test-rtmp.sh --server     # start server only (no player)
#   ./test-rtmp.sh --help       # show this help

set -euo pipefail

RTMP_PORT=1935
RTSP_PORT=8554
HLS_PORT=8888
STREAM_KEY="ja"
RTMP_URL="rtmp://localhost:${RTMP_PORT}/live/${STREAM_KEY}"

PIDS=()

cleanup() {
    echo ""
    echo "[test-rtmp] Shutting down..."
    for pid in "${PIDS[@]}"; do
        if kill -0 "$pid" 2>/dev/null; then
            kill "$pid" 2>/dev/null || true
            wait "$pid" 2>/dev/null || true
        fi
    done
    echo "[test-rtmp] Done."
}

trap cleanup EXIT INT TERM

check_port() {
    if lsof -iTCP:$1 -sTCP:LISTEN &>/dev/null; then
        echo "[test-rtmp] ERROR: Port $1 already has a listener."
        lsof -iTCP:$1 -sTCP:LISTEN | head -5
        return 1
    fi
}

print_urls() {
    echo ""
    echo "=============================================="
    echo "  Brivva RTMP Test Infrastructure"
    echo "=============================================="
    echo ""
    echo "  Local RTMP server: mediamtx v1.17.1"
    echo "  Ports: RTMP=$RTMP_PORT  RTSP=$RTSP_PORT  HLS=$HLS_PORT"
    echo ""
    echo "  --- RTMP URLs for Brivva UI ---"
    echo ""
    echo "  Japanese (ja) -> local:"
    echo "    RTMP URL:    rtmp://localhost:${RTMP_PORT}/live"
    echo "    Stream Key:  ${STREAM_KEY}"
    echo "    Full URL:    ${RTMP_URL}"
    echo ""
    echo "  Chinese (zh) -> YouTube:"
    echo "    RTMP URL:    rtmp://a.rtmp.youtube.com/live2"
    echo "    Stream Key:  <your-youtube-stream-key>"
    echo ""
    echo "  --- Playback ---"
    echo ""
    echo "  View Japanese stream:"
    echo "    mpv ${RTMP_URL}"
    echo "    mpv rtsp://localhost:${RTSP_PORT}/live/${STREAM_KEY}"
    echo "    Browser: http://localhost:${HLS_PORT}/live/${STREAM_KEY}"
    echo ""
    echo "=============================================="
    echo ""
}

start_server() {
    check_port "$RTMP_PORT"

    echo "[test-rtmp] Starting mediamtx..."
    mediamtx /opt/homebrew/etc/mediamtx/mediamtx.yml &
    PIDS+=($!)

    # Wait for server to be ready
    for i in {1..10}; do
        if lsof -iTCP:${RTMP_PORT} -sTCP:LISTEN &>/dev/null; then
            echo "[test-rtmp] mediamtx ready (RTMP on port ${RTMP_PORT})."
            return 0
        fi
        sleep 0.5
    done
    echo "[test-rtmp] WARNING: mediamtx may not be ready yet."
}

start_player() {
    echo "[test-rtmp] Waiting for Japanese stream on ${RTMP_URL}..."
    echo "[test-rtmp] Start Brivva and begin streaming. Player retries every 3s."
    echo "[test-rtmp] Press Ctrl+C to stop everything."
    echo ""

    while true; do
        mpv \
            --force-media-title="Brivva Japanese Stream" \
            --profile=low-latency \
            --cache=no \
            --msg-level=all=warn \
            "$RTMP_URL" 2>/dev/null || true
        echo "[test-rtmp] Stream not available yet, retrying in 3s..."
        sleep 3
    done
}

show_help() {
    echo "Usage: $0 [OPTIONS]"
    echo ""
    echo "Options:"
    echo "  --server    Start RTMP server only (no player)"
    echo "  --help      Show this help message"
    echo ""
    echo "Default: Start server + mpv player for Japanese stream"
}

# --- Main ---

case "${1:-}" in
    --help|-h)
        show_help
        exit 0
        ;;
    --server)
        start_server
        print_urls
        echo "[test-rtmp] Server-only mode. Press Ctrl+C to stop."
        wait
        ;;
    *)
        start_server
        print_urls
        start_player
        ;;
esac
